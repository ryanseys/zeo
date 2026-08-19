//! Compiled-object allocation and slot-indexed ivar access -- the
//! `CompiledObject`/`LAYOUTS` surface.

use super::dispatch::status_out;
use crate::compiled_object::{self, CompiledObject};
use crate::{RubyValue, Signal};
use zeo_abi::ClassId;
use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};

/// A fresh, `initialize`-less instance of compiled class `cid` (`Class#
/// allocate`'s storage half; `zeo_rt_class_new_instance` is the `new`
/// path). A class without a `LAYOUTS` row is an internal compiler error --
/// only classes `register_program` described allocate here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_object_alloc(cid: u32, out: *mut RubyValue) {
    let id = ClassId(cid);
    let layout = compiled_object::layout_of(id)
        .unwrap_or_else(|| panic!("zeo_rt_object_alloc: no layout registered for class {cid}"));
    unsafe { out.write(RubyValue::Object(CompiledObject::alloc(id, layout))) };
}

/// `Class#new` for a compiled class: allocate, then run `initialize` (if
/// any ancestor defines one; with none, arguments are rejected as CRuby's
/// zero-arity `Object#initialize` would).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_class_new_instance(
    cid: u32,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let id = ClassId(cid);
    let layout = match compiled_object::layout_of(id) {
        Some(l) => l,
        None => panic!("zeo_rt_class_new_instance: no layout registered for class {cid}"),
    };
    let args = if argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    };
    let block = if blk.is_null() {
        None
    } else {
        Some(unsafe { std::ptr::read(blk) })
    };
    let obj = CompiledObject::alloc(id, layout);
    let init: Result<RubyValue, Signal> =
        crate::dispatch::run_initialize(id, &obj, args, block).map(|()| RubyValue::Object(obj));
    status_out(init, out)
}

/// Read one ivar by its compile-time slot index. The receiver is always a
/// compiled `Object` (the emitter's static-self guarantee).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ivar_get_slot(
    obj: *const RubyValue,
    slot: usize,
    out: *mut RubyValue,
) {
    let v = match unsafe { &*obj } {
        RubyValue::Object(o) => o.ivar_slot_get(slot),
        other => {
            debug_assert!(false, "ivar_get_slot on a non-object receiver: {other:?}");
            RubyValue::Nil
        }
    };
    unsafe { out.write(v) };
}

/// Write one ivar by slot, with the frozen check inside (a frozen receiver
/// raises `FrozenError` and the value at `v` is released, not stored --
/// either way the slot at `v` is dead afterward).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ivar_set_slot(
    obj: *const RubyValue,
    slot: usize,
    v: *mut RubyValue,
) -> i32 {
    let recv = unsafe { &*obj };
    let value = unsafe { std::ptr::read(v) };
    if let Err(sig) = crate::builtins::check_frozen(recv) {
        crate::signal::set_pending(sig);
        return STATUS_SIGNAL;
    }
    match recv {
        RubyValue::Object(o) => o.ivar_slot_set(slot, value),
        other => debug_assert!(false, "ivar_set_slot on a non-object receiver: {other:?}"),
    }
    STATUS_OK
}

/// The bare frozen guard (`FrozenError` channel), for the emitted write
/// paths whose storage is not a compiled-object slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_frozen_check(v: *const RubyValue) -> i32 {
    match crate::builtins::check_frozen(unsafe { &*v }) {
        Ok(()) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}
