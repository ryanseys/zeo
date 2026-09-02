//! The variadic entries that pack their arguments into an array.
//!
//! `rb_funcall(recv, mid, 3, a, b, c)`, `rb_scan_args(argc, argv, "11", &a,
//! &b)` and `rb_rescue2(..., cls, 0)` take arguments whose count and types
//! are described only at run time: a leading count, a NUL or `0`
//! terminator, or a format. Each entry here reads exactly what that
//! description says, at the type it says, and hands the array to the entry
//! that does the real work. The raising and formatting variadics are in
//! `error.rs` and `fmt.rs`.

use super::call::{rescue2, scan_args_plan, scan_slice, yield_to_block};
use super::convert::{to_value, value_of};
use super::object::{args_of, cstr};
use super::symbol::Id;
use super::value::{self, Value};
use std::ffi::{VaList, c_char, c_int, c_long, c_void};
use zeo_rt::{RubyValue, Signal};

/// MRI's own ceiling: `rb_funcall` past this is a bug in the extension, and
/// MRI's `rb_funcall` has the same fixed buffer for the same reason.
const MAX_ARGS: usize = 64;
/// `rb_scan_args` names at most this many slots; MRI's own limit is smaller.
const MAX_SLOTS: usize = 32;

/// `n` `VALUE`s, as `rb_funcall` and `rb_yield_values` count them.
///
/// # Safety
///
/// `ap` must hold `n` `VALUE`s.
unsafe fn values(ap: &mut VaList<'_>, n: c_long) -> Vec<Value> {
    let n = (n.max(0) as usize).min(MAX_ARGS);
    (0..n).map(|_| unsafe { ap.next_arg::<Value>() }).collect()
}

/// A `0`-terminated class list, as `rb_rescue2` takes one.
///
/// # Safety
///
/// `ap` must hold `VALUE`s up to a `0`.
unsafe fn classes(ap: &mut VaList<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    while out.len() < MAX_ARGS {
        let c = unsafe { ap.next_arg::<Value>() };
        if c == 0 {
            break;
        }
        out.push(c);
    }
    out
}

/// A NULL-terminated member list, as the Struct and Data entries take one.
///
/// # Safety
///
/// `ap` must hold `const char *`s up to a NULL.
unsafe fn names(ap: &mut VaList<'_>) -> Vec<*const c_char> {
    let mut out = Vec::new();
    while out.len() < MAX_SLOTS {
        let m = unsafe { ap.next_arg::<*const c_char>() };
        if m.is_null() {
            break;
        }
        out.push(m);
    }
    out
}

/// `rb_scan_args`' slot walk. The format says how many required, optional,
/// splat and block slots follow, and each is a `VALUE *`; an optional slot
/// with no argument is Qnil. Answers how many positional arguments were
/// consumed, as MRI does.
///
/// # Safety
///
/// `argv` must name `argc` `VALUE`s, and `ap` the slots the format names.
unsafe fn scan(
    argc: c_int,
    argv: *const Value,
    fmt: *const c_char,
    ap: &mut VaList<'_>,
) -> Result<c_int, Signal> {
    let Some((required, optional, splat, block)) = scan_args_plan(&unsafe { cstr(fmt) }) else {
        return Ok(0);
    };
    let argc = argc.max(0) as usize;
    let mut taken = (required + optional).min(argc);
    for i in 0..required + optional {
        let slot = unsafe { ap.next_arg::<*mut Value>() };
        if !slot.is_null() {
            let v = if i < taken {
                unsafe { argv.add(i).read() }
            } else {
                value::Q_NIL
            };
            unsafe { slot.write(v) };
        }
    }
    if splat {
        let slot = unsafe { ap.next_arg::<*mut Value>() };
        if !slot.is_null() {
            unsafe { slot.write(scan_slice(argc, argv, taken, argc)?) };
        }
        taken = argc;
    }
    if block {
        let slot = unsafe { ap.next_arg::<*mut Value>() };
        if !slot.is_null() {
            // The block is not in argv.
            unsafe { slot.write(value::Q_NIL) };
        }
    }
    Ok(taken as c_int)
}

crate::cext_va_fn! {
    fn rb_funcall(recv: Value, mid: Id, n: c_int; ap) -> Value {
        let args = unsafe { values(ap, c_long::from(n)) };
        let recv = unsafe { value_of(recv) };
        let args: Vec<RubyValue> = args.iter().map(|v| unsafe { value_of(*v) }).collect();
        to_value(&super::call::call(&recv, mid, &args)?)
    }

    /// `rb_raise(exc, fmt, ...)`'s neighbours are in `error.rs`; this is
    /// `rb_rescue2(body, barg, rescue, rarg, cls1, cls2, ..., 0)`.
    fn rb_rescue2(
        body: unsafe extern "C-unwind" fn(Value) -> Value,
        barg: Value,
        resc: unsafe extern "C-unwind" fn(Value, Value) -> Value,
        rarg: Value;
        ap
    ) -> Value {
        let wanted = unsafe { classes(ap) };
        unsafe { rescue2(body, barg, resc, rarg, &wanted) }
    }

    fn rb_scan_args(argc: c_int, argv: *const Value, fmt: *const c_char; ap) -> c_int {
        unsafe { scan(argc, argv, fmt, ap) }
    }

    /// zeo peels a trailing keyword Hash from the argument list at the
    /// call, so `kw_splat` describes a distinction that is already gone by
    /// the time a C body runs. The rest is `rb_scan_args` exactly.
    fn rb_scan_args_kw(
        _kw_splat: c_int,
        argc: c_int,
        argv: *const Value,
        fmt: *const c_char;
        ap
    ) -> c_int {
        unsafe { scan(argc, argv, fmt, ap) }
    }

    fn rb_struct_define(name: *const c_char; ap) -> Value {
        let members = unsafe { names(ap) };
        unsafe { super::misc::struct_define(0, name, &members) }
    }

    fn rb_struct_define_under(outer: Value, name: *const c_char; ap) -> Value {
        let members = unsafe { names(ap) };
        unsafe { super::misc::struct_define(outer, name, &members) }
    }

    /// `rb_struct_define_without_accessor(name, super, alloc, "a", "b",
    /// NULL)`. The `alloc` argument is MRI's own allocator override, and it
    /// is DROPPED: zeo's Struct allocates through the class, and installing
    /// a C allocator here would run it instead of the one that builds the
    /// members. No gem in the census passes a non-NULL one.
    fn rb_struct_define_without_accessor(
        name: *const c_char,
        super_class: Value,
        _alloc: *const c_void;
        ap
    ) -> Value {
        let members = unsafe { names(ap) };
        unsafe { super::r#final::struct_define_noaccessor(0, name, super_class, &members) }
    }

    fn rb_struct_define_without_accessor_under(
        outer: Value,
        name: *const c_char,
        super_class: Value,
        _alloc: *const c_void;
        ap
    ) -> Value {
        let members = unsafe { names(ap) };
        unsafe { super::r#final::struct_define_noaccessor(outer, name, super_class, &members) }
    }

    /// Unlike the member list, this one is not terminated: MRI reads exactly
    /// as many values as the struct has members, so the struct is asked.
    fn rb_struct_new(klass: Value; ap) -> Value {
        let want = unsafe { super::misc::struct_size(klass)? };
        let vals = unsafe { values(ap, want) };
        unsafe { super::misc::struct_new(klass, &vals) }
    }

    /// `rb_data_define(super, "a", "b", NULL)` -- `Data.define`, whose
    /// member list is NULL-terminated like `rb_struct_define`'s.
    fn rb_data_define(super_class: Value; ap) -> Value {
        let members = unsafe { names(ap) };
        unsafe { super::r#final::data_define(super_class, &members) }
    }

    fn rb_ary_new_from_args(n: c_long; ap) -> Value {
        let items = unsafe { values(ap, n) };
        let items: Vec<RubyValue> = items.iter().map(|v| unsafe { value_of(*v) }).collect();
        to_value(&RubyValue::Array(zeo_rt::value::collections::array_new(items)))
    }

    fn rb_yield_values(n: c_int; ap) -> Value {
        let items = unsafe { values(ap, c_long::from(n)) };
        let items: Vec<RubyValue> = items.iter().map(|v| unsafe { value_of(*v) }).collect();
        to_value(&yield_to_block(&items)?)
    }
}

crate::cext_fn! {
    fn rb_funcall2(recv: Value, mid: Id, argc: c_int, argv: *const Value) -> Value {
        let recv = unsafe { value_of(recv) };
        let args = unsafe { args_of(argc, argv) };
        to_value(&super::call::call(&recv, mid, &args)?)
    }

    fn rb_funcall3(recv: Value, mid: Id, argc: c_int, argv: *const Value) -> Value {
        let recv = unsafe { value_of(recv) };
        let args = unsafe { args_of(argc, argv) };
        to_value(&super::call::call(&recv, mid, &args)?)
    }

    /// No class list means `StandardError`, which `rescue2` reads an empty
    /// list as.
    fn rb_rescue(
        body: unsafe extern "C-unwind" fn(Value) -> Value,
        barg: Value,
        resc: unsafe extern "C-unwind" fn(Value, Value) -> Value,
        rarg: Value,
    ) -> Value {
        unsafe { rescue2(body, barg, resc, rarg, &[]) }
    }

    /// `rb_vrescue2` is `rb_rescue2` with the class list already in a
    /// `va_list`.
    fn rb_vrescue2(
        body: unsafe extern "C-unwind" fn(Value) -> Value,
        barg: Value,
        resc: unsafe extern "C-unwind" fn(Value, Value) -> Value,
        rarg: Value,
        ap: VaList<'_>,
    ) -> Value {
        let mut ap = ap;
        let wanted = unsafe { classes(&mut ap) };
        unsafe { rescue2(body, barg, resc, rarg, &wanted) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `rb_scan_args` through a real variadic call: required, optional and
    /// splat slots, a NULL slot skipped, and the count MRI answers.
    #[test]
    fn scan_args_fills_the_slots_the_format_names() {
        let _scope = super::super::scope::Scope::enter();
        let argv = [value::fixnum(1), value::fixnum(2), value::fixnum(3)];
        let (mut a, mut b, mut c) = (0 as Value, 0 as Value, 0 as Value);
        let n = unsafe {
            rb_scan_args(
                3,
                argv.as_ptr(),
                c"12".as_ptr(),
                &raw mut a,
                &raw mut b,
                &raw mut c,
            )
        };
        assert_eq!((n, a, b, c), (3, argv[0], argv[1], argv[2]));

        let (mut a, mut b) = (0 as Value, 0 as Value);
        let n = unsafe { rb_scan_args(1, argv.as_ptr(), c"11".as_ptr(), &raw mut a, &raw mut b) };
        assert_eq!((n, a, b), (1, argv[0], value::Q_NIL));

        let mut a = 0 as Value;
        let n = unsafe {
            rb_scan_args(
                1,
                argv.as_ptr(),
                c"1".as_ptr(),
                std::ptr::null_mut::<Value>(),
            )
        };
        assert_eq!(n, 1);
        assert_eq!(
            unsafe { rb_scan_args(0, argv.as_ptr(), c"?".as_ptr(), &raw mut a) },
            0
        );
        assert_eq!(a, 0, "an unparsable format touched a slot");
    }
}
