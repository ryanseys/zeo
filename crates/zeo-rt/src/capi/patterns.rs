//! `case/in` pattern matching: the deconstruct protocol, the element and
//! key accessors a lowered pattern reads through, and the failure
//! recorders whose messages `NoMatchingPatternError` renders.
//!
//! The control flow itself is emitted (a chain of checks with one failure
//! landing per pattern); everything here is a thin wrapper over the
//! runtime's own pattern helpers, so the two cannot drift.

use super::dispatch::status_out;
use crate::{RubyValue, Symbol};
use zeo_abi::ClassId;

/// `#deconstruct`'s array-pattern protocol: a runtime Array is itself, a
/// value that answers `#deconstruct` is asked, and anything else simply
/// does not match (no raise) after recording why.
///
/// `matched` is written 1/0; the status is the ordinary signal protocol
/// (a raising `#deconstruct` propagates).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_deconstruct(
    v: *const RubyValue,
    out: *mut RubyValue,
    matched: *mut u8,
) -> i32 {
    let recv = unsafe { &*v };
    let dec = Symbol::intern("deconstruct");
    match recv {
        RubyValue::Array(_) => {
            unsafe { matched.write(1) };
            status_out(Ok(recv.clone()), out)
        }
        other if crate::dispatch::responds_to(other.class_id(), dec, false) => {
            match crate::dispatch::send_value(other, dec, &[], None) {
                // The protocol's return value is CHECKED here, where it
                // arrives. Written through unconditionally, `matched = 1`
                // sent a non-Array on to `zeo_rt_pat_array_len`, whose
                // `as_array_ref` panics -- and this is an `extern "C"`
                // boundary, so the panic ended the process instead of
                // raising. CRuby raises rather than falling through to the
                // next `in` clause.
                Ok(RubyValue::Array(a)) => {
                    unsafe { matched.write(1) };
                    status_out(Ok(RubyValue::Array(a)), out)
                }
                Ok(_) => {
                    crate::signal::set_pending(crate::dispatch::raise_error(
                        "TypeError",
                        "deconstruct must return Array".to_string(),
                    ));
                    zeo_abi::abi::STATUS_SIGNAL
                }
                Err(sig) => {
                    crate::signal::set_pending(sig);
                    zeo_abi::abi::STATUS_SIGNAL
                }
            }
        }
        other => {
            crate::builtins::exception::pattern_fail_deconstruct(other, false);
            unsafe { matched.write(0) };
            zeo_abi::abi::STATUS_OK
        }
    }
}

/// [`zeo_rt_pat_deconstruct`]'s hash twin (`#deconstruct_keys`).
///
/// `keys`/`n_keys` are the keys the pattern NAMES, as interned symbol
/// ids; a null pointer is CRuby's `nil`, which it passes for a pattern
/// that can take everything (one with a `**rest`, `**nil`, or no keys at
/// all). The argument is the whole reason the protocol takes one: an
/// implementation that builds only what was asked for cannot tell the two
/// cases apart otherwise, and one that BRANCHES on nil takes the wrong
/// branch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_deconstruct_keys(
    v: *const RubyValue,
    keys: *const u32,
    n_keys: usize,
    out: *mut RubyValue,
    matched: *mut u8,
) -> i32 {
    let recv = unsafe { &*v };
    let dec = Symbol::intern("deconstruct_keys");
    let named = if keys.is_null() {
        RubyValue::Nil
    } else {
        RubyValue::Array(crate::array_new(
            (0..n_keys)
                .map(|i| RubyValue::Symbol(Symbol::from_u32(unsafe { *keys.add(i) })))
                .collect(),
        ))
    };
    match recv {
        RubyValue::Hash(_) => {
            unsafe { matched.write(1) };
            status_out(Ok(recv.clone()), out)
        }
        other if crate::dispatch::responds_to(other.class_id(), dec, false) => {
            match crate::dispatch::send_value(other, dec, &[named], None) {
                // Checked where it arrives, as the array twin above is, and
                // for the same reason: a non-Hash reached an accessor that
                // panics on a non-unwinding boundary.
                Ok(RubyValue::Hash(h)) => {
                    unsafe { matched.write(1) };
                    status_out(Ok(RubyValue::Hash(h)), out)
                }
                Ok(_) => {
                    crate::signal::set_pending(crate::dispatch::raise_error(
                        "TypeError",
                        "deconstruct_keys must return Hash".to_string(),
                    ));
                    zeo_abi::abi::STATUS_SIGNAL
                }
                Err(sig) => {
                    crate::signal::set_pending(sig);
                    zeo_abi::abi::STATUS_SIGNAL
                }
            }
        }
        other => {
            crate::builtins::exception::pattern_fail_deconstruct(other, true);
            unsafe { matched.write(0) };
            zeo_abi::abi::STATUS_OK
        }
    }
}

/// The deconstructed array's length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_array_len(v: *const RubyValue) -> usize {
    unsafe { &*v }.as_array_ref().lock().len()
}

/// One element of the deconstructed array, cloned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_array_get(v: *const RubyValue, i: usize, out: *mut RubyValue) {
    let arr = unsafe { &*v }.as_array_ref();
    let elem = arr.lock().get(i).cloned().unwrap_or(RubyValue::Nil);
    super::leakcheck::created(&elem);
    unsafe { out.write(elem) };
}

/// `arr[from...to]` as a NEW Array -- what `*rest` binds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_array_slice(
    v: *const RubyValue,
    from: usize,
    to: usize,
    out: *mut RubyValue,
) {
    let arr = unsafe { &*v }.as_array_ref();
    let slice: Vec<RubyValue> = {
        let guard = arr.lock();
        let hi = to.min(guard.len());
        let lo = from.min(hi);
        guard[lo..hi].to_vec()
    };
    let value = RubyValue::Array(crate::value::collections::array_new(slice));
    super::leakcheck::created(&value);
    unsafe { out.write(value) };
}

/// Whether the deconstructed hash holds `key` (a Symbol id).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_hash_has_key(v: *const RubyValue, key: u32) -> u8 {
    let h = unsafe { &*v }.as_hash_unchecked();
    u8::from(crate::value::collections::hash_has_key(
        &h,
        &RubyValue::Symbol(Symbol::from_u32(key)),
    ))
}

/// One value out of the deconstructed hash, cloned (`nil` when absent --
/// the caller checked presence first).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_hash_get(v: *const RubyValue, key: u32, out: *mut RubyValue) {
    let h = unsafe { &*v }.as_hash_unchecked();
    let value = crate::value::collections::hash_get(&h, &RubyValue::Symbol(Symbol::from_u32(key)));
    super::leakcheck::created(&value);
    unsafe { out.write(value) };
}

/// The deconstructed hash's entry count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_hash_len(v: *const RubyValue) -> usize {
    let h = unsafe { &*v }.as_hash_unchecked();
    usize::try_from(crate::value::collections::hash_len(&h)).unwrap_or(0)
}

/// Everything the pattern did NOT name, as a new Hash -- `**rest`'s value
/// and the `**nil` failure record's subject.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_hash_except(
    v: *const RubyValue,
    keys: *const u32,
    n: usize,
    out: *mut RubyValue,
) {
    let h = unsafe { &*v }.as_hash_unchecked();
    let ids = if n == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(keys, n) }
    };
    let names: Vec<String> = ids
        .iter()
        .map(|&k| Symbol::from_u32(k).name().to_string())
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let rest = RubyValue::Hash(crate::value::collections::hash_except_keys(&h, &refs));
    super::leakcheck::created(&rest);
    unsafe { out.write(rest) };
}

/// `is_a?` over the linearized ancestry -- a class-check pattern's test for
/// a class the emitter resolved statically.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_is_a(v: *const RubyValue, cid: u32) -> u8 {
    u8::from(crate::dispatch::is_a_value(unsafe { &*v }, ClassId(cid)))
}

/// `P === v does not return true` -- the sentence every leaf pattern with a
/// nameable left-hand side records when it rejects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_fail_case_eq(pat: *const RubyValue, matchee: *const RubyValue) {
    crate::builtins::exception::pattern_fail_case_eq(unsafe { &*pat }, unsafe { &*matchee });
}

/// An array pattern's length mismatch (`open` = the pattern has a `*`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_fail_length(
    matchee: *const RubyValue,
    expected: usize,
    open: u8,
) {
    crate::builtins::exception::pattern_fail_length(unsafe { &*matchee }, expected, open != 0);
}

/// `in {}` / `**nil` leftovers (`rest` = the `**nil`-with-keys wording).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_fail_not_empty(matchee: *const RubyValue, rest: u8) {
    crate::builtins::exception::pattern_fail_not_empty(unsafe { &*matchee }, rest != 0);
}

/// A find pattern that scanned every window without a match.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_fail_find(matchee: *const RubyValue) {
    crate::builtins::exception::pattern_fail_find(unsafe { &*matchee });
}

/// The pattern matched and the guard rejected -- its own reason.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_fail_guard() {
    crate::builtins::exception::pattern_fail_guard();
}

/// A hash pattern's MISSING key -- `NoMatchingPatternKeyError`'s subject,
/// kept apart from a present key whose value did not match.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_key_miss_record(key: u32, matchee: *const RubyValue) {
    crate::builtins::exception::pattern_key_miss_record(
        &RubyValue::Symbol(Symbol::from_u32(key)),
        unsafe { &*matchee },
    );
}

/// Clears any recorded key miss before a `case/in` whose raise arm will
/// read one (the single-clause shape).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_key_miss_clear() {
    crate::builtins::exception::pattern_key_miss_clear();
}

/// `NoMatchingPatternError` with the detailed reason a single-clause
/// `case/in` (or `expr => pattern`) reports.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_match_error(subject: *const RubyValue) -> i32 {
    crate::signal::set_pending(crate::builtins::exception::pattern_match_error(unsafe {
        &*subject
    }));
    zeo_abi::abi::STATUS_SIGNAL
}

/// The bare form: with several `in` clauses ruby names only the subject --
/// no single failing sub-test describes a match that had branches to try.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pat_match_error_bare(subject: *const RubyValue) -> i32 {
    crate::signal::set_pending(crate::builtins::exception::pattern_match_error_bare(
        unsafe { &*subject },
    ));
    zeo_abi::abi::STATUS_SIGNAL
}
