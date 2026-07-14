//! Minimal `Array`/`Hash`/`String` runtime support (Phase 3) -- concrete
//! `Arc<parking_lot::Mutex<_>>`-backed collection types, mirroring the same
//! shared-mutable-identity model `ruby_class!`'s generated structs already
//! use (`Mutex` ivars) and the `Arc<ConcreteStruct>` wrapping `New`
//! constructed objects settled on (see `codegen::call::emit_new`'s docs):
//! assigning a collection to another local aliases the same underlying
//! storage rather than deep-copying it, matching Ruby's own reference
//! semantics for these types. `Arc`/`Mutex` (not `Rc`/`RefCell`) so every
//! `RubyValue` is genuinely `Send + Sync` -- see the plan's Part 9 for the
//! full rationale; this migration needs no `unsafe` anywhere, since
//! `Send`/`Sync` auto-derive through any compound type built entirely from
//! `Send + Sync` leaves.
//!
//! Deliberately NOT a general Enumerable implementation -- see the plan's
//! Phase 3 scope-cut: `[]`/`[]=`/`length` only. `each`/`map`/`select`/etc.
//! are written in Ruby once blocks (Part 1.3) and modules (Part 1.2) exist,
//! not hand-implemented here.

use crate::RubyValue;
use parking_lot::Mutex;
use std::sync::Arc;

pub type RArray = Arc<Mutex<Vec<RubyValue>>>;

/// Hash storage is a plain association *list*, not a real hash table: every
/// lookup/insert is an O(n) linear scan compared via `RubyValue::rb_eq`. A
/// real `HashMap` needs `Hash`/`Eq` on `RubyValue`, which in turn needs a
/// user-overridable `#hash`/`#eql?` protocol for `Object` keys that doesn't
/// exist yet -- a deliberate, documented spike scope-cut, not an oversight.
/// Fine for the tiny hashes the spike's examples use; revisit once
/// user-defined `#hash` exists.
pub type RHash = Arc<Mutex<Vec<(RubyValue, RubyValue)>>>;

pub type RStr = Arc<Mutex<String>>;

pub fn array_new(elems: Vec<RubyValue>) -> RArray {
    Arc::new(Mutex::new(elems))
}

/// Ruby's own `Array#[]`: negative indices count from the end, and an
/// out-of-range index returns `nil` rather than raising/panicking.
pub fn array_get(arr: &RArray, index: i64) -> RubyValue {
    let arr = arr.lock();
    resolve_index(index, arr.len())
        .and_then(|i| arr.get(i).cloned())
        .unwrap_or(RubyValue::Nil)
}

/// Ruby's `Array#[]=`: an index past the current end pads with `nil` up to
/// it (`a = []; a[3] = :x` gives `[nil, nil, nil, :x]`); a negative index
/// that's still out of range panics -- a real `IndexError` needs exceptions
/// (Phase 9), so this is a loud failure, not a silent one, in the meantime.
pub fn array_set(arr: &RArray, index: i64, value: RubyValue) -> RubyValue {
    let mut arr = arr.lock();
    let i = if index < 0 {
        let from_end = arr.len() as i64 + index;
        usize::try_from(from_end).unwrap_or_else(|_| {
            panic!("index {index} too small for array of length {}", arr.len())
        })
    } else {
        index as usize
    };
    if i >= arr.len() {
        arr.resize(i + 1, RubyValue::Nil);
    }
    arr[i] = value.clone();
    value
}

pub fn array_len(arr: &RArray) -> i64 {
    arr.lock().len() as i64
}

/// Flattens a `*splat` array-literal element in place -- panics (not a
/// silent no-op) if the splatted value isn't actually an `Array`, since
/// there's no static type-checker here to catch that earlier (same posture
/// as `RubyValue::as_int_unchecked` etc.).
pub fn array_splat_into(out: &mut Vec<RubyValue>, value: &RubyValue) {
    match value {
        RubyValue::Array(a) => out.extend(a.lock().iter().cloned()),
        other => panic!("expected an Array to splat, got {}", other.to_display_string()),
    }
}

fn resolve_index(index: i64, len: usize) -> Option<usize> {
    let i = if index < 0 { index + len as i64 } else { index };
    usize::try_from(i).ok().filter(|&i| i < len)
}

pub fn hash_new(pairs: Vec<(RubyValue, RubyValue)>) -> RHash {
    let h: RHash = Arc::new(Mutex::new(Vec::new()));
    for (k, v) in pairs {
        hash_set(&h, k, v);
    }
    h
}

/// `Hash#[]`: a missing key returns `nil` (the no-default-block spike
/// scope-cut -- real Ruby's per-instance `Hash.new(default)`/
/// `Hash#default_proc` aren't modeled).
pub fn hash_get(h: &RHash, key: &RubyValue) -> RubyValue {
    h.lock()
        .iter()
        .find(|(k, _)| k.rb_eq(key))
        .map(|(_, v)| v.clone())
        .unwrap_or(RubyValue::Nil)
}

/// `Hash#[]=`: replaces an existing key's value in place (preserving
/// insertion order, matching real Ruby) rather than appending a duplicate.
pub fn hash_set(h: &RHash, key: RubyValue, value: RubyValue) -> RubyValue {
    let mut h = h.lock();
    match h.iter_mut().find(|(k, _)| k.rb_eq(&key)) {
        Some((_, v)) => *v = value.clone(),
        None => h.push((key, value.clone())),
    }
    value
}

pub fn hash_len(h: &RHash) -> i64 {
    h.lock().len() as i64
}

/// Whether `key` is actually present -- distinct from `hash_get` returning
/// non-`Nil`, since a key whose VALUE happens to be Ruby `nil` is a real,
/// present entry (`{a: nil}` has key `:a`; `{}` does not). Needed for hash
/// PATTERN matching (`case/in`): `in {a: nil}` must fail against `{}`, which
/// a `hash_get(...).is_nil()`-based check alone couldn't distinguish.
pub fn hash_has_key(h: &RHash, key: &RubyValue) -> bool {
    h.lock().iter().any(|(k, _)| k.rb_eq(key))
}

/// A new Hash containing every pair from `h` whose key ISN'T in `keys` --
/// backs a hash pattern's `**rest` binding (the leftover key/value pairs not
/// matched by any explicit `key:` entry).
pub fn hash_except_keys(h: &RHash, keys: &[&str]) -> RHash {
    let excluded: Vec<RubyValue> = keys.iter().map(|k| RubyValue::Symbol(crate::Symbol::intern(k))).collect();
    let pairs: Vec<(RubyValue, RubyValue)> = h
        .lock()
        .iter()
        .filter(|(k, _)| !excluded.iter().any(|e| e.rb_eq(k)))
        .cloned()
        .collect();
    Arc::new(Mutex::new(pairs))
}

pub fn string_new(s: String) -> RStr {
    Arc::new(Mutex::new(s))
}

/// Character-indexed (not byte-indexed), matching Ruby's own UTF-8-aware
/// `String#[]` -- negative indices count from the end, out-of-range returns
/// `nil`. Re-walking `.chars()` on every call is a real inefficiency for long
/// strings (documented, not fixed -- a byte-offset cache is a
/// straightforward later optimization, not a spike blocker).
pub fn string_get(s: &RStr, index: i64) -> RubyValue {
    let s = s.lock();
    let chars: Vec<char> = s.chars().collect();
    match resolve_index(index, chars.len()) {
        Some(i) => RubyValue::Str(string_new(chars[i].to_string())),
        None => RubyValue::Nil,
    }
}

/// `String#[]=` with a single-character replacement value (the common case);
/// panics if `value` isn't a `Str` or the index is out of range --
/// multi-character splice-replace (`s[1] = "ab"`) isn't supported yet (spike
/// scope, same posture as `array_set`'s negative-out-of-range panic).
pub fn string_set(s: &RStr, index: i64, value: &RubyValue) -> RubyValue {
    let RubyValue::Str(new_chars) = value else {
        panic!("expected a String, got {}", value.to_display_string());
    };
    let new_chars = new_chars.lock().clone();
    let mut s = s.lock();
    let mut chars: Vec<char> = s.chars().collect();
    let i = resolve_index(index, chars.len()).unwrap_or_else(|| {
        panic!("index {index} out of range for string of length {}", chars.len())
    });
    chars.splice(i..=i, new_chars.chars());
    *s = chars.into_iter().collect();
    value.clone()
}

pub fn string_len(s: &RStr) -> i64 {
    s.lock().chars().count() as i64
}

/// Ruby multi-assignment's `a, b = ...` / `a, *b, c = ...` destructuring:
/// splits `elems` into the fixed prefix (`n_before` positions), an optional
/// splat-captured middle slice, and the fixed suffix (`n_after` positions).
/// Lenient like Ruby itself: missing prefix/suffix positions become `Nil`
/// (`elems` shorter than `n_before + n_after`), extra values are silently
/// dropped when `has_splat` is `false` (nothing to catch them), and the
/// splat captures whatever's left over between the prefix and suffix -- an
/// empty `Vec`, not `Nil`, when there's nothing there. Verified against real
/// `ruby`'s exact leniency behavior for every case codegen's `emit_for`/
/// `emit_multi_write_lets` can produce (see `codegen::loops`'s tests).
pub fn multi_assign(
    elems: &[RubyValue],
    n_before: usize,
    has_splat: bool,
    n_after: usize,
) -> (Vec<RubyValue>, Vec<RubyValue>, Vec<RubyValue>) {
    let get = |i: usize| elems.get(i).cloned().unwrap_or(RubyValue::Nil);
    let before: Vec<RubyValue> = (0..n_before).map(get).collect();
    if !has_splat {
        return (before, Vec::new(), Vec::new());
    }
    let rest = elems.get(n_before..).unwrap_or(&[]);
    let split_at = rest.len().saturating_sub(n_after);
    let (splat_part, after_part) = rest.split_at(split_at);
    let pad = n_after.saturating_sub(after_part.len());
    let after: Vec<RubyValue> = std::iter::repeat_n(RubyValue::Nil, pad)
        .chain(after_part.iter().cloned())
        .collect();
    (before, splat_part.to_vec(), after)
}

#[cfg(test)]
mod multi_assign_tests {
    use super::*;

    fn ints(vs: &[i64]) -> Vec<RubyValue> {
        vs.iter().map(|&i| RubyValue::Int(i)).collect()
    }

    /// `nil` displays as an empty string, so a plain `to_display_string()`
    /// join is a simple, panic-free way to assert on a mix of real values
    /// and nil-padding in one go.
    fn display(vs: &[RubyValue]) -> Vec<String> {
        vs.iter().map(RubyValue::to_display_string).collect()
    }

    /// Every case here is oracle-verified against real `ruby`'s exact
    /// destructuring leniency (see `codegen::loops`'s module docs).
    #[test]
    fn matches_real_ruby_leniency() {
        // (input, n_before, has_splat, n_after) -> (before, splat, after)
        let (b, s, a) = multi_assign(&ints(&[1, 2, 3, 4, 5]), 1, true, 1);
        assert_eq!(display(&b), ["1"]);
        assert_eq!(display(&s), ["2", "3", "4"]);
        assert_eq!(display(&a), ["5"]);

        let (b, s, a) = multi_assign(&ints(&[1]), 1, true, 1);
        assert_eq!(display(&b), ["1"]);
        assert!(s.is_empty());
        assert_eq!(display(&a), [""]); // nil-padded

        let (b, s, a) = multi_assign(&ints(&[1, 2]), 1, true, 1);
        assert_eq!(display(&b), ["1"]);
        assert!(s.is_empty());
        assert_eq!(display(&a), ["2"]);

        let (b, s, a) = multi_assign(&ints(&[1, 2, 3]), 1, true, 0);
        assert_eq!(display(&b), ["1"]);
        assert_eq!(display(&s), ["2", "3"]);
        assert!(a.is_empty());

        let (b, s, a) = multi_assign(&ints(&[1, 2, 3]), 0, true, 1);
        assert!(b.is_empty());
        assert_eq!(display(&s), ["1", "2"]);
        assert_eq!(display(&a), ["3"]);
    }

    #[test]
    fn no_splat_drops_extras_and_pads_missing() {
        let (before, splat, after) = multi_assign(&ints(&[1, 2, 3]), 2, false, 0);
        assert_eq!(display(&before), ["1", "2"]);
        assert!(splat.is_empty());
        assert!(after.is_empty());

        let (before, ..) = multi_assign(&ints(&[1]), 2, false, 0);
        assert_eq!(display(&before), ["1", ""]); // nil-padded
    }
}
