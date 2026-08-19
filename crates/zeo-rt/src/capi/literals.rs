//! Literal construction: strings (frozen-interned and fresh), symbols,
//! arrays, hashes, ranges.

use super::dispatch::status_out;
use crate::encoding::{EncodingId, StrBuf};
use crate::{RubyValue, Symbol};

/// A frozen, interned string literal -- one object per `(bytes, encoding)`
/// process-wide, exactly the `LitPool` identity rule.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_str_lit(ptr: *const u8, len: usize, enc: u8, out: *mut RubyValue) {
    let bytes = unsafe { super::byte_slice(ptr, len) }.to_vec();
    let buf = StrBuf::from_bytes(bytes, EncodingId(enc));
    let v = RubyValue::Str(crate::value::collections::intern_frozen(buf));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A FRESH, mutable string from literal bytes -- every evaluation of a
/// non-frozen literal allocates, as ruby's do.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_str_new(ptr: *const u8, len: usize, enc: u8, out: *mut RubyValue) {
    let bytes = unsafe { super::byte_slice(ptr, len) }.to_vec();
    let buf = StrBuf::from_bytes(bytes, EncodingId(enc));
    let v = RubyValue::Str(std::sync::Arc::new(crate::collections::Freezable::new(buf)));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Intern a symbol name -- `zeo_unit_init` fills the program's `zeo_syms`
/// table through this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_sym_intern(ptr: *const u8, len: usize) -> u32 {
    Symbol::intern(unsafe { super::str_slice(ptr, len) }).to_u32()
}

/// A `Symbol` value from its interned id.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_sym_value(id: u32, out: *mut RubyValue) {
    unsafe { out.write(RubyValue::Symbol(Symbol::from_u32(id))) };
}

/// A fresh array with room for `cap` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_new(cap: usize, out: *mut RubyValue) {
    let v = RubyValue::Array(crate::value::collections::array_new(Vec::with_capacity(
        cap,
    )));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Push the value moved from `v` -- literal construction's append (the
/// receiver is the fresh, unfrozen literal, so no frozen check).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_push(a: *const RubyValue, v: *mut RubyValue) {
    super::leakcheck::consumed(unsafe { &*v });
    let value = unsafe { std::ptr::read(v) };
    match unsafe { &*a } {
        RubyValue::Array(arr) => {
            crate::array_push(arr, value);
        }
        other => panic!("zeo_rt_array_push on a non-array: {other:?}"),
    }
}

/// The array's length (a live view, for fused iteration).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_len(a: *const RubyValue) -> usize {
    match unsafe { &*a } {
        RubyValue::Array(arr) => arr.lock().len(),
        other => panic!("zeo_rt_array_len on a non-array: {other:?}"),
    }
}

/// One element, cloned out (`nil` past the end -- Ruby's `[]`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_get(a: *const RubyValue, i: usize, out: *mut RubyValue) {
    let v = match unsafe { &*a } {
        RubyValue::Array(arr) => arr.lock().get(i).cloned().unwrap_or(RubyValue::Nil),
        other => panic!("zeo_rt_array_get on a non-array: {other:?}"),
    };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A fresh empty hash.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_hash_new(out: *mut RubyValue) {
    let v = RubyValue::Hash(crate::value::collections::hash_new(Vec::new()));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Store `k => v` (both moved) -- literal construction's insert.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_hash_set(
    h: *const RubyValue,
    k: *mut RubyValue,
    v: *mut RubyValue,
) {
    super::leakcheck::consumed(unsafe { &*k });
    super::leakcheck::consumed(unsafe { &*v });
    let key = unsafe { std::ptr::read(k) };
    let value = unsafe { std::ptr::read(v) };
    match unsafe { &*h } {
        RubyValue::Hash(hash) => {
            crate::value::collections::hash_set(hash, key, value);
        }
        other => panic!("zeo_rt_hash_set on a non-hash: {other:?}"),
    }
}

/// A range literal (`a..b` / `a...b`); endpoints moved (nil = open). The
/// comparability check can raise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_range_new(
    begin: *mut RubyValue,
    end: *mut RubyValue,
    exclusive: i8,
    out: *mut RubyValue,
) -> i32 {
    let ep = |p: *mut RubyValue| {
        if p.is_null() {
            None
        } else {
            super::leakcheck::consumed(unsafe { &*p });
            match unsafe { std::ptr::read(p) } {
                RubyValue::Nil => None,
                v => Some(v),
            }
        }
    };
    status_out(
        crate::value::range_checked(ep(begin), ep(end), exclusive != 0),
        out,
    )
}

/// A multiple assignment's split: destructure `value` against a
/// `before/splat/after` target shape into `out`'s
/// `n_before + has_splat + n_after` slots (each OWNED; the splat slot, when
/// present, gets a fresh Array). `value` coerces exactly as the rustc
/// emission does -- an Array destructures directly, anything else goes
/// through the `to_ary` rule ([`crate::block_auto_splat`], which can
/// raise). Slots are nil-filled BEFORE the coercion, so an error path
/// releases safely.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_multi_split(
    value: *const RubyValue,
    n_before: usize,
    has_splat: u8,
    n_after: usize,
    out: *mut RubyValue,
) -> i32 {
    use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};
    let has_splat = has_splat != 0;
    let n_out = n_before + usize::from(has_splat) + n_after;
    for i in 0..n_out {
        unsafe { out.add(i).write(RubyValue::Nil) };
    }
    let v = unsafe { &*value };
    let elems: Vec<RubyValue> = match v {
        RubyValue::Array(a) => a.lock().iter().cloned().collect(),
        other => match crate::block_auto_splat(std::slice::from_ref(other)) {
            Ok(cow) => cow.into_owned(),
            Err(sig) => {
                crate::signal::set_pending(sig);
                return STATUS_SIGNAL;
            }
        },
    };
    let (before, splat, after) =
        crate::value::collections::multi_assign(&elems, n_before, has_splat, n_after);
    let mut s = 0usize;
    let mut put = |v: RubyValue| {
        super::leakcheck::created(&v);
        unsafe { out.add(s).write(v) };
        s += 1;
    };
    for v in before {
        put(v);
    }
    if has_splat {
        put(RubyValue::Array(crate::value::collections::array_new(
            splat,
        )));
    }
    for v in after {
        put(v);
    }
    debug_assert_eq!(s, n_out);
    STATUS_OK
}
