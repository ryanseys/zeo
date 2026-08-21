//! Compiled-object allocation and slot-indexed ivar access -- the
//! `CompiledObject`/`LAYOUTS` surface.

use super::dispatch::status_out;
use crate::compiled_object::{self, CompiledObject};
use crate::{RubyValue, Signal, Symbol};
use zeo_abi::ClassId;
use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};

/// A fresh, `initialize`-less instance of compiled class `cid` (`Class#
/// allocate`'s storage half; `zeo_rt_class_new_instance` is the `new`
/// path). A class without a `LAYOUTS` row is an internal compiler error --
/// only classes `register_program` described allocate here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_object_alloc(cid: u32, out: *mut RubyValue) {
    let id = ClassId(cid);
    let layout = compiled_object::alloc_layout_of(id)
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

/// `zeo_rt_const_get_cref`'s `flags`: the owner's chain defines a user
/// `const_missing`, so a miss DISPATCHES it.
pub const CONST_CREF_HOOK: u8 = 1;
/// `zeo_rt_const_get_cref`'s `flags`: the read runs inside a BOX, so the
/// tail past the box's surrogate reaches the MASTER constants.
pub const CONST_CREF_MASTER: u8 = 2;

/// A BARE constant read resolved through its compile-time cref chain:
/// `ids` = the owner first, then each enclosing cref scope, then the top
/// (the emitter's `emit_const_read` order); each entry searches its own
/// ancestry (`const_get`). Miss = the `const_miss_signal` NameError whose
/// message carries the pre-qualified name (`Store::Cart::DEFAULT` for a
/// nested miss, bare at the top level), or -- when [`CONST_CREF_HOOK`] is
/// set because the owner's chain defines one -- the `const_missing`
/// dispatch whose answer this read then takes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_const_get_cref(
    ids: *const u32,
    n_ids: usize,
    name: *const u8,
    name_len: usize,
    qualified: *const u8,
    qualified_len: usize,
    flags: u8,
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
    // Inside a BOX the chain ends at the box's own surrogate, and the tail
    // past it reaches only the MASTER constants -- the ones installed
    // before the main program ran. Falling through to `Object` would hand
    // the box main's own top-level constants, which a box (a copy of
    // master) never sees.
    if flags & CONST_CREF_MASTER != 0
        && let Some(v) = crate::constants::const_get_master(name)
    {
        super::leakcheck::created(&v);
        unsafe { out.write(v) };
        return STATUS_OK;
    }
    if flags & CONST_CREF_HOOK != 0 {
        return status_out(crate::dispatch::const_miss(ClassId(ids[0]), name), out);
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
    hook: u8,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    match crate::constants::const_get_scoped(owner, name) {
        Some(v) => {
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            STATUS_OK
        }
        None if hook != 0 => status_out(crate::dispatch::const_miss(ClassId(owner), name), out),
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

/// `alias $new $old` -- a real, bidirectional alias of the STORAGE, so
/// writing either name is visible through both.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_gvar_alias(
    box_id: u32,
    new: *const u8,
    new_len: usize,
    old: *const u8,
    old_len: usize,
) {
    let new = unsafe { super::str_slice(new, new_len) };
    let old = unsafe { super::str_slice(old, old_len) };
    crate::globals::global_alias(box_id, new, old);
}

/// The `private constant X::Y referenced` NameError. A GUARD rather than a
/// compile-time refusal, because a later `X.public_constant :Y` restores
/// the name and only the runtime flag knows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_raise_private_constant(
    owner: u32,
    name: *const u8,
    name_len: usize,
    path: *const u8,
    path_len: usize,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    let path = unsafe { super::str_slice(path, path_len) };
    crate::signal::set_pending(Signal::Raise(crate::dispatch::stamp_backtrace(
        crate::dispatch::make_name_error(
            format!("private constant {path} referenced"),
            name,
            RubyValue::Class(ClassId(owner)),
        ),
    )));
    STATUS_SIGNAL
}

/// A method REDEFINITION applied at its document position: `f` becomes
/// the class's current body of `name` in the overlay, so code running
/// between two same-name `def`s dispatches to the earlier one (ruby's
/// install-where-it-stands, which zeo's static tables flatten away).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_runtime_replace_method(
    class: u32,
    name: *const u8,
    name_len: usize,
    f: super::ValueFn,
) {
    let name = unsafe { super::str_slice(name, name_len) };
    crate::runtime_meta::runtime_replace_method_c(ClassId(class), Symbol::intern(name), f);
}

/// `Foo::NAME`'s lenient half (`||=`'s read, `defined?`'s probe): an
/// absent constant -- or a scope class the compiler never registered --
/// is `nil`, never a raise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_const_get_or_nil(
    owner: u32,
    name: *const u8,
    name_len: usize,
    out: *mut RubyValue,
) {
    let name = unsafe { super::str_slice(name, name_len) };
    let v = crate::constants::const_get(owner, name).unwrap_or(RubyValue::Nil);
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// `obj::NAME` -- the scope operator on a VALUE, so the whole search runs
/// here: the scoped search (not `const_get`'s, which reaches through
/// `Object` and past a `private_constant`), the privacy gate, and the
/// `const_missing` dispatch on a miss.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_scope_const_get(
    scope: *const RubyValue,
    name: *const u8,
    name_len: usize,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    status_out(
        crate::builtins::rmodule::scope_const_get(unsafe { &*scope }, name),
        out,
    )
}

/// The lenient half of `obj::NAME ||= v`: nil where the strict read would
/// raise `NameError`. A non-module scope is still a `TypeError` -- the
/// leniency is about the NAME, not the receiver.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_scope_const_get_or_nil(
    scope: *const RubyValue,
    name: *const u8,
    name_len: usize,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    status_out(
        crate::builtins::rmodule::scope_const_get_or_nil(unsafe { &*scope }, name),
        out,
    )
}

/// `defined?(obj::NAME)`'s membership test -- a scope that is not a module
/// answers false rather than raising.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_scope_const_defined(
    scope: *const RubyValue,
    name: *const u8,
    name_len: usize,
) -> u8 {
    let name = unsafe { super::str_slice(name, name_len) };
    u8::from(crate::builtins::rmodule::scope_const_defined(
        unsafe { &*scope },
        name,
    ))
}

/// `obj::NAME = value` -- writes the scope's OWN table (assignment never
/// walks an ancestry), records the writing line, fires `const_added`, and
/// answers the value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_scope_const_set(
    scope: *const RubyValue,
    name: *const u8,
    name_len: usize,
    v: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    status_out(
        crate::builtins::rmodule::scope_const_set(unsafe { &*scope }, name, unsafe { &*v }.clone()),
        out,
    )
}

/// `Scope::NAME` where the SCOPE is only a runtime constant (a
/// `Struct.new` result, a class built under a computed superclass): the
/// caller read the scope path and hands the value here, and the leaf is
/// looked up with the scope operator's own search. A non-module scope is
/// ruby's TypeError; a missing leaf the NameError naming the scope class
/// as its receiver (rustc's runtime-scope arm, verbatim).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_const_get_on_value(
    scope: *const RubyValue,
    name: *const u8,
    name_len: usize,
    qualified: *const u8,
    qualified_len: usize,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(name, name_len) };
    let scope = unsafe { &*scope };
    let RubyValue::Class(cid) = scope else {
        crate::signal::set_pending(crate::dispatch::raise_error(
            "TypeError",
            format!("{} is not a class/module", scope.inspect_string()),
        ));
        return STATUS_SIGNAL;
    };
    match crate::constants::const_get_scoped(cid.0, name) {
        Some(v) => {
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            STATUS_OK
        }
        None => {
            let qualified = unsafe { super::str_slice(qualified, qualified_len) };
            crate::signal::set_pending(Signal::Raise(crate::dispatch::stamp_backtrace(
                crate::dispatch::make_name_error(
                    format!("uninitialized constant {qualified}"),
                    name,
                    RubyValue::Class(*cid),
                ),
            )));
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
    body_vis: u8,
    out: *mut RubyValue,
) -> i32 {
    // The proc is MOVED in (the caller hands over its reference).
    let body = unsafe { body.read() };
    super::leakcheck::consumed(&body);
    // `body_vis`: 0 = none, 1 = private, 2 = protected -- the enclosing
    // body's running default, applied to whatever definee the runtime picks.
    let body_vis = match body_vis {
        1 => Some(crate::dispatch::MethodVisibility::Private),
        2 => Some(crate::dispatch::MethodVisibility::Protected),
        _ => None,
    };
    super::dispatch::status_out(
        crate::dispatch::define_in_default_definee_vis(
            unsafe { &*cref },
            unsafe { &*slf },
            crate::Symbol::from_u32(name),
            body,
            private != 0,
            body_vis,
        ),
        out,
    )
}

/// A `def` written inside a run-time `eval`, installed where CRuby installs
/// it -- the `mode` byte is `eval_vm::EvalMode`, and the rule is the
/// interpreter's own `initial_definee`: `instance_eval` installs on the
/// receiver's SINGLETON, `class_eval` on the receiver (a Class), and a
/// plain `Kernel#eval` on the receiver's class -- privately when that
/// receiver is `main`, which is the whole of what makes a top-level `def`
/// private.
///
/// The definee cannot be a compile-time constant the way a program's is:
/// one snippet is compiled once and may be evaluated against any number
/// of receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_define(
    mode: u8,
    slf: *const RubyValue,
    name: u32,
    body: *mut RubyValue,
    vis: u8,
    out: *mut RubyValue,
) -> i32 {
    let body = unsafe { body.read() };
    super::leakcheck::consumed(&body);
    let slf = unsafe { &*slf };
    let name = crate::Symbol::from_u32(name);
    super::dispatch::status_out(
        crate::dispatch::eval_define(mode, slf, name, body, vis),
        out,
    )
}

/// The class a definition-level statement written inside a run-time `eval`
/// names -- `alias`, `undef`, `private`, `module_function`,
/// `private_constant`. Same rule as [`zeo_rt_eval_define`]'s definee, with
/// an `instance_eval`'s singleton materialized, since those verbs are
/// Module's and have to reach a class.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_definee(
    mode: u8,
    slf: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    super::dispatch::status_out(crate::dispatch::eval_definee(mode, unsafe { &*slf }), out)
}

/// The visibility a runtime-installed `def` carries from its class body's
/// running default (`private`/`protected` mode), and the one a REOPEN's
/// `private :m` applies at its document position. Verb 0 = private, 1 =
/// protected, 2 = public.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_runtime_set_visibility(cid: u32, name: u32, verb: u8) -> i32 {
    let vis = match verb {
        0 => crate::dispatch::MethodVisibility::Private,
        1 => crate::dispatch::MethodVisibility::Protected,
        _ => crate::dispatch::MethodVisibility::Public,
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

/// The FROZEN-REOPEN guard: a class body that would install a name the
/// class does not already carry raises `FrozenError` when the class was
/// frozen, and retires the names it could not install. `names` is a
/// `.rodata` Str array, in the emitter's sorted order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_guard_class_reopen(
    cid: u32,
    names: *const zeo_abi::abi::Str,
    n: usize,
) -> i32 {
    let rows = if n == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(names, n) }
    };
    let names: Vec<&str> = rows
        .iter()
        .map(|s| unsafe { super::str_slice(s.ptr, s.len) })
        .collect();
    match crate::dispatch::guard_class_reopen(ClassId(cid), &names) {
        Ok(()) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// A reopen's `private_class_method :m` / `public_class_method :m`, applied
/// at its document position (analyze leaves those in the site's statements
/// precisely so they do).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_runtime_class_method_visibility(
    cid: u32,
    name: u32,
    private: u8,
) -> i32 {
    match crate::runtime_meta::runtime_class_method_visibility(
        ClassId(cid),
        &[RubyValue::Symbol(crate::Symbol::from_u32(name))],
        private != 0,
    ) {
        Ok(()) => zeo_abi::abi::STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            zeo_abi::abi::STATUS_SIGNAL
        }
    }
}

/// An `undef` in a REOPENED `class << self`, applied where it stands.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_runtime_undef_class_method(cid: u32, name: u32) -> i32 {
    match crate::runtime_meta::runtime_undef_class_method_names(
        ClassId(cid),
        &[crate::Symbol::from_u32(name).name().as_str()],
    ) {
        Ok(()) => zeo_abi::abi::STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            zeo_abi::abi::STATUS_SIGNAL
        }
    }
}

/// A runtime-conditional class starts CONCEALED: its shape is registered
/// (the static MRO needs one) but nothing may treat the constant as
/// existing until its guarded body runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_conceal_class(cid: u32) {
    crate::constants::conceal_class(cid);
}

/// The guarded definition just RAN: the constant exists from here on.
/// Idempotent -- a reopened conditional class reveals at every site.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_reveal_class(cid: u32) {
    crate::constants::reveal_class(cid);
}

/// A reference to a runtime-CONDITIONAL class: `NameError` while the
/// guarded body has not run (`uninitialized constant Foo`), the Class
/// value once it has.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_conditional_class_ref(
    cid: u32,
    fq: *const u8,
    fq_len: usize,
    owner: u32,
    out: *mut RubyValue,
) -> i32 {
    let name = unsafe { super::str_slice(fq, fq_len) };
    status_out(
        crate::constants::conditional_class_ref(ClassId(cid), name, ClassId(owner)),
        out,
    )
}

/// [`crate::runtime_meta::with_pending_defs`]' two halves: the definition
/// hook's send runs with `names` marked as not yet defined on `class`, so
/// the hook body sees the half-built class ruby shows it. The emitter
/// pairs them around the send (and its raise path).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pending_defs_begin(class: u32, syms: *const u32, n: usize) {
    let ids = if n == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(syms, n) }
    };
    let names: Vec<Symbol> = ids.iter().map(|&s| Symbol::from_u32(s)).collect();
    crate::runtime_meta::pending_defs_begin(ClassId(class), &names);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pending_defs_end() {
    crate::runtime_meta::pending_defs_end();
}
