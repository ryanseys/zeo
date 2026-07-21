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

/// The structural projection (the default, `eql?`/`hash`-based). Every
/// context-free caller -- `Object#hash`, symbol-key rebuild, `Lazy#uniq` --
/// wants this, never identity.
pub(crate) fn hash_key(v: &RubyValue) -> HashKey {
    hash_key_in(v, false)
}

/// Projects a value to its `IndexMap` key. `by_identity` (a hash's
/// `compare_by_identity` flag) makes the heap value-like kinds -- `Str`,
/// `Array`, `Range`, `BigInt`, `Rational`, `Complex`, and any `Object`
/// (ignoring a user `#hash`) -- key by object identity instead of structure.
/// Immediates (`Nil`/`Bool`/`Int`/`Float`/`Symbol`/`Class`) have identity ==
/// value in Ruby, so they stay structural; the reference kinds (`Hash`/`Proc`/
/// `Regexp`/...) already key by identity in the structural path below.
pub(crate) fn hash_key_in(v: &RubyValue, by_identity: bool) -> HashKey {
    if by_identity {
        let ident = match v {
            RubyValue::Str(s) => Some(Arc::as_ptr(s) as *const () as usize),
            RubyValue::Array(a) => Some(Arc::as_ptr(a) as *const () as usize),
            RubyValue::BigInt(b) => Some(Arc::as_ptr(b) as *const () as usize),
            RubyValue::Rational(r) => Some(Arc::as_ptr(r) as *const () as usize),
            RubyValue::Complex(c) => Some(Arc::as_ptr(c) as *const () as usize),
            RubyValue::Object(o) => Some(Arc::as_ptr(o) as *const () as usize),
            // `Range` is a value type (inline `Box`es), not an `Arc`-shared
            // object, so it has no stable shared identity: two distinct
            // `Range` values key apart (matching `(1..2).equal?(1..2)` being
            // false), but a reused `Range` binding will NOT alias itself here.
            // Rare enough to accept; documented.
            RubyValue::Range(..) => Some(v as *const RubyValue as usize),
            _ => None,
        };
        if let Some(p) = ident {
            return HashKey::Identity(p);
        }
    }
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
            // A value-builtin subclass (D3) with no `hash` override keys by its
            // payload -- `Tag.new("k")` is the same Hash key as `"k"`.
            None => match o.builtin_payload() {
                Some(p) => hash_key_in(&p, by_identity),
                None => HashKey::Identity(Arc::as_ptr(o) as *const () as usize),
            },
        },
        RubyValue::Proc(p) => HashKey::Identity(p.ptr_id()),
        // Same identity-only fallback as `Object`/`Proc` above -- neither has
        // a user-overridable `#hash`/`#eql?` protocol yet (see `HashKey`'s
        // own docs on this documented, narrow scope-cut).
        RubyValue::Regexp(r) => HashKey::Identity(Arc::as_ptr(r) as *const () as usize),
        // Value-based, like CRuby's `MatchData#eql?`/`#hash`: two matches with
        // the same subject, pattern, and captured regions are one key even
        // though they are distinct objects (so `"abc".match(/b/).hash` is
        // stable across separate calls).
        RubyValue::MatchData(m) => {
            let mut parts = vec![
                HashKey::Str(m.haystack.clone().into_bytes(), 0),
                HashKey::Str(m.regexp.source.clone().into_bytes(), 0),
                HashKey::Bool(m.regexp.ignore_case),
                HashKey::Bool(m.regexp.multiline),
                HashKey::Bool(m.regexp.extended),
            ];
            parts.extend(m.groups.iter().map(|g| match g {
                Some((s, e)) => {
                    HashKey::Array(vec![HashKey::Int(*s as i64), HashKey::Int(*e as i64)])
                }
                None => HashKey::Nil,
            }));
            HashKey::Array(parts)
        }
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
/// A Hash's payload: its ordered entries plus the per-instance default a
/// missing key falls back to. `Hash.new(default)` stores a `default` VALUE;
/// `Hash.new { |h, k| ... }` stores a `default_proc`. The two channels are
/// mutually exclusive in real Ruby (`#default` reports `nil` for a
/// proc-backed hash and vice versa), so they sit side by side and are read
/// independently. Deref/DerefMut to the map keep the many `h.lock().<map op>`
/// call sites (insert/get/iter/len/...) compiling unchanged.
pub struct RHashData {
    map: IndexMap<HashKey, (RubyValue, RubyValue)>,
    pub default: RubyValue,
    pub default_proc: Option<RubyValue>,
    /// `Hash#compare_by_identity`: when set, keys project by object identity
    /// (`equal?`/`object_id`) instead of structure -- see `hash_key_in`.
    pub compare_by_identity: bool,
}

impl RHashData {
    fn new() -> RHashData {
        RHashData {
            map: IndexMap::new(),
            default: RubyValue::Nil,
            default_proc: None,
            compare_by_identity: false,
        }
    }
}

impl std::ops::Deref for RHashData {
    type Target = IndexMap<HashKey, (RubyValue, RubyValue)>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl std::ops::DerefMut for RHashData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map
    }
}

pub type RHash = Arc<Freezable<RHashData>>;

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
    let h: RHash = Arc::new(Freezable::new(RHashData::new()));
    for (k, v) in pairs {
        hash_set(&h, k, v);
    }
    h
}

/// An empty Hash carrying a per-instance default (`Hash.new(default)`) or
/// default block (`Hash.new { |h, k| ... }`) -- the constructor behind
/// `Hash.new`'s two argument shapes.
pub fn hash_new_with_default(default: RubyValue, default_proc: Option<RubyValue>) -> RHash {
    Arc::new(Freezable::new(RHashData {
        map: IndexMap::new(),
        default,
        default_proc,
        compare_by_identity: false,
    }))
}

/// A plain lookup: the stored value, or `nil` for a missing key -- WITHOUT
/// triggering any per-instance default. This is the internal read used by
/// `merge`/`dig`/keyword extraction/pattern matching, none of which invoke a
/// hash's default in real Ruby. The default-triggering `Hash#[]` is
/// `hash_index`.
pub fn hash_get(h: &RHash, key: &RubyValue) -> RubyValue {
    let g = h.lock();
    let k = hash_key_in(key, g.compare_by_identity);
    g.get(&k).map(|(_, v)| v.clone()).unwrap_or(RubyValue::Nil)
}

/// `Hash#[]`: the stored value, or the per-instance default on a miss -- the
/// stored default VALUE, the result of the default PROC (called with the hash
/// and key, and free to mutate the hash), or `nil` when neither is set. The
/// lock is released before the proc runs, since the proc commonly writes back
/// into the same hash (`Hash.new { |h, k| h[k] = ... }`).
pub fn hash_index(h: &RHash, key: &RubyValue) -> Result<RubyValue, crate::Signal> {
    {
        let g = h.lock();
        let k = hash_key_in(key, g.compare_by_identity);
        if let Some((_, v)) = g.get(&k) {
            return Ok(v.clone());
        }
    }
    let (default, proc) = {
        let g = h.lock();
        (g.default.clone(), g.default_proc.clone())
    };
    match proc {
        Some(p) => crate::dispatch::send_value(
            &p,
            crate::Symbol::intern("call"),
            &[RubyValue::Hash(h.clone()), key.clone()],
            None,
        ),
        None => Ok(default),
    }
}

/// `Hash#[]=`: replaces an existing key's VALUE in place, preserving its
/// original insertion position (matching real Ruby) rather than moving it to
/// the end -- `IndexMap::insert`'s own documented behavior for a
/// re-inserted, already-present key.
pub fn hash_set(h: &RHash, key: RubyValue, value: RubyValue) -> RubyValue {
    let mut g = h.lock();
    let k = hash_key_in(&key, g.compare_by_identity);
    g.insert(k, (key, value.clone()));
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
    let g = h.lock();
    let k = hash_key_in(key, g.compare_by_identity);
    g.contains_key(&k)
}

/// `Hash#delete`: removes the entry, returning its value (`nil` when the
/// key was absent -- the no-default-block scope-cut, same as `hash_get`).
/// `shift_remove` (not plain `swap_remove`) preserves the remaining
/// entries' insertion order, Ruby's own guarantee.
pub fn hash_delete(h: &RHash, key: &RubyValue) -> RubyValue {
    let mut g = h.lock();
    let k = hash_key_in(key, g.compare_by_identity);
    g.shift_remove(&k).map(|(_, v)| v).unwrap_or(RubyValue::Nil)
}

/// `Hash#compare_by_identity`: switch the hash to identity keying and
/// re-project every existing entry's stored key by identity, so keys inserted
/// under the old structural regime remain reachable by their own object (and
/// distinct-but-equal keys no longer collide). Returns nothing -- the caller
/// row returns `self`. The frozen check is the caller's (only codegen/the row
/// can render the `FrozenError` receiver).
pub fn hash_enable_compare_by_identity(h: &RHash) {
    let mut g = h.lock();
    if g.compare_by_identity {
        return;
    }
    g.compare_by_identity = true;
    // Rebuild the map keyed by identity, preserving insertion order.
    let old: Vec<(RubyValue, RubyValue)> = g.map.values().cloned().collect();
    g.map.clear();
    for (k, v) in old {
        let ik = hash_key_in(&k, true);
        g.map.insert(ik, (k, v));
    }
}

/// `Hash#keys` -- the ORIGINAL key values, in insertion order.
pub fn hash_keys(h: &RHash) -> RubyValue {
    RubyValue::Array(array_new(h.lock().values().map(|(k, _)| k.clone()).collect()))
}

/// `Hash#values`, in insertion order.
pub fn hash_values(h: &RHash) -> RubyValue {
    RubyValue::Array(array_new(h.lock().values().map(|(_, v)| v.clone()).collect()))
}

/// Every `(key, value)` pair in insertion order -- the primitive serializers
/// (JSON/YAML) walk to emit a Hash without needing the private `map` field.
pub fn hash_pairs(h: &RHash) -> Vec<(RubyValue, RubyValue)> {
    h.lock().values().cloned().collect()
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
    Arc::new(Freezable::new(RHashData {
        map: pairs,
        default: RubyValue::Nil,
        default_proc: None,
        compare_by_identity: false,
    }))
}

pub fn string_new(s: String) -> RStr {
    Arc::new(Freezable::new(crate::encoding::StrBuf::from_utf8(s)))
}

/// The key of the frozen-string pool: a string's exact identity is its bytes
/// PLUS their encoding (`"a".b` and `"a"` are different frozen strings).
type FrozenKey = (Vec<u8>, crate::encoding::EncodingId);

/// The process-lifetime pool of immortal frozen strings (CRuby's fstring
/// table), keyed by `(bytes, encoding)` -- see [`intern_frozen`].
static FROZEN_STRINGS: std::sync::LazyLock<Mutex<std::collections::HashMap<FrozenKey, RStr>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Intern a string by CONTENT: two strings with equal `(bytes, encoding)`
/// return the SAME immortal frozen `RStr`, so `"x".dedup.equal?("x".dedup)`
/// and a `# frozen_string_literal: true` literal share one object. The
/// pool holds every interned string for the process lifetime, matching the
/// immortality real Ruby gives its fstrings. NUL bytes are safe -- the key
/// is the full byte vector, never a C string. Backs `String#-@`/`#dedup`
/// and the frozen-string-literal codegen path.
pub fn intern_frozen(buf: crate::encoding::StrBuf) -> RStr {
    let key = (buf.bytes().to_vec(), buf.encoding());
    let mut pool = FROZEN_STRINGS.lock();
    if let Some(existing) = pool.get(&key) {
        return existing.clone();
    }
    let s = Arc::new(Freezable::new(buf));
    s.set_frozen();
    pool.insert(key, s.clone());
    s
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
mod hash_default_tests {
    use super::*;

    #[test]
    fn plain_hash_misses_to_nil() {
        let h = hash_new(vec![(RubyValue::Int(1), RubyValue::Int(10))]);
        assert!(matches!(hash_index(&h, &RubyValue::Int(1)).unwrap(), RubyValue::Int(10)));
        assert!(matches!(hash_index(&h, &RubyValue::Int(2)).unwrap(), RubyValue::Nil));
    }

    #[test]
    fn default_value_returns_on_miss_without_inserting() {
        let h = hash_new_with_default(RubyValue::Int(0), None);
        assert!(matches!(hash_index(&h, &RubyValue::Symbol(crate::Symbol::intern("x"))).unwrap(), RubyValue::Int(0)));
        // The default is NOT stored -- a plain default only reads back.
        assert_eq!(hash_len(&h), 0);
        // A present key still wins over the default.
        hash_set(&h, RubyValue::Symbol(crate::Symbol::intern("y")), RubyValue::Int(5));
        assert!(matches!(hash_index(&h, &RubyValue::Symbol(crate::Symbol::intern("y"))).unwrap(), RubyValue::Int(5)));
    }
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

    #[test]
    fn intern_frozen_returns_one_frozen_object_per_content() {
        use crate::encoding::StrBuf;
        let a = intern_frozen(StrBuf::from_utf8("hi".to_string()));
        let b = intern_frozen(StrBuf::from_utf8("hi".to_string()));
        assert!(a.is_frozen());
        // Equal content interns to the very same allocation.
        assert!(Arc::ptr_eq(&a, &b));
        // NUL bytes are part of the key, never a truncation point.
        let n1 = intern_frozen(StrBuf::from_utf8("a\0b".to_string()));
        let n2 = intern_frozen(StrBuf::from_utf8("a\0b".to_string()));
        assert!(Arc::ptr_eq(&n1, &n2));
        assert_eq!(n1.lock().bytesize(), 3);
        // Different content, different object.
        let c = intern_frozen(StrBuf::from_utf8("bye".to_string()));
        assert!(!Arc::ptr_eq(&a, &c));
    }
}

#[cfg(test)]
mod compare_by_identity_tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }

    fn is_int(v: &RubyValue, n: i64) -> bool {
        matches!(v, RubyValue::Int(i) if *i == n)
    }

    /// Immediates (Int/Symbol/nil/bool/Float) key by VALUE even under
    /// identity mode -- identity == value for them in Ruby.
    #[test]
    fn immediates_stay_structural_under_identity() {
        for v in [
            RubyValue::Int(7),
            RubyValue::Symbol(crate::Symbol::intern("x")),
            RubyValue::Nil,
            RubyValue::Bool(true),
            RubyValue::Float(1.5),
        ] {
            assert!(hash_key_in(&v, true) == hash_key_in(&v, false));
        }
    }

    /// Two equal-but-distinct Strings collapse to one structural key but stay
    /// two distinct identity keys.
    #[test]
    fn equal_strings_split_under_identity() {
        let a = s("hi");
        let b = s("hi");
        assert!(hash_key_in(&a, false) == hash_key_in(&b, false));
        assert!(hash_key_in(&a, true) != hash_key_in(&b, true));
        // The SAME object keys to itself either way.
        assert!(hash_key_in(&a, true) == hash_key_in(&a, true));
    }

    /// `Hash#[]`/`#[]=`/`#delete`/`#key?` honor the flag once set, and
    /// enabling it re-projects pre-existing entries so the stored object
    /// still hits.
    #[test]
    fn ops_honor_identity_and_reprojection() {
        let h = hash_new(vec![]);
        let a = s("k");
        let b = s("k");
        hash_set(&h, a.clone(), RubyValue::Int(1));
        hash_set(&h, b.clone(), RubyValue::Int(2));
        // Structural: same key, size 1, last write wins.
        assert_eq!(hash_len(&h), 1);

        let h2 = hash_new(vec![]);
        hash_set(&h2, a.clone(), RubyValue::Int(1));
        hash_enable_compare_by_identity(&h2);
        // The very object inserted before the switch still resolves.
        assert!(is_int(&hash_get(&h2, &a), 1));
        // A different, equal String misses.
        assert!(matches!(hash_get(&h2, &b), RubyValue::Nil));
        assert!(hash_has_key(&h2, &a));
        assert!(!hash_has_key(&h2, &b));
        // Two distinct equal Strings now coexist.
        hash_set(&h2, b.clone(), RubyValue::Int(2));
        assert_eq!(hash_len(&h2), 2);
        assert!(is_int(&hash_delete(&h2, &a), 1));
        assert_eq!(hash_len(&h2), 1);
    }

    /// Enabling identity is idempotent and never loses entries.
    #[test]
    fn enable_is_idempotent() {
        let h = hash_new(vec![]);
        hash_set(&h, s("a"), RubyValue::Int(1));
        hash_enable_compare_by_identity(&h);
        let n = hash_len(&h);
        hash_enable_compare_by_identity(&h);
        assert_eq!(hash_len(&h), n);
        assert!(h.lock().compare_by_identity);
    }
}
