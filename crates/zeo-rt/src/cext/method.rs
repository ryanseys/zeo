//! `rb_define_*`: the entry an extension's `Init_` actually uses.
//!
//! `rb_define_method` is the second most-called function in the census (695
//! uses, behind only `rb_intern`), because it is how every extension puts
//! itself into the world. What lands here is a raw C function pointer and an
//! `argc` code; what zeo's dispatch wants is an `RProc`. The bridge is a
//! trampoline, and there is exactly one, in [`method_proc`].
//!
//! # The three argc conventions
//!
//! MRI's `argc` argument is not an arity, it is a shape:
//!
//! | `argc` | The C function is |
//! |---|---|
//! | `0..=15` | `VALUE (*)(VALUE self, VALUE a1, ..., VALUE aN)` |
//! | `-1` | `VALUE (*)(int argc, VALUE *argv, VALUE self)` |
//! | `-2` | `VALUE (*)(VALUE self, VALUE args_array)` |
//!
//! All three are here. The fixed-arity form is a `match` over sixteen arms
//! because each one is a different function type, and transmuting to the
//! widest and passing garbage for the rest would be a real ABI violation
//! rather than the harmless one the stubs make -- these functions RUN, and
//! read every argument they declare.
//!
//! # What wraps every call
//!
//! A trampoline is a Ruby-to-C boundary, so it does what every one of them
//! does: push a [`super::scope::Scope`] so the arguments' handles outlive the
//! call, and run inside [`super::jmp::protect`] so an `rb_raise` from deep
//! inside the extension comes back as an `Err` instead of flying past Rust
//! frames.

use super::convert::{to_value, value_of};
use super::scope::Scope;
use super::symbol::{Id, symbol_of};
use super::value::{self, Value};
use crate::dispatch::ClassId;
use crate::{RProc, RubyValue, Signal, Symbol};
use std::ffi::{c_char, c_int, c_void};

/// The widest fixed-arity shape MRI accepts.
const MAX_FIXED_ARITY: c_int = 15;
/// `VALUE (*)(int argc, VALUE *argv, VALUE self)`.
const ARGC_VARIADIC: c_int = -1;
/// `VALUE (*)(VALUE self, VALUE args)`.
const ARGC_ARRAY: c_int = -2;

/// An untyped C method body. The real type is decided by `argc`, and
/// [`call_c`] is the only place it is recovered.
pub type MethodPtr = *const c_void;

/// # Safety
///
/// `p` must be a NUL-terminated C string.
unsafe fn cname(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract.
    unsafe { String::from_utf8_lossy(std::ffi::CStr::from_ptr(p).to_bytes()).into_owned() }
}

/// Call `f` with `args` in whichever shape `argc` names.
///
/// # Safety
///
/// `f` must be a C function of exactly the shape `argc` describes, and every
/// `VALUE` in `args` must be live. Both come from the extension's own
/// `rb_define_method` call.
unsafe fn call_c(f: MethodPtr, argc: c_int, this: Value, args: &[Value]) -> Value {
    /// One `Value` per repetition, discarding the name.
    macro_rules! arg_ty {
        ($_name:ident) => {
            Value
        };
    }
    macro_rules! fixed {
        ($($n:literal => ($($a:ident),*),)*) => {
            match argc {
                $($n => {
                    // SAFETY: the caller's contract -- `argc` IS the shape.
                    // One `Value` parameter per name in the arm's list;
                    // `arg_ty!` discards the name and keeps the count.
                    let f: unsafe extern "C" fn(Value $(, arg_ty!($a))*) -> Value =
                        unsafe { std::mem::transmute(f) };
                    let [$($a),*] = <[Value; $n]>::try_from(args)
                        .expect("the arity guard already counted these");
                    unsafe { f(this $(, $a)*) }
                })*
                _ => unreachable!("call_c reached with an unhandled argc {argc}"),
            }
        };
    }
    if argc == ARGC_VARIADIC {
        // SAFETY: the caller's contract.
        let f: unsafe extern "C" fn(c_int, *const Value, Value) -> Value =
            unsafe { std::mem::transmute(f) };
        return unsafe { f(args.len() as c_int, args.as_ptr(), this) };
    }
    fixed! {
        0 => (),
        1 => (a),
        2 => (a, b),
        3 => (a, b, c),
        4 => (a, b, c, d),
        5 => (a, b, c, d, e),
        6 => (a, b, c, d, e, f6),
        7 => (a, b, c, d, e, f6, g),
        8 => (a, b, c, d, e, f6, g, h),
        9 => (a, b, c, d, e, f6, g, h, i),
        10 => (a, b, c, d, e, f6, g, h, i, j),
        11 => (a, b, c, d, e, f6, g, h, i, j, k),
        12 => (a, b, c, d, e, f6, g, h, i, j, k, l),
        13 => (a, b, c, d, e, f6, g, h, i, j, k, l, m),
        14 => (a, b, c, d, e, f6, g, h, i, j, k, l, m, n),
        15 => (a, b, c, d, e, f6, g, h, i, j, k, l, m, n, o),
    }
}

/// The three shapes MRI accepts, and nothing else. A predicate rather than a
/// `Result`: building the `ArgumentError` needs the class registry, and this
/// is the half that can be checked without one.
fn legal_argc(argc: c_int) -> bool {
    (ARGC_ARRAY..=MAX_FIXED_ARITY).contains(&argc)
}

fn bad_argc(argc: c_int) -> Signal {
    crate::dispatch::raise_error(
        "ArgumentError",
        format!("arity out of range: {argc} for -2..{MAX_FIXED_ARITY}"),
    )
}

/// The `RProc` zeo's dispatch installs for a C method body.
///
/// # Safety
///
/// `f` must be a C function of the shape `argc` names, and must live as long
/// as the process -- which is true of every function in a loaded extension.
pub unsafe fn method_proc(f: MethodPtr, argc: c_int) -> Result<RProc, Signal> {
    if !legal_argc(argc) {
        return Err(bad_argc(argc));
    }
    // `usize` rather than the pointer, so the closure is `Send + Sync`. The
    // function lives in the extension's image for the life of the process.
    let addr = f as usize;
    Ok(crate::rproc::ProcBuilder::from_rust(
        move |recv, args, block| {
            let scope = Scope::enter();
            // `rb_yield` reads the FRAME's block, and this trampoline IS the
            // C method's frame.
            let _block = super::call::BlockFrame::enter(block.clone());
            let this = to_value(recv)?;
            let raw: Vec<Value> = args.iter().map(to_value).collect::<Result<_, _>>()?;

            // MRI checks the count itself for a fixed-arity body; the C
            // function reads exactly `argc` arguments and reading a missing
            // one is what a wrong count would do.
            if argc >= 0 && raw.len() != argc as usize {
                return Err(crate::dispatch::wrong_arity(raw.len(), &argc.to_string()));
            }

            let (fixed, packed);
            let (argc, args) = if argc == ARGC_ARRAY {
                packed = [to_value(&RubyValue::Array(
                    crate::value::collections::array_new(args.to_vec()),
                ))?];
                (1, &packed[..])
            } else {
                fixed = raw;
                (argc, &fixed[..])
            };

            let out = super::jmp::protect(|| {
                // SAFETY: the caller of `method_proc` promised the shape, and
                // every `VALUE` here was pinned two statements ago.
                unsafe { call_c(addr as MethodPtr, argc, this, args) }
            })?;
            // The answer has to outlive this scope's pop, and the pop is the
            // `drop(scope)` on the next line.
            scope.keep(out);
            let answer = unsafe { value_of(out) };
            drop(scope);
            Ok(answer)
        },
        RubyValue::Nil,
        argc,
        true,
    )
    .build())
}

/// # Safety
///
/// `v` must be a live `VALUE` naming a Class or Module.
unsafe fn as_class(v: Value) -> Result<ClassId, Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Class(cid) => Ok(cid),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "wrong argument type {} (expected Class/Module)",
                crate::dispatch::class_name(other.class_id()).unwrap_or("Object".into())
            ),
        )),
    }
}

fn install(owner: ClassId, name: Symbol, body: RProc) -> Result<(), Signal> {
    crate::runtime_meta::runtime_define_method(owner, name, body)?;
    Ok(())
}

/// Mint a class and give it a name and a home, the way `rb_define_class_id_under`
/// does: the constant is what makes it findable, and the name is what
/// `#inspect` prints.
fn new_class(outer: ClassId, name: &str, superclass: Option<RubyValue>) -> Result<Value, Signal> {
    let cls = crate::runtime_meta::runtime_class_new(superclass, None)?;
    if let RubyValue::Class(cid) = &cls {
        let full = qualified(outer, name);
        crate::runtime_meta::name_runtime_class_if_anonymous(*cid, &full);
        crate::constants::const_set(outer.0, name, cls.clone());
    }
    to_value(&cls)
}

fn qualified(outer: ClassId, name: &str) -> String {
    match crate::dispatch::class_name(outer) {
        Some(o) if outer != zeo_abi::OBJECT_CLASS => format!("{o}::{name}"),
        _ => name.to_string(),
    }
}

crate::cext_fn! {
    /// `rb_define_class(name, super)` -- a top-level class, so its home is
    /// `Object`, which is where a top-level constant lives in ruby too.
    fn rb_define_class(name: *const c_char, superclass: Value) -> Value {
        let name = unsafe { cname(name) };
        let sup = unsafe { value_of(superclass) };
        new_class(zeo_abi::OBJECT_CLASS, &name, Some(sup))
    }

    fn rb_define_class_under(outer: Value, name: *const c_char, superclass: Value) -> Value {
        let outer = unsafe { as_class(outer)? };
        let name = unsafe { cname(name) };
        let sup = unsafe { value_of(superclass) };
        new_class(outer, &name, Some(sup))
    }

    fn rb_define_module(name: *const c_char) -> Value {
        let name = unsafe { cname(name) };
        new_module(zeo_abi::OBJECT_CLASS, &name)
    }

    fn rb_define_module_under(outer: Value, name: *const c_char) -> Value {
        let outer = unsafe { as_class(outer)? };
        let name = unsafe { cname(name) };
        new_module(outer, &name)
    }

    fn rb_define_method(klass: Value, name: *const c_char, f: MethodPtr, argc: c_int) -> () {
        let owner = unsafe { as_class(klass)? };
        let name = Symbol::intern(&unsafe { cname(name) });
        install(owner, name, unsafe { method_proc(f, argc)? })
    }

    fn rb_define_private_method(klass: Value, name: *const c_char, f: MethodPtr, argc: c_int) -> () {
        let owner = unsafe { as_class(klass)? };
        let sym = Symbol::intern(&unsafe { cname(name) });
        install(owner, sym, unsafe { method_proc(f, argc)? })?;
        crate::runtime_meta::runtime_set_visibility(
            owner,
            &[RubyValue::Symbol(sym)],
            crate::dispatch::MethodVisibility::Private,
        )?;
        Ok(())
    }

    fn rb_define_protected_method(klass: Value, name: *const c_char, f: MethodPtr, argc: c_int) -> () {
        let owner = unsafe { as_class(klass)? };
        let sym = Symbol::intern(&unsafe { cname(name) });
        install(owner, sym, unsafe { method_proc(f, argc)? })?;
        crate::runtime_meta::runtime_set_visibility(
            owner,
            &[RubyValue::Symbol(sym)],
            crate::dispatch::MethodVisibility::Protected,
        )?;
        Ok(())
    }

    fn rb_define_singleton_method(obj: Value, name: *const c_char, f: MethodPtr, argc: c_int) -> () {
        let recv = unsafe { value_of(obj) };
        let sym = Symbol::intern(&unsafe { cname(name) });
        crate::runtime_meta::runtime_define_singleton_method(
            &recv,
            sym,
            unsafe { method_proc(f, argc)? },
        )?;
        Ok(())
    }

    /// `rb_define_alias(klass, new, old)`. Note the argument order: MRI puts
    /// the NEW name first, the opposite of `alias_method`'s C spelling in
    /// several other engines.
    fn rb_define_alias(klass: Value, new: *const c_char, old: *const c_char) -> () {
        let owner = unsafe { as_class(klass)? };
        let new = Symbol::intern(&unsafe { cname(new) });
        let old = Symbol::intern(&unsafe { cname(old) });
        crate::runtime_meta::runtime_alias_method(owner, new, old)?;
        Ok(())
    }

    fn rb_define_const(klass: Value, name: *const c_char, val: Value) -> () {
        let owner = unsafe { as_class(klass)? };
        let name = unsafe { cname(name) };
        crate::constants::const_set(owner.0, &name, unsafe { value_of(val) });
        Ok(())
    }

    /// An extension writes `rb_ivar_set(self, rb_intern("@x"), v)`, so the
    /// `ID` already carries the `@`. `instance_variable_get` wants the same
    /// spelling, so the symbol passes straight through.
    fn rb_ivar_get(obj: Value, id: Id) -> Value {
        let recv = unsafe { value_of(obj) };
        let name = RubyValue::Symbol(symbol_of(id));
        to_value(&crate::dispatch::instance_variable_get(&recv, &name)?)
    }

    fn rb_ivar_set(obj: Value, id: Id, val: Value) -> Value {
        let recv = unsafe { value_of(obj) };
        let name = RubyValue::Symbol(symbol_of(id));
        crate::dispatch::instance_variable_set(&recv, &name, unsafe { value_of(val) })?;
        Ok(val)
    }
}

fn new_module(outer: ClassId, name: &str) -> Result<Value, Signal> {
    let m = crate::runtime_meta::runtime_module_new(None)?;
    if let RubyValue::Class(cid) = &m {
        let full = qualified(outer, name);
        crate::runtime_meta::name_runtime_class_if_anonymous(*cid, &full);
        crate::constants::const_set(outer.0, name, m.clone());
    }
    to_value(&m)
}

/// `Qnil`'s spelling, for the `-> ()` entries above whose C prototype is
/// `void`.
const _: () = assert!(value::Q_NIL == 0x04);

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn zero_args(_self: Value) -> Value {
        value::fixnum(100)
    }
    unsafe extern "C" fn two_args(_self: Value, a: Value, b: Value) -> Value {
        value::fixnum(value::fixnum_value(a) + value::fixnum_value(b))
    }
    unsafe extern "C" fn variadic(argc: c_int, argv: *const Value, _self: Value) -> Value {
        let mut sum = 0;
        for i in 0..argc {
            // SAFETY: the caller passed `argc` readable slots.
            sum += value::fixnum_value(unsafe { argv.offset(i as isize).read() });
        }
        value::fixnum(sum)
    }

    /// Each fixed arity is a different C function type, so the transmute has
    /// to pick the right one. Passing the widest and hoping is what this
    /// arm-per-arity `match` exists to avoid.
    #[test]
    fn each_argc_shape_reaches_the_c_function() {
        let _scope = Scope::enter();
        let this = value::Q_NIL;
        assert_eq!(
            unsafe { call_c(zero_args as MethodPtr, 0, this, &[]) },
            value::fixnum(100)
        );
        assert_eq!(
            unsafe {
                call_c(
                    two_args as MethodPtr,
                    2,
                    this,
                    &[value::fixnum(2), value::fixnum(3)],
                )
            },
            value::fixnum(5)
        );
        assert_eq!(
            unsafe {
                call_c(
                    variadic as MethodPtr,
                    ARGC_VARIADIC,
                    this,
                    &[value::fixnum(1), value::fixnum(2), value::fixnum(4)],
                )
            },
            value::fixnum(7)
        );
    }

    #[test]
    fn an_argc_outside_the_three_shapes_refuses() {
        assert!(!legal_argc(16), "16 is past MRI's widest fixed arity");
        assert!(!legal_argc(-3), "-3 is not one of the two negative shapes");
        for argc in [ARGC_ARRAY, ARGC_VARIADIC, 0, MAX_FIXED_ARITY] {
            assert!(legal_argc(argc), "argc {argc} was refused");
            assert!(unsafe { method_proc(zero_args as MethodPtr, argc) }.is_ok());
        }
    }

    /// A trampoline is a Ruby-to-C boundary, so it must release the handles
    /// it pinned for the arguments -- otherwise every call to a C method
    /// leaks one per argument, for the life of the process.
    #[test]
    fn a_call_releases_the_handles_it_pinned_for_its_arguments() {
        let before = super::super::handles::live_count();
        let p = unsafe { method_proc(two_args as MethodPtr, 2) }.expect("argc 2 is legal");
        let out = p.call(&[RubyValue::Int(20), RubyValue::Int(22)]);
        assert!(matches!(out, Ok(RubyValue::Int(42))), "got {out:?}");
        assert_eq!(
            super::super::handles::live_count(),
            before,
            "the trampoline leaked a handle"
        );
    }

    /// A fixed-arity C body reads exactly `argc` slots. Handing it fewer is
    /// what a missing arity check would turn into a read of whatever was in
    /// the register, so the count is checked before the call.
    ///
    /// `raise_error` panics with the message registry-less, which is its
    /// documented unit-test behaviour and what makes the refusal visible.
    #[test]
    #[should_panic(expected = "ArgumentError: wrong number of arguments")]
    fn a_wrong_argument_count_raises_rather_than_reading_a_missing_slot() {
        let p = unsafe { method_proc(two_args as MethodPtr, 2) }.expect("argc 2 is legal");
        let _ = p.call(&[RubyValue::Int(1)]);
    }
}
