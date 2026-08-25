//! Instance- and class-variable access: the compiled slot fast path and
//! the name-keyed overflow, plus the class that owns a `@@name`.

use super::ctx::Fx;
use super::ownership;
use crate::codegen_error::CResult;
use crate::hir::NodeId;
use cranelift_codegen::ir::InstBuilder;

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
pub(crate) fn ivar_read_op(fx: &mut Fx, name: &str) -> CResult<super::operand::Operand> {
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
pub(super) fn name_keyed_ivar_read(fx: &mut Fx, name: &str) -> CResult<super::operand::Operand> {
    let recv = dyn_ivar_recv(fx);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let (nptr, nlen) = super::expr::rodata_name(fx, name);
    let status = fx.call_status("zeo_rt_ivar_get_dyn", &[recv, nptr, nlen, out]);
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
pub(crate) fn ivar_write_op(fx: &mut Fx, name: &str, op: super::operand::Operand) -> CResult<()> {
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
        let status = fx.call_status("zeo_rt_ivar_set_dyn", &[recv, nptr, nlen, ptr]);
        fx.fallible(status);
        return Ok(());
    }
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    {
        let slot = slot.expect("the slotless case took the name-keyed path");
        let ptr = ownership::move_ptr(fx, &op);
        let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
        let status = fx.call_status("zeo_rt_ivar_set_slot", &[self_ptr, slot_v, ptr]);
        fx.fallible(status);
    }
    Ok(())
}

/// `@name = value`: evaluate then write (see [`ivar_write_op`]).
pub(super) fn lower_ivar_write(fx: &mut Fx, name: &str, value: NodeId) -> CResult<()> {
    let op = super::expr::lower_expr(fx, value)?;
    ivar_write_op(fx, name, op)
}

/// The compiled slot `@name` occupies on this body's class, or `None` for
/// an ivar the class's list does not carry -- one a body only ever reads,
/// or one written on a receiver of another class. rustc keeps those in the
/// same object's name-keyed overflow (`set_named`/`get_named`); the CLIF
/// twin is the name-keyed capi, which reaches the identical storage.
pub(super) fn ivar_slot_of(fx: &Fx, name: &str) -> Option<usize> {
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

/// The class that OWNS `@@name` at this lowering site -- the rustc
/// emitter's `cvar_owner_id` rule: the lexically enclosing class (`Object`
/// at the toplevel), looked through a `class << self` surrogate, then
/// resolved through the analyzer's `cvar_owners` claim map (a subclass
/// writing a parent-declared cvar stores on the parent).
pub(crate) fn cvar_owner(fx: &Fx, name: &str) -> u32 {
    // A snippet's cvar belongs to its cref, which is a RUN-TIME class the
    // fresh compiler has no entry for: no owner walk to do, and the
    // runtime's own `cvar_get`/`set` climb the live ancestry from there.
    if let Some(cid) = fx.eval_cref.as_ref().and_then(|c| c.chain.first().copied()) {
        return cid;
    }
    // Where the code was WRITTEN, never the receiver that reaches it: a
    // class method inherited by a subclass still reads its own class's
    // storage (`Sub.note` writes `Base`'s `@@subs`), so a materialized
    // copy's `defining_class` -- not `method_class` -- is the question.
    let defining = fx
        .defining_class
        .or(fx.method_class)
        .unwrap_or(crate::compiler::OBJECT_CLASS);
    let defining = if fx.an.compiler.is_singleton_surrogate(defining) {
        fx.an
            .compiler
            .class(defining)
            .lexical_parent
            .unwrap_or(defining)
    } else {
        defining
    };
    fx.an
        .compiler
        .class(defining)
        .cvar_owners
        .get(name)
        .copied()
        .unwrap_or(defining)
        .0
}
