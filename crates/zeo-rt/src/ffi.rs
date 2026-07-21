//! Marshaling helpers for compile-time FFI (#204): convert between `RubyValue`
//! and C scalar types at an `attach_function` boundary.
//!
//! The generated program declares the `extern "C"` symbols itself (fn-locally,
//! with `#[link(name = ..)]`, so no build-step change is needed) and calls them;
//! these helpers keep the emitted per-argument/return conversions small and in
//! one place. The coercions mirror the real `ffi` gem: an integer type wants an
//! Integer, a float wants a Float (or Integer), a `:string` wants a String with
//! no interior NUL. A wrong type is a `TypeError`, exactly as the gem raises.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};

use crate::builtins::{arg_error, type_error};
use crate::dispatch::class_name;
use crate::signal::Signal;
use crate::value::RubyValue;

fn type_err(what: &str, v: &RubyValue) -> Signal {
    let got = class_name(v.class_id()).unwrap_or_else(|| "?".to_string());
    type_error!("cannot convert {got} into an FFI {what}")
}

/// A Ruby value bound to a C integer argument (`:int`/`:long`/`:uintN`/…). The
/// caller narrows the returned `i64` to the exact C width with an `as` cast.
pub fn to_i64(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Int(i) => Ok(*i),
        _ => Err(type_err("integer", v)),
    }
}

/// A Ruby value bound to a C float argument (`:float`/`:double`). An Integer is
/// accepted and widened, matching the gem.
pub fn to_f64(v: &RubyValue) -> Result<f64, Signal> {
    match v {
        RubyValue::Float(f) => Ok(*f),
        RubyValue::Int(i) => Ok(*i as f64),
        _ => Err(type_err("float", v)),
    }
}

/// A Ruby value bound to a `:bool` argument -- Ruby truthiness.
pub fn to_bool(v: &RubyValue) -> bool {
    v.truthy()
}

/// A Ruby String bound to a `:string` (`const char *`) argument: a
/// NUL-terminated copy the caller keeps alive for the duration of the call. An
/// interior NUL is an `ArgumentError`, as the gem raises.
pub fn to_cstring(v: &RubyValue) -> Result<CString, Signal> {
    match v {
        RubyValue::Str(s) => {
            let bytes = s.lock().bytes().to_vec();
            CString::new(bytes).map_err(|_| arg_error!("string contains null byte"))
        }
        _ => Err(type_err("string", v)),
    }
}

/// A Ruby value bound to a `:pointer` argument -> the raw C address it carries.
/// An `FFI::Pointer`/`MemoryPointer` yields its address; `nil` is a NULL
/// pointer, exactly as the gem accepts. Anything else is a `TypeError`.
#[cfg(feature = "ext-ffi")]
pub fn to_pointer(v: &RubyValue) -> Result<*mut c_void, Signal> {
    if let Some(addr) = crate::ext::ffi::address_of(v) {
        return Ok(addr as *mut c_void);
    }
    // An `FFI::Struct` (or anything answering `to_ptr`) passes its own memory,
    // exactly as the gem auto-converts a struct given for a `:pointer` argument.
    if let RubyValue::Object(_) = v {
        if let Ok(p) = crate::dispatch::send_value(v, crate::Symbol::intern("to_ptr"), &[], None) {
            if let Some(addr) = crate::ext::ffi::address_of(&p) {
                return Ok(addr as *mut c_void);
            }
        }
    }
    Err(type_err("pointer", v))
}

/// A `:pointer` C return value -> an `FFI::Pointer` wrapping the address (a NULL
/// pointer wraps to an `FFI::Pointer` whose `#null?` is true, matching the gem).
#[cfg(feature = "ext-ffi")]
pub fn from_pointer(p: *const c_void) -> RubyValue {
    crate::ext::ffi::wrap_address(p as usize)
}

/// A Ruby value bound to an `enum` argument -> the underlying `int`. A Symbol
/// maps through the enum's member table; an Integer passes through unchanged
/// (the gem accepts a raw value); an unknown Symbol is an `ArgumentError`, as
/// the gem raises.
pub fn enum_to_int(v: &RubyValue, members: &[(&str, i64)]) -> Result<i64, Signal> {
    match v {
        RubyValue::Symbol(s) => {
            let name = s.name();
            members
                .iter()
                .find(|(n, _)| *n == name.as_str())
                .map(|(_, i)| *i)
                .ok_or_else(|| arg_error!("invalid enum value, :{name}"))
        }
        RubyValue::Int(i) => Ok(*i),
        _ => Err(type_err("enum", v)),
    }
}

/// An `int` enum return -> its Symbol if the value is a named member, else the
/// raw Integer (the gem's `Enum#from_native` behavior).
pub fn int_to_enum(i: i64, members: &[(&str, i64)]) -> RubyValue {
    members
        .iter()
        .find(|(_, v)| *v == i)
        .map(|(n, _)| RubyValue::Symbol(crate::Symbol::intern(n)))
        .unwrap_or(RubyValue::Int(i))
}

pub fn from_i64(i: i64) -> RubyValue {
    RubyValue::Int(i)
}

pub fn from_f64(f: f64) -> RubyValue {
    RubyValue::Float(f)
}

pub fn from_bool(b: bool) -> RubyValue {
    RubyValue::Bool(b)
}

/// Read a `:string` return value (a NUL-terminated C string) back into a Ruby
/// String; a NULL pointer becomes `nil`, matching the gem.
///
/// # Safety
/// `p` must be NULL or a valid pointer to a NUL-terminated string that outlives
/// this read -- the same contract the `ffi` gem places on a `:string` return.
pub unsafe fn from_cstr(p: *const c_char) -> RubyValue {
    unsafe {
        if p.is_null() {
            return RubyValue::Nil;
        }
        let bytes = CStr::from_ptr(p).to_bytes().to_vec();
        RubyValue::Str(crate::string_from_bytes(
            bytes,
            crate::encoding::default_external(),
        ))
    }
}
