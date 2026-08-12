//! Minimal `Array`/`Hash`/`String` runtime support -- concrete
//! `Arc<parking_lot::Mutex<_>>`-backed collection types, mirroring the same
//! shared-mutable-identity model `ruby_class!`'s generated structs already
//! use (`Mutex` ivars) and the `Arc<ConcreteStruct>` wrapping `New`
//! constructed objects settled on (see `codegen::call::emit_new`'s docs):
//! assigning a collection to another local aliases the same underlying
//! storage rather than deep-copying it, matching Ruby's own reference
//! semantics for these types. `Arc`/`Mutex` (not `Rc`/`RefCell`) so every
//! `RubyValue` is genuinely `Send + Sync`. This needs no `unsafe`:
//! `Send`/`Sync` auto-derive through any compound type built entirely from
//! `Send + Sync` leaves.
//!
//! Deliberately NOT a general Enumerable implementation: `[]`/`[]=`/`length`
//! only. `each`/`map`/`select`/etc. live in `builtins::enumerable`, not
//! hand-implemented here.

use crate::RubyValue;
use crate::builtins::type_error;
use indexmap::IndexMap;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// The shared storage cell behind every mutable built-in value: the
/// `Mutex`-guarded payload plus its `.freeze` flag, mirroring
/// how CRuby keeps `FL_FREEZE` as one bit on the object header next to the
/// data rather than as a separate registry. `lock()` is deliberately an
/// inherent method with the exact signature `Mutex::lock` had when
/// `RArray`/`RHash`/`RStr` were bare `Arc<Mutex<_>>` aliases, so every
/// pre-existing call site -- including every codegen-EMITTED `.lock()` in
/// generated programs -- keeps compiling unchanged.
///
/// `Ordering::Relaxed` is sufficient for the flags: freezing only needs to
/// prevent FUTURE mutations observed through ordinary program order (CRuby's
/// own flag is a plain bit with no fence either); it synchronizes nothing
/// else. The frozen CHECK itself lives in codegen-emitted guards, not here
/// -- only codegen can construct the `FrozenError` to raise (same division
/// of labor as `array_set`'s `IndexError` contract below).
///
/// One `AtomicU8` rather than two bools: FROZEN and MOVED (a `Ractor` move's
/// poison bit -- see `ractor::cross_graph`) share the byte, so every probe
/// that asks about either reads the same word `is_frozen` always read.
pub struct Freezable<T> {
    flags: AtomicU8,
    payload: Mutex<T>,
    /// Debug-only overlap detector for the sole-thread fast path -- see
    /// [`Freezable::lock`]. Atomic only because `Freezable` must stay `Sync`;
    /// on the fast path a single thread owns it by construction.
    #[cfg(debug_assertions)]
    fast_held: std::sync::atomic::AtomicBool,
}

const FLAG_FROZEN: u8 = 1;
const FLAG_MOVED: u8 = 2;

/// What [`Freezable::lock`] hands out: payload access through either the
/// sole-thread direct pointer or the real mutex guard. Derefs like the
/// `MutexGuard` every call site was written against.
pub struct FreezeGuard<'a, T> {
    inner: GuardInner<'a, T>,
}

#[cfg(debug_assertions)]
thread_local! {
    /// Debug census of live fast guards on THIS thread, so the thread-spawn
    /// transition can assert none is held -- see `gvl::note_thread_spawn`.
    static LIVE_FAST_GUARDS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(debug_assertions)]
pub fn debug_assert_no_live_fast_guards(context: &str) {
    LIVE_FAST_GUARDS.with(|c| {
        assert!(
            c.get() == 0,
            "{context} while {} sole-thread container guard(s) are live on \
             this thread -- in a release build that is an unprotected &mut \
             once the second thread runs",
            c.get()
        );
    });
}

enum GuardInner<'a, T> {
    Fast {
        payload: &'a mut T,
        #[cfg(debug_assertions)]
        held: &'a std::sync::atomic::AtomicBool,
    },
    Locked(parking_lot::MutexGuard<'a, T>),
}

impl<T> std::ops::Deref for FreezeGuard<'_, T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        match &self.inner {
            GuardInner::Fast { payload, .. } => payload,
            GuardInner::Locked(g) => g,
        }
    }
}

impl<T> std::ops::DerefMut for FreezeGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        match &mut self.inner {
            GuardInner::Fast { payload, .. } => payload,
            GuardInner::Locked(g) => g,
        }
    }
}

#[cfg(debug_assertions)]
impl<T> Drop for FreezeGuard<'_, T> {
    fn drop(&mut self) {
        if let GuardInner::Fast { held, .. } = &self.inner {
            held.store(false, Ordering::Relaxed);
            LIVE_FAST_GUARDS.with(|c| c.set(c.get() - 1));
        }
    }
}

impl<T> Freezable<T> {
    pub fn new(payload: T) -> Freezable<T> {
        Freezable {
            flags: AtomicU8::new(0),
            payload: Mutex::new(payload),
            #[cfg(debug_assertions)]
            fast_held: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Lock the payload -- or, while the program is provably single-threaded
    /// (`gvl::sole_thread`, the same claim `IvarCell`/`CivarSlot` already
    /// rely on), hand out the payload directly and skip the mutex CAS pair.
    /// This sits under EVERY array index, hash probe and string byte access,
    /// once per element inside the fused iteration loops.
    ///
    /// SAFETY of the fast arm: `sole_thread()` guarantees no other thread can
    /// reach this cell, and two OVERLAPPING guards on one cell from the same
    /// thread cannot exist in a working program -- on the locked path that
    /// exact shape is a parking_lot self-deadlock, so any code that did it
    /// would already hang the corpus today. Debug builds (which run the whole
    /// golden corpus) assert the no-overlap invariant per cell below rather
    /// than trusting it.
    #[inline]
    pub fn lock(&self) -> FreezeGuard<'_, T> {
        if crate::gvl::sole_thread() {
            #[cfg(debug_assertions)]
            {
                self.assert_unheld();
                LIVE_FAST_GUARDS.with(|c| c.set(c.get() + 1));
            }
            // SAFETY: see above -- sole thread, no live guard on this cell.
            let payload = unsafe { &mut *self.payload.data_ptr() };
            return FreezeGuard {
                inner: GuardInner::Fast {
                    payload,
                    #[cfg(debug_assertions)]
                    held: &self.fast_held,
                },
            };
        }
        FreezeGuard {
            inner: GuardInner::Locked(self.payload.lock()),
        }
    }

    /// Debug-build half of the fast path's safety argument: a second fast
    /// guard while one is live means aliased `&mut` in a release build, so
    /// it must fail HERE, where the whole corpus runs.
    #[cfg(debug_assertions)]
    fn assert_unheld(&self) {
        assert!(
            !self.fast_held.swap(true, Ordering::Relaxed),
            "re-entrant container access on the sole-thread fast path: this \
             thread already holds this container's guard. On the locked path \
             this is a self-deadlock; on the fast path it would alias &mut."
        );
    }

    pub fn is_frozen(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & FLAG_FROZEN != 0
    }

    pub fn set_frozen(&self) {
        self.flags.fetch_or(FLAG_FROZEN, Ordering::Relaxed);
    }

    /// Whether a `send(obj, move: true)` gutted this container -- every
    /// later send to it must raise `Ractor::MovedError`.
    pub fn is_moved(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & FLAG_MOVED != 0
    }

    /// Set strictly BEFORE the payload is gutted (`ractor::cross_graph`'s
    /// commit), so a reader that beats the gut still sees the poison.
    pub fn set_moved(&self) {
        self.flags.fetch_or(FLAG_MOVED, Ordering::Relaxed);
    }
}

/// `RArray`'s storage: a `Vec` plus a HEAD OFFSET, so `shift` is O(1) --
/// CRuby's own beg-offset array design. `bm_so_lists` (a shift/push queue
/// churn) spent 98% of its samples in `Vec::remove(0)`'s memmove before
/// this.
///
/// The struct `Deref`s to the LIVE slice (`buf[beg..]`), so every read-only
/// consumer -- `get`/`len`/`iter`/`last`/indexing/the slice sort family --
/// compiles unchanged, and `DerefMut` covers in-place element writes. The
/// O(1) ends (`push`/`pop`/`shift`) are inherent methods; every other
/// STRUCTURAL `Vec` operation goes through [`ArrayStore::vec`], which first
/// compacts the dead prefix away so indices mean what the caller thinks.
/// A shifted-out slot is overwritten with `Nil` immediately (values drop
/// eagerly; a `Nil` slot holds no heap), and the prefix itself is reclaimed
/// when the store empties, when it exceeds half the buffer past a
/// threshold (amortized O(1) -- each compaction pays for at least as many
/// shifts), or on the next structural op.
#[derive(Default)]
pub struct ArrayStore {
    beg: usize,
    buf: Vec<RubyValue>,
}

impl ArrayStore {
    /// The raw `Vec`, with the dead prefix compacted away first -- the
    /// funnel every structural operation (insert/remove/drain/split_off/
    /// retain/...) takes.
    pub fn vec(&mut self) -> &mut Vec<RubyValue> {
        if self.beg > 0 {
            self.buf.drain(..self.beg);
            self.beg = 0;
        }
        &mut self.buf
    }

    pub fn push(&mut self, v: RubyValue) {
        self.buf.push(v);
    }

    pub fn pop(&mut self) -> Option<RubyValue> {
        if self.buf.len() > self.beg {
            self.buf.pop()
        } else {
            None
        }
    }

    /// The O(1) head removal `shift` exists for.
    pub fn shift(&mut self) -> Option<RubyValue> {
        if self.buf.len() == self.beg {
            return None;
        }
        let v = std::mem::replace(&mut self.buf[self.beg], RubyValue::Nil);
        self.beg += 1;
        if self.beg == self.buf.len() {
            self.buf.clear();
            self.beg = 0;
        } else if self.beg >= 512 && self.beg * 2 >= self.buf.len() {
            self.buf.drain(..self.beg);
            self.beg = 0;
        }
        Some(v)
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.beg = 0;
    }

    /// Append-only, so the prefix can stay: `extend` never re-indexes.
    pub fn extend<I: IntoIterator<Item = RubyValue>>(&mut self, items: I) {
        self.buf.extend(items);
    }

    pub fn append(&mut self, other: &mut Vec<RubyValue>) {
        self.buf.append(other);
    }

    pub fn insert(&mut self, index: usize, v: RubyValue) {
        self.vec().insert(index, v);
    }

    pub fn remove(&mut self, index: usize) -> RubyValue {
        self.vec().remove(index)
    }

    pub fn truncate(&mut self, len: usize) {
        let beg = self.beg;
        self.buf.truncate(beg + len);
    }

    pub fn split_off(&mut self, at: usize) -> Vec<RubyValue> {
        let v = self.vec();
        v.split_off(at)
    }

    pub fn retain<F: FnMut(&RubyValue) -> bool>(&mut self, f: F) {
        self.vec().retain(f);
    }

    pub fn drain<R>(&mut self, range: R) -> std::vec::Drain<'_, RubyValue>
    where
        R: std::ops::RangeBounds<usize>,
    {
        self.vec().drain(range)
    }

    pub fn resize(&mut self, new_len: usize, v: RubyValue) {
        self.vec().resize(new_len, v);
    }

    pub fn into_vec(mut self) -> Vec<RubyValue> {
        if self.beg > 0 {
            self.buf.drain(..self.beg);
        }
        self.buf
    }
}

impl From<Vec<RubyValue>> for ArrayStore {
    fn from(buf: Vec<RubyValue>) -> ArrayStore {
        ArrayStore { beg: 0, buf }
    }
}

impl FromIterator<RubyValue> for ArrayStore {
    fn from_iter<I: IntoIterator<Item = RubyValue>>(iter: I) -> ArrayStore {
        ArrayStore::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl IntoIterator for ArrayStore {
    type Item = RubyValue;
    type IntoIter = std::vec::IntoIter<RubyValue>;
    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a> IntoIterator for &'a ArrayStore {
    type Item = &'a RubyValue;
    type IntoIter = std::slice::Iter<'a, RubyValue>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Clone for ArrayStore {
    fn clone(&self) -> ArrayStore {
        ArrayStore::from(self.to_vec())
    }
}

impl std::ops::Deref for ArrayStore {
    type Target = [RubyValue];
    fn deref(&self) -> &[RubyValue] {
        &self.buf[self.beg..]
    }
}

impl std::ops::DerefMut for ArrayStore {
    fn deref_mut(&mut self) -> &mut [RubyValue] {
        let beg = self.beg;
        &mut self.buf[beg..]
    }
}

pub type RArray = Arc<Freezable<ArrayStore>>;

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
/// `Object#hash` before a user overrides it. (A user-defined `#hash` IS
/// consulted for `Object` keys -- see `hash_key`'s `Object` arm below.)
// `Ord` only so a `Hash` key's pairs can be put in a canonical order -- see
// that variant. The order itself has no meaning to ruby.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
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
    /// A Regexp key, by SOURCE and flags -- ruby's own `Regexp#eql?`/`#hash`
    /// rule, under which `/a/` written twice is one key. The flags travel as
    /// a bitmask (ignore-case 1, extended 2, multiline 4) so `/a/i` and `/a/`
    /// stay apart.
    Regexp(String, u8),
    /// A first-class class/module value: keyed by class
    /// identity, exactly real Ruby's `Class#hash`/`#eql?` (two references
    /// to the same class are one key).
    Class(u32),
    /// An Object key whose class defines its own `hash`: the
    /// projection of that method's RESULT. Wrapped (not flattened into the
    /// result's own variant) so a user-hashed object never collides with a
    /// plain value that happens to equal its hash. Documented
    /// approximation: two keys with `hash`-equal results are ONE key here
    /// even if their `eql?` would disagree (this table has no second
    /// eql?-verification pass); in practice classes define the two
    /// consistently.
    Computed(Box<HashKey>),
    Identity(usize),
    /// The numeric-tower keys -- `BigInt` never overlaps
    /// `Int` (demotion invariant), `Rational` is always reduced, `Complex`
    /// keys by its component keys.
    BigInt(num_bigint::BigInt),
    Rational(num_bigint::BigInt, num_bigint::BigInt),
    Complex(Box<(HashKey, HashKey)>),
    /// A Hash key, by its PAIRS. Ruby's `Hash#hash`/`#eql?` are order
    /// INSENSITIVE, so the pairs are sorted into a canonical order when the
    /// key is built (`hash_key`) rather than compared as a multiset here --
    /// which keeps the derived `PartialEq` correct without hand-writing one
    /// for the whole enum, on the hottest equality surface in this module.
    Hash(Vec<(HashKey, HashKey)>),
}

/// `HashKey::Str`'s hash tag -- shared with [`StrProbe`], whose whole point
/// is hashing identically to the owned variant.
const HK_STR_TAG: u8 = 5;

/// Hand-written (not derived) so the borrowed [`StrProbe`] can reproduce the
/// `Str` arm exactly: explicit variant tags instead of the derive's opaque
/// discriminant hashing, same field order. `&[u8]` and `Vec<u8>` hash
/// identically by std's own slice rule, which is what makes the borrowed
/// twin sound.
impl std::hash::Hash for HashKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            HashKey::Nil => state.write_u8(0),
            HashKey::Bool(b) => {
                state.write_u8(1);
                b.hash(state);
            }
            HashKey::Int(i) => {
                state.write_u8(2);
                i.hash(state);
            }
            HashKey::Float(f) => {
                state.write_u8(3);
                f.hash(state);
            }
            HashKey::Symbol(s) => {
                state.write_u8(4);
                s.hash(state);
            }
            HashKey::Str(b, t) => {
                state.write_u8(HK_STR_TAG);
                b.hash(state);
                t.hash(state);
            }
            HashKey::Array(v) => {
                state.write_u8(6);
                v.hash(state);
            }
            // Sorted at construction, so hashing the `Vec` in order is already
            // the order-insensitive answer ruby gives.
            HashKey::Hash(pairs) => {
                state.write_u8(17);
                pairs.hash(state);
            }
            HashKey::Regexp(src, flags) => {
                state.write_u8(16);
                src.hash(state);
                flags.hash(state);
            }
            HashKey::Range(a, b, x) => {
                state.write_u8(7);
                a.hash(state);
                b.hash(state);
                x.hash(state);
            }
            HashKey::Class(c) => {
                state.write_u8(8);
                c.hash(state);
            }
            HashKey::Computed(k) => {
                state.write_u8(9);
                k.hash(state);
            }
            HashKey::Identity(p) => {
                state.write_u8(10);
                p.hash(state);
            }
            HashKey::BigInt(b) => {
                state.write_u8(11);
                b.hash(state);
            }
            HashKey::Rational(n, d) => {
                state.write_u8(12);
                n.hash(state);
                d.hash(state);
            }
            HashKey::Complex(c) => {
                state.write_u8(13);
                c.hash(state);
            }
        }
    }
}

/// A borrowed string-key probe, `Hash + Equivalent<HashKey>`: the structural
/// projection in `hash_key_in` copies every string key's whole byte buffer
/// per LOOKUP; probing by `&[u8]` + tag skips that on the read paths (and on
/// `hash_set`'s already-present path). Only meaningful against a structural
/// (non-identity) table -- every user gates on `compare_by_identity` first.
struct StrProbe<'a>(&'a [u8], u8);

impl std::hash::Hash for StrProbe<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u8(HK_STR_TAG);
        self.0.hash(state);
        self.1.hash(state);
    }
}

impl indexmap::Equivalent<HashKey> for StrProbe<'_> {
    fn equivalent(&self, key: &HashKey) -> bool {
        matches!(key, HashKey::Str(b, t) if *t == self.1 && b.as_slice() == self.0)
    }
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
        RubyValue::Complex(c) => HashKey::Complex(Box::new((hash_key(&c.real), hash_key(&c.imag)))),
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
        RubyValue::Hash(h) => {
            // A SNAPSHOT, not the live map: projecting a value can dispatch a
            // user `hash`, which may read the very hash being projected, and
            // the payload lock is not reentrant.
            let mut pairs: Vec<(HashKey, HashKey)> = hash_pairs_snapshot(h)
                .iter()
                .map(|(k, v)| (hash_key(k), hash_key(v)))
                .collect();
            pairs.sort();
            HashKey::Hash(pairs)
        }
        // `Arc<dyn Trait>`'s pointer is a FAT pointer (data + vtable) -- cast
        // through `*const ()` first to get a plain, `usize`-castable thin
        // pointer to the data alone (the vtable half is irrelevant to
        // identity).
        //
        // A user-defined `hash` (dispatched once per
        // insertion/lookup through the registry) projects the object
        // through its RESULT -- see `HashKey::Computed`'s docs; identity
        // stays the default (real Ruby's own `Object#hash`). A `hash` that
        // raises is a loud panic (no exception channel here).
        RubyValue::Object(o) => match crate::dispatch::call_user_method(o, "hash", &[]) {
            Some(Ok(v)) => HashKey::Computed(Box::new(hash_key(&v))),
            Some(Err(_)) => panic!(
                "a user-defined `hash` raised inside a Hash key lookup (zeo limitation: no exception channel here)"
            ),
            // A value-builtin subclass (D3) with no `hash` override keys by its
            // payload -- `Tag.new("k")` is the same Hash key as `"k"`.
            None => match o.builtin_payload() {
                Some(p) => hash_key_in(&p, by_identity),
                None => HashKey::Identity(Arc::as_ptr(o) as *const () as usize),
            },
        },
        RubyValue::Proc(p) => HashKey::Identity(p.ptr_id()),
        // Value-based, like ruby's own `Regexp#eql?`/`#hash`: two regexps with
        // the same source and flags are ONE key, however they were built.
        RubyValue::Regexp(r) => HashKey::Regexp(
            r.source.clone(),
            u8::from(r.ignore_case) | u8::from(r.extended) << 1 | u8::from(r.multiline) << 2,
        ),
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

/// `Object#hash`'s universal answer: an `i64` digest of the
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
/// The entry table's concrete type: insertion-ordered, foldhash-hashed.
/// INTERNAL hashing only -- Ruby-visible `Object#hash` values stay on
/// `DefaultHasher` (see `value_hash_code`), and iteration order is the
/// insertion order `IndexMap` maintains regardless of hasher.
pub type HashPairs = IndexMap<HashKey, (RubyValue, RubyValue), foldhash::fast::RandomState>;

/// One stored entry: the projected key beside the `(original key, value)`
/// pair -- the same `(K, V)` shape the `IndexMap` holds, so the two reprs
/// answer every question identically.
type HashRow = (HashKey, (RubyValue, RubyValue));

/// CRuby keeps a hash under this many entries as a flat array (`ar_table`)
/// and promotes to a real table on growth; the same trade holds here. A
/// small hash is the overwhelming shape -- every kwargs bundle, every small
/// literal -- and a `Vec` of rows is one allocation (none while empty)
/// against the table's two, with a linear probe that beats hashing at these
/// lengths.
const SMALL_HASH_MAX: usize = 8;

/// The two storages behind a Hash. `Small` never holds more than
/// [`SMALL_HASH_MAX`] rows; growth past that promotes to `Big` and never
/// demotes (CRuby's own behavior), so a hash that was ever big stays a
/// table.
enum HashRepr {
    Small(Vec<HashRow>),
    Big(HashPairs),
}

pub struct RHashData {
    repr: HashRepr,
    pub default: RubyValue,
    pub default_proc: Option<RubyValue>,
    /// `Hash#compare_by_identity`: when set, keys project by object identity
    /// (`equal?`/`object_id`) instead of structure -- see `hash_key_in`.
    pub compare_by_identity: bool,
    /// Live iterations over this hash (nested `each`es stack) -- while
    /// nonzero, inserting a NEW key raises ruby's "can't add a new key into
    /// hash during iteration". Updates to existing keys, and deletes, stay
    /// legal, as in CRuby. See [`hash_iter_guard`]/[`hash_set_checked`].
    pub(crate) iterating: u32,
    /// The caller WROTE keywords -- CRuby's `rb_keyword_given_p`, carried on
    /// the trailing hash a dynamic call builds from them (`**h` splat sites
    /// and `send`'s literal kwargs both set it). Read by callees that decide
    /// behavior from keyword-vs-positional-Hash: a `**nil` declaration
    /// refuses a marked hash, `Struct#initialize` binds one by member name.
    /// Never copied by `dup`/`merge` (both rebuild via [`hash_new`]).
    pub(crate) kw_marked: bool,
}

impl RHashData {
    fn new() -> RHashData {
        RHashData {
            // Empty and allocation-free until the first insert.
            repr: HashRepr::Small(Vec::new()),
            default: RubyValue::Nil,
            default_proc: None,
            compare_by_identity: false,
            iterating: 0,
            kw_marked: false,
        }
    }

    /// Build straight from projected rows, choosing the repr by count.
    fn from_rows(rows: Vec<HashRow>) -> RHashData {
        let mut data = RHashData::new();
        data.repr = if rows.len() <= SMALL_HASH_MAX {
            HashRepr::Small(rows)
        } else {
            HashRepr::Big(rows.into_iter().collect())
        };
        data
    }

    /// `IndexMap::insert`'s exact contract on either repr: an equivalent
    /// existing key keeps its place (and the stored `HashKey`), its value is
    /// replaced and the old one returned. A `Small` repr past
    /// [`SMALL_HASH_MAX`] promotes first, so the vec never grows beyond the
    /// scan length that makes it worth having.
    pub fn insert(
        &mut self,
        key: HashKey,
        value: (RubyValue, RubyValue),
    ) -> Option<(RubyValue, RubyValue)> {
        match &mut self.repr {
            HashRepr::Small(rows) => {
                if let Some((_, slot)) = rows.iter_mut().find(|(k, _)| *k == key) {
                    return Some(std::mem::replace(slot, value));
                }
                if rows.len() < SMALL_HASH_MAX {
                    rows.push((key, value));
                    return None;
                }
                let mut map: HashPairs = std::mem::take(rows).into_iter().collect();
                let old = map.insert(key, value);
                self.repr = HashRepr::Big(map);
                old
            }
            HashRepr::Big(map) => map.insert(key, value),
        }
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&(RubyValue, RubyValue)>
    where
        Q: std::hash::Hash + indexmap::Equivalent<HashKey> + ?Sized,
    {
        match &self.repr {
            HashRepr::Small(rows) => {
                rows.iter().find(|(k, _)| key.equivalent(k)).map(|(_, v)| v)
            }
            HashRepr::Big(map) => map.get(key),
        }
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        Q: std::hash::Hash + indexmap::Equivalent<HashKey> + ?Sized,
    {
        match &self.repr {
            HashRepr::Small(rows) => rows.iter().any(|(k, _)| key.equivalent(k)),
            HashRepr::Big(map) => map.contains_key(key),
        }
    }

    pub fn get_index_of<Q>(&self, key: &Q) -> Option<usize>
    where
        Q: std::hash::Hash + indexmap::Equivalent<HashKey> + ?Sized,
    {
        match &self.repr {
            HashRepr::Small(rows) => rows.iter().position(|(k, _)| key.equivalent(k)),
            HashRepr::Big(map) => map.get_index_of(key),
        }
    }

    pub fn get_index_mut(&mut self, i: usize) -> Option<(&HashKey, &mut (RubyValue, RubyValue))> {
        match &mut self.repr {
            HashRepr::Small(rows) => rows.get_mut(i).map(|(k, v)| (&*k, v)),
            HashRepr::Big(map) => map.get_index_mut(i),
        }
    }

    /// Order-preserving removal, `IndexMap::shift_remove`'s contract.
    pub fn shift_remove<Q>(&mut self, key: &Q) -> Option<(RubyValue, RubyValue)>
    where
        Q: std::hash::Hash + indexmap::Equivalent<HashKey> + ?Sized,
    {
        match &mut self.repr {
            HashRepr::Small(rows) => {
                let i = rows.iter().position(|(k, _)| key.equivalent(k))?;
                Some(rows.remove(i).1)
            }
            HashRepr::Big(map) => map.shift_remove(key),
        }
    }

    pub fn len(&self) -> usize {
        match &self.repr {
            HashRepr::Small(rows) => rows.len(),
            HashRepr::Big(map) => map.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clears the entries; the repr stays what it was (a big hash never
    /// demotes, CRuby's own behavior).
    pub fn clear(&mut self) {
        match &mut self.repr {
            HashRepr::Small(rows) => rows.clear(),
            HashRepr::Big(map) => map.clear(),
        }
    }

    pub fn values(&self) -> HashValuesIter<'_> {
        match &self.repr {
            HashRepr::Small(rows) => HashValuesIter::Small(rows.iter()),
            HashRepr::Big(map) => HashValuesIter::Big(map.values()),
        }
    }

    pub fn iter(&self) -> HashPairsIter<'_> {
        match &self.repr {
            HashRepr::Small(rows) => HashPairsIter::Small(rows.iter()),
            HashRepr::Big(map) => HashPairsIter::Big(map.iter()),
        }
    }
}

/// [`RHashData::values`] over either repr -- `&(original key, value)` rows
/// in insertion order, mirroring `IndexMap::values`.
pub enum HashValuesIter<'a> {
    Small(std::slice::Iter<'a, HashRow>),
    Big(indexmap::map::Values<'a, HashKey, (RubyValue, RubyValue)>),
}

impl<'a> Iterator for HashValuesIter<'a> {
    type Item = &'a (RubyValue, RubyValue);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            HashValuesIter::Small(i) => i.next().map(|(_, v)| v),
            HashValuesIter::Big(i) => i.next(),
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            HashValuesIter::Small(i) => i.size_hint(),
            HashValuesIter::Big(i) => i.size_hint(),
        }
    }
}

impl DoubleEndedIterator for HashValuesIter<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            HashValuesIter::Small(i) => i.next_back().map(|(_, v)| v),
            HashValuesIter::Big(i) => i.next_back(),
        }
    }
}

impl ExactSizeIterator for HashValuesIter<'_> {}

/// [`RHashData::iter`] over either repr -- `(&HashKey, &(key, value))`
/// pairs in insertion order, mirroring `IndexMap::iter`.
pub enum HashPairsIter<'a> {
    Small(std::slice::Iter<'a, HashRow>),
    Big(indexmap::map::Iter<'a, HashKey, (RubyValue, RubyValue)>),
}

impl<'a> Iterator for HashPairsIter<'a> {
    type Item = (&'a HashKey, &'a (RubyValue, RubyValue));

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            HashPairsIter::Small(i) => i.next().map(|(k, v)| (k, v)),
            HashPairsIter::Big(i) => i.next(),
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            HashPairsIter::Small(i) => i.size_hint(),
            HashPairsIter::Big(i) => i.size_hint(),
        }
    }
}

impl DoubleEndedIterator for HashPairsIter<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            HashPairsIter::Small(i) => i.next_back().map(|(k, v)| (k, v)),
            HashPairsIter::Big(i) => i.next_back(),
        }
    }
}

impl ExactSizeIterator for HashPairsIter<'_> {}

/// Tag `h` as a keyword set (see [`RHashData::kw_marked`]).
pub fn hash_mark_kwargs(h: &RHash) {
    h.lock().kw_marked = true;
}

/// Whether the caller wrote keywords to build `h` (see [`RHashData::kw_marked`]).
pub fn hash_is_kwargs(h: &RHash) -> bool {
    h.lock().kw_marked
}

pub type RHash = Arc<Freezable<RHashData>>;

pub type RStr = Arc<Freezable<crate::encoding::StrBuf>>;

#[inline]
pub fn array_new(elems: Vec<RubyValue>) -> RArray {
    Arc::new(Freezable::new(ArrayStore::from(elems)))
}

/// The pairs snapshot `Hash#each` iterates -- cloned under ONE lock
/// acquisition, so the block runs lock-free and may mutate the receiver
/// (today's runtime rule). Public because the fused inline `each` loop
/// walks the very same snapshot.
pub fn hash_pairs_snapshot(h: &RHash) -> Vec<(RubyValue, RubyValue)> {
    h.lock().values().cloned().collect()
}

/// `hash_pairs_snapshot`'s twin for arrays: the elements cloned under ONE
/// lock acquisition, so whatever walks them afterwards runs lock-free.
///
/// Use this wherever the walk can re-enter the receiver -- a user `inspect`
/// that reads the array it is being printed from, or an element that IS the
/// receiver. The payload `Mutex` is not reentrant, so holding the guard
/// across such a call hangs the process with no output and no error (the
/// same failure mode fb30c7be fixed for `s + s`).
pub fn array_snapshot(a: &RArray) -> Vec<RubyValue> {
    a.lock().iter().cloned().collect()
}

/// Ruby's own `Array#[]`: negative indices count from the end, and an
/// out-of-range index returns `nil` rather than raising/panicking.
#[inline]
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
/// proper `Signal::Raise(IndexError.new(...))`. The actual message/exception
/// CONSTRUCTION happens in codegen, not here, since only codegen has the class
/// registry needed to build an `IndexError` value.
#[inline]
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

#[inline]
pub fn array_len(arr: &RArray) -> i64 {
    arr.lock().len() as i64
}

/// The moved-container guard the fallible `_checked` twins below share, and
/// codegen's own emission for the mutator fast paths when the program can
/// reach `Ractor` at all (`try_collection_dispatch`'s switch). Checked ahead
/// of any frozen guard: `Ractor::MovedError` beats `FrozenError`.
pub fn check_not_moved<T>(c: &Freezable<T>) -> Result<(), crate::Signal> {
    if c.is_moved() {
        return Err(crate::ractor::moved_object_error());
    }
    Ok(())
}

// The infallible container fast-path helpers above serve programs that can
// never move an object; a program that names `Ractor` compiles against these
// fallible twins instead, so a husk left by `send(obj, move: true)` raises
// rather than answering from its gutted payload.
#[inline]
pub fn array_get_checked(arr: &RArray, index: i64) -> Result<RubyValue, crate::Signal> {
    check_not_moved(arr)?;
    Ok(array_get(arr, index))
}

#[inline]
pub fn array_len_checked(arr: &RArray) -> Result<i64, crate::Signal> {
    check_not_moved(arr)?;
    Ok(array_len(arr))
}

pub fn hash_len_checked(h: &RHash) -> Result<i64, crate::Signal> {
    check_not_moved(h)?;
    Ok(hash_len(h))
}

#[inline]
pub fn string_get_checked(s: &RStr, index: i64) -> Result<RubyValue, crate::Signal> {
    check_not_moved(s)?;
    Ok(string_get(s, index))
}

pub fn string_len_checked(s: &RStr) -> Result<i64, crate::Signal> {
    check_not_moved(s)?;
    Ok(string_len(s))
}

/// Flattens a `*splat` element in place, with Ruby's own coercion rules --
/// splatting a non-Array is ordinary Ruby, not an error.
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
            let to_a = crate::symbol::wk::to_a();
            if crate::dispatch::responds_to(other.class_id(), to_a, false) {
                let arr = crate::dispatch::send_value(other, to_a, &[], None)?;
                match arr {
                    RubyValue::Array(a) => out.extend(a.lock().iter().cloned()),
                    // A `to_a` that doesn't answer an Array is CRuby's
                    // "can't convert X to Array" TypeError.
                    bad => {
                        return Err(type_error!(
                            "can't convert {} to Array ({}#to_a gives {})",
                            crate::builtins::class_name_of(other),
                            crate::builtins::class_name_of(other),
                            crate::builtins::class_name_of(&bad)
                        ));
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

#[inline]
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
    let mut data = RHashData::new();
    data.default = default;
    data.default_proc = default_proc;
    Arc::new(Freezable::new(data))
}

/// Copy `src`'s per-instance default (value/proc) and `compare_by_identity`
/// flag onto `dst`. `Hash#dup`/`#clone` preserve these, and `Hash#merge`
/// inherits them from the left receiver -- both of which build a fresh hash
/// via `hash_new` (default-less) and would otherwise lose them.
pub fn copy_hash_meta(src: &RHash, dst: &RHash) {
    let s = src.lock();
    let mut d = dst.lock();
    d.default = s.default.clone();
    d.default_proc = s.default_proc.clone();
    d.compare_by_identity = s.compare_by_identity;
}

/// A plain lookup: the stored value, or `nil` for a missing key -- WITHOUT
/// triggering any per-instance default. This is the internal read used by
/// `merge`/`dig`/keyword extraction/pattern matching, none of which invoke a
/// hash's default in real Ruby. The default-triggering `Hash#[]` is
/// `hash_index`.
#[inline]
pub fn hash_get(h: &RHash, key: &RubyValue) -> RubyValue {
    let g = h.lock();
    if let RubyValue::Str(s) = key
        && !g.compare_by_identity
    {
        let sb = s.lock();
        return g
            .get(&StrProbe(sb.bytes(), sb.hash_key_tag()))
            .map(|(_, v)| v.clone())
            .unwrap_or(RubyValue::Nil);
    }
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
        let hit = if let (RubyValue::Str(s), false) = (key, g.compare_by_identity) {
            let sb = s.lock();
            g.get(&StrProbe(sb.bytes(), sb.hash_key_tag()))
        } else {
            let k = hash_key_in(key, g.compare_by_identity);
            g.get(&k)
        };
        if let Some((_, v)) = hit {
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
            crate::symbol::wk::call(),
            &[RubyValue::Hash(h.clone()), key.clone()],
            None,
        ),
        None => Ok(default),
    }
}

/// CRuby snapshots a String key on store (`rb_hash_aset` ->
/// `rb_hash_key_str`): the hash keeps a FROZEN COPY, so mutating the
/// caller's string afterwards cannot rewrite a key that is already in the
/// table. Two keys stay exempt -- an already-frozen string (nothing can
/// change it, so the copy would be waste) and any key under identity
/// comparison (there the caller's object IS the key).
///
/// Only KEYS. A stored value is deliberately the caller's shared handle:
/// `h[:k] << "!"` must be visible through `h`.
fn snapshot_key(key: RubyValue, by_identity: bool) -> RubyValue {
    match &key {
        RubyValue::Str(s) if !by_identity && !s.is_frozen() => {
            let copy = string_wrap(s.lock().clone());
            copy.set_frozen();
            RubyValue::Str(copy)
        }
        _ => key,
    }
}

/// `Hash#[]=`: replaces an existing key's VALUE in place, preserving its
/// original insertion position (matching real Ruby) rather than moving it to
/// the end -- `IndexMap::insert`'s own documented behavior for a
/// re-inserted, already-present key.
///
/// The one insertion point every literal, every `[]=`/`store`/`merge` row and
/// every runtime hash builder funnels through, which is what makes
/// [`snapshot_key`] a single edit rather than an audit.
#[inline]
pub fn hash_set(h: &RHash, key: RubyValue, value: RubyValue) -> RubyValue {
    let mut g = h.lock();
    if let RubyValue::Str(s) = &key
        && !g.compare_by_identity
    {
        // Probe borrowed first: a re-assigned string key (the common
        // accumulate-into-hash loop) skips the byte-buffer projection
        // entirely. The guard must drop before `snapshot_key` re-locks
        // the same string (a non-reentrant Mutex).
        let hit = {
            let sb = s.lock();
            g.get_index_of(&StrProbe(sb.bytes(), sb.hash_key_tag()))
        };
        if let Some(i) = hit {
            let key = snapshot_key(key, false);
            if let Some((_, pair)) = g.get_index_mut(i) {
                *pair = (key, value.clone());
            }
            return value;
        }
    }
    let k = hash_key_in(&key, g.compare_by_identity);
    let key = snapshot_key(key, g.compare_by_identity);
    g.insert(k, (key, value.clone()));
    value
}

/// [`hash_set`] behind ruby's iteration guard: while any `each` is live on
/// this hash, adding a NEW key raises RuntimeError ("can't add a new key
/// into hash during iteration"); updating an existing key stays legal. The
/// user-visible write rows (`[]=`/`store`, codegen's index-assign fast path)
/// come through here; internal builders (literals, kwargs assembly) keep the
/// plain form -- they build hashes nothing can be iterating yet.
pub fn hash_set_checked(
    h: &RHash,
    key: RubyValue,
    value: RubyValue,
) -> Result<RubyValue, crate::Signal> {
    if h.lock().iterating > 0 && !hash_has_key(h, &key) {
        return Err(crate::dispatch::raise_error(
            "RuntimeError",
            "can't add a new key into hash during iteration".to_string(),
        ));
    }
    Ok(hash_set(h, key, value))
}

/// Marks `h` under iteration for the new-key guard, un-marking on drop
/// (nested iterations stack).
pub fn hash_iter_guard(h: &RHash) -> HashIterGuard {
    h.lock().iterating += 1;
    HashIterGuard(h.clone())
}

pub struct HashIterGuard(RHash);

impl Drop for HashIterGuard {
    fn drop(&mut self) {
        self.0.lock().iterating -= 1;
    }
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
    if let RubyValue::Str(s) = key
        && !g.compare_by_identity
    {
        let sb = s.lock();
        return g.contains_key(&StrProbe(sb.bytes(), sb.hash_key_tag()));
    }
    let k = hash_key_in(key, g.compare_by_identity);
    g.contains_key(&k)
}

/// `Hash#delete`: removes the entry, returning its value (`nil` when the
/// key was absent -- the no-default-block scope-cut, same as `hash_get`).
/// `shift_remove` (not plain `swap_remove`) preserves the remaining
/// entries' insertion order, Ruby's own guarantee.
pub fn hash_delete(h: &RHash, key: &RubyValue) -> RubyValue {
    let mut g = h.lock();
    if let RubyValue::Str(s) = key
        && !g.compare_by_identity
    {
        let sb = s.lock();
        let probe = StrProbe(sb.bytes(), sb.hash_key_tag());
        let removed = g.shift_remove(&probe);
        return removed.map(|(_, v)| v).unwrap_or(RubyValue::Nil);
    }
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
    // Rebuild the entries keyed by identity, preserving insertion order.
    let old: Vec<(RubyValue, RubyValue)> = g.values().cloned().collect();
    g.clear();
    for (k, v) in old {
        let ik = hash_key_in(&k, true);
        g.insert(ik, (k, v));
    }
}

/// `Hash#keys` -- the ORIGINAL key values, in insertion order.
pub fn hash_keys(h: &RHash) -> RubyValue {
    RubyValue::Array(array_new(
        h.lock().values().map(|(k, _)| k.clone()).collect(),
    ))
}

/// `Hash#values`, in insertion order.
pub fn hash_values(h: &RHash) -> RubyValue {
    RubyValue::Array(array_new(
        h.lock().values().map(|(_, v)| v.clone()).collect(),
    ))
}

/// Every `(key, value)` pair in insertion order -- the primitive serializers
/// (JSON/YAML) walk to emit a Hash without needing the private `map` field.
pub fn hash_pairs(h: &RHash) -> Vec<(RubyValue, RubyValue)> {
    h.lock().values().cloned().collect()
}

/// `Array#<<`/`#push` -- returns the array itself (Ruby's chaining
/// contract), as an already-boxed value for dynamic-dispatch callers.
#[inline]
pub fn array_push(arr: &RArray, value: RubyValue) -> RubyValue {
    arr.lock().push(value);
    RubyValue::Array(arr.clone())
}

/// `Array#include?` -- `==`-based membership (`RubyValue::rb_eq`), matching
/// real Ruby's `==` (not `eql?`) rule for `include?`.
pub fn array_include(arr: &RArray, value: &RubyValue) -> bool {
    // Per-element lock round-trips: `rb_eq` can re-enter a user `==`, which
    // must not run under the receiver's (non-reentrant) payload lock.
    let mut i = 0usize;
    loop {
        let e = {
            let guard = arr.lock();
            match guard.get(i) {
                Some(e) => e.clone(),
                None => return false,
            }
        };
        if e.rb_eq(value) {
            return true;
        }
        i += 1;
    }
}

/// A new Hash containing every pair from `h` whose key ISN'T in `keys` --
/// backs a hash pattern's `**rest` binding (the leftover key/value pairs not
/// matched by any explicit `key:` entry).
pub fn hash_except_keys(h: &RHash, keys: &[&str]) -> RHash {
    let excluded: Vec<HashKey> = keys
        .iter()
        .map(|k| hash_key(&RubyValue::Symbol(crate::Symbol::intern(k))))
        .collect();
    let rows: Vec<HashRow> = h
        .lock()
        .iter()
        .filter(|(k, _)| !excluded.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Arc::new(Freezable::new(RHashData::from_rows(rows)))
}

#[inline]
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
    Arc::new(Freezable::new(crate::encoding::StrBuf::from_bytes(
        bytes, enc,
    )))
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
/// straightforward later optimization, not a blocker).
#[inline]
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
        assert!(matches!(
            hash_index(&h, &RubyValue::Int(1)).unwrap(),
            RubyValue::Int(10)
        ));
        assert!(matches!(
            hash_index(&h, &RubyValue::Int(2)).unwrap(),
            RubyValue::Nil
        ));
    }

    #[test]
    fn default_value_returns_on_miss_without_inserting() {
        let h = hash_new_with_default(RubyValue::Int(0), None);
        assert!(matches!(
            hash_index(&h, &RubyValue::Symbol(crate::Symbol::intern("x"))).unwrap(),
            RubyValue::Int(0)
        ));
        // The default is NOT stored -- a plain default only reads back.
        assert_eq!(hash_len(&h), 0);
        // A present key still wins over the default.
        hash_set(
            &h,
            RubyValue::Symbol(crate::Symbol::intern("y")),
            RubyValue::Int(5),
        );
        assert!(matches!(
            hash_index(&h, &RubyValue::Symbol(crate::Symbol::intern("y"))).unwrap(),
            RubyValue::Int(5)
        ));
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

    /// Splatting a non-Array is ordinary Ruby, not an error.
    /// Every case here is oracle-verified against ruby 4.0.6.
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

    /// `Hash#[]`/`#[]=`/`#delete`/`#key?` honor the flag once set, and a
    /// String key stored BEFORE the switch becomes unreachable by the object
    /// it was stored under -- the hash kept a frozen snapshot of it, and
    /// under identity keying that snapshot is a different object. Verified
    /// against ruby 4.0.6, which answers `nil` there too.
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
        // Neither the object stored nor an equal one reaches the snapshot.
        assert!(matches!(hash_get(&h2, &a), RubyValue::Nil));
        assert!(matches!(hash_get(&h2, &b), RubyValue::Nil));
        assert!(!hash_has_key(&h2, &a));
        assert!(!hash_has_key(&h2, &b));
        // The entry is still there, keyed by the snapshot -- which is frozen,
        // and is not the object it was made from.
        assert_eq!(hash_len(&h2), 1);
        let stored = h2.lock().values().next().unwrap().0.clone();
        let (RubyValue::Str(stored), RubyValue::Str(a_str)) = (&stored, &a) else {
            panic!("a String key stays a String")
        };
        assert!(stored.is_frozen());
        assert!(!Arc::ptr_eq(stored, a_str));

        // Under identity, keys stored AFTER the switch are the caller's own
        // objects, so two distinct equal Strings coexist and each is reachable.
        hash_set(&h2, a.clone(), RubyValue::Int(2));
        hash_set(&h2, b.clone(), RubyValue::Int(3));
        assert_eq!(hash_len(&h2), 3);
        assert!(is_int(&hash_get(&h2, &a), 2));
        assert!(is_int(&hash_delete(&h2, &a), 2));
        assert_eq!(hash_len(&h2), 2);
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

    /// The borrowed probe must hash exactly like the owned `HashKey::Str` it
    /// stands in for -- the whole soundness condition of `StrProbe`.
    #[test]
    fn str_probe_hashes_like_the_owned_key() {
        use std::hash::{BuildHasher, Hash, Hasher};
        let state = foldhash::fast::RandomState::default();
        for text in ["", "a", "key with spaces", "ünïcode"] {
            let sb = crate::encoding::StrBuf::from_utf8(text.to_string());
            let owned = HashKey::Str(sb.bytes().to_vec(), sb.hash_key_tag());
            let (mut h1, mut h2) = (state.build_hasher(), state.build_hasher());
            owned.hash(&mut h1);
            StrProbe(sb.bytes(), sb.hash_key_tag()).hash(&mut h2);
            assert_eq!(
                h1.finish(),
                h2.finish(),
                "probe/owned diverged for {text:?}"
            );
        }
        // And end to end: a string key stored owned is found via the probe
        // paths (get/has_key/delete all take them).
        let h = hash_new(vec![]);
        hash_set(&h, s("k"), RubyValue::Int(7));
        assert!(is_int(&hash_get(&h, &s("k")), 7));
        assert!(hash_has_key(&h, &s("k")));
        assert!(is_int(&hash_delete(&h, &s("k")), 7));
        assert!(!hash_has_key(&h, &s("k")));
    }
}
