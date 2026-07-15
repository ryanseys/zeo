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
use indexmap::IndexMap;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The shared storage cell behind every mutable built-in value: the
/// `Mutex`-guarded payload plus its `.freeze` flag (Phase 13.1), mirroring
/// how CRuby keeps `FL_FREEZE` as one bit on the object header next to the
/// data rather than as a separate registry. `lock()` is deliberately an
/// inherent method with the exact signature `Mutex::lock` had when
/// `RArray`/`RHash`/`RStr` were bare `Arc<Mutex<_>>` aliases, so every
/// pre-existing call site -- including every codegen-EMITTED `.lock()` in
/// generated programs -- keeps compiling unchanged.
///
/// `Ordering::Relaxed` is sufficient for the flag: freezing only needs to
/// prevent FUTURE mutations observed through ordinary program order (CRuby's
/// own flag is a plain bit with no fence either); it synchronizes nothing
/// else. The frozen CHECK itself lives in codegen-emitted guards, not here
/// -- only codegen can construct the `FrozenError` to raise (same division
/// of labor as `array_set`'s `IndexError` contract below).
pub struct Freezable<T> {
    frozen: AtomicBool,
    payload: Mutex<T>,
}

impl<T> Freezable<T> {
    pub fn new(payload: T) -> Freezable<T> {
        Freezable {
            frozen: AtomicBool::new(false),
            payload: Mutex::new(payload),
        }
    }

    pub fn lock(&self) -> parking_lot::MutexGuard<'_, T> {
        self.payload.lock()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }

    pub fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
}

pub type RArray = Arc<Freezable<Vec<RubyValue>>>;

/// A structural, hashable projection of a `RubyValue` -- the actual
/// `IndexMap` key (see `RHash` below), so `Hash#[]`/`#[]=` are real
/// O(1)-average lookups instead of an O(n) linear scan compared via
/// `RubyValue::rb_eq`. Built-in immutable/value-like types (`Nil`/`Bool`/
/// `Int`/`Float`/`Symbol`/`Str`/an `Array` of hashable elements/`Range`)
/// hash and compare STRUCTURALLY, matching real Ruby's own `#hash`/`#eql?`
/// for these types (note `eql?`, not `==`: `1` and `1.0` are DISTINCT keys,
/// matching `Integer#eql?`'s stricter same-class rule -- see `Int`/`Float`
/// staying separate variants below). Everything else (`Object`/`Proc`/a
/// nested `Hash`) falls back to pointer IDENTITY -- real Ruby's own default
/// `Object#hash` before a user overrides it, and this spike has no
/// user-overridable `#hash`/`#eql?` protocol yet (a documented, narrow
/// scope-cut -- see the plan's Part 10, Tier 1 #10).
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum HashKey {
    Nil,
    Bool(bool),
    Int(i64),
    /// Bit-pattern equality/hashing, not IEEE `==` -- a `Float::NAN` key
    /// compares equal to an IDENTICAL NaN bit pattern (unlike real Ruby's
    /// `Float#eql?`, where NaN never equals anything, even itself); a
    /// documented, narrow approximation for a vanishingly rare key shape.
    Float(u64),
    Symbol(crate::Symbol),
    Str(String),
    Array(Vec<HashKey>),
    Range(Option<Box<HashKey>>, Option<Box<HashKey>>, bool),
    Identity(usize),
}

fn hash_key(v: &RubyValue) -> HashKey {
    match v {
        RubyValue::Nil => HashKey::Nil,
        RubyValue::Bool(b) => HashKey::Bool(*b),
        RubyValue::Int(i) => HashKey::Int(*i),
        RubyValue::Float(f) => HashKey::Float(f.to_bits()),
        RubyValue::Symbol(s) => HashKey::Symbol(*s),
        RubyValue::Str(s) => HashKey::Str(s.lock().clone()),
        RubyValue::Array(a) => HashKey::Array(a.lock().iter().map(hash_key).collect()),
        RubyValue::Range(start, end, exclusive) => HashKey::Range(
            start.as_ref().map(|b| Box::new(hash_key(b))),
            end.as_ref().map(|b| Box::new(hash_key(b))),
            *exclusive,
        ),
        RubyValue::Hash(h) => HashKey::Identity(Arc::as_ptr(h) as usize),
        // `Arc<dyn Trait>`'s pointer is a FAT pointer (data + vtable) -- cast
        // through `*const ()` first to get a plain, `usize`-castable thin
        // pointer to the data alone (the vtable half is irrelevant to
        // identity).
        RubyValue::Object(o) => HashKey::Identity(Arc::as_ptr(o) as *const () as usize),
        RubyValue::Proc(p) => HashKey::Identity(Arc::as_ptr(p) as *const () as usize),
        // Same identity-only fallback as `Object`/`Proc` above -- neither has
        // a user-overridable `#hash`/`#eql?` protocol yet (see `HashKey`'s
        // own docs on this documented, narrow scope-cut).
        RubyValue::Regexp(r) => HashKey::Identity(Arc::as_ptr(r) as *const () as usize),
        RubyValue::MatchData(m) => HashKey::Identity(Arc::as_ptr(m) as *const () as usize),
        RubyValue::Fiber(f) => HashKey::Identity(Arc::as_ptr(f) as *const () as usize),
        RubyValue::Thread(t) => HashKey::Identity(Arc::as_ptr(t) as *const () as usize),
        RubyValue::Mutex(m) => HashKey::Identity(Arc::as_ptr(m) as *const () as usize),
        RubyValue::Queue(q) => HashKey::Identity(Arc::as_ptr(q) as *const () as usize),
        RubyValue::Ractor(r) => HashKey::Identity(Arc::as_ptr(r) as *const () as usize),
    }
}

/// A real hash table (`IndexMap`, not a linear-scan association list),
/// preserving Ruby's own insertion-order iteration guarantee for free
/// (`IndexMap`'s whole reason for existing over a plain `HashMap`). Stores
/// each entry's ORIGINAL `RubyValue` key alongside its `HashKey` projection
/// (not just the value), since `HashKey` is lossy for e.g. an `Object` key
/// (identity-only) -- anything that needs to iterate/display/rebuild the
/// actual key (`to_display_string`, `#deconstruct_keys` pattern binding)
/// needs the real value back.
pub type RHash = Arc<Freezable<IndexMap<HashKey, (RubyValue, RubyValue)>>>;

pub type RStr = Arc<Freezable<String>>;

pub fn array_new(elems: Vec<RubyValue>) -> RArray {
    Arc::new(Freezable::new(elems))
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
/// that's still out of range raises a real `IndexError` -- `None` here,
/// which `codegen::call`'s `[]=` dispatch (the only caller) turns into a
/// proper `Signal::Raise(IndexError.new(...))` (exceptions
/// exist now, so this is no longer the "loud panic in the meantime" it was
/// before Phase 9 landed). The actual message/exception CONSTRUCTION happens
/// in codegen, not here, since only codegen has the class registry needed to
/// build an `IndexError` value.
pub fn array_set(arr: &RArray, index: i64, value: RubyValue) -> Option<RubyValue> {
    let mut arr = arr.lock();
    let i = if index < 0 {
        let from_end = arr.len() as i64 + index;
        usize::try_from(from_end).ok()?
    } else {
        index as usize
    };
    if i >= arr.len() {
        arr.resize(i + 1, RubyValue::Nil);
    }
    arr[i] = value.clone();
    Some(value)
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
    let h: RHash = Arc::new(Freezable::new(IndexMap::new()));
    for (k, v) in pairs {
        hash_set(&h, k, v);
    }
    h
}

/// `Hash#[]`: a missing key returns `nil` (the no-default-block spike
/// scope-cut -- real Ruby's per-instance `Hash.new(default)`/
/// `Hash#default_proc` aren't modeled).
pub fn hash_get(h: &RHash, key: &RubyValue) -> RubyValue {
    h.lock().get(&hash_key(key)).map(|(_, v)| v.clone()).unwrap_or(RubyValue::Nil)
}

/// `Hash#[]=`: replaces an existing key's VALUE in place, preserving its
/// original insertion position (matching real Ruby) rather than moving it to
/// the end -- `IndexMap::insert`'s own documented behavior for a
/// re-inserted, already-present key.
pub fn hash_set(h: &RHash, key: RubyValue, value: RubyValue) -> RubyValue {
    h.lock().insert(hash_key(&key), (key, value.clone()));
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
    h.lock().contains_key(&hash_key(key))
}

/// A new Hash containing every pair from `h` whose key ISN'T in `keys` --
/// backs a hash pattern's `**rest` binding (the leftover key/value pairs not
/// matched by any explicit `key:` entry).
pub fn hash_except_keys(h: &RHash, keys: &[&str]) -> RHash {
    let excluded: Vec<HashKey> = keys
        .iter()
        .map(|k| hash_key(&RubyValue::Symbol(crate::Symbol::intern(k))))
        .collect();
    let pairs: IndexMap<HashKey, (RubyValue, RubyValue)> = h
        .lock()
        .iter()
        .filter(|(k, _)| !excluded.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Arc::new(Freezable::new(pairs))
}

pub fn string_new(s: String) -> RStr {
    Arc::new(Freezable::new(s))
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
