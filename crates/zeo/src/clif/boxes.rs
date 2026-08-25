//! Box and lexical-scope helpers: the cref chain, the box handle and
//! box-top class, the guarded accessor fold, `Ruby::Box.current`, and
//! `Module.nesting` -- the emitter's compile-time view of lexical scope.

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::codegen_error::CResult;
use crate::hir::{ArrayElem, HirNode, NodeId};
use cranelift_codegen::ir::{InstBuilder, types};

/// The lexical cref chain enclosing the current body, outermost first --
/// `Compiler::cref_of`'s frozen answer for the emitting class; empty at
/// the top level (rustc's `Ctx::cref_chain`).
pub(super) fn cref_chain<'a>(fx: &'a Fx) -> &'a [crate::compiler::ClassId] {
    lexical_class(fx)
        .map(|c| fx.an.compiler.cref_of_ref(c))
        .unwrap_or(&[])
}

/// The run-time handle for a box, gate and all -- see the `BoxHandle` arm.
pub(crate) fn box_handle(fx: &mut Fx, box_id: u32) -> CResult<Operand> {
    let bx = fx.b.ins().iconst(types::I32, i64::from(box_id));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status("zeo_rt_box_handle", &[bx, out]);
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The class a cref-less constant belongs to: `Object`, or -- inside a BOX
/// -- the box's own SURROGATE. A box is a copy of MASTER, so its top-level
/// constants must not land in (or be read from) main's `Object` table;
/// rustc's `box_top_owner` draws the same line.
pub(crate) fn box_top(fx: &Fx) -> crate::compiler::ClassId {
    if fx.box_id == 0 {
        return crate::compiler::OBJECT_CLASS;
    }
    fx.an
        .compiler
        .box_surrogate(fx.box_id)
        .expect("analyze registers a surrogate for every allocated box")
}

/// The class a LEXICAL question resolves against: the singleton surrogate
/// when the body was written in a constant-bearing `class << self`, else the
/// class the body was WRITTEN in. `Scope::lexical_home`'s rule over rustc's
/// `cref_chain`, which reads `defining_class`.
///
/// The owner is the last resort, not the first: a method materialized onto
/// a subclass or an includer keeps the cref it was written in, so `rescue
/// Boom` inside `M::Base#go` still names `M::Boom` when `Sub` runs it.
pub(crate) fn lexical_class(fx: &Fx) -> Option<crate::compiler::ClassId> {
    fx.lexical_home.or(fx.defining_class).or(fx.method_class)
}

/// Resolve a class name against the current cref and BOX (rustc's
/// `Ctx::resolve_class`).
/// A compile-time fold under the run-time-redefinition gate: the fold on one
/// arm, the ordinary dispatch on the other, joined through a temp slot the
/// way [`if_expr`] joins an `if`. One call to `zeo_rt_is_live` and a branch.
pub(super) fn guarded_fold(
    fx: &mut Fx,
    fast: impl FnOnce(&mut Fx) -> CResult<Operand>,
    slow: impl FnOnce(&mut Fx) -> CResult<Operand>,
) -> CResult<Operand> {
    let live = fx.call_status("zeo_rt_is_live", &[]);
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let b_fast = fx.b.create_block();
    let b_slow = fx.b.create_block();
    let join = fx.b.create_block();
    fx.b.ins().brif(live, b_slow, &[], b_fast, &[]);
    fx.b.switch_to_block(b_fast);
    let op = fast(fx)?;
    ownership::write_move_into(fx, &op, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(b_slow);
    let op = slow(fx)?;
    ownership::write_move_into(fx, &op, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A receiverless (or literal-`self`) call naming an accessor of THIS
/// body's own class, replaced by the ivar access itself: no dispatch, no
/// trampoline, no frame, with `self` as the statically-typed receiver.
///
/// `Compiler::accessor_shape` carries the gate that matters -- a
/// HAND-written accessor keeps its body wherever instrumentation can
/// observe the call (TracePoint, line coverage), while an `attr_*`
/// GENERATED one is iseq-less either way, exactly as CRuby compiles it --
/// and `ivar_read_op`/`ivar_write_op` carry the rest: a dynamic or class
/// `self`, a native-backed owner, and a name the layout has no slot for
/// all take the name-keyed path on their own.
///
/// A name a run-time definition can reach keeps the fold, under a guard:
/// the site reads `zeo_rt_is_live` and takes the ordinary dispatch once
/// anything has been defined at run time. Without it one object answers two
/// different values for one method depending on who asks -- a `define_method`
/// wins from outside the class and loses to the fold inside it.
///
/// The guard is emitted ONLY for a name
/// [`Compiler::may_be_patched_at_runtime`] answers for, so a program that
/// redefines nothing emits exactly what it emitted before.
///
/// The WRITER half takes no guard: both arms would lower the argument, and
/// the slow arm re-lowers it from the same node. It stands down instead,
/// which is correct by falling through to dispatch.
pub(super) fn inline_accessor(
    fx: &mut Fx,
    id: crate::hir::NodeId,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[crate::hir::KwArg],
    block: Option<crate::hir::NodeId>,
    block_arg: Option<crate::hir::NodeId>,
) -> Option<CResult<Operand>> {
    use crate::compiler::AccessorKind;
    if !kwargs.is_empty() || block.is_some() || block_arg.is_some() {
        return None;
    }
    if fx.self_is_dynamic || fx.self_is_class || fx.dyn_ivars {
        return None;
    }
    let cid = fx.method_class?;
    let (_owner, scope_id) = fx.an.compiler.method_in_chain(cid, name)?;
    let scope = fx.an.compiler.scope(scope_id);
    let shape = fx.an.compiler.accessor_shape(cid, scope)?;
    let ivar = shape.ivar.clone();
    let patchable = fx.an.compiler.may_be_patched_at_runtime(name);
    match (shape.kind, args) {
        (AccessorKind::Reader, []) if patchable => {
            let name = name.to_string();
            Some(guarded_fold(
                fx,
                move |fx| super::ivars::ivar_read_op(fx, &ivar),
                move |fx| super::call::implicit_send(fx, id, &name, &[]),
            ))
        }
        (AccessorKind::Writer, _) if patchable => None,
        (AccessorKind::Reader, []) => Some(super::ivars::ivar_read_op(fx, &ivar)),
        (AccessorKind::Writer, [ArrayElem::Single(arg)]) => {
            let arg = *arg;
            Some((|| {
                let op = super::expr::lower_expr(fx, arg)?;
                // `obj.x = v` answers `v`, so the write takes a copy and
                // the value is handed back.
                let p = ownership::borrow_ptr(fx, &op);
                let tag = op.tag();
                if op.owned() {
                    ownership::pool_owned(fx, p, tag);
                }
                let borrowed = || Operand::Ptr {
                    addr: p,
                    owned: false,
                    tag,
                };
                super::ivars::ivar_write_op(fx, &ivar, borrowed())?;
                Ok(borrowed())
            })())
        }
        // An argument count the accessor does not take must still raise
        // `ArgumentError`, which is the trampoline's job.
        _ => None,
    }
}

/// An explicit-receiver accessor site the analyzer nominated
/// (`Compiler::accessor_sites`): one call to the guarded runtime attr
/// entry, whose fast arm is a bare slot access and whose slow arm is
/// the full explicit send. Operands lower once, in ruby's order.
pub(super) fn explicit_accessor(
    fx: &mut Fx,
    id: crate::hir::NodeId,
    recv: crate::hir::NodeId,
    name: &str,
    args: &[ArrayElem],
    site: crate::compiler::AccessorSite,
) -> CResult<Operand> {
    use cranelift_codegen::ir::types;
    let bypass = super::expr::bypasses_visibility(fx, Some(recv));
    let later = super::expr::later_nodes(args, &[], None);
    let recv_op = super::expr::lower_expr(fx, recv)?;
    let recv_op = super::expr::park_reassignable(fx, Some(recv), recv_op, &later);
    let recv_ptr = ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    let vptr = if site.writer {
        let [ArrayElem::Single(arg)] = args else {
            unreachable!("nomination admits one plain argument");
        };
        let op = super::expr::lower_expr(fx, *arg)?;
        let p = ownership::borrow_ptr(fx, &op);
        if op.owned() {
            ownership::pool_owned(fx, p, op.tag());
        }
        Some(p)
    } else {
        None
    };
    super::stmt::stamp_call_line(fx, id);
    let sym = fx.sym_id(name);
    let cid_v = fx.b.ins().iconst(types::I32, i64::from(site.cid.0));
    let slot_v = fx.b.ins().iconst(fx.em.ptr, i64::from(site.slot));
    let caller = super::call::caller_class(fx, bypass);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = match vptr {
        Some(v) => fx.call_status(
            "zeo_rt_attr_write",
            &[recv_ptr, cid_v, slot_v, sym, caller, v, out],
        ),
        None => fx.call_status(
            "zeo_rt_attr_read",
            &[recv_ptr, cid_v, slot_v, sym, caller, out],
        ),
    };
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

pub(crate) fn resolve_class_here(fx: &Fx, name: &str) -> Option<crate::compiler::ClassId> {
    // A snippet under a run-time cref may see a constant that shadows the
    // one this (fresh) compiler would fold to, and the compiler cannot
    // know: the class was minted by a compile that is already over. So the
    // fold stands down and every name takes the run-time walk.
    if fx.eval_cref.is_some() {
        return None;
    }
    fx.an
        .compiler
        .resolve_class(name, cref_chain(fx), fx.box_id)
}

/// `Ruby::Box.current` -- the box the SITE runs in.
///
/// No method row could answer this: a `MethodFn` takes no box, so a
/// builtin row cannot see its caller's. The emitter can, because
/// `Ctx.box_id` is baked at every site including a snippet's, so the
/// literal spelling folds here and the row is left for a computed send.
pub(super) fn box_current(
    fx: &mut Fx,
    receiver: Option<crate::hir::NodeId>,
    name: &str,
    args: &[ArrayElem],
) -> CResult<Option<Operand>> {
    if name != "current" || !args.is_empty() {
        return Ok(None);
    }
    let Some(r) = receiver else { return Ok(None) };
    // A whole-program compile folds `Ruby::Box` to a `ClassRef`; a SNIPPET
    // folds nothing, so the same source arrives as an unresolved constant
    // path -- and a snippet is exactly where this fold matters most.
    // A whole-program compile folds `Ruby::Box` to a `ClassRef`; a SNIPPET
    // folds nothing, so the same source arrives as a scoped constant read
    // -- and a snippet is exactly where this fold matters most.
    let names =
        |scope: &str, leaf: &str| (scope.trim_start_matches("::").to_string(), leaf.to_string());
    let (scope, leaf) = match &fx.an.compiler.hir[r] {
        HirNode::ClassRef(n) => {
            let (s, l) = crate::hir::split_const_path(n.trim_start_matches("::"));
            names(s.unwrap_or(""), l)
        }
        HirNode::QualifiedConstRead(s, n) => names(s, n),
        HirNode::ConstReadOrNil(s, n) => names(s.as_deref().unwrap_or(""), n),
        _ => return Ok(None),
    };
    if !(scope == "Ruby" && leaf == "Box") {
        return Ok(None);
    }
    let bx = fx.box_v();
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status("zeo_rt_box_current", &[bx, out]);
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// `Module.nesting` -- the lexical class/module chain at THIS call site,
/// innermost first. It is compile-time knowledge and nothing else: a builtin
/// row runs with no view of its caller's lexical scope, so folding here is
/// the only way to answer anything but `[]` (rustc folds it the same way).
/// `cref_chain` is outermost-first.
pub(super) fn module_nesting(
    fx: &mut Fx,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
) -> CResult<Option<Operand>> {
    if name != "nesting" || !args.is_empty() {
        return Ok(None);
    }
    let Some(r) = receiver else { return Ok(None) };
    if !matches!(&fx.an.compiler.hir[r], HirNode::ClassRef(n) if n == "Module") {
        return Ok(None);
    }
    let chain: Vec<crate::compiler::ClassId> = cref_chain(fx).iter().rev().copied().collect();
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let cap = fx.b.ins().iconst(fx.em.ptr, chain.len() as i64);
    fx.call("zeo_rt_array_new", &[cap, out]);
    fx.owned_created += 1;
    for cid in chain {
        let op = super::consts::class_immediate(fx, cid);
        let p = ownership::move_ptr(fx, &op);
        fx.call("zeo_rt_array_push", &[out, p]);
    }
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(zeo_abi::abi::ValueTag::Array as u8),
    }))
}
