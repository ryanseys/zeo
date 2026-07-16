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
    /// A string key as `(bytes, encoding-tag)` -- see
    /// `encoding::StrBuf::hash_key_tag` for the cross-encoding `eql?` rule
    /// the tag encodes (ascii-only strings share tag `0`).
    Str(Vec<u8>, u8),
    Array(Vec<HashKey>),
    Range(Option<Box<HashKey>>, Option<Box<HashKey>>, bool),
    /// A first-class class/module value (Phase 16.1): keyed by class
    /// identity, exactly real Ruby's `Class#hash`/`#eql?` (two references
    /// to the same class are one key).
    Class(u32),
    /// An Object key whose class defines its own `hash` (Phase 16.2): the
    /// projection of that method's RESULT. Wrapped (not flattened into the
    /// result's own variant) so a user-hashed object never collides with a
    /// plain value that happens to equal its hash. Documented
    /// approximation: two keys with `hash`-equal results are ONE key here
    /// even if their `eql?` would disagree (this table has no second
    /// eql?-verification pass); in practice classes define the two
    /// consistently.
    Computed(Box<HashKey>),
    Identity(usize),
    /// The numeric-tower keys (Phase 17.1) -- `BigInt` never overlaps
    /// `Int` (demotion invariant), `Rational` is always reduced, `Complex`
    /// keys by its component keys.
    BigInt(num_bigint::BigInt),
    Rational(num_bigint::BigInt, num_bigint::BigInt),
    Complex(Box<(HashKey, HashKey)>),
}

fn hash_key(v: &RubyValue) -> HashKey {
    match v {
        RubyValue::Nil => HashKey::Nil,
        RubyValue::Bool(b) => HashKey::Bool(*b),
        RubyValue::Int(i) => HashKey::Int(*i),
        // Canonical thanks to the demotion/reduction invariants: a BigInt
        // never aliases an Int value, a Rational is always reduced.
        RubyValue::BigInt(b) => HashKey::BigInt((**b).clone()),
        RubyValue::Rational(r) => HashKey::Rational(r.num.clone(), r.den.clone()),
        RubyValue::Complex(c) => {
            HashKey::Complex(Box::new((hash_key(&c.real), hash_key(&c.imag))))
        }
        RubyValue::Float(f) => HashKey::Float(f.to_bits()),
        RubyValue::Symbol(s) => HashKey::Symbol(*s),
        RubyValue::Str(s) => {
            let s = s.lock();
            HashKey::Str(s.bytes().to_vec(), s.hash_key_tag())
        }
        RubyValue::Class(cid) => HashKey::Class(cid.0),
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
        //
        // A user-defined `hash` (Phase 16.2, dispatched once per
        // insertion/lookup through the registry) projects the object
        // through its RESULT -- see `HashKey::Computed`'s docs; identity
        // stays the default (real Ruby's own `Object#hash`). A `hash` that
        // raises is a loud panic (no exception channel here).
        RubyValue::Object(o) => match crate::dispatch::call_user_method(o, "hash", &[]) {
            Some(Ok(v)) => HashKey::Computed(Box::new(hash_key(&v))),
            Some(Err(_)) => panic!(
                "a user-defined `hash` raised inside a Hash key lookup (spike scope: no exception channel here)"
            ),
            None => HashKey::Identity(Arc::as_ptr(o) as *const () as usize),
        },
        RubyValue::Proc(p) => HashKey::Identity(p.ptr_id()),
        // Same identity-only fallback as `Object`/`Proc` above -- neither has
        // a user-overridable `#hash`/`#eql?` protocol yet (see `HashKey`'s
        // own docs on this documented, narrow scope-cut).
        RubyValue::Regexp(r) => HashKey::Identity(Arc::as_ptr(r) as *const () as usize),
        RubyValue::MatchData(m) => HashKey::Identity(Arc::as_ptr(m) as *const () as usize),
        RubyValue::Fiber(f) => HashKey::Identity(Arc::as_ptr(f) as *const () as usize),
        RubyValue::Enumerator(e) => HashKey::Identity(Arc::as_ptr(e) as *const () as usize),
        RubyValue::Yielder(y) => HashKey::Identity(y.ptr_id()),
        RubyValue::Thread(t) => HashKey::Identity(Arc::as_ptr(t) as *const () as usize),
        RubyValue::Mutex(m) => HashKey::Identity(Arc::as_ptr(m) as *const () as usize),
        RubyValue::Queue(q) => HashKey::Identity(Arc::as_ptr(q) as *const () as usize),
        RubyValue::Ractor(r) => HashKey::Identity(Arc::as_ptr(r) as *const () as usize),
    }
}

/// `Object#hash`'s universal answer (Phase 16.2): an `i64` digest of the
/// value's own `HashKey` projection -- so `"a".hash == "a".hash`,
/// `[1, 2].hash` is structural, and an Object without a user `hash` digests
/// by identity, exactly mirroring which values this module's Hash table
/// would treat as the same key. (Not CRuby's salted SipHash values -- only
/// the EQUALITY of two hashes is observable behavior worth matching.)
pub fn value_hash_code(v: &RubyValue) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hash_key(v).hash(&mut hasher);
    hasher.finish() as i64
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

pub type RStr = Arc<Freezable<crate::encoding::StrBuf>>;

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

/// Flattens a `*splat` element in place, with Ruby's own coercion rules --
/// splatting a non-Array is ordinary Ruby, not an error. It used to panic
/// ("expected an Array to splat"), which took down `a, b = *1`.
///
/// The rules, all oracle-verified:
///
/// ```text
/// [*[1, 2]]        => [1, 2]      an Array is itself
/// [*nil]           => []          nil splats to NOTHING
/// [*1]             => [1]         no to_a: wrapped
/// [*(1..3)]        => [1, 2, 3]   Range#to_a
/// [*{a: 1}]        => [[:a, 1]]   Hash#to_a
/// [*"str"]         => ["str"]     NOT chars -- String has no to_a
/// ```
///
/// `"str"` is the case worth stating: it looks like it should splat into
/// characters, and it doesn't, because modern Ruby's String simply has no
/// `to_a`. Probing `respond_to?` rather than special-casing types is what
/// gets that right for free -- and gets a user class with its own `to_a`
/// right too.
///
/// Fallible now: `to_a` is a real dispatch and can raise.
pub fn array_splat_into(out: &mut Vec<RubyValue>, value: &RubyValue) -> Result<(), crate::Signal> {
    match value {
        RubyValue::Array(a) => out.extend(a.lock().iter().cloned()),
        RubyValue::Nil => {}
        other => {
            let to_a = crate::Symbol::intern("to_a");
            if crate::dispatch::responds_to(other.class_id(), to_a, false) {
                let arr = crate::dispatch::send_value(other, to_a, &[], None)?;
                match arr {
                    RubyValue::Array(a) => out.extend(a.lock().iter().cloned()),
                    // A `to_a` that doesn't answer an Array is CRuby's
                    // "can't convert X to Array" TypeError.
                    bad => {
                        return Err(crate::dispatch::raise_error(
                            "TypeError",
                            format!(
                                "can't convert {} to Array ({}#to_a gives {})",
                                crate::builtins::class_name_of(other),
                                crate::builtins::class_name_of(other),
                                crate::builtins::class_name_of(&bad)
                            ),
                        ))
                    }
                }
            } else {
                out.push(other.clone());
            }
        }
    }
    Ok(())
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

/// `Hash#delete`: removes the entry, returning its value (`nil` when the
/// key was absent -- the no-default-block scope-cut, same as `hash_get`).
/// `shift_remove` (not plain `swap_remove`) preserves the remaining
/// entries' insertion order, Ruby's own guarantee.
pub fn hash_delete(h: &RHash, key: &RubyValue) -> RubyValue {
    h.lock()
        .shift_remove(&hash_key(key))
        .map(|(_, v)| v)
        .unwrap_or(RubyValue::Nil)
}

/// `Hash#keys` -- the ORIGINAL key values, in insertion order.
pub fn hash_keys(h: &RHash) -> RubyValue {
    RubyValue::Array(array_new(h.lock().values().map(|(k, _)| k.clone()).collect()))
}

/// `Hash#values`, in insertion order.
pub fn hash_values(h: &RHash) -> RubyValue {
    RubyValue::Array(array_new(h.lock().values().map(|(_, v)| v.clone()).collect()))
}

/// `Array#<<`/`#push` -- returns the array itself (Ruby's chaining
/// contract), as an already-boxed value for dynamic-dispatch callers.
pub fn array_push(arr: &RArray, value: RubyValue) -> RubyValue {
    arr.lock().push(value);
    RubyValue::Array(arr.clone())
}

/// `Array#include?` -- `==`-based membership (`RubyValue::rb_eq`), matching
/// real Ruby's `==` (not `eql?`) rule for `include?`.
pub fn array_include(arr: &RArray, value: &RubyValue) -> bool {
    arr.lock().iter().any(|e| e.rb_eq(value))
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
    Arc::new(Freezable::new(crate::encoding::StrBuf::from_utf8(s)))
}

/// A string from raw bytes tagged with an explicit encoding -- what
/// `String#b`, `force_encoding`, IO byte reads, and `\xNN`-bearing literals
/// build (the byte-level sibling of `string_new`'s UTF-8 text path).
pub fn string_from_bytes(bytes: Vec<u8>, enc: crate::encoding::EncodingId) -> RStr {
    Arc::new(Freezable::new(crate::encoding::StrBuf::from_bytes(bytes, enc)))
}

/// Wraps an already-built `StrBuf` (carrying its own encoding) as an `RStr` --
/// the constructor for the encoding-aware string builders (`char_at`,
/// `reversed`, `upcased`, ...).
pub fn string_wrap(buf: crate::encoding::StrBuf) -> RStr {
    Arc::new(Freezable::new(buf))
}

/// Character-indexed (not byte-indexed), matching Ruby's own UTF-8-aware
/// `String#[]` -- negative indices count from the end, out-of-range returns
/// `nil`. Re-walking `.chars()` on every call is a real inefficiency for long
/// strings (documented, not fixed -- a byte-offset cache is a
/// straightforward later optimization, not a spike blocker).
pub fn string_get(s: &RStr, index: i64) -> RubyValue {
    // Encoding-aware: a character is a UTF-8 sequence or a single byte, and
    // the result keeps the receiver's encoding (a BINARY byte stays BINARY).
    match s.lock().char_at(index) {
        Some(buf) => RubyValue::Str(string_wrap(buf)),
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
    let new_chars = new_chars.lock().char_vec();
    let mut s = s.lock();
    let mut chars: Vec<char> = s.char_vec();
    let i = resolve_index(index, chars.len()).unwrap_or_else(|| {
        panic!("index {index} out of range for string of length {}", chars.len())
    });
    chars.splice(i..=i, new_chars);
    s.replace_utf8(chars.into_iter().collect());
    value.clone()
}

pub fn string_len(s: &RStr) -> i64 {
    s.lock().char_len() as i64
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
    // Padding goes on the END, not the front: when there aren't enough
    // values left to fill the post-splat targets, Ruby assigns what it has to
    // the EARLIEST of them and nils the tail -- `w, *x, y, z = [1, 2]` is
    // `w=1, x=[], y=2, z=nil`, not `z=2` (oracle-verified). The post targets
    // are anchored to the end of the value list only when there are enough
    // values to reach them; underfull, they fill left-to-right like any
    // other target list.
    let pad = n_after.saturating_sub(after_part.len());
    let after: Vec<RubyValue> = after_part
        .iter()
        .cloned()
        .chain(std::iter::repeat_n(RubyValue::Nil, pad))
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

    /// Splatting a non-Array is ordinary Ruby, not an error -- this used to
    /// panic ("expected an Array to splat"), which took down `a, b = *1`.
    /// Every case here is oracle-verified against ruby 4.0.5.
    #[test]
    fn splatting_an_array_flattens_it_and_nil_contributes_nothing() {
        let mut out = Vec::new();
        let arr = RubyValue::Array(array_new(vec![RubyValue::Int(1), RubyValue::Int(2)]));
        array_splat_into(&mut out, &arr).unwrap();
        assert_eq!(display(&out), ["1", "2"]);

        // `[*nil]` is `[]` -- nil splats to NOTHING; it is not wrapped.
        let mut out = Vec::new();
        array_splat_into(&mut out, &RubyValue::Nil).unwrap();
        assert!(out.is_empty());
    }

    /// A value with no `to_a` is WRAPPED, not rejected: `[*1]` is `[1]`.
    #[test]
    fn splatting_a_value_without_to_a_wraps_it() {
        let mut out = Vec::new();
        array_splat_into(&mut out, &RubyValue::Int(1)).unwrap();
        assert_eq!(display(&out), ["1"]);
    }

    /// The case that looks wrong and isn't: `[*"str"]` is `["str"]`, NOT
    /// its characters -- modern Ruby's String simply has no `to_a`. This is
    /// exactly why the implementation asks `respond_to?(:to_a)` instead of
    /// special-casing types: getting String right falls out of asking, and
    /// so does getting a user class with its own `to_a` right.
    #[test]
    fn splatting_a_string_wraps_it_rather_than_splitting_into_chars() {
        let mut out = Vec::new();
        let s = RubyValue::Str(string_new("str".to_string()));
        array_splat_into(&mut out, &s).unwrap();
        assert_eq!(display(&out), ["str"]);
    }

    /// A Range DOES have `to_a`, so it splats through it.
    #[test]
    fn splatting_a_range_goes_through_its_to_a() {
        let mut out = Vec::new();
        let r = RubyValue::Range(
            Some(Box::new(RubyValue::Int(1))),
            Some(Box::new(RubyValue::Int(3))),
            false,
        );
        array_splat_into(&mut out, &r).unwrap();
        assert_eq!(display(&out), ["1", "2", "3"]);
    }
}
