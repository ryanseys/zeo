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
    let v = RubyValue::Object(CompiledObject::alloc(id, layout));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// `Object.new` -- a bare sentinel instance of the runtime root, whose
/// container holds no ivar layout at all (top-level `def`s are free
/// functions). Each call mints a fresh `Arc`, so identity is distinct --
/// the sentinel idiom's whole point. The rustc twin is the inline
/// `RubyValue::Object(Arc::new(Object::default()))` in `codegen::call::new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_object_new_sentinel(out: *mut RubyValue) {
    let v = RubyValue::Object(std::sync::Arc::new(crate::Object::default()));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
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
        super::leakcheck::consumed(unsafe { &*blk });
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
    super::leakcheck::created(&v);
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
    super::leakcheck::consumed(unsafe { &*v });
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

/// An uncached constant read on `owner` (0 = `Object`, the toplevel).
/// Miss = CRuby's `NameError: uninitialized constant <name>` (the
/// `ConstSite` caching layer is a later milestone).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_const_get_at(
    owner: u32,
    name: *const u8,
    name_len: usize,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    match crate::constants::const_get(owner, name) {
        Some(v) => {
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            STATUS_OK
        }
        None => {
            crate::signal::set_pending(crate::dispatch::raise_error(
                "NameError",
                format!("uninitialized constant {name}"),
            ));
            STATUS_SIGNAL
        }
    }
}

/// A global variable read (`$foo`) from box `box_id`'s table. Never-assigned
/// = nil (Ruby's rule), so this is infallible. The `$!`/`$?` specials read
/// dedicated runtime slots and have their own entries below.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_gvar_get(
    box_id: u32,
    name: *const u8,
    name_len: usize,
    out: *mut RubyValue,
) {
    let name = unsafe { super::str_slice(name, name_len) };
    let v = crate::globals::global_get(box_id, name);
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A global variable assignment. `v` is BORROWED (cloned into the table),
/// so the emitter's temporary keeps its own ownership. Fallible: read-only
/// globals and alias hooks raise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_gvar_assign(
    box_id: u32,
    name: *const u8,
    name_len: usize,
    v: *const RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    match crate::globals::global_assign(box_id, name, unsafe { &*v }.clone()) {
        Ok(()) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// `$!` -- the exception currently being handled (the same slot a bare
/// `raise` re-raises), nil outside any rescue. NOT the `$foo` table.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_gvar_err_info(out: *mut RubyValue) {
    let v = crate::current_exception().unwrap_or(RubyValue::Nil);
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// `$?` -- the last child's wait status (set by `system`/backticks), nil
/// until the first child runs. NOT the `$foo` table.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_gvar_child_status(out: *mut RubyValue) {
    let v = crate::last_child_status();
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A class variable read whose never-assigned answer is nil -- the
/// `@@x ||= v` read half. Every other read is the checked twin below.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cvar_get(
    owner: u32,
    name: *const u8,
    name_len: usize,
    out: *mut RubyValue,
) {
    let name = unsafe { super::str_slice(name, name_len) };
    let v = crate::cvars::cvar_get(owner, name);
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A class variable read: unassigned raises CRuby's NameError.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cvar_get_checked(
    owner: u32,
    name: *const u8,
    name_len: usize,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    super::dispatch::status_out(crate::cvars::cvar_get_checked(owner, name), out)
}

/// A class variable write. `v` is BORROWED (cloned into the table).
/// Fallible: writing over a parent's cvar from a child raises.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cvar_set(
    owner: u32,
    name: *const u8,
    name_len: usize,
    v: *const RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    match crate::cvars::cvar_set(owner, name, unsafe { &*v }.clone()) {
        Ok(()) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// A constant assignment with its source location (what
/// `Module#const_source_location` reads back; a re-assignment warns and
/// restamps, exactly [`crate::constants::const_set_at`]'s contract).
/// `v` is BORROWED (cloned into the table); `file` must point at `.rodata`
/// (the `'static` the location table keeps).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_const_set_at(
    owner: u32,
    name: *const u8,
    name_len: usize,
    v: *const RubyValue,
    file: *const u8,
    file_len: usize,
    line: u32,
) {
    let name = unsafe { super::str_slice(name, name_len) };
    let file = unsafe { super::static_str(file, file_len) };
    crate::constants::const_set_at(owner, name, unsafe { &*v }.clone(), file, line);
}

/// A BARE constant read resolved through its compile-time cref chain:
/// `ids` = the owner first, then each enclosing cref scope, then the top
/// (the emitter's `emit_const_read` order); each entry searches its own
/// ancestry (`const_get`). Miss = the `const_miss_signal` NameError whose
/// message carries the pre-qualified name (`Store::Cart::DEFAULT` for a
/// nested miss, bare at the top level).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_const_get_cref(
    ids: *const u32,
    n_ids: usize,
    name: *const u8,
    name_len: usize,
    qualified: *const u8,
    qualified_len: usize,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    let ids = unsafe { std::slice::from_raw_parts(ids, n_ids) };
    for &id in ids {
        if let Some(v) = crate::constants::const_get(id, name) {
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            return STATUS_OK;
        }
    }
    let qualified = unsafe { super::str_slice(qualified, qualified_len) };
    crate::signal::set_pending(crate::builtins::rmodule::const_miss_signal(
        ClassId(ids[0]),
        name,
        &format!("uninitialized constant {qualified}"),
    ));
    STATUS_SIGNAL
}

/// An explicit `Scope::NAME` read: the scope operator's own search
/// (`const_get_scoped` -- excludes `Object`'s constants for a non-root
/// scope, ruby's `exclude` flag). Miss = the same NameError shape, with
/// the path as written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_const_get_scoped(
    owner: u32,
    name: *const u8,
    name_len: usize,
    qualified: *const u8,
    qualified_len: usize,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    match crate::constants::const_get_scoped(owner, name) {
        Some(v) => {
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            STATUS_OK
        }
        None => {
            let qualified = unsafe { super::str_slice(qualified, qualified_len) };
            crate::signal::set_pending(crate::builtins::rmodule::const_miss_signal(
                ClassId(owner),
                name,
                &format!("uninitialized constant {qualified}"),
            ));
            STATUS_SIGNAL
        }
    }
}

/// Where a `class`/`module` DECLARATION bound its name -- the
/// `Module#const_source_location` record for constants that live outside
/// the value table (the class registry holds them). Recorded when the
/// declaring site runs, CRuby's timing; a reopen creates nothing and
/// records nothing. `file` must point at `.rodata`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_record_const_location(
    owner: u32,
    name: *const u8,
    name_len: usize,
    file: *const u8,
    file_len: usize,
    line: u32,
) {
    let name = unsafe { super::str_slice(name, name_len) };
    let file = unsafe { super::static_str(file, file_len) };
    crate::constants::record_const_location(owner, name, file, line);
}

/// Read `@name` from a NATIVE-BACKED self (an exception/value-subclass
/// instance): name-keyed, behind CRuby's Ractor guard -- the rustc
/// backend's `ivar_get_dyn_isolated`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ivar_get_dyn(
    recv: *const RubyValue,
    name: *const u8,
    len: usize,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, len) };
    status_out(
        crate::dispatch::ivar_get_dyn_isolated(unsafe { &*recv }, name),
        out,
    )
}

/// Write `@name` on a native-backed self; `v` is BORROWED (cloned in).
/// The frozen check lives inside, exactly the rustc `ivar_set_dyn` path;
/// the echoed value is dropped here (the emitter reads the ivar back when
/// the assignment's value is consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ivar_set_dyn(
    recv: *const RubyValue,
    name: *const u8,
    len: usize,
    v: *const RubyValue,
) -> i32 {
    use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};
    let name = unsafe { super::str_slice(name, len) };
    match crate::dispatch::ivar_set_dyn(unsafe { &*recv }, name, unsafe { &*v }.clone()) {
        Ok(_echo) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// A runtime `def` in expression position: install `name` on the DEFAULT
/// DEFINEE -- the cref's, unless an `*_eval`/`Class.new` on the stack
/// replaced it, which only the runtime can say, so both candidates go.
/// `private` marks the top-level rule (a bare top-level `def` is a private
/// instance method of `Object`). The value is the method-name Symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_define_in_default_definee(
    cref: *const RubyValue,
    slf: *const RubyValue,
    name: u32,
    body: *mut RubyValue,
    private: u8,
    out: *mut RubyValue,
) -> i32 {
    // The proc is MOVED in (the caller hands over its reference).
    let body = unsafe { body.read() };
    super::leakcheck::consumed(&body);
    super::dispatch::status_out(
        crate::dispatch::define_in_default_definee(
            unsafe { &*cref },
            unsafe { &*slf },
            crate::Symbol::from_u32(name),
            body,
            private != 0,
        ),
        out,
    )
}

/// The visibility a runtime-installed `def` carries from its class body's
/// running default (`private`/`protected` mode). Verb 0 = private, 1 =
/// protected.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_runtime_set_visibility(cid: u32, name: u32, verb: u8) -> i32 {
    let vis = if verb == 0 {
        crate::dispatch::MethodVisibility::Private
    } else {
        crate::dispatch::MethodVisibility::Protected
    };
    match crate::runtime_meta::runtime_set_visibility(
        ClassId(cid),
        &[RubyValue::Symbol(crate::Symbol::from_u32(name))],
        vis,
    ) {
        Ok(_) => zeo_abi::abi::STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            zeo_abi::abi::STATUS_SIGNAL
        }
    }
}
