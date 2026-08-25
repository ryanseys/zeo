//! Multiple assignment: the before/splat/after split against a to_ary
//! coerced value, nested target groups, and the constant target form.

use super::ctx::Fx;
use super::ownership;
use crate::hir::NodeId;
use cranelift_codegen::ir::{InstBuilder, types};

/// A multiple assignment (`a, b = ...`, `a, *r, c = arr`, nested
/// groups): the runtime splits the (to_ary-coerced) value against the
/// before/splat/after shape into owned slots; each target consumes its
/// slot. `value_ptr` is a BORROW of the right-hand side.
pub(crate) fn lower_multi_group(
    fx: &mut Fx,
    site: NodeId,
    group: &crate::hir::MultiTargetGroup,
    value_ptr: cranelift_codegen::ir::Value,
) -> Result<(), String> {
    let n_before = group.before.len();
    let n_after = group.after.len();
    let has_splat = group.splat.is_some();
    let n_out = n_before + usize::from(has_splat) + n_after;
    let slots =
        fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (n_out.max(1) as u32) * super::ctx::VALUE_SIZE,
            3,
        ));
    let slots_ptr = fx.slot_addr(slots, 0);
    let nb = fx.b.ins().iconst(fx.em.ptr, n_before as i64);
    let hs = fx.b.ins().iconst(types::I8, i64::from(has_splat));
    let na = fx.b.ins().iconst(fx.em.ptr, n_after as i64);
    let status = fx.call_status("zeo_rt_multi_split", &[value_ptr, nb, hs, na, slots_ptr]);
    fx.fallible(status);
    fx.owned_created += n_out;
    let mut s = 0usize;
    let addr_of_slot = |fx: &mut Fx, s: usize| fx.slot_addr(slots, (s as u32 * 24) as i32);
    for t in &group.before {
        let addr = addr_of_slot(fx, s);
        write_multi_target(fx, site, t, addr)?;
        s += 1;
    }
    match &group.splat {
        Some(Some(t)) => {
            let addr = addr_of_slot(fx, s);
            write_multi_target(fx, site, t, addr)?;
            s += 1;
        }
        Some(None) => {
            // An anonymous `*` collects and discards.
            let addr = addr_of_slot(fx, s);
            fx.call("zeo_rt_release", &[addr]);
            fx.owned_consumed += 1;
            s += 1;
        }
        None => {}
    }
    for t in &group.after {
        let addr = addr_of_slot(fx, s);
        write_multi_target(fx, site, t, addr)?;
        s += 1;
    }
    debug_assert_eq!(s, n_out);
    Ok(())
}

/// One multi-assignment target's write; `addr` holds the OWNED slot value
/// the target consumes.
pub(super) fn write_multi_target(
    fx: &mut Fx,
    site: NodeId,
    target: &crate::hir::MultiTarget,
    addr: cranelift_codegen::ir::Value,
) -> Result<(), String> {
    use crate::hir::MultiTarget;
    let op = super::operand::Operand::Ptr {
        addr,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    };
    match target {
        MultiTarget::Local(name) => {
            ownership::write_local(fx, name, &op);
            Ok(())
        }
        MultiTarget::Ivar(name) => super::ivars::ivar_write_op(fx, name, op),
        MultiTarget::Call {
            write_call,
            tmp_name,
        } => {
            // Bind the write's synthetic hidden local, then run the write
            // call itself through the ordinary expression path.
            ownership::write_local(fx, tmp_name, &op);
            let r = super::expr::lower_expr(fx, *write_call)?;
            ownership::discard(fx, r);
            Ok(())
        }
        MultiTarget::Nested(group) => {
            // The nested group destructures the slot's value (borrowing it
            // for the split), then the slot itself is done.
            let group = group.clone();
            lower_multi_group(fx, site, &group, addr)?;
            fx.call("zeo_rt_release", &[addr]);
            fx.owned_consumed += 1;
            Ok(())
        }
        // The storage forms: each routes through the same write the
        // single-assignment arm uses, with the slot's value as the rhs.
        MultiTarget::ClassVar(name) => {
            let owner = super::ivars::cvar_owner(fx, name);
            let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner));
            let (nptr, nlen) = super::stmt::name_pair(fx, name);
            let status = fx.call_status("zeo_rt_cvar_set", &[owner_v, nptr, nlen, addr]);
            fx.fallible(status);
            fx.call("zeo_rt_release", &[addr]);
            fx.owned_consumed += 1;
            Ok(())
        }
        MultiTarget::Global(name) => {
            let bx = fx.box_v();
            let (nptr, nlen) = super::stmt::name_pair(fx, name);
            let status = fx.call_status("zeo_rt_gvar_assign", &[bx, nptr, nlen, addr]);
            fx.fallible(status);
            fx.call("zeo_rt_release", &[addr]);
            fx.owned_consumed += 1;
            Ok(())
        }
        MultiTarget::Const(name) => {
            let name = name.clone();
            const_multi_write(fx, site, None, &name, addr)
        }
        MultiTarget::ScopedConst { scope, name } => {
            let (scope, name) = (scope.clone(), name.clone());
            const_multi_write(fx, site, Some(&scope), &name, addr)
        }
    }
}

/// A constant multi-assignment target's write -- `const_set_at` with the
/// declaring site's location, exactly as the single-assignment arm does.
pub(super) fn const_multi_write(
    fx: &mut Fx,
    site: NodeId,
    scope: Option<&str>,
    name: &str,
    addr: cranelift_codegen::ir::Value,
) -> Result<(), String> {
    let owner_class = match scope {
        Some(s) => match super::boxes::resolve_class_here(fx, s) {
            Some(cid) => cid,
            None => return fx.unsupported(site, "a constant multi-assignment on a runtime scope"),
        },
        None => fx.method_class.unwrap_or_else(|| super::boxes::box_top(fx)),
    };
    let owner = fx
        .an
        .compiler
        .class_opt(owner_class)
        .and_then(|c| c.const_owners.get(name))
        .copied()
        .unwrap_or(owner_class)
        .0;
    let Some((file, line)) = crate::analyze::source::source_location(&fx.an.compiler, site) else {
        return fx.unsupported(site, "a span-less constant multi-assignment");
    };
    let (file, line) = (file.to_string(), line);
    let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner));
    let (nptr, nlen) = super::stmt::name_pair(fx, name);
    let (fptr, flen) = super::stmt::name_pair(fx, &file);
    let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
    fx.call(
        "zeo_rt_const_set_at",
        &[owner_v, nptr, nlen, addr, fptr, flen, line_v],
    );
    fx.call("zeo_rt_release", &[addr]);
    fx.owned_consumed += 1;
    Ok(())
}
