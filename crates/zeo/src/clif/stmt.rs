//! Statement lowering to Cranelift IR: assignments, control flow, loop
//! signals (`break`/`next`/`redo`), definitions, and expression
//! statements. An unsupported shape refuses loudly with its source location.

use super::ctx::{Fx, LoopCtl, VALUE_SIZE};
use super::expr::lower_expr;
use super::ownership;
use crate::hir::{ArrayElem, HirNode, NodeId};
use cranelift_codegen::ir::{InstBuilder, StackSlotData, StackSlotKind, types};
use cranelift_module::Module;

pub(crate) fn lower_stmts(fx: &mut Fx, stmts: &[NodeId]) -> Result<(), String> {
    let mut i = 0;
    while i < stmts.len() {
        // A `class << self` body's statements are SPLICED into the
        // enclosing class body, tagged with the marker they came from
        // (`Hir::singleton_frame_stmts`). Each contiguous run gets its own
        // `singleton class` backtrace frame, rustc's `emit_singleton_frame`.
        if let Some(&origin) = fx.an.compiler.hir.singleton_frame_stmts.get(&stmts[i]) {
            let mut j = i + 1;
            while j < stmts.len()
                && fx.an.compiler.hir.singleton_frame_stmts.get(&stmts[j]) == Some(&origin)
            {
                j += 1;
            }
            singleton_frame(fx, origin, &stmts[i..j], None)?;
            i = j;
            continue;
        }
        let mark = fx.stmt_mark();
        let pool = fx.drain_temps.then(|| {
            fx.call("zeo_rt_pool_mark", &[])
                .expect("pool_mark returns the watermark")
        });
        lower_stmt(fx, stmts[i])?;
        if let Some(pool) = pool {
            fx.call("zeo_rt_pool_reset", &[pool]);
        }
        fx.end_stmt(mark);
        i += 1;
    }
    Ok(())
}

/// One `class << self` group inside its own frame. Two positions have to
/// be right: the frame itself is labelled `singleton class` and tracks the
/// group's own lines, and the ENCLOSING frame is left reading the `class
/// << self` KEYWORD's line -- so the head stamps that line BEFORE the
/// push rather than letting the first grouped statement stamp it
/// underneath. `frame_label` carries down so a block written here is
/// `block in singleton class`.
fn singleton_frame(
    fx: &mut Fx,
    origin: NodeId,
    group: &[NodeId],
    dst: Option<cranelift_codegen::ir::Value>,
) -> Result<(), String> {
    let Some((file, line)) = fx.location(origin) else {
        return lower_group(fx, group, dst);
    };
    let (file, line) = (file.to_string(), line);
    stamp_line(fx, origin);
    let end_line = crate::analyze::source::source_end_line(&fx.an.compiler, origin);
    let label = "singleton class";
    let foff = fx.em.intern_rodata(file.as_bytes());
    let loff = fx.em.intern_rodata(label.as_bytes());
    let fptr = fx.rod(foff);
    let flen = fx.b.ins().iconst(fx.em.ptr, file.len() as i64);
    let lptr = fx.rod(loff);
    let llen = fx.b.ins().iconst(fx.em.ptr, label.len() as i64);
    let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
    let end_v = fx.b.ins().iconst(types::I32, i64::from(end_line));
    fx.call(
        "zeo_rt_frame_push",
        &[fptr, flen, lptr, llen, line_v, end_v],
    );
    // The error path pops this frame before the enclosing landing runs.
    let outer_land = fx.land;
    let pop_land = fx.b.create_block();
    fx.land = pop_land;
    let saved_label = std::mem::replace(&mut fx.frame_label, label.to_string());
    let saved_line = fx.prev_line.take();
    let r = lower_group(fx, group, dst);
    fx.frame_label = saved_label;
    fx.land = outer_land;
    fx.prev_line = saved_line;
    r?;
    fx.call("zeo_rt_frame_pop", &[]);
    let after = fx.b.create_block();
    fx.b.ins().jump(after, &[]);
    fx.b.switch_to_block(pop_land);
    fx.call("zeo_rt_frame_pop", &[]);
    fx.b.ins().jump(outer_land, &[]);
    fx.b.switch_to_block(after);
    Ok(())
}

/// [`lower_stmts`]' loop without the singleton grouping -- so a group can
/// reuse it without re-detecting itself. With a `dst` the group's last
/// statement is its VALUE.
fn lower_group(
    fx: &mut Fx,
    stmts: &[NodeId],
    dst: Option<cranelift_codegen::ir::Value>,
) -> Result<(), String> {
    let Some(dst) = dst else {
        for &stmt in stmts {
            let mark = fx.stmt_mark();
            lower_stmt(fx, stmt)?;
            fx.end_stmt(mark);
        }
        return Ok(());
    };
    let Some((&tail, init)) = stmts.split_last() else {
        ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
        return Ok(());
    };
    for &stmt in init {
        let mark = fx.stmt_mark();
        lower_stmt(fx, stmt)?;
        fx.end_stmt(mark);
    }
    stamp_line(fx, tail);
    let op = lower_tail_expr(fx, tail)?;
    ownership::write_move_into(fx, &op, dst);
    Ok(())
}

/// A body in VALUE position (a method body, an `if`-expression arm): the
/// leading statements run as statements, the tail's value MOVES into the
/// caller's `dst` slot. An empty body is nil.
pub(crate) fn lower_value_body_into(
    fx: &mut Fx,
    stmts: &[NodeId],
    dst: cranelift_codegen::ir::Value,
) -> Result<(), String> {
    let Some((&tail, init)) = stmts.split_last() else {
        ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
        return Ok(());
    };
    // A tail inside a `class << self` group supplies the body's value, so
    // its frame block is an expression rather than a statement.
    if let Some(&origin) = fx.an.compiler.hir.singleton_frame_stmts.get(&tail) {
        let mut start = stmts.len() - 1;
        while start > 0
            && fx
                .an
                .compiler
                .hir
                .singleton_frame_stmts
                .get(&stmts[start - 1])
                == Some(&origin)
        {
            start -= 1;
        }
        lower_stmts(fx, &stmts[..start])?;
        return singleton_frame(fx, origin, &stmts[start..], Some(dst));
    }
    lower_stmts(fx, init)?;
    stamp_line(fx, tail);
    let op = lower_tail_expr(fx, tail)?;
    ownership::write_move_into(fx, &op, dst);
    Ok(())
}

/// The receiver for a NAME-KEYED ivar access: `self`, or the `main`
/// object at the toplevel (rustc's `ivar_get_dyn(&main_object(), ..)`).
pub(crate) fn dyn_ivar_recv(fx: &mut Fx) -> cranelift_codegen::ir::Value {
    // Every scope's prologue seats its own `self`: `main` for the toplevel,
    // the receiver for a runtime-installed body, the class for a class
    // body, and the block fn's `self_` parameter for a block -- which is
    // what `instance_exec` REBINDS, so a re-homed block's `@x = 1` must
    // read it rather than assume the enclosing scope's.
    fx.self_ptr.expect("self_ptr is set in the prologue")
}

/// `@name` read into a fresh owned temp -- slot-indexed for a compiled
/// class; NAME-KEYED (fallible: the Ractor guard) for a native-backed one,
/// the rustc `ivar_get_dyn_isolated` shape.
pub(crate) fn ivar_read_op(fx: &mut Fx, name: &str) -> Result<super::operand::Operand, String> {
    use super::operand::{Operand, TagInfo};
    if fx.dyn_ivars || fx.self_is_dynamic || fx.self_is_class || fx.method_class.is_none() {
        // A `self_is_class` body's `@x` is a CLASS-level ivar; the runtime's
        // name-keyed path routes a `RubyValue::Class` receiver to `civars`,
        // so the same call serves both.
        //
        // A DYNAMIC self has no statically-known layout to take a slot
        // from at all: an `*_eval` or a `Ractor.new` block rebinds the
        // receiver, and the name-keyed entry carries CRuby's Ractor guard
        // with it (rustc asks `self_is_dynamic` first for the same reason).
        return name_keyed_ivar_read(fx, name);
    }
    let Some(slot) = ivar_slot_of(fx, name) else {
        return name_keyed_ivar_read(fx, name);
    };
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    {
        let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
        fx.call("zeo_rt_ivar_get_slot", &[self_ptr, slot_v, out]);
    }
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// `@name` through the runtime's NAME-keyed path: a native-backed or
/// class receiver, and any ivar the compiled layout has no slot for.
fn name_keyed_ivar_read(fx: &mut Fx, name: &str) -> Result<super::operand::Operand, String> {
    let recv = dyn_ivar_recv(fx);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let (nptr, nlen) = super::expr::rodata_name(fx, name);
    let status = fx
        .call("zeo_rt_ivar_get_dyn", &[recv, nptr, nlen, out])
        .expect("ivar_get_dyn returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(super::operand::Operand::Slot {
        ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    })
}

/// `@name = <op>`: the slot write MOVES the value in; the name-keyed
/// write BORROWS it (the runtime clones), so an owned operand parks in
/// the pool first -- the write's frozen check can raise.
pub(crate) fn ivar_write_op(
    fx: &mut Fx,
    name: &str,
    op: super::operand::Operand,
) -> Result<(), String> {
    let slot = ivar_slot_of(fx, name);
    if slot.is_none()
        || fx.dyn_ivars
        || fx.self_is_dynamic
        || fx.self_is_class
        || fx.method_class.is_none()
    {
        let recv = dyn_ivar_recv(fx);
        let tag = op.tag();
        let ptr = ownership::borrow_ptr(fx, &op);
        if op.owned() {
            ownership::pool_owned(fx, ptr, tag);
        }
        let (nptr, nlen) = super::expr::rodata_name(fx, name);
        let status = fx
            .call("zeo_rt_ivar_set_dyn", &[recv, nptr, nlen, ptr])
            .expect("ivar_set_dyn returns a status");
        fx.fallible(status);
        return Ok(());
    }
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    {
        let slot = slot.expect("the slotless case took the name-keyed path");
        let ptr = ownership::move_ptr(fx, &op);
        let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
        let status = fx
            .call("zeo_rt_ivar_set_slot", &[self_ptr, slot_v, ptr])
            .expect("ivar_set_slot returns a status");
        fx.fallible(status);
    }
    Ok(())
}

/// `@name = value`: evaluate then write (see [`ivar_write_op`]).
fn lower_ivar_write(fx: &mut Fx, name: &str, value: NodeId) -> Result<(), String> {
    let op = lower_expr(fx, value)?;
    ivar_write_op(fx, name, op)
}

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
    let status = fx
        .call("zeo_rt_multi_split", &[value_ptr, nb, hs, na, slots_ptr])
        .expect("multi_split returns a status");
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
fn write_multi_target(
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
        MultiTarget::Ivar(name) => ivar_write_op(fx, name, op),
        MultiTarget::Call {
            write_call,
            tmp_name,
        } => {
            // Bind the write's synthetic hidden local, then run the write
            // call itself through the ordinary expression path.
            ownership::write_local(fx, tmp_name, &op);
            let r = lower_expr(fx, *write_call)?;
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
            let owner = super::expr::cvar_owner(fx, name);
            let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner));
            let (nptr, nlen) = name_pair(fx, name);
            let status = fx
                .call("zeo_rt_cvar_set", &[owner_v, nptr, nlen, addr])
                .expect("cvar_set returns a status");
            fx.fallible(status);
            fx.call("zeo_rt_release", &[addr]);
            fx.owned_consumed += 1;
            Ok(())
        }
        MultiTarget::Global(name) => {
            let bx = fx.box_v();
            let (nptr, nlen) = name_pair(fx, name);
            let status = fx
                .call("zeo_rt_gvar_assign", &[bx, nptr, nlen, addr])
                .expect("gvar_assign returns a status");
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
fn const_multi_write(
    fx: &mut Fx,
    site: NodeId,
    scope: Option<&str>,
    name: &str,
    addr: cranelift_codegen::ir::Value,
) -> Result<(), String> {
    let owner_class = match scope {
        Some(s) => match super::expr::resolve_class_here(fx, s) {
            Some(cid) => cid,
            None => return fx.unsupported(site, "a constant multi-assignment on a runtime scope"),
        },
        None => fx.method_class.unwrap_or_else(|| super::expr::box_top(fx)),
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
    let (nptr, nlen) = name_pair(fx, name);
    let (fptr, flen) = name_pair(fx, &file);
    let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
    fx.call(
        "zeo_rt_const_set_at",
        &[owner_v, nptr, nlen, addr, fptr, flen, line_v],
    );
    fx.call("zeo_rt_release", &[addr]);
    fx.owned_consumed += 1;
    Ok(())
}

/// The compiled slot `@name` occupies on this body's class, or `None` for
/// an ivar the class's list does not carry -- one a body only ever reads,
/// or one written on a receiver of another class. rustc keeps those in the
/// same object's name-keyed overflow (`set_named`/`get_named`); the CLIF
/// twin is the name-keyed capi, which reaches the identical storage.
fn ivar_slot_of(fx: &Fx, name: &str) -> Option<usize> {
    let class = fx.method_class?;
    // Only a COMPILED class carries a slot layout at run time. The
    // toplevel's own methods land on Object, whose instance is the runtime
    // `main` object -- analyze still lists the toplevel's ivars there, and
    // indexing them would read a layout no receiver has.
    let info = fx.an.compiler.class(class);
    if class == crate::compiler::OBJECT_CLASS || info.is_builtin || info.is_bootstrap {
        return None;
    }
    crate::analyze::class_query::slot_of(&fx.an.compiler, class, name)
}

/// A tail position accepts a few statement-shaped nodes whose value Ruby
/// defines: an assignment answers the assigned value, a loop answers nil.
fn lower_tail_expr(fx: &mut Fx, tail: NodeId) -> Result<super::operand::Operand, String> {
    use super::operand::{Operand, TagInfo};
    match &fx.an.compiler.hir[tail] {
        // A `def` answers its method-name Symbol; the install itself is the
        // ordinary expression lowering.
        HirNode::DefMethod { .. } => super::expr::lower_expr(fx, tail),
        HirNode::LocalWrite(name, _) => {
            let name = name.clone();
            lower_stmt(fx, tail)?;
            Ok(ownership::read_local(fx, &name).expect("just assigned"))
        }
        HirNode::While { .. } | HirNode::For { .. } | HirNode::Loop { .. } => loop_value(fx, tail),
        HirNode::Break(..)
        | HirNode::Next(..)
        | HirNode::Redo
        | HirNode::Retry
        | HirNode::Raise(..)
        | HirNode::Return(..) => {
            // The jump/signal leaves this block unreachable; the nil is
            // never read.
            lower_stmt(fx, tail)?;
            Ok(Operand::Nil)
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            let (body, rescues, else_body, ensure_body) = (
                body.clone(),
                rescues.clone(),
                else_body.clone(),
                ensure_body.clone(),
            );
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            super::control::lower_begin(
                fx,
                tail,
                &body,
                &rescues,
                else_body.as_deref(),
                ensure_body.as_deref(),
                Some(dst),
            )?;
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::If { .. }
        | HirNode::IntegerLit(..)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
        | HirNode::ImaginaryLit(..)
        | HirNode::FloatLit(..)
        | HirNode::BoolLit(..)
        | HirNode::NilLit
        | HirNode::StringLit(..)
        | HirNode::LocalRead(..)
        | HirNode::IvarRead(..)
        | HirNode::IvarWrite(..)
        | HirNode::Or(..)
        | HirNode::And(..)
        | HirNode::ClassRef(..)
        | HirNode::New { .. }
        | HirNode::SelfRef
        | HirNode::Yield(..)
        | HirNode::BlockGiven
        | HirNode::MultiWrite { .. }
        | HirNode::Lambda { .. }
        | HirNode::ArrayLit(..)
        | HirNode::HashLit(..)
        | HirNode::SymbolLit(..)
        | HirNode::Seq(..)
        | HirNode::CaseWhen { .. }
        | HirNode::CaseIn { .. }
        | HirNode::MatchPredicate { .. }
        | HirNode::MatchRequired { .. }
        | HirNode::RangeLit { .. }
        | HirNode::GlobalRead(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ClassVarRead(..)
        | HirNode::ClassVarWrite(..)
        | HirNode::ConstWrite { .. }
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::DynConstRead { .. }
        | HirNode::DynConstWrite { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::FeatureLoaded { .. }
        | HirNode::CExtLoaded { .. }
        | HirNode::RegexpLit(..)
        | HirNode::Ffi(..)
        | HirNode::BoxScope { .. }
        | HirNode::BoxHandle(_)
        | HirNode::LastMatchRef(..)
        | HirNode::SuperCall { .. }
        | HirNode::Defined(..)
        | HirNode::Call { .. } => lower_expr(fx, tail),
        // A `class`/`module` in TAIL position: the site runs and answers
        // its body's last value; a body ending in a definition-level
        // construct answers nil, since nothing necessarily reads it.
        HirNode::ClassDef { .. } if fx.eval_mode.is_some() => eval_class_def(fx, tail),
        HirNode::ClassDef { .. } => class_body_value(fx, tail, false),
        // A snippet's definition-level statements run and answer what ruby
        // answers: the KEYWORD forms (`alias`, `undef`, `private_constant`)
        // are nil, and the CALL forms answer what the send does.
        HirNode::AliasMethod { .. }
        | HirNode::Undef(..)
        | HirNode::ClassMethodUndef(..)
        | HirNode::ConstantVisibility { .. }
            if fx.eval_mode.is_some() =>
        {
            lower_stmt(fx, tail)?;
            Ok(Operand::Nil)
        }
        // `using M` answers the receiver it activated on -- `main` at a
        // snippet's own level, the class under `class_eval`.
        HirNode::Using(..) if fx.eval_mode.is_some() => {
            lower_stmt(fx, tail)?;
            let addr = fx.self_ptr.expect("self_ptr is set in the prologue");
            Ok(Operand::Ptr {
                addr,
                owned: false,
                tag: super::operand::TagInfo::Unknown,
            })
        }
        HirNode::MethodVisibility { name, visibility } if fx.eval_mode.is_some() => {
            let (name, visibility) = (name.clone(), *visibility);
            let verb = match visibility {
                crate::hir::Visibility::Private => "private",
                crate::hir::Visibility::Protected => "protected",
                crate::hir::Visibility::Public => "public",
            };
            eval_definee_send_value(fx, verb, std::slice::from_ref(&name), false)
        }
        HirNode::ClassMethodVisibility { name, visibility } if fx.eval_mode.is_some() => {
            let (name, visibility) = (name.clone(), *visibility);
            let verb = match visibility {
                crate::hir::Visibility::Private => "private_class_method",
                _ => "public_class_method",
            };
            eval_definee_send_value(fx, verb, std::slice::from_ref(&name), false)
        }
        HirNode::ModuleFunction(name) if fx.eval_mode.is_some() => {
            let name = name.clone();
            eval_definee_send_value(fx, "module_function", std::slice::from_ref(&name), false)
        }
        // `include M`/`extend`/`prepend` answer the receiver, which the
        // send already does -- in a snippet they ARE a send (nothing was
        // edited into an ancestry at compile time).
        HirNode::Include(module) | HirNode::Extend(module) | HirNode::Prepend(module)
            if fx.eval_mode.is_some() =>
        {
            let module = module.clone();
            let verb = mixin_verb(&fx.an.compiler.hir[tail]);
            eval_mixin_send_value(fx, tail, &module, verb)
        }
        other => {
            let what = format!("this tail expression ({})", statement_kind(other));
            fx.unsupported(tail, &what)
        }
    }
}

pub(crate) fn lower_stmt(fx: &mut Fx, stmt: NodeId) -> Result<(), String> {
    stamp_line(fx, stmt);
    // A call the SITE decides (a refinement-covered name, `Ractor.new`)
    // lowers as an expression whose value is discarded: the arms below
    // include shapes -- a literal block, an inline iterator -- that would
    // otherwise take a dispatch path the site is entitled to override.
    if super::expr::site_decided_call(fx, stmt) {
        let op = lower_expr(fx, stmt)?;
        ownership::discard(fx, op);
        return Ok(());
    }
    match &fx.an.compiler.hir[stmt] {
        HirNode::LocalWrite(name, value) => {
            let name = name.clone();
            let value = *value;
            let op = lower_expr(fx, value)?;
            ownership::write_local(fx, &name, &op);
            Ok(())
        }
        HirNode::MultiWrite { targets, value } => {
            let (targets, value) = (targets.clone(), *value);
            let op = lower_expr(fx, value)?;
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, op.tag());
            }
            lower_multi_group(fx, stmt, &targets, ptr)
        }
        HirNode::Call {
            receiver: None,
            name,
            args,
            kwargs,
            block: None,
            block_arg: None,
            safe: false,
        } if name == "puts"
            && kwargs.is_empty()
            && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) =>
        {
            let args = args.clone();
            lower_puts(fx, stmt, &args)
        }
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
            // A build-time guard (`if defined?(Gone)`, a version gate) decides
            // WHICH BRANCH IS EMITTED, so the other one is never lowered. That
            // matters beyond code size: the unreached branch is written for a
            // ruby zeo is not, and may name constructs this emitter would
            // refuse.
            if let Some(taken) = static_cond(fx, cond) {
                // The fold decides the BRANCH; it must not eat the
                // condition's own effect. A build gate written `=~` still
                // writes `$~`, and the line after it may read one. Only the
                // match is re-run: the fold succeeded, so both of its
                // operands are static and nothing else in it can raise.
                if let Some(m) = folded_match(fx, cond) {
                    let v = lower_expr(fx, m)?;
                    ownership::discard(fx, v);
                }
                return lower_stmts(fx, if taken { &then_body } else { &else_body });
            }
            let c = lower_expr(fx, cond)?;
            let t = ownership::truthy(fx, c);
            let b_then = fx.b.create_block();
            let b_else = fx.b.create_block();
            let join = fx.b.create_block();
            fx.b.ins().brif(t, b_then, &[], b_else, &[]);
            fx.b.switch_to_block(b_then);
            lower_stmts(fx, &then_body)?;
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(b_else);
            lower_stmts(fx, &else_body)?;
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(join);
            Ok(())
        }
        HirNode::While {
            cond,
            body,
            negate,
            post,
        } => {
            let (cond, body, negate, post) = (*cond, body.clone(), *negate, *post);
            lower_loop(fx, Some((cond, negate)), &body, post, None)
        }
        HirNode::Loop { body } => {
            let body = body.clone();
            lower_loop(fx, None, &body, false, None)
        }
        HirNode::For {
            target,
            iterable,
            body,
        } => {
            let (target, iterable, body) = (target.clone(), *iterable, body.clone());
            lower_for(fx, stmt, &target, iterable, &body, None)
        }
        HirNode::Break(value) => {
            let value = *value;
            if fx.loops.is_empty() {
                // A `break` at the top of a `define_method` body RETURNS
                // from the method: the block became the method, so there is
                // no yielding call left to break out of, and CRuby's rule
                // for a block-turned-method is the lambda one.
                if fx.define_method_body
                    && let Some((out, ret_ok)) = fx.ret.or(fx.block_next)
                {
                    if fx.ensure_depth != 0 {
                        return signal_jump(fx, value, zeo_abi::abi::SignalKind::Next);
                    }
                    let op = match value {
                        Some(v) => lower_expr(fx, v)?,
                        None => super::operand::Operand::Nil,
                    };
                    ownership::write_move_into(fx, &op, out);
                    fx.pop_handling_to(0);
                    fx.b.ins().jump(ret_ok, &[]);
                    fx.continue_unreachable();
                    return Ok(());
                }
                // In an escaping block, `break` arms the Break signal the
                // send-site's catch_break receives.
                if fx.block_next.is_some() {
                    let op = match value {
                        Some(v) => lower_expr(fx, v)?,
                        None => super::operand::Operand::Nil,
                    };
                    let ptr = ownership::move_ptr(fx, &op);
                    let kind =
                        fx.b.ins()
                            .iconst(types::I8, i64::from(zeo_abi::abi::SignalKind::Break as u8));
                    // The landing chain pops what it pushed -- popping `$!`
                    // here as well would drop an ENCLOSING clause's entry.
                    fx.call("zeo_rt_signal_set", &[kind, ptr]);
                    let land = fx.land;
                    fx.b.ins().jump(land, &[]);
                    fx.continue_unreachable();
                    return Ok(());
                }
                return fx.unsupported(stmt, "`break` outside a loop");
            }
            if fx.loops.last().expect("checked above").depth != fx.ensure_depth {
                return signal_jump(fx, value, zeo_abi::abi::SignalKind::Break);
            }
            let result = fx.loops.last().expect("checked above").result;
            match (value, result) {
                // `break v` in a value-position loop: v IS the loop's value.
                (Some(v), Some(dst)) => {
                    let op = lower_expr(fx, v)?;
                    ownership::write_move_into(fx, &op, dst);
                }
                (Some(v), None) => {
                    let op = lower_expr(fx, v)?;
                    ownership::discard(fx, op);
                }
                (None, Some(dst)) => {
                    ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
                }
                (None, None) => {}
            }
            let ctl = fx.loops.last().expect("checked above");
            let (exit, handling) = (ctl.exit, ctl.handling);
            fx.pop_handling_to(handling);
            fx.b.ins().jump(exit, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Next(value) => {
            let value = *value;
            if fx.loops.is_empty()
                && let Some((out, ret_ok)) = fx.block_next
            {
                if fx.ensure_depth != 0 {
                    return signal_jump(fx, value, zeo_abi::abi::SignalKind::Next);
                }
                // In an escaping block, `next v` IS the block's return.
                match value {
                    Some(v) => {
                        let op = lower_expr(fx, v)?;
                        ownership::write_move_into(fx, &op, out);
                    }
                    None => {
                        ownership::write_move_into(fx, &super::operand::Operand::Nil, out);
                    }
                }
                fx.pop_handling_to(0);
                fx.b.ins().jump(ret_ok, &[]);
                fx.continue_unreachable();
                return Ok(());
            }
            let Some(ctl) = fx.loops.last() else {
                return fx.unsupported(stmt, "`next` outside a loop");
            };
            if ctl.depth != fx.ensure_depth {
                return signal_jump(fx, value, zeo_abi::abi::SignalKind::Next);
            }
            if let Some(v) = value {
                let op = lower_expr(fx, v)?;
                ownership::discard(fx, op);
            }
            let ctl = fx.loops.last().expect("checked above");
            let (latch, handling) = (ctl.latch, ctl.handling);
            fx.pop_handling_to(handling);
            fx.b.ins().jump(latch, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Redo => {
            let Some(ctl) = fx.loops.last() else {
                // `redo` in a block re-runs the block from its binding head
                // (the bindings re-run too -- they sit inside the loop).
                let Some(head) = fx.block_redo else {
                    return fx.unsupported(stmt, "`redo` outside a loop");
                };
                if fx.ensure_depth != 0 {
                    return signal_jump(fx, None, zeo_abi::abi::SignalKind::Redo);
                }
                fx.pop_handling_to(0);
                fx.b.ins().jump(head, &[]);
                fx.continue_unreachable();
                return Ok(());
            };
            if ctl.depth != fx.ensure_depth {
                return signal_jump(fx, None, zeo_abi::abi::SignalKind::Redo);
            }
            let ctl = fx.loops.last().expect("checked above");
            let (body, handling) = (ctl.body, ctl.handling);
            fx.pop_handling_to(handling);
            fx.b.ins().jump(body, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::IvarWrite(name, value) => {
            let (name, value) = (name.clone(), *value);
            lower_ivar_write(fx, &name, value)
        }
        HirNode::Return(value) => {
            let value = *value;
            let Some((out, ret_ok)) = fx.ret else {
                // In an escaping block, `return` arms the Return signal;
                // the runtime resolves it against the proc's captured home
                // (dead home -> LocalJumpError) and the defining method's
                // boundary folds a targeted one.
                // ...and at the TOP LEVEL, where a `return` ends the program
                // silently and successfully: the same signal, settled by
                // `zeo_rt_main` rather than by any method boundary. (Ruby
                // rejects a `return` in a class body at parse time, so this
                // is the only other body without one.)
                let op = match value {
                    Some(v) => lower_expr(fx, v)?,
                    None => super::operand::Operand::Nil,
                };
                let ptr = ownership::move_ptr(fx, &op);
                let kind =
                    fx.b.ins()
                        .iconst(types::I8, i64::from(zeo_abi::abi::SignalKind::Return as u8));
                fx.call("zeo_rt_signal_set", &[kind, ptr]);
                let land = fx.land;
                fx.b.ins().jump(land, &[]);
                fx.continue_unreachable();
                return Ok(());
            };
            // The returned expression runs BEFORE `$!` unwinds -- inside a
            // rescue clause `return $!.message` reads the rescued one.
            let op = match value {
                Some(v) => lower_expr(fx, v)?,
                None => super::operand::Operand::Nil,
            };
            if fx.ensure_depth != 0 {
                // An `ensure` sits between here and the method's exit, so
                // the return travels as a signal: the bracket saves and
                // restores it around the ensure body, and this method's own
                // boundary folds it (an unmarked `Return` belongs to the
                // nearest catcher). The landing chain pops `$!` on the way.
                let ptr = ownership::move_ptr(fx, &op);
                let kind =
                    fx.b.ins()
                        .iconst(types::I8, i64::from(zeo_abi::abi::SignalKind::Return as u8));
                fx.call("zeo_rt_signal_set", &[kind, ptr]);
                let land = fx.land;
                fx.b.ins().jump(land, &[]);
                fx.continue_unreachable();
                return Ok(());
            }
            fx.pop_handling_to(0);
            ownership::write_move_into(fx, &op, out);
            fx.b.ins().jump(ret_ok, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            let (body, rescues, else_body, ensure_body) = (
                body.clone(),
                rescues.clone(),
                else_body.clone(),
                ensure_body.clone(),
            );
            super::control::lower_begin(
                fx,
                stmt,
                &body,
                &rescues,
                else_body.as_deref(),
                ensure_body.as_deref(),
                None,
            )
        }
        HirNode::Raise(args, cause) => {
            // `raise ..., cause: c`: the cause is explicit, so the automatic
            // `$!` chaining the row does is skipped and the runtime builds,
            // causes and stamps the exception in one entry.
            if let crate::hir::RaiseCause::Explicit(node) = cause {
                let (args, node) = (args.clone(), *node);
                let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
                let argv = super::call::build_argv(fx, stmt, &elems)?;
                let cause_op = super::expr::lower_expr(fx, node)?;
                let cause_ptr = ownership::borrow_ptr(fx, &cause_op);
                if cause_op.owned() {
                    ownership::pool_owned(fx, cause_ptr, cause_op.tag());
                }
                let argc = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
                let status = fx
                    .call("zeo_rt_raise_with_explicit_cause", &[argv, argc, cause_ptr])
                    .expect("raise_with_explicit_cause returns a status");
                fx.fallible(status);
                return Ok(());
            }
            let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
            // `raise` IS `Kernel#raise` -- the builtin row constructs,
            // stamps, and signals; the Ok arm is unreachable.
            let op = super::call::implicit_send(fx, stmt, "raise", &elems)?;
            ownership::discard(fx, op);
            Ok(())
        }
        HirNode::Retry => {
            let Some(&(target, depth, handling)) = fx.retries.last() else {
                return fx.unsupported(stmt, "`retry` outside a rescue clause");
            };
            if depth != fx.ensure_depth {
                return fx.unsupported(stmt, "a `retry` across an `ensure` boundary");
            }
            fx.pop_handling_to(handling);
            fx.b.ins().jump(target, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::ClassDef { .. } if fx.eval_mode.is_some() => {
            let op = eval_class_def(fx, stmt)?;
            ownership::discard(fx, op);
            Ok(())
        }
        HirNode::ClassDef { .. } => {
            // Registration happened at startup; the marker runs the body
            // site (statements + the declaration's const-location record).
            // A miss is a hoisted or statement-free site: nothing to run.
            let call = fx.em.class_bodies.get(&stmt).cloned();
            match call {
                Some(call) => emit_class_body_call(fx, &call),
                None => Ok(()),
            }
        }
        // A mixin's ANCESTRY edit happened at compile time (analyze); what
        // remains where it was written is the module's hook send --
        // `M.included(C)` and siblings -- which is Module's own no-op
        // unless the module defines one.
        // `alias new old` is pure REGISTRATION: analyze resolved it into a
        // copy scope (user source) or an alias row (builtin source), and a
        // builtin row's source is validated at this body's END
        // (`validate_class_aliases`) -- the statement itself runs nothing.
        // In a SNIPPET nothing was registered, so the keyword is the send
        // ruby writes on the run-time definee.
        HirNode::AliasMethod {
            new_name,
            old_name,
            is_class_method,
        } if fx.eval_mode.is_some() => {
            let names = vec![new_name.clone(), old_name.clone()];
            let singleton = *is_class_method;
            eval_definee_send(fx, "alias_method", &names, singleton)
        }
        HirNode::AliasMethod { .. } => Ok(()),
        // A `def` reached HERE is one analyze did not register statically
        // (written inside a method body or a block): a RUNTIME install,
        // whose Symbol value the statement position drops. The toplevel
        // and class-body positions filter their defs out before lowering.
        HirNode::DefMethod { .. } => {
            let op = super::expr::lower_expr(fx, stmt)?;
            ownership::discard(fx, op);
            Ok(())
        }
        // Visibility is POSITIONAL: a reopen's re-mark of a name the class
        // already owns, and a unit body's re-mark, are the ones analyze
        // leaves in the site's statements rather than folding into the
        // start-of-program override rows -- so they apply HERE.
        HirNode::MethodVisibility { name, visibility } => {
            let (name, visibility) = (name.clone(), *visibility);
            // In a snippet the class is only a run-time fact, and the verb
            // is the one ruby writes: `definee.private(:name)`.
            if fx.eval_mode.is_some() {
                let verb = match visibility {
                    crate::hir::Visibility::Private => "private",
                    crate::hir::Visibility::Protected => "protected",
                    crate::hir::Visibility::Public => "public",
                };
                return eval_definee_send(fx, verb, std::slice::from_ref(&name), false);
            }
            let verb = match visibility {
                crate::hir::Visibility::Private => 0,
                crate::hir::Visibility::Protected => 1,
                crate::hir::Visibility::Public => 2,
            };
            apply_visibility(fx, stmt, "zeo_rt_runtime_set_visibility", &name, Some(verb))
        }
        HirNode::ClassMethodVisibility { name, visibility } => {
            let (name, visibility) = (name.clone(), *visibility);
            if fx.eval_mode.is_some() {
                let verb = match visibility {
                    crate::hir::Visibility::Private => "private_class_method",
                    _ => "public_class_method",
                };
                return eval_definee_send(fx, verb, std::slice::from_ref(&name), false);
            }
            let private = u8::from(visibility == crate::hir::Visibility::Private);
            apply_visibility(
                fx,
                stmt,
                "zeo_rt_runtime_class_method_visibility",
                &name,
                Some(private),
            )
        }
        // An `undef` in a REOPENED `class << self`: a call written between
        // the two bodies still answers, so the retirement has a position.
        HirNode::ClassMethodUndef(names) => {
            let names = names.clone();
            if fx.eval_mode.is_some() {
                return eval_definee_send(fx, "undef_method", &names, true);
            }
            for name in names {
                apply_visibility(fx, stmt, "zeo_rt_runtime_undef_class_method", &name, None)?;
            }
            Ok(())
        }
        // `undef` and `module_function` stay pure REGISTRATION: analyze
        // stamped the tables (undefined marks, module-function copies) and
        // the statements run nothing. In a SNIPPET analyze registered
        // nothing, so each is the send ruby writes on the run-time definee.
        HirNode::Undef(names) if fx.eval_mode.is_some() => {
            let names = names.clone();
            eval_definee_send(fx, "undef_method", &names, false)
        }
        HirNode::ModuleFunction(name) if fx.eval_mode.is_some() => {
            let name = name.clone();
            eval_definee_send(fx, "module_function", std::slice::from_ref(&name), false)
        }
        HirNode::ConstantVisibility { names, private } if fx.eval_mode.is_some() => {
            let (names, private) = (names.clone(), *private);
            let verb = if private {
                "private_constant"
            } else {
                "public_constant"
            };
            eval_definee_send(fx, verb, &names, false)
        }
        // `private_constant :A` / `public_constant :A` in a PROGRAM: a
        // registration for reflection, and a run-time flag a later
        // directive restores -- so it runs where it is WRITTEN rather than
        // at startup, or a read between the two sees the wrong answer.
        HirNode::ConstantVisibility { names, private } => {
            let (names, private) = (names.clone(), *private);
            let owner_v = fx.self_ptr.expect("self_ptr is set in the prologue");
            let private_v = fx.b.ins().iconst(types::I8, i64::from(u8::from(private)));
            for name in &names {
                let (nptr, nlen) = super::expr::rodata_name(fx, name);
                fx.call("zeo_rt_const_visibility", &[owner_v, nptr, nlen, private_v]);
            }
            Ok(())
        }
        HirNode::Undef(..) | HirNode::ModuleFunction(..) => Ok(()),
        // A `refine` marker analyze already CONSUMED runs nothing where it
        // was written: the holder module owns the methods and the
        // `Refinement` row says what they refine. It reaches statement
        // position only when the class-body walk kept the enclosing
        // statement whole -- power_assert (in the rspec stack) writes its
        // refinements under a runtime `if`, which hit this refusal. An
        // UNregistered marker is a different thing and still refuses,
        // rather than losing the refinement silently.
        HirNode::Refine { .. } if fx.an.compiler.refinement_marker_registered(stmt) => Ok(()),
        // `using M` in a SNIPPET: the module is a run-time constant, so
        // the site resolves it and fills its activation slot. Every call
        // site the `using` covers reads that slot -- which outlives the
        // call, so a `def` written after it here still sees the refinement.
        HirNode::Using(module) if fx.eval_mode.is_some() => {
            let module = module.clone();
            let Some(slot) = fx.an.compiler.eval_activation_slot(stmt) else {
                return Err("a `using` inside an `eval` that analyze did not place".to_string());
            };
            let op = super::expr::const_read(fx, stmt, &module)?;
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, op.tag());
            }
            let slot_v =
                fx.b.ins()
                    .iconst(types::I32, i64::from(fx.using_base + slot));
            let status = fx
                .call("zeo_rt_eval_using", &[ptr, slot_v])
                .expect("eval_using returns a status");
            fx.fallible(status);
            Ok(())
        }
        HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_) => {
            let Some((module, hook, primitive)) = mixin_parts(&fx.an.compiler.hir[stmt]) else {
                unreachable!("guarded by the four mixin arms")
            };
            let (module, hook, primitive) = (module.clone(), hook, primitive);
            // In an eval snippet NOTHING edited an ancestry at compile
            // time -- there is no class table to edit into -- so the mixin
            // is the ordinary send ruby writes: a receiverless call on
            // whatever `self` turns out to be (`main` at a snippet's own
            // level, the receiver under a `class_eval`). The hooks fire
            // from the runtime's own implementation of it.
            if fx.eval_mode.is_some() {
                return eval_mixin_send(fx, stmt, &module, mixin_verb(&fx.an.compiler.hir[stmt]));
            }
            // The PRIMITIVE first when the module overrides it -- it is what
            // performs the mixin, and analyze suppressed the static edit on
            // the strength of it. The notification follows either way,
            // exactly as ruby fires `included` even when an override skipped
            // the splice.
            let overrides = super::expr::resolve_class_here(fx, &module)
                .is_some_and(|mid| fx.an.compiler.overrides_mixin_primitive(mid, primitive));
            if overrides {
                mixin_hook_send(fx, &module, primitive)?;
            }
            mixin_hook_send(fx, &module, hook)
        }
        // A definition report -- `Klass.method_added(:name)` and its five
        // siblings -- spliced back in at the position of a `def` the analyze
        // walk consumed. An FCALL, not a visibility-checked call: ruby
        // reaches its hooks that way, so a `private def self.method_added`
        // still runs.
        // A redefinition applied at its document position: the install
        // replaces the overlay body, and the definition's own
        // `method_added` report is a separate `DefHook` right after it.
        HirNode::MethodRedefine {
            class,
            name,
            scope,
            singleton,
        } => {
            let (class, name, scope, singleton) = (*class, name.clone(), *scope, *singleton);
            let tramp = fx.em.redef_tramps[&(class, scope)];
            let f_ref = fx.em.module.declare_func_in_func(tramp, fx.b.func);
            let f_addr = fx.b.ins().func_addr(fx.em.ptr, f_ref);
            let cid = fx.b.ins().iconst(types::I32, i64::from(class));
            let (nptr, nlen) = super::expr::rodata_name(fx, &name);
            // The two channels are different overlay maps, and a class method
            // must not land on the instance one -- `C.t` would keep answering
            // the frozen row while `C.new.t` gained a method ruby never
            // defined.
            let entry = match singleton {
                true => "zeo_rt_runtime_replace_class_method",
                false => "zeo_rt_runtime_replace_method",
            };
            fx.call(entry, &[cid, nptr, nlen, f_addr]);
            // The body is only half of a `def`. Its reflection row -- arity,
            // parameters, source location -- installs at the same position,
            // or the window keeps answering for the last body written.
            let meta = fx.em.redef_metas[&(class, scope)];
            let idx = fx.b.ins().iconst(types::I32, i64::from(meta));
            fx.call("zeo_rt_install_meta_row", &[idx]);
            // A `def` carries a VISIBILITY as well as a body, and both belong
            // at the `def`'s own position. This mark is also what CLEARS an
            // earlier one: ruby's re-`def` resets a name to the class body's
            // running default, so a `private :v` written between two bodies
            // stops applying here.
            let vis = match fx
                .an
                .compiler
                .scope(crate::compiler::ScopeId(scope))
                .visibility
            {
                crate::hir::Visibility::Private => 0i64,
                crate::hir::Visibility::Protected => 1,
                crate::hir::Visibility::Public => 2,
            };
            let sym = fx.sym_id(&name);
            let verb = fx.b.ins().iconst(types::I8, vis);
            let chan = fx.b.ins().iconst(types::I8, i64::from(singleton));
            fx.call(
                "zeo_rt_install_positional_visibility",
                &[cid, sym, verb, chan],
            );
            Ok(())
        }
        HirNode::DefHook {
            class,
            hook,
            name,
            pending,
        } => {
            let (class, hook, name, pending) =
                (*class, hook.clone(), name.clone(), pending.clone());
            let recv = super::expr::class_immediate(fx, crate::compiler::ClassId(class));
            let recv_ptr = ownership::borrow_ptr(fx, &recv);
            let arg_ss = fx.temp_slot();
            let argv = fx.slot_addr(arg_ss, 0);
            let arg_sym = fx.sym_id(&name);
            fx.call("zeo_rt_sym_value", &[arg_sym, argv]);
            let sym = fx.sym_id(&hook);
            // The half-built view the hook body must see: the names still in
            // the FUTURE here are marked pending for as long as the send is
            // on the stack. Omitted when nothing is left, so the last
            // definition in a class pays nothing.
            if !pending.is_empty() {
                let (ptr, n) = sym_id_array(fx, &pending);
                let cid_v = fx.b.ins().iconst(types::I32, i64::from(class));
                fx.call("zeo_rt_pending_defs_begin", &[cid_v, ptr, n]);
            }
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            let zero_box = fx.box_v();
            let argc = fx.b.ins().iconst(fx.em.ptr, 1);
            let null = fx.b.ins().iconst(fx.em.ptr, 0);
            let status = fx
                .call(
                    "zeo_rt_send_value_in",
                    &[zero_box, recv_ptr, sym, argv, argc, null, out],
                )
                .expect("send returns a status");
            if pending.is_empty() {
                fx.fallible(status);
            } else {
                // The pop must run on the RAISE path too: pair it before the
                // landing jump.
                let ok = fx.b.create_block();
                let bad = fx.b.create_block();
                fx.b.ins().brif(status, bad, &[], ok, &[]);
                fx.b.switch_to_block(bad);
                fx.call("zeo_rt_pending_defs_end", &[]);
                fx.b.ins().jump(fx.land, &[]);
                fx.b.switch_to_block(ok);
                fx.call("zeo_rt_pending_defs_end", &[]);
            }
            fx.owned_created += 1;
            ownership::discard(
                fx,
                super::operand::Operand::Slot {
                    ss,
                    owned: true,
                    tag: super::operand::TagInfo::Unknown,
                },
            );
            Ok(())
        }
        HirNode::Seq(stmts) => {
            let stmts = stmts.clone();
            lower_stmts(fx, &stmts)
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block: Some(blk),
            block_arg: None,
            safe: false,
        } if kwargs.is_empty() => {
            let (receiver, name, args, blk) = (*receiver, name.clone(), args.clone(), *blk);
            // A typed-receiver `arr.each` fuses here too -- and this is
            // where most of them are written, since `each`'s value is
            // rarely wanted.
            if args.is_empty()
                && super::iter::fusable_block(fx, blk)
                && let Some(r) = receiver
                && fx.an.compiler.inline_iter_sites.get(&blk)
                    == Some(&crate::compiler::InlineIterKind::ArrayEach)
            {
                super::iter::lower_array_each(fx, stmt, r, blk, false)?;
                return Ok(());
            }
            if args.is_empty()
                && super::iter::fusable_block(fx, blk)
                && let Some(counted) = super::iter::counted_of(fx, receiver, &name, true)
            {
                return super::iter::lower_counted(fx, stmt, &counted, blk, None);
            }
            let op = if receiver.is_none()
                && let Some(decl) = fx.em.methods.get(&name)
                // `plain` as well as the count: a richer signature binds
                // through the trampoline, and its body takes one slot per
                // PARAMETER, not per argument. Without this the statement
                // arm built a call the direct entry could not accept
                // (minitest's `describe(desc, additional_desc = nil, &blk)`
                // written as a statement).
                && decl.plain
                && decl.arity == args.len()
                && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
                && !super::expr::method_class_shadows(fx, &name)
            {
                super::call::direct_call(fx, stmt, &name, &args, Some(blk))?
            } else {
                super::blocks::block_send(fx, stmt, receiver, &name, &args, blk)?
            };
            ownership::discard(fx, op);
            Ok(())
        }
        // Anything else in statement position: try the expression lowering
        // and discard the value (it refuses on its own for shapes outside
        // the slice).
        HirNode::IntegerLit(..)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
        | HirNode::ImaginaryLit(..)
        | HirNode::FloatLit(..)
        | HirNode::BoolLit(..)
        | HirNode::NilLit
        | HirNode::StringLit(..)
        | HirNode::LocalRead(..)
        | HirNode::IvarRead(..)
        | HirNode::Or(..)
        | HirNode::And(..)
        | HirNode::ClassRef(..)
        | HirNode::New { .. }
        | HirNode::SelfRef
        | HirNode::Yield(..)
        | HirNode::BlockGiven
        | HirNode::SymbolLit(..)
        | HirNode::ArrayLit(..)
        | HirNode::HashLit(..)
        | HirNode::Lambda { .. }
        | HirNode::CaseWhen { .. }
        | HirNode::CaseIn { .. }
        | HirNode::MatchPredicate { .. }
        | HirNode::MatchRequired { .. }
        | HirNode::RangeLit { .. }
        | HirNode::GlobalRead(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ClassVarRead(..)
        | HirNode::ClassVarWrite(..)
        | HirNode::ConstWrite { .. }
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::DynConstRead { .. }
        | HirNode::DynConstWrite { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::FeatureLoaded { .. }
        | HirNode::CExtLoaded { .. }
        | HirNode::RegexpLit(..)
        | HirNode::Ffi(..)
        | HirNode::BoxScope { .. }
        | HirNode::BoxHandle(_)
        | HirNode::LastMatchRef(..)
        | HirNode::SuperCall { .. }
        | HirNode::Defined(..)
        | HirNode::Call { .. } => {
            let op = lower_expr(fx, stmt)?;
            ownership::discard(fx, op);
            Ok(())
        }
        other => {
            let what = format!("this statement ({})", statement_kind(other));
            fx.unsupported(stmt, &what)
        }
    }
}

/// A stack array of runtime symbol ids -- the ids come from `zeo_syms`,
/// which `zeo_unit_init` fills, so they are loads rather than constants
/// and the array cannot live in rodata.
fn sym_id_array(
    fx: &mut Fx,
    names: &[String],
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    let ss =
        fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (names.len() * 4) as u32,
            2,
        ));
    let fl = cranelift_codegen::ir::MemFlagsData::trusted();
    for (i, name) in names.iter().enumerate() {
        let id = fx.sym_id(name);
        let at = fx.slot_addr(ss, (i * 4) as i32);
        fx.b.ins().store(fl, id, at, 0);
    }
    let base = fx.slot_addr(ss, 0);
    let n = fx.b.ins().iconst(fx.em.ptr, names.len() as i64);
    (base, n)
}

/// `(module name, notification hook, mix-in primitive)` for the four mixin
/// nodes -- `include` is `append_features` then `included`, and its
/// siblings follow the same shape (`eval.c`'s `rb_mod_include`). The
/// singleton form fires the same hooks with the SINGLETON class as their
/// argument, which zeo has no compile-time class for; analyze rejects a
/// module that defines either, so it is named only to stay true.
fn mixin_parts(node: &HirNode) -> Option<(&String, &'static str, &'static str)> {
    match node {
        HirNode::Include(m) => Some((m, "included", "append_features")),
        HirNode::Prepend(m) | HirNode::ClassMethodPrepend(m) => {
            Some((m, "prepended", "prepend_features"))
        }
        HirNode::Extend(m) => Some((m, "extended", "extend_object")),
        _ => None,
    }
}

/// `M.hook(Target)` -- the send a mixin leaves behind where it was
/// written, its ancestry edit having happened at compile time. Nothing is
/// emitted when nobody defines the hook (Module's own default is a no-op);
/// a PRIMITIVE reaches here only when the module overrides it, and it is
/// what performs the mixin at all.
/// The verb a mixin marker spells, for the run-time send an eval needs.
fn mixin_verb(node: &HirNode) -> &'static str {
    match node {
        HirNode::Include(_) => "include",
        HirNode::Prepend(_) | HirNode::ClassMethodPrepend(_) => "prepend",
        HirNode::Extend(_) => "extend",
        other => unreachable!("guarded by the four mixin arms; got {other:?}"),
    }
}

/// `include M` inside an `eval`: the receiverless send, with the module
/// read as an ordinary constant. Receiverless because ruby's own
/// `Module#include` is public but `main.include` is not -- the same
/// FCALL barrier a bare `include` at the top level passes.
fn eval_mixin_send(fx: &mut Fx, site: NodeId, module: &str, verb: &str) -> Result<(), String> {
    let op = eval_mixin_send_value(fx, site, module, verb)?;
    ownership::discard(fx, op);
    Ok(())
}

fn eval_mixin_send_value(
    fx: &mut Fx,
    site: NodeId,
    module: &str,
    verb: &str,
) -> Result<super::operand::Operand, String> {
    let arg = super::expr::const_read(fx, site, module)?;
    let tag = arg.tag();
    let argv = ownership::borrow_ptr(fx, &arg);
    if arg.owned() {
        ownership::pool_owned(fx, argv, tag);
    }
    super::call::implicit_send_ptr(fx, verb, argv, 1)
}

/// `class Foo < Bar ... end` / `module M ... end` written inside a run-time
/// `eval`.
///
/// The HEADER runs here: the owner is the snippet's own cref (or the scope
/// `A::B` names), the superclass is an ordinary constant read in THIS
/// scope, and the runtime reuses or mints the class.
///
/// The BODY runs as one more `class_eval` of its own source text. That is
/// not a shortcut -- a class body in CRuby is a separate iseq with its own
/// cref and its own locals, sharing nothing with the scope around it, and
/// this compiler's `class_eval` path already answers every question such a
/// body raises: where a `def` lands, what a bare constant resolves
/// against, which class owns `@@x`. Lowering the body HERE would need all
/// of that a second time, against a class id no compile can know.
pub(crate) fn eval_class_def(fx: &mut Fx, stmt: NodeId) -> Result<super::operand::Operand, String> {
    use super::operand::{Operand, TagInfo};
    let HirNode::ClassDef {
        name,
        superclass,
        body,
        is_module,
    } = &fx.an.compiler.hir[stmt]
    else {
        unreachable!("guarded by the ClassDef arm")
    };
    let (name, superclass, body, is_module) =
        (name.clone(), superclass.clone(), body.clone(), *is_module);
    if name == crate::compiler::SINGLETON_SURROGATE {
        return fx.unsupported(stmt, "a `class << self` inside an `eval`");
    }
    let (scope, leaf) = crate::hir::split_const_path(&name);

    // The owner of the bare name: the scope when one is written, else the
    // snippet's own cref -- and the top level when it has none.
    let owner = match scope.filter(|s| !s.is_empty()) {
        Some(s) => super::expr::const_path_read(fx, stmt, s)?,
        None => {
            // The snippet's own cref when it has one, else its TOP LEVEL --
            // which inside a box is the box's surrogate, not `Object`.
            // Binding on `Object` let main reach a class the box wrote.
            let cid = fx
                .eval_cref
                .as_ref()
                .and_then(|c| c.chain.first().copied())
                .map_or_else(|| super::expr::box_top(fx), crate::compiler::ClassId);
            super::expr::class_immediate(fx, cid)
        }
    };
    let owner_ptr = ownership::borrow_ptr(fx, &owner);
    if owner.owned() {
        ownership::pool_owned(fx, owner_ptr, owner.tag());
    }
    let super_ptr = match &superclass {
        Some(sup) => {
            let op = super::expr::const_path_read(fx, stmt, sup)?;
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            p
        }
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    let (nptr, nlen) = super::expr::rodata_name(fx, leaf);
    let is_module_v = fx.b.ins().iconst(types::I8, i64::from(u8::from(is_module)));
    let class_ss = fx.temp_slot();
    let class_ptr = fx.slot_addr(class_ss, 0);
    let status = fx
        .call(
            "zeo_rt_eval_class_open",
            &[owner_ptr, nptr, nlen, super_ptr, is_module_v, class_ptr],
        )
        .expect("eval_class_open returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    ownership::pool_owned(
        fx,
        class_ptr,
        TagInfo::Known(zeo_abi::abi::ValueTag::Class as u8),
    );

    let (src, file, line) = eval_body_source(fx, stmt, &body)?;
    let (sptr, slen) = super::expr::rodata_name(fx, &src);
    let (fptr, flen) = super::expr::rodata_name(fx, &file);
    let kind = if is_module { "module" } else { "class" };
    let (lptr, llen) = super::expr::rodata_name(fx, &format!("<{kind}:{leaf}>"));
    let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
    let bx = fx.box_v();
    // The chain THIS scope searches, which the body prepends its own class
    // to: `class Inside` written in `Wrap.class_eval` still sees
    // `Wrap::IN_WRAP`.
    let outer: Vec<u32> = fx
        .eval_cref
        .as_ref()
        .map(|c| c.chain.clone())
        .unwrap_or_default();
    let bytes: Vec<u8> = outer.iter().flat_map(|c| c.to_le_bytes()).collect();
    let outer_off = fx.em.intern_rodata_aligned(&bytes, 4);
    let outer_ptr = fx.rod(outer_off);
    let n_outer = fx.b.ins().iconst(fx.em.ptr, outer.len() as i64);
    let out_ss = fx.temp_slot();
    let out = fx.slot_addr(out_ss, 0);
    let status = fx
        .call(
            "zeo_rt_eval_class_body",
            &[
                class_ptr, sptr, slen, fptr, flen, line_v, lptr, llen, bx, outer_ptr, n_outer, out,
            ],
        )
        .expect("eval_class_body returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss: out_ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The SOURCE TEXT of a body written inside a snippet, sliced from the
/// statements' own spans -- plus the file and first line they report, so a
/// backtrace row raised inside it names the same place the snippet does.
fn eval_body_source(
    fx: &Fx,
    stmt: NodeId,
    body: &[NodeId],
) -> Result<(String, String, u32), String> {
    let hir = &fx.an.compiler.hir;
    let Some((file, line)) = crate::analyze::source::source_location(&fx.an.compiler, stmt) else {
        return Err("a span-less `class` inside an `eval`".to_string());
    };
    let (file, mut line) = (file.to_string(), line);
    let (Some(first), Some(last)) = (body.first(), body.last()) else {
        return Ok((String::new(), file, line));
    };
    let (Some(a), Some(b)) = (hir.span(*first), hir.span(*last)) else {
        return Err("a span-less statement in a `class` inside an `eval`".to_string());
    };
    // The body's TEXT is what runs -- a class body inside a snippet is one
    // more `class_eval` of its own source -- so the usual case is one slice
    // of the snippet between the first and last statement, which keeps every
    // line number exactly where the snippet put it.
    let own = hir
        .span(stmt)
        .and_then(|s| s.known())
        .ok_or_else(|| "a span-less `class` inside an `eval`".to_string())?;
    if a.file == own.file && a.start >= own.start && b.end <= own.end {
        let src = hir
            .files
            .get(a.file.0 as usize)
            .ok_or_else(|| "a `class` body from an unknown file".to_string())?;
        // To the class's own `end`, not to the last STATEMENT's end. A
        // `class << self` is not a statement -- `lower::defs` splices it into
        // the enclosing body as a surrogate reopen plus the retagged `def`s --
        // so the last statement stops short of the `end` that closes the
        // singleton body, and prism then reports an unterminated `class`.
        let closing = (own.end as usize)
            .checked_sub(3)
            .filter(|&at| src.source.get(at..at + 3) == Some("end"))
            .filter(|&at| at >= b.end as usize);
        let (start, end) = (
            a.start as usize,
            closing.unwrap_or(b.end as usize).min(src.source.len()),
        );
        if start > end {
            return Err("a `class` body whose statements run backwards".to_string());
        }
        line = src.line_at(a.start);
        return Ok((src.source[start..end].trim_end().to_string(), file, line));
    }
    // A body analyze REWROTE reaches here: some of its statements are
    // synthesized, and their spans point into a source of analyze's own
    // making rather than into the snippet. An `FFI::Struct` subclass is the
    // shape that does it -- its `layout` is replaced in place by the
    // accessors `lower::ffi::synthesize_ffi_struct` writes -- and that
    // synthesized text is real Ruby, so the body is still recoverable: slice
    // each RUN of same-file statements from its own file and join the runs.
    // What this cannot keep is the line numbering, since the runs come from
    // different files; the class reports its own line for the whole body,
    // which is where the compiled tier attributes a synthesized accessor
    // too.
    let mut parts: Vec<String> = Vec::new();
    let mut run: Option<(crate::hir::FileId, u32, u32)> = None;
    let flush = |run: Option<(crate::hir::FileId, u32, u32)>,
                 parts: &mut Vec<String>|
     -> Result<(), String> {
        let Some((f, start, end)) = run else {
            return Ok(());
        };
        let src = hir
            .files
            .get(f.0 as usize)
            .ok_or_else(|| "a `class` body from an unknown file".to_string())?;
        let (start, end) = (start as usize, (end as usize).min(src.source.len()));
        if start > end {
            return Err("a `class` body whose statements run backwards".to_string());
        }
        parts.push(src.source[start..end].to_string());
        Ok(())
    };
    for id in body {
        let span = hir
            .span(*id)
            .and_then(|s| s.known())
            .ok_or_else(|| "a span-less statement in a `class` inside an `eval`".to_string())?;
        run = match run {
            Some((f, start, end)) if f == span.file && span.start >= end => {
                Some((f, start, span.end))
            }
            other => {
                flush(other, &mut parts)?;
                Some((span.file, span.start, span.end))
            }
        };
    }
    flush(run, &mut parts)?;
    Ok((parts.join("\n"), file, line))
}

/// The class a definition-level statement written in a snippet names --
/// `alias`, `undef`, `private`, `module_function`, `private_constant`. Only
/// the run time knows it (`zeo_rt_eval_definee`), and one compiled snippet
/// may be evaluated against any number of receivers.
fn eval_definee(fx: &mut Fx, singleton: bool) -> Result<super::operand::Operand, String> {
    use super::operand::Operand;
    let mode = fx.eval_mode.expect("guarded by the eval arms");
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let mode_v = fx.b.ins().iconst(types::I8, i64::from(mode));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call("zeo_rt_eval_definee", &[mode_v, self_ptr, out])
        .expect("eval_definee returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    let definee = Operand::Slot {
        ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    };
    if !singleton {
        return Ok(definee);
    }
    let ptr = ownership::borrow_ptr(fx, &definee);
    ownership::pool_owned(fx, ptr, definee.tag());
    let empty = fx.b.ins().iconst(fx.em.ptr, 0);
    eval_definee_call(fx, ptr, "singleton_class", empty, 0)
}

/// `definee.<verb>(:a, :b, ...)` -- ruby writes these as private methods of
/// `Module`, so the send lowers the visibility barrier the way an implicit
/// receiver does.
fn eval_definee_send(
    fx: &mut Fx,
    verb: &str,
    names: &[String],
    singleton: bool,
) -> Result<(), String> {
    let op = eval_definee_send_value(fx, verb, names, singleton)?;
    ownership::discard(fx, op);
    Ok(())
}

pub(crate) fn eval_definee_send_value(
    fx: &mut Fx,
    verb: &str,
    names: &[String],
    singleton: bool,
) -> Result<super::operand::Operand, String> {
    let definee = eval_definee(fx, singleton)?;
    let recv = ownership::borrow_ptr(fx, &definee);
    if definee.owned() {
        ownership::pool_owned(fx, recv, definee.tag());
    }
    let argv =
        fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (names.len().max(1) * zeo_abi::abi::VALUE_SIZE) as u32,
            3,
        ));
    for (i, name) in names.iter().enumerate() {
        let sym = fx.sym_id(name);
        let slot = fx.slot_addr(argv, (i * zeo_abi::abi::VALUE_SIZE) as i32);
        fx.call("zeo_rt_sym_value", &[sym, slot]);
    }
    let a0 = fx.slot_addr(argv, 0);
    eval_definee_call(fx, recv, verb, a0, names.len())
}

fn eval_definee_call(
    fx: &mut Fx,
    recv: cranelift_codegen::ir::Value,
    verb: &str,
    argv: cranelift_codegen::ir::Value,
    argc: usize,
) -> Result<super::operand::Operand, String> {
    use super::operand::Operand;
    let sym = fx.sym_id(verb);
    let bx = fx.box_v();
    let argc_v = fx.b.ins().iconst(fx.em.ptr, argc as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_send_value_in",
            &[bx, recv, sym, argv, argc_v, null, out],
        )
        .expect("send returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    })
}

fn mixin_hook_send(fx: &mut Fx, module: &str, hook: &str) -> Result<(), String> {
    let (Some(mid), true) = (
        super::expr::resolve_class_here(fx, module),
        fx.self_is_class,
    ) else {
        return Ok(());
    };
    let Some(target) = fx.method_class else {
        return Ok(());
    };
    if fx.an.compiler.class_method_in_chain(mid, hook).is_none()
        && !fx.an.compiler.overrides_mixin_primitive(mid, hook)
    {
        return Ok(());
    }
    let recv = super::expr::class_immediate(fx, mid);
    let recv_ptr = ownership::borrow_ptr(fx, &recv);
    let arg = super::expr::class_immediate(fx, target);
    let argv = ownership::borrow_ptr(fx, &arg);
    let sym = fx.sym_id(hook);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let zero_box = fx.box_v();
    let argc = fx.b.ins().iconst(fx.em.ptr, 1);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let status = fx
        .call(
            "zeo_rt_send_value_in",
            &[zero_box, recv_ptr, sym, argv, argc, null, out],
        )
        .expect("send returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    ownership::discard(
        fx,
        super::operand::Operand::Slot {
            ss,
            owned: true,
            tag: super::operand::TagInfo::Unknown,
        },
    );
    Ok(())
}

/// A short label for the refusal message.
fn statement_kind(node: &HirNode) -> String {
    match node {
        HirNode::ClassDef { .. } => "a class definition".to_string(),
        HirNode::DefMethod { .. } => "a method definition".to_string(),
        HirNode::Begin { .. } => "a begin/rescue/ensure".to_string(),
        other => format!("the node kind `{}`", super::expr::variant_name(other)),
    }
}

/// `puts` with arbitrary slice-lowerable arguments: a contiguous argv
/// array of borrowed copies (owned temps hand their value to the pool
/// first), then the status-protocol call.
fn lower_puts(fx: &mut Fx, stmt: NodeId, args: &[ArrayElem]) -> Result<(), String> {
    let argc = args.len();
    let argv = (argc > 0).then(|| {
        fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            argc as u32 * VALUE_SIZE,
            3,
        ))
    });
    for (i, arg) in args.iter().enumerate() {
        let ArrayElem::Single(id) = arg else {
            return fx.unsupported(stmt, "a splat argument");
        };
        let op = lower_expr(fx, *id)?;
        if op.owned() {
            let tag = op.tag();
            let addr = ownership::addr_of(fx, &op);
            ownership::pool_owned(fx, addr, tag);
        }
        let argv = argv.expect("argc > 0 here");
        let dst = fx.slot_addr(argv, (i as u32 * VALUE_SIZE) as i32);
        ownership::write_borrow(fx, &op, dst);
    }
    let argv_ptr = match argv {
        Some(ss) => fx.slot_addr(ss, 0),
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    let argc_v = fx.b.ins().iconst(fx.em.ptr, argc as i64);
    let out_ss = fx.temp_slot();
    let out = fx.slot_addr(out_ss, 0);
    let status = fx
        .call("zeo_rt_kernel_puts", &[argv_ptr, argc_v, out])
        .expect("kernel_puts returns a status");
    fx.fallible(status);
    // `puts` answers nil -- an immediate, nothing to release.
    Ok(())
}

/// A native loop: `while`/`until` (pre- or post-test) and bare `loop`.
/// The release pool is bracketed -- marked at entry, reset at the latch
/// and at the exit -- so a long loop never accumulates temps.
fn lower_loop(
    fx: &mut Fx,
    cond: Option<(NodeId, bool)>,
    body: &[NodeId],
    post: bool,
    result: Option<cranelift_codegen::ir::Value>,
) -> Result<(), String> {
    let mark = fx
        .call("zeo_rt_pool_mark", &[])
        .expect("pool_mark returns the watermark");
    let head = fx.b.create_block();
    let body_blk = fx.b.create_block();
    let latch = fx.b.create_block();
    let exit_normal = fx.b.create_block();
    let exit = fx.b.create_block();
    let first = if post { body_blk } else { head };
    fx.b.ins().jump(first, &[]);

    fx.b.switch_to_block(head);
    match cond {
        Some((cond_id, negate)) => {
            let c = lower_expr(fx, cond_id)?;
            let mut t = ownership::truthy(fx, c);
            if negate {
                // `until`: flip the low bit of the 0/1 truthiness.
                t =
                    fx.b.ins()
                        .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, t, 0);
            }
            fx.b.ins().brif(t, body_blk, &[], exit_normal, &[]);
        }
        None => {
            fx.b.ins().jump(body_blk, &[]);
        }
    }

    fx.b.switch_to_block(body_blk);
    let status = fx
        .call("zeo_rt_check_ints", &[])
        .expect("check_ints status");
    fx.fallible(status);
    fx.loops.push(LoopCtl {
        exit,
        latch,
        body: body_blk,
        result,
        depth: fx.ensure_depth,
        handling: fx.handling_depth,
    });
    lower_stmts(fx, body)?;
    fx.loops.pop();
    fx.b.ins().jump(latch, &[]);

    fx.b.switch_to_block(latch);
    fx.call("zeo_rt_pool_reset", &[mark]);
    fx.b.ins().jump(head, &[]);

    // Ran-to-completion (condition went false): a loop's own value is nil;
    // `break v` bypasses this write.
    fx.b.switch_to_block(exit_normal);
    if let Some(dst) = result {
        ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
    }
    fx.b.ins().jump(exit, &[]);

    fx.b.switch_to_block(exit);
    fx.call("zeo_rt_pool_reset", &[mark]);
    Ok(())
}

/// `for target in iterable` -- ruby performs NO type dispatch here (it
/// compiles to `iterable.each { |target| .. }`), so the runtime driver
/// decides per receiver: a live Array indexes the receiver itself (CRuby
/// re-reads its length every step), an Int-bounded Range counts, and
/// everything else walks what `each` yielded. The loop's own value is the
/// collection; a `break v` supplies its own.
fn lower_for(
    fx: &mut Fx,
    site: NodeId,
    target: &crate::hir::MultiTarget,
    iterable: NodeId,
    body: &[NodeId],
    result: Option<cranelift_codegen::ir::Value>,
) -> Result<(), String> {
    use crate::hir::MultiTarget;
    let coll = lower_expr(fx, iterable)?;
    let coll_ptr = ownership::borrow_ptr(fx, &coll);
    if coll.owned() {
        ownership::pool_owned(fx, coll_ptr, coll.tag());
    }
    // `for x in obj` is one parameter, so a multi-value yield binds its
    // FIRST value; `for k, v in obj` packs.
    let packed = u8::from(matches!(target, MultiTarget::Nested(_)));
    let state_ss = fx.b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        u32::try_from(zeo_abi::abi::FOR_STATE_SIZE).expect("the state slot fits a u32"),
        3,
    ));
    let state = fx.slot_addr(state_ss, 0);
    let packed_v = fx.b.ins().iconst(types::I8, i64::from(packed));
    let status = fx
        .call("zeo_rt_for_begin", &[coll_ptr, packed_v, state])
        .expect("for_begin returns a status");
    fx.fallible(status);

    let mark = fx
        .call("zeo_rt_pool_mark", &[])
        .expect("pool_mark returns the watermark");
    let head = fx.b.create_block();
    let body_blk = fx.b.create_block();
    let latch = fx.b.create_block();
    let exit_normal = fx.b.create_block();
    let exit = fx.b.create_block();
    // The error path releases the state before the enclosing landing runs.
    let outer_land = fx.land;
    let end_land = fx.b.create_block();
    fx.b.ins().jump(head, &[]);

    fx.b.switch_to_block(head);
    let elem_ss = fx.temp_slot();
    let elem = fx.slot_addr(elem_ss, 0);
    let more_ss = fx.temp_slot();
    let more_ptr = fx.slot_addr(more_ss, 0);
    fx.call("zeo_rt_for_next", &[state, elem, more_ptr]);
    let fl = cranelift_codegen::ir::MemFlagsData::trusted();
    let more = fx.b.ins().load(types::I8, fl, more_ptr, 0);
    fx.b.ins().brif(more, body_blk, &[], exit_normal, &[]);

    fx.b.switch_to_block(body_blk);
    fx.owned_created += 1;
    write_multi_target(fx, site, target, elem)?;
    let status = fx
        .call("zeo_rt_check_ints", &[])
        .expect("check_ints status");
    fx.land = end_land;
    fx.fallible(status);
    fx.loops.push(LoopCtl {
        exit,
        latch,
        body: body_blk,
        result,
        depth: fx.ensure_depth,
        handling: fx.handling_depth,
    });
    let r = lower_stmts(fx, body);
    fx.loops.pop();
    fx.land = outer_land;
    r?;
    fx.b.ins().jump(latch, &[]);

    fx.b.switch_to_block(latch);
    fx.call("zeo_rt_pool_reset", &[mark]);
    fx.b.ins().jump(head, &[]);

    // Ran to completion: the loop answers the collection it walked.
    fx.b.switch_to_block(exit_normal);
    if let Some(dst) = result {
        fx.call("zeo_rt_for_result", &[state, dst]);
        fx.owned_created += 1;
        fx.owned_consumed += 1;
    }
    fx.b.ins().jump(exit, &[]);

    fx.b.switch_to_block(end_land);
    fx.call("zeo_rt_for_end", &[state]);
    fx.b.ins().jump(outer_land, &[]);

    fx.b.switch_to_block(exit);
    fx.call("zeo_rt_for_end", &[state]);
    fx.call("zeo_rt_pool_reset", &[mark]);
    Ok(())
}

/// A loop-targeted `break`/`next`/`redo` with an `ensure` between it and
/// its loop. Ruby runs that ensure first, so the jump travels as a signal
/// down the landing chain and the ensure-carrying `begin` settles it back
/// onto the loop's own targets ([`super::control::lower_begin`]). `$!` is
/// left to that chain, which pops exactly what it pushed.
fn signal_jump(
    fx: &mut Fx,
    value: Option<NodeId>,
    kind: zeo_abi::abi::SignalKind,
) -> Result<(), String> {
    let ptr = match value {
        Some(v) => {
            let op = lower_expr(fx, v)?;
            ownership::move_ptr(fx, &op)
        }
        // `Redo` carries nothing; the other two answer nil without one.
        None if matches!(kind, zeo_abi::abi::SignalKind::Redo) => fx.b.ins().iconst(fx.em.ptr, 0),
        None => ownership::move_ptr(fx, &super::operand::Operand::Nil),
    };
    let k = fx.b.ins().iconst(types::I8, i64::from(kind as u8));
    fx.call("zeo_rt_signal_set", &[k, ptr]);
    fx.ensure_jumps += 1;
    let land = fx.land;
    fx.b.ins().jump(land, &[]);
    fx.continue_unreachable();
    Ok(())
}

/// A `while`/`until`/`loop`/`for` in VALUE position: its own value is nil
/// (the collection, for `for`), and a `break v` supplies its own.
pub(crate) fn loop_value(fx: &mut Fx, node: NodeId) -> Result<super::operand::Operand, String> {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    match &fx.an.compiler.hir[node] {
        HirNode::While {
            cond,
            body,
            negate,
            post,
        } => {
            let (cond, body, negate, post) = (*cond, body.clone(), *negate, *post);
            lower_loop(fx, Some((cond, negate)), &body, post, Some(dst))?;
        }
        HirNode::Loop { body } => {
            let body = body.clone();
            lower_loop(fx, None, &body, false, Some(dst))?;
        }
        HirNode::For {
            target,
            iterable,
            body,
        } => {
            let (target, iterable, body) = (target.clone(), *iterable, body.clone());
            lower_for(fx, node, &target, iterable, &body, Some(dst))?;
        }
        other => {
            let what = format!("the node kind `{}`", super::expr::variant_name(other));
            return fx.unsupported(node, &what);
        }
    }
    fx.owned_created += 1;
    Ok(super::operand::Operand::Slot {
        ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    })
}

/// Stamp the frame's line for a CALL whose method NAME sits on a later
/// line than the expression it belongs to (`recv\n  .m`, `end.m(..)`).
///
/// Emitted after the receiver and the arguments, because those belong to
/// their own lines: a raise inside one must still report where it was
/// written, which is what ruby does.
///
/// The line only, never a coverage hit. Ruby counts a multi-line call's
/// FIRST line in `Coverage` while reporting the name's line in a
/// backtrace, so the two answers cannot come from one stamp.
pub(crate) fn stamp_call_line(fx: &mut Fx, site: NodeId) {
    let Some(line) = fx.an.compiler.hir.call_line(site) else {
        return;
    };
    if fx.prev_line == Some(line) {
        return;
    }
    fx.prev_line = Some(line);
    let v = fx.b.ins().iconst(types::I32, i64::from(line));
    fx.call("zeo_rt_set_line", &[v]);
}

/// Stamp a `set_line` only when the statement's line differs from the
/// previous stamp.
fn stamp_line(fx: &mut Fx, stmt: NodeId) {
    let Some((file, line)) = fx.location(stmt) else {
        return;
    };
    let file = file.to_string();
    // The DWARF row is stamped BEFORE the dedup: `set_srcloc` emits no
    // code, and skipping it whenever the previous statement shared a line
    // would leave whole blocks attributed to whatever the last STAMPED
    // statement was -- the dedup exists to save a runtime CALL, which is
    // a different question.
    if let Some(debug) = fx.em.debug.as_mut() {
        let loc = debug.srcloc(&file, line);
        fx.b.set_srcloc(loc);
    }
    if fx.prev_line == Some(line) && fx.prev_file.as_deref() == Some(file.as_str()) {
        return;
    }
    // The main script never qualifies for coverage: its top level began
    // before `Coverage.start` ran. `entry_file_name`, not `files[0]` -- the
    // vendored corelib registers its own sources ahead of the main file, so
    // the corelib -> main transition looked like entering a spliced file and
    // reported the main script.
    let entering_spliced_file = fx.prev_file.as_deref().is_some_and(|f| f != file)
        && fx
            .an
            .compiler
            .hir
            .entry_file_name()
            .is_none_or(|name| name != file);
    fx.prev_line = Some(line);
    fx.prev_file = Some(file.clone());
    let v = fx.b.ins().iconst(types::I32, i64::from(line));
    fx.call("zeo_rt_set_line", &[v]);
    // Line coverage's one hit per stamped statement -- a program without
    // the `require` emits nothing here at all.
    if fx.em.cov_active {
        let (fptr, flen) = super::expr::rodata_name(fx, &file);
        if entering_spliced_file {
            fx.call("zeo_rt_cov_file_loaded", &[fptr, flen]);
        }
        fx.call("zeo_rt_cov_line", &[fptr, flen, v]);
        fx.em.cov_lines.entry(file).or_default().insert(line);
    }
}

/// One class-body site's marker-time emission: record the declaration's
/// `const_source_location`, then call the compiled body with the class as
/// `self` (its value -- ruby's class-body tail -- is discarded here; a
/// `class` expression in value position still refuses).
/// A class-body marker in STATEMENT position: run the site, drop its
/// value.
pub(crate) fn emit_class_body_call(
    fx: &mut Fx,
    call: &super::emit::ClassBodyCall,
) -> Result<(), String> {
    let op = class_body_site(fx, call)?;
    ownership::discard(fx, op);
    Ok(())
}

/// The site's Ruby VALUE -- what a `class`/`module` written where a value
/// is read evaluates to. `expression` distinguishes the two positions: a
/// tail whose value zeo cannot name is nil (nothing necessarily reads
/// it), an expression's is a refusal.
pub(crate) fn class_body_value(
    fx: &mut Fx,
    site: NodeId,
    expression: bool,
) -> Result<super::operand::Operand, String> {
    use super::emit::BodyTail;
    let Some(call) = fx.em.class_bodies.get(&site).cloned() else {
        // A hoisted or statement-free site: its body already ran (or has
        // nothing to run), and ruby's value is the body's own tail.
        return Ok(super::operand::Operand::Nil);
    };
    let op = class_body_site(fx, &call)?;
    match &call.tail {
        BodyTail::Own => Ok(op),
        BodyTail::Sym(name) => {
            ownership::discard(fx, op);
            let name = name.clone();
            super::expr::symbol_value(fx, &name)
        }
        BodyTail::OwnClass => {
            ownership::discard(fx, op);
            Ok(super::expr::class_immediate(
                fx,
                crate::compiler::ClassId(call.class),
            ))
        }
        BodyTail::Unknown(kind) if expression => {
            let kind = *kind;
            fx.unsupported(
                site,
                &format!("a class body read for its VALUE ending in {kind}"),
            )
        }
        BodyTail::Unknown(_) => {
            ownership::discard(fx, op);
            Ok(super::operand::Operand::Nil)
        }
    }
}

/// One class-body site, under its trailing `if`/`unless` when it has one.
///
/// The condition is lowered HERE, in the enclosing scope, because that is
/// where ruby runs it and where the locals it reads live -- `class Set ...
/// end if set_pp` in pp.rb reads a top-level local. Everything the site does
/// rides inside the branch: a class whose guard failed was never declared,
/// so it announces nothing and reveals nothing, and the site's value is nil.
fn class_body_site(
    fx: &mut Fx,
    call: &super::emit::ClassBodyCall,
) -> Result<super::operand::Operand, String> {
    use cranelift_codegen::ir::InstBuilder;
    let Some((cond, run_when)) = call.guard else {
        return class_body_site_run(fx, call);
    };
    let c = super::expr::lower_expr(fx, cond)?;
    let truthy = ownership::truthy(fx, c);
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let b_run = fx.b.create_block();
    let b_skip = fx.b.create_block();
    let join = fx.b.create_block();
    let (t, f) = match run_when {
        true => (b_run, b_skip),
        false => (b_skip, b_run),
    };
    fx.b.ins().brif(truthy, t, &[], f, &[]);
    fx.b.switch_to_block(b_run);
    let op = class_body_site_run(fx, call)?;
    ownership::write_move_into(fx, &op, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(b_skip);
    ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(super::operand::Operand::Slot {
        ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    })
}

fn class_body_site_run(
    fx: &mut Fx,
    call: &super::emit::ClassBodyCall,
) -> Result<super::operand::Operand, String> {
    use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};
    use cranelift_module::Module;
    // The frozen-reopen guard runs FIRST: a frozen class raises before the
    // body's declaration bookkeeping, let alone its statements.
    if !call.freeze_guard.is_empty() {
        let names: Vec<&str> = call.freeze_guard.iter().map(String::as_str).collect();
        let (ptr, n) = super::statics::str_array(fx, &names);
        let cid = fx.b.ins().iconst(types::I32, i64::from(call.class));
        let st = fx
            .call("zeo_rt_guard_class_reopen", &[cid, ptr, n])
            .expect("guard_class_reopen returns a status");
        fx.fallible(st);
    }
    // The reopen is POSITIONAL: from here on the reopened bodies answer for
    // themselves, and above this line they defer to the rows they replaced.
    // Set before the body's own statements, which may call one.
    if !call.reopen_flags.is_empty() {
        let gv = fx
            .em
            .module
            .declare_data_in_func(fx.em.reopen_flags_id, fx.b.func);
        let base = fx.b.ins().symbol_value(fx.em.ptr, gv);
        let one = fx.b.ins().iconst(types::I8, 1);
        for &idx in &call.reopen_flags {
            fx.b.ins()
                .store(MemFlagsData::trusted(), one, base, idx as i32);
        }
    }
    // A runtime-conditional class's guarded definition just RAN: the
    // constant exists from here on, before any declaration bookkeeping and
    // at EVERY site (whichever branch runs must reveal).
    if call.reveal {
        let cid = fx.b.ins().iconst(types::I32, i64::from(call.class));
        fx.call("zeo_rt_reveal_class", &[cid]);
    }
    // The constant is set, so ruby announces it -- before `inherited` and
    // before the body, the order `vm_declare_class` hard-codes.
    if let Some((owner, name)) = &call.const_added {
        super::expr::const_added_announce(fx, *owner, name)?;
    }
    if let Some((owner, name, file, line)) = &call.const_loc {
        let owner_v = fx.b.ins().iconst(types::I32, i64::from(*owner));
        let (nptr, nlen) = name_pair(fx, name);
        let (fptr, flen) = name_pair(fx, file);
        let line_v = fx.b.ins().iconst(types::I32, i64::from(*line));
        fx.call(
            "zeo_rt_record_const_location",
            &[owner_v, nptr, nlen, fptr, flen, line_v],
        );
    }
    // ...then `Super.inherited(C)`, the order `vm_declare_class` hard-codes
    // (the constant is set, the hook fires, then the body runs).
    if let Some(parent) = call.inherited {
        let recv = super::expr::class_immediate(fx, crate::compiler::ClassId(parent));
        let recv_ptr = ownership::borrow_ptr(fx, &recv);
        let arg = super::expr::class_immediate(fx, crate::compiler::ClassId(call.class));
        let argv = ownership::borrow_ptr(fx, &arg);
        let sym = fx.sym_id("inherited");
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let zero_box = fx.box_v();
        let argc = fx.b.ins().iconst(fx.em.ptr, 1);
        let null = fx.b.ins().iconst(fx.em.ptr, 0);
        let status = fx
            .call(
                "zeo_rt_send_value_in",
                &[zero_box, recv_ptr, sym, argv, argc, null, out],
            )
            .expect("send returns a status");
        fx.fallible(status);
        fx.owned_created += 1;
        ownership::discard(
            fx,
            super::operand::Operand::Slot {
                ss,
                owned: true,
                tag: super::operand::TagInfo::Unknown,
            },
        );
    }
    // `alias`'s builtin source validates as this body finishes -- CRuby's
    // timing, run at the CALL site so an alias-only (empty-statement) body
    // still checks (rustc emits the check even for an otherwise empty
    // body).
    let validate = |fx: &mut Fx| {
        let has = !fx.an.compiler.classes[call.class as usize]
            .builtin_aliases
            .is_empty();
        if has {
            let cid = fx.b.ins().iconst(types::I32, i64::from(call.class));
            let st = fx
                .call("zeo_rt_validate_class_aliases", &[cid])
                .expect("validate_class_aliases returns a status");
            fx.fallible(st);
        }
    };
    let Some(func) = call.func else {
        validate(fx);
        return Ok(super::operand::Operand::Nil);
    };
    // `self` = the class, materialized as a Class immediate.
    let self_ss = fx.temp_slot();
    let self_addr = fx.slot_addr(self_ss, 0);
    let fl = MemFlagsData::trusted();
    let z = fx.b.ins().iconst(types::I64, 0);
    for off in [0, 8, 16] {
        fx.b.ins().store(fl, z, self_addr, off);
    }
    let tag =
        fx.b.ins()
            .iconst(types::I8, i64::from(zeo_abi::abi::ValueTag::Class as u8));
    fx.b.ins().store(fl, tag, self_addr, 0);
    let cid = fx.b.ins().iconst(types::I32, i64::from(call.class));
    fx.b.ins()
        .store(fl, cid, self_addr, zeo_abi::abi::PAYLOAD_OFFSET as i32);
    let out_ss = fx.temp_slot();
    let out = fx.slot_addr(out_ss, 0);
    let fref = fx.em.module.declare_func_in_func(func, fx.b.func);
    let inst = fx.b.ins().call(fref, &[self_addr, out]);
    let status = fx.b.func.dfg.inst_results(inst)[0];
    fx.fallible(status);
    validate(fx);
    fx.owned_created += 1;
    Ok(super::operand::Operand::Slot {
        ss: out_ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    })
}

/// A `&str`'s `.rodata` `(ptr, len)` pair.
fn name_pair(fx: &mut Fx, s: &str) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    use cranelift_codegen::ir::InstBuilder;
    let off = fx.em.intern_rodata(s.as_bytes());
    let ptr = fx.rod(off);
    let len = fx.b.ins().iconst(fx.em.ptr, s.len() as i64);
    (ptr, len)
}

/// One positional definition-surface retag: the enclosing class body's own
/// class, the name as a Symbol, and the entry's verb byte when it takes one.
fn apply_visibility(
    fx: &mut Fx,
    stmt: NodeId,
    entry: &'static str,
    name: &str,
    verb: Option<u8>,
) -> Result<(), String> {
    use cranelift_codegen::ir::InstBuilder;
    let Some(cid) = fx.defining_class.or(fx.method_class) else {
        return fx.unsupported(stmt, "a visibility retag outside a class body");
    };
    let cid_v =
        fx.b.ins()
            .iconst(cranelift_codegen::ir::types::I32, i64::from(cid.0));
    let sym = fx.sym_id(name);
    let mut args = vec![cid_v, sym];
    if let Some(v) = verb {
        args.push(
            fx.b.ins()
                .iconst(cranelift_codegen::ir::types::I8, i64::from(v)),
        );
    }
    let status = fx.call(entry, &args).expect("the entry returns a status");
    fx.fallible(status);
    Ok(())
}

/// The compile-time verdict on an `if` condition, or `None` when it has to be
/// asked at run time. See `analyze::constfold::static_cond`.
/// The `=~` a folded condition performed, if the condition IS one. Its
/// operands are static by construction (that is why the fold succeeded), so
/// re-running it for `$~` cannot raise or repeat work.
fn folded_match(fx: &Fx<'_, '_>, cond: crate::hir::NodeId) -> Option<crate::hir::NodeId> {
    match &fx.an.compiler.hir[cond] {
        crate::hir::HirNode::Call { name, .. } if name == "=~" => Some(cond),
        _ => None,
    }
}

pub(crate) fn static_cond(fx: &Fx<'_, '_>, cond: crate::hir::NodeId) -> Option<bool> {
    let env = crate::analyze::constfold::ConstEnv {
        compiler: &fx.an.compiler,
        defining_class: fx.defining_class.or(fx.method_class),
        box_id: fx.box_id,
    };
    crate::analyze::constfold::static_cond(&env, cond)
}
