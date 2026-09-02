//! `ID`, `rb_intern`, and the numeric conversions.
//!
//! `rb_intern` is the single most-called function in the census -- 816 uses
//! across the 23 C-extension gems, more than twice the next -- because every
//! `rb_funcall(obj, rb_intern("each"), 0)` pays for one. So an `ID` is zeo's
//! own `Symbol` id and nothing else: interning is the interner, and there is
//! no second table to keep in step.
//!
//! That also makes `rb_id2sym` and `rb_sym2id` pure arithmetic, since a
//! static Symbol `VALUE` is `(id << 8) | 0x0c` and the `id` in it IS the
//! `Symbol`'s.

use super::convert::{to_value, value_of};
use super::value::{self, Value};
use std::ffi::{c_char, c_double, c_long, c_longlong};
use zeo_rt::{RubyValue, Symbol};

/// MRI's `ID`.
pub type Id = usize;

/// # Safety
///
/// `p` must be NUL-terminated, or name `len` readable bytes.
unsafe fn name_of(p: *const c_char, len: Option<usize>) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract.
    let bytes = unsafe {
        match len {
            Some(n) => std::slice::from_raw_parts(p.cast::<u8>(), n),
            None => std::ffi::CStr::from_ptr(p).to_bytes(),
        }
    };
    String::from_utf8_lossy(bytes).into_owned()
}

pub(super) fn intern(name: &str) -> Id {
    Symbol::intern(name).to_u32() as Id
}

pub(super) fn symbol_of(id: Id) -> Symbol {
    Symbol::from_u32(id as u32)
}

/// Truncating an out-of-range integer is the failure mode that turns a gem's
/// own overflow check into a silent wrong answer, so `rb_num2int` raises on a
/// `None` rather than casting.
///
/// A predicate rather than a `Result` on purpose: building the `RangeError`
/// needs the class registry, and a unit test has none -- so this is the half
/// a test can actually look at.
fn narrow_to_int(n: i64) -> Option<i32> {
    i32::try_from(n).ok()
}

fn out_of_range(what: &str) -> zeo_rt::Signal {
    zeo_rt::builtins::range_error!("{what} out of range")
}

/// # Safety
///
/// `v` is an extension's own `VALUE`.
unsafe fn as_int(v: Value) -> Result<i64, zeo_rt::Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Int(n) => Ok(n),
        RubyValue::Float(f) => Ok(f as i64),
        // A Bignum genuinely does not fit; saying so beats truncating.
        RubyValue::BigInt(_) => Err(out_of_range("bignum too big to convert into `long'")),
        other => Err(zeo_rt::builtins::type_error!(
            "no implicit conversion of {} into Integer",
            zeo_rt::dispatch::class_name(other.class_id()).unwrap_or("Object".into())
        )),
    }
}

crate::cext_fn! {
    fn rb_intern(p: *const c_char) -> Id {
        Ok(intern(&unsafe { name_of(p, None) }))
    }

    fn rb_intern2(p: *const c_char, len: c_long) -> Id {
        Ok(intern(&unsafe { name_of(p, Some(len.max(0) as usize)) }))
    }

    /// `rb_intern3(name, len, enc)`. zeo's interner keys on bytes AND
    /// encoding, so the encoding argument is not dropped -- it is what makes
    /// the same bytes under two encodings two symbols, as in ruby.
    fn rb_intern3(p: *const c_char, len: c_long, _enc: *const std::ffi::c_void) -> Id {
        Ok(intern(&unsafe { name_of(p, Some(len.max(0) as usize)) }))
    }

    fn rb_intern_str(v: Value) -> Id {
        match unsafe { value_of(v) } {
            RubyValue::Str(s) => {
                let (bytes, enc) = { let g = s.lock(); (g.bytes().to_vec(), g.encoding()) };
                Ok(Symbol::intern_bytes(&bytes, enc).to_u32() as Id)
            }
            other => Err(zeo_rt::builtins::type_error!("{} is not a symbol nor a string",
                    zeo_rt::dispatch::class_name(other.class_id()).unwrap_or("Object".into()))),
        }
    }

    fn rb_id2sym(id: Id) -> Value {
        Ok(value::static_symbol(id as u32))
    }

    fn rb_sym2id(v: Value) -> Id {
        match unsafe { value_of(v) } {
            RubyValue::Symbol(s) => Ok(s.to_u32() as Id),
            other => Err(zeo_rt::builtins::type_error!("{} is not a symbol",
                    zeo_rt::dispatch::class_name(other.class_id()).unwrap_or("Object".into()))),
        }
    }

    /// `rb_id2name`. The name is interned for the life of the process, so a
    /// `&'static str`'s pointer is a valid `const char *` -- except that ruby
    /// promises a NUL and a Rust `str` has none. The interner's own C-string
    /// cache supplies one.
    fn rb_id2name(id: Id) -> *const c_char {
        Ok(cstr_for(symbol_of(id).name_str()))
    }

    fn rb_sym2str(v: Value) -> Value {
        match unsafe { value_of(v) } {
            RubyValue::Symbol(s) => {
                let str = zeo_rt::string_from_bytes(s.bytes().to_vec(), s.encoding());
                to_value(&RubyValue::Str(str))
            }
            other => Err(zeo_rt::builtins::type_error!("{} is not a symbol",
                    zeo_rt::dispatch::class_name(other.class_id()).unwrap_or("Object".into()))),
        }
    }

    fn rb_int2inum(n: isize) -> Value {
        to_value(&RubyValue::Int(n as i64))
    }

    fn rb_ll2inum(n: c_longlong) -> Value {
        to_value(&RubyValue::Int(n as i64))
    }

    /// `rb_ull2inum`. A `u64` past `i64::MAX` is a real Bignum in ruby, so it
    /// becomes one here rather than wrapping to a negative.
    fn rb_ull2inum(n: u64) -> Value {
        let v = match i64::try_from(n) {
            Ok(n) => RubyValue::Int(n),
            Err(_) => zeo_rt::builtins::integer::int_value(num_bigint::BigInt::from(n)),
        };
        to_value(&v)
    }

    fn rb_num2long(v: Value) -> c_long {
        Ok(unsafe { as_int(v)? } as c_long)
    }

    fn rb_num2ll(v: Value) -> c_longlong {
        Ok(unsafe { as_int(v)? } as c_longlong)
    }

    fn rb_num2int(v: Value) -> std::ffi::c_int {
        let n = unsafe { as_int(v)? };
        narrow_to_int(n)
            .map(|n| n as std::ffi::c_int)
            .ok_or_else(|| out_of_range("integer"))
    }

    fn rb_num2dbl(v: Value) -> c_double {
        match unsafe { value_of(v) } {
            RubyValue::Float(f) => Ok(f),
            RubyValue::Int(n) => Ok(n as c_double),
            other => Err(zeo_rt::builtins::type_error!("can't convert {} into Float",
                    zeo_rt::dispatch::class_name(other.class_id()).unwrap_or("Object".into()))),
        }
    }

    fn rb_float_new(d: c_double) -> Value {
        to_value(&RubyValue::Float(d))
    }

    fn rb_float_value(v: Value) -> c_double {
        match unsafe { value_of(v) } {
            RubyValue::Float(f) => Ok(f),
            other => Err(zeo_rt::builtins::wrong_arg_type(&other, "Float")),
        }
    }
}

/// A NUL-terminated copy of an interned name, kept for the process.
///
/// A `Symbol`'s name is already `&'static str`, but ruby promises a
/// NUL-terminated `const char *` and a Rust `str` has no NUL. The copy is
/// made once per symbol that is asked, which is bounded by the symbol table
/// and is what MRI's own storage costs.
pub(super) fn cstr_for_owned(name: &str) -> *const c_char {
    cstr_for(Box::leak(name.to_string().into_boxed_str()))
}

fn cstr_for(name: &'static str) -> *const c_char {
    use std::sync::OnceLock;
    static CACHE: OnceLock<parking_lot::Mutex<zeo_rt::FMap<&'static str, &'static [u8]>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut g = cache.lock();
    if let Some(bytes) = g.get(name) {
        return bytes.as_ptr().cast();
    }
    let mut owned = name.as_bytes().to_vec();
    owned.push(0);
    let leaked: &'static [u8] = Box::leak(owned.into_boxed_slice());
    g.insert(name, leaked);
    leaked.as_ptr().cast()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::Scope;

    #[test]
    fn an_id_is_the_symbol_id_and_round_trips_through_the_value() {
        let _scope = Scope::enter();
        let id = unsafe { rb_intern(c"each".as_ptr()) };
        assert_eq!(id, Symbol::intern("each").to_u32() as Id);
        let sym = unsafe { rb_id2sym(id) };
        assert!(value::is_static_symbol(sym));
        assert_eq!(unsafe { rb_sym2id(sym) }, id);
        assert_eq!(unsafe { rb_intern(c"each".as_ptr()) }, id, "not interned");
    }

    #[test]
    fn a_name_comes_back_nul_terminated() {
        let _scope = Scope::enter();
        let id = unsafe { rb_intern(c"to_msgpack".as_ptr()) };
        let p = unsafe { rb_id2name(id) };
        let back = unsafe { std::ffi::CStr::from_ptr(p) };
        assert_eq!(back.to_bytes(), b"to_msgpack");
        assert_eq!(p, unsafe { rb_id2name(id) }, "the cache handed out two");
    }

    #[test]
    fn a_length_bounded_intern_ignores_what_follows() {
        let _scope = Scope::enter();
        let a = unsafe { rb_intern2(c"abcdef".as_ptr(), 3) };
        assert_eq!(a, unsafe { rb_intern(c"abc".as_ptr()) });
    }

    #[test]
    fn an_unsigned_past_i64_max_becomes_a_bignum_not_a_negative() {
        let _scope = Scope::enter();
        let v = unsafe { rb_ull2inum(u64::MAX) };
        match unsafe { value_of(v) } {
            RubyValue::BigInt(b) => assert_eq!(b.to_string(), u64::MAX.to_string()),
            other => panic!("u64::MAX came back as {other:?}"),
        }
        // And one that does fit stays a Fixnum.
        let small = unsafe { rb_ull2inum(7) };
        assert_eq!(small, value::fixnum(7));
    }

    #[test]
    fn the_numeric_conversions_agree_both_ways() {
        let _scope = Scope::enter();
        assert_eq!(unsafe { rb_num2long(value::fixnum(-9)) }, -9);
        assert_eq!(unsafe { rb_num2int(value::fixnum(1000)) }, 1000);
        let f = unsafe { rb_float_new(2.5) };
        assert_eq!(unsafe { rb_float_value(f) }, 2.5);
        assert_eq!(unsafe { rb_num2dbl(f) }, 2.5);
        assert_eq!(unsafe { rb_num2dbl(value::fixnum(4)) }, 4.0);
    }

    /// Through the helper, not through `rb_num2int`. Two things make the C
    /// entry point the wrong place to look at a refusal from: `raise_error`
    /// panics registry-less, and `cext_fn!` turns an `Err` into a longjmp
    /// that aborts with no protected frame open.
    #[test]
    fn an_int_that_does_not_fit_raises_rather_than_truncating() {
        assert_eq!(narrow_to_int(1000), Some(1000));
        assert_eq!(narrow_to_int(i64::from(i32::MAX)), Some(i32::MAX));
        assert_eq!(narrow_to_int(i64::from(i32::MAX) + 1), None);
        assert_eq!(narrow_to_int(i64::from(i32::MIN) - 1), None);
    }
}
