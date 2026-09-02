//! A heap `VALUE` is a handle.
//!
//! # Why a handle and not the object
//!
//! zeo's heap values are `Arc`s over Rust types -- an `RStr` is
//! `Arc<Freezable<StrBuf>>` and an `RObj` is an `Arc<dyn RubyObject>`. None of
//! them begins with MRI's `struct RBasic`, none of them has a stable byte
//! buffer at a fixed offset, and an `Arc<dyn _>` is not even one word wide.
//! So a `VALUE` cannot be the object's own address.
//!
//! It is instead the address of a [`Handle`]: a heap cell zeo owns, whose
//! first two words ARE a real `struct RBasic` and whose remaining fields hold
//! the strong reference and the `Data`/`TypedData` slot. That buys three
//! things at once, and each pays for a header that stays unpatched:
//!
//! * `RBASIC(v)->flags`, `RBASIC_CLASS`, `RB_FL_TEST_RAW`, `RB_BUILTIN_TYPE`,
//!   `RB_TYPE_P` and `OBJ_FROZEN` read the truth with no patch at all.
//! * A handle is 8-aligned and non-zero, so `RB_SPECIAL_CONST_P` is right
//!   unpatched too -- see [`super::value`] for the other half of that.
//! * A handle is **canonical per object**, so `a == b` on two `VALUE`s is
//!   object identity, exactly as it is in MRI.
//!
//! # Lifetime
//!
//! A handle holds a strong `RubyValue`, so while it lives the object cannot
//! be collected. What decides how long it lives is the scope
//! ([`super::scope`]): every Ruby-to-C entry pushes one, every handle a
//! `rb_*` hands out is pinned in the current scope, and scope pop releases
//! those pins. A handle whose last pin goes and that no
//! `rb_gc_register_address` root names is dropped, and the identity is free
//! to be handed to a later object.
//!
//! That is the reason the table is keyed by identity rather than being a
//! bare leak: a program that builds a million strings inside one C call must
//! not keep a million handles after it returns.
//!
//! # Threads
//!
//! Loading a C extension arms the GVL, so at most one thread is inside C at a
//! time. `flags` is still an atomic: C writes it with a plain non-atomic RMW
//! (`FL_SET` is `|=`), and Rust must be able to read it without that being
//! undefined. MRI has the identical arrangement for the identical reason.

use super::value::{self, Value};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use zeo_rt::RubyValue;
use zeo_rt::dispatch::ClassId;

/// `ruby_value_type`, from `ruby/internal/value_type.h`. Every one of these
/// was read off the oracle with `ObjectSpace.dump`, not guessed: `Range` is
/// `T_STRUCT` and `Proc` is `T_DATA`, and neither is obvious.
///
/// `T_FILE` is absent DELIBERATELY, and an IO is not refused a handle -- it
/// is a `RubyValue::Object`, so it crosses as `T_OBJECT`. The tag is what is
/// withheld: `T_FILE` promises C that `RFILE(v)->fptr` reads an `rb_io_t`,
/// and zeo has no `rb_io_t` to put there. A wrong `RB_TYPE_P(v, T_FILE)` is
/// an answer a gem can branch on; a garbage `fptr` is a read through a
/// pointer that was never written. The entries that used to need the tag ask
/// `builtins::io::as_rio` instead -- see `rb_io_check_io`.
mod t {
    pub const OBJECT: usize = 0x01;
    pub const CLASS: usize = 0x02;
    pub const MODULE: usize = 0x03;
    pub const FLOAT: usize = 0x04;
    pub const STRING: usize = 0x05;
    pub const REGEXP: usize = 0x06;
    pub const ARRAY: usize = 0x07;
    pub const HASH: usize = 0x08;
    pub const STRUCT: usize = 0x09;
    pub const BIGNUM: usize = 0x0a;
    pub const DATA: usize = 0x0c;
    pub const MATCH: usize = 0x0d;
    pub const COMPLEX: usize = 0x0e;
    pub const RATIONAL: usize = 0x0f;
    pub const SYMBOL: usize = 0x14;
}

/// `RUBY_FL_FREEZE`, from `ruby/internal/fl_type.h`.
const FL_FREEZE: usize = 1 << 11;
/// `RUBY_TYPED_FL_IS_TYPED_DATA` (`RUBY_FL_USERPRIV0`). Setting it here is
/// what lets `rbimpl_rtypeddata_p` stay unpatched.
const FL_IS_TYPED_DATA: usize = 1 << 6;

/// MRI's `struct RBasic`, and the first two words of every handle.
///
/// `#[repr(C)]` and this field order are load-bearing: C reads them through
/// the vendored `rbasic.h`, which zeo does not patch.
#[repr(C)]
struct RBasic {
    flags: AtomicUsize,
    klass: AtomicUsize,
}

/// One heap `VALUE`'s cell.
///
/// C sees only `basic`. Everything after it is zeo's, and no vendored header
/// describes it -- which is why every payload struct is opaque
/// (`cext/patches/0001`) rather than merely inaccurate.
#[repr(C, align(8))]
pub struct Handle {
    basic: RBasic,
    /// The strong reference. While this handle lives, the object does.
    value: RubyValue,
    /// The table key, so a handle can find its own entry without a search.
    identity: usize,
}

impl Handle {
    pub fn as_value(&self) -> Value {
        std::ptr::from_ref(self) as Value
    }

    /// The Ruby value this handle stands for.
    pub fn value(&self) -> &RubyValue {
        &self.value
    }

    /// The object behind this handle, when it is a `T_DATA`.
    fn cdata(&self) -> Option<&super::data::CData> {
        match &self.value {
            RubyValue::Object(o) => o.as_any().downcast_ref::<super::data::CData>(),
            _ => None,
        }
    }

    /// The slot `DATA_PTR` and `RTYPEDDATA_DATA` hand back, which C assigns
    /// through.
    ///
    /// It lives in the [`super::data::CData`], not in the handle: the object
    /// is what owns the pointer, and a handle is only how C names the object.
    /// Two slots would be two answers.
    pub fn data_slot(&self) -> Option<*mut *mut c_void> {
        self.cdata().map(super::data::CData::slot)
    }

    /// `RTYPEDDATA_TYPE`. Null for the untyped `Data_Wrap_Struct` form.
    pub fn data_type(&self) -> *const super::data::DataType {
        self.cdata()
            .map_or(std::ptr::null(), super::data::CData::data_type)
    }
}

/// What makes two `VALUE`s the same object.
enum Identity {
    /// An `Arc`-backed value: its payload address, which is also what an edge
    /// pointing at it spells.
    Addr(usize),
    /// A class or module. `ClassId` is zeo's identity for one, and there is
    /// no allocation to take an address of.
    Class(ClassId),
    /// No identity to share. A Bignum outside the Fixnum range is a fresh
    /// object every time it is built, in MRI too, so two equal ones are
    /// correctly not `==` as `VALUE`s.
    Fresh,
}

fn identity_of(v: &RubyValue) -> Identity {
    // `weak_owner` already knows every Arc-backed variant and answers the
    // payload address for each. Reusing it is what keeps this from being a
    // second list that drifts from the first.
    if let Some(w) = zeo_rt::value::weak_owner(v) {
        return Identity::Addr(w.as_ptr() as *const () as usize);
    }
    match v {
        RubyValue::Class(cid) => Identity::Class(*cid),
        _ => Identity::Fresh,
    }
}

/// The `RUBY_T_*` tag for a value, or `None` when zeo does not know MRI's
/// answer.
///
/// `None` is a refusal, not a default: a wrong tag makes `RB_TYPE_P` lie, and
/// a lie is worse than a `NotImplementedError` at the call.
fn builtin_type(v: &RubyValue) -> Option<usize> {
    use RubyValue as V;
    Some(match v {
        V::Object(_) => t::OBJECT,
        V::Class(cid) => match zeo_rt::dispatch::class_is_module(*cid) {
            Some(true) => t::MODULE,
            Some(false) => t::CLASS,
            // An id the class table does not know is not a class; refusing
            // beats tagging it T_CLASS and letting `rb_define_method` write
            // into nothing.
            None => return None,
        },
        V::Float(_) => t::FLOAT,
        V::Str(_) => t::STRING,
        V::Regexp(_) => t::REGEXP,
        V::Array(_) => t::ARRAY,
        V::Hash(_) => t::HASH,
        // Oracle-checked: a Range is a `T_STRUCT` in CRuby, not a T_OBJECT
        // and not a T_DATA.
        V::Range(_) => t::STRUCT,
        V::BigInt(_) => t::BIGNUM,
        V::MatchData(_) => t::MATCH,
        V::Complex(_) => t::COMPLEX,
        V::Rational(_) => t::RATIONAL,
        V::Symbol(_) => t::SYMBOL,
        // Every one of these is a `TypedData_Wrap_Struct` in CRuby.
        V::Proc(_) | V::Fiber(_) | V::Thread(_) | V::Mutex(_) | V::Queue(_) | V::Enumerator(_) => {
            t::DATA
        }
        _ => return None,
    })
}

/// A handle's address, made `Send` so the table can hold it.
///
/// The claim is not that a `Handle` is thread-safe in general. It is that a
/// handle is only ever read through C, that loading a C extension arms the
/// GVL, and that the GVL admits one thread to C at a time. Rust-side reads go
/// through the table's own lock.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Addr(usize);
// SAFETY: see the doc comment above -- the GVL is the serializer.
unsafe impl Send for Addr {}

struct Entry {
    /// Owns the allocation. `Box` so the address is stable for as long as C
    /// can hold it.
    handle: Box<Handle>,
    /// How many live scopes have pinned it, plus one for each
    /// `rb_gc_register_address` root.
    pins: u32,
}

#[derive(Default)]
struct Table {
    by_identity: HashMap<usize, Addr>,
    entries: HashMap<Addr, Entry>,
}

/// Class identities are tagged odd. Every real payload address is 8-aligned,
/// so the two spaces cannot collide.
fn class_key(cid: ClassId) -> usize {
    ((cid.0 as usize) << 1) | 1
}

static TABLE: Mutex<Option<Table>> = Mutex::new(None);

fn with_table<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    let mut guard = TABLE.lock();
    f(guard.get_or_insert_with(Table::default))
}

/// The `VALUE` for `v`, pinned in the current scope.
///
/// Returns `None` for a value whose `RUBY_T_*` tag zeo does not know; the
/// caller raises. Immediates never reach here -- [`super::value::immediate_of`]
/// answers those without a handle.
pub fn pin(v: &RubyValue) -> Option<Value> {
    if let Some(imm) = value::immediate_of(v) {
        return Some(imm);
    }
    let tag = builtin_type(v)?;
    let addr = with_table(|t| intern(t, v, tag));
    // A CLASS handle never unpins.
    //
    // Every `Init_` opens with `cFoo = rb_define_class(...)` and stores the
    // `VALUE` in a C static that outlives the call. On MRI that is safe
    // because a class is never collected. Here the handle is a box the scope
    // pop frees, and the allocator hands the same address to the next
    // handle -- so `msgpack`'s `cMessagePack_Packer` came back as a
    // `Factory`, and `TypedData_Wrap_Struct` wrapped the wrong class.
    //
    // A class costs one box and its ClassId is stable for the process, so
    // keeping it is both cheap and exactly MRI's own guarantee. Anything
    // else an extension stores past its scope still needs
    // `rb_gc_register_address`, as it does on MRI.
    if is_immortal(v) {
        return Some(addr.0);
    }
    super::scope::pin(addr.0);
    Some(addr.0)
}

/// Whether a handle for `v` never unpins.
///
/// A predicate rather than an inline match: the end-to-end proof is a gem
/// loading, and this is the half a unit test can look at -- `pin` itself
/// needs the class registry to answer a `RUBY_T_*` tag at all.
fn is_immortal(v: &RubyValue) -> bool {
    matches!(v, RubyValue::Class(_))
}

fn intern(t: &mut Table, v: &RubyValue, tag: usize) -> Addr {
    let key = match identity_of(v) {
        Identity::Addr(a) => Some(a),
        Identity::Class(cid) => Some(class_key(cid)),
        Identity::Fresh => None,
    };
    if let Some(key) = key
        && let Some(&addr) = t.by_identity.get(&key)
    {
        t.entries.get_mut(&addr).expect("an interned handle").pins += 1;
        return addr;
    }

    let mut flags = tag;
    if v.is_frozen() {
        flags |= FL_FREEZE;
    }
    // What `rbimpl_rtypeddata_p` reads. Setting it here is what lets that
    // function stay unpatched.
    if let RubyValue::Object(o) = v
        && o.as_any()
            .downcast_ref::<super::data::CData>()
            .is_some_and(super::data::CData::is_typed)
    {
        flags |= FL_IS_TYPED_DATA;
    }
    // The SHAPE flags, which decide which arm upstream's own accessors take.
    // zeo's views fill `as.heap` for both -- there is no run of bytes or of
    // ivars inside the object to embed -- so the object has to say so, or
    // `RSTRING_PTR` reads `as.embed.ary` and `ROBJECT_FIELDS` reads
    // `as.ary`. `RARRAY_EMBED_FLAG` needs no line: it is already clear, and
    // clear is the heap arm.
    flags |= match tag {
        t::STRING => super::layout::RSTRING_NOEMBED,
        t::OBJECT => super::layout::ROBJECT_HEAP,
        _ => 0,
    };
    // `klass` is the object's class as a `VALUE`. A class is itself a handle,
    // so this would recurse; the class handle is minted lazily on the first
    // `RBASIC_CLASS` instead, and the field starts as `Qnil`.
    let handle = Box::new(Handle {
        basic: RBasic {
            flags: AtomicUsize::new(flags),
            klass: AtomicUsize::new(value::Q_NIL),
        },
        value: v.clone(),
        identity: key.unwrap_or(0),
    });
    let addr = Addr(std::ptr::from_ref(handle.as_ref()) as usize);
    if let Some(key) = key {
        t.by_identity.insert(key, addr);
    }
    t.entries.insert(addr, Entry { handle, pins: 1 });
    addr
}

/// Take one more pin on a handle already in the table, by address. Only
/// [`super::scope::Scope::keep`] needs this: it re-pins into a parent scope.
pub(super) fn pin_raw(addr: usize) {
    with_table(|t| {
        if let Some(e) = t.entries.get_mut(&Addr(addr)) {
            e.pins += 1;
        }
    });
}

/// Release one pin. The handle goes when the last one does.
///
/// The removed entry drops AFTER the table lock is released: dropping it
/// can drop the last reference to a `CData`, whose own `Drop` releases the
/// handles ITS mark function pinned -- an `unpin` inside an `unpin`, which
/// deadlocked on the non-reentrant table mutex while the entry dropped
/// under the lock.
pub(super) fn unpin(addr: usize) {
    let dropped = with_table(|t| {
        let addr = Addr(addr);
        let entry = t.entries.get_mut(&addr)?;
        entry.pins -= 1;
        if entry.pins > 0 {
            return None;
        }
        let entry = t.entries.remove(&addr).expect("just looked it up");
        if entry.handle.identity != 0 {
            t.by_identity.remove(&entry.handle.identity);
        }
        Some(entry)
    });
    drop(dropped);
}

/// The handle a `VALUE` points at.
///
/// # Safety
///
/// `v` must be a `VALUE` a live handle produced. Every path that reaches here
/// has already ruled out an immediate.
pub unsafe fn deref<'a>(v: Value) -> &'a Handle {
    debug_assert!(!value::is_special_const(v), "{v:#x} is not a handle");
    // SAFETY: the caller's contract. The address came from a `Box<Handle>`
    // the table still owns, so it is valid and aligned.
    unsafe { &*(v as *const Handle) }
}

crate::cext_fn! {
    /// `RBASIC_CLASS(obj)` and the `CLASS_OF` behind it: the handle's
    /// `klass` word, minted on the first read and memoized into the word --
    /// minting it at handle creation would recurse, because a class handle's
    /// own `klass` is another handle (see the mint site). A Class answers
    /// its metaclass, which is where MRI keeps a class handle's `klass`
    /// pointer and what `rb_undef_method(CLASS_OF(c), "m")` -- date's
    /// class-method undef -- relies on.
    fn rb_zeo_rbasic_class(obj: Value) -> Value {
        // SAFETY: `RBASIC_CLASS` asserts the argument is no special const,
        // and the inline `rb_class_of` answers immediates from the globals
        // before ever reaching here.
        let h = unsafe { deref(obj) };
        let memo = h.basic.klass.load(Ordering::Relaxed);
        if memo != value::Q_NIL {
            return Ok(memo);
        }
        let k = match &h.value {
            v @ RubyValue::Class(_) => super::object::send(v, "singleton_class", &[])?,
            v => RubyValue::Class(v.class_id()),
        };
        let kv = super::convert::to_value(&k)?;
        // A racing second mint stores an equal answer; last write wins.
        h.basic.klass.store(kv, Ordering::Relaxed);
        Ok(kv)
    }
}

/// Is `v` an address the handle table still owns?
///
/// `rb_gc_mark_maybe` is the reason this exists: the caller found the word by
/// scanning memory, so it may be anything at all, and dereferencing it
/// unchecked is the bug that entry exists to avoid.
pub fn is_live(v: Value) -> bool {
    !value::is_special_const(v) && with_table(|t| t.entries.contains_key(&Addr(v)))
}

/// Pin a handle for the life of the process.
///
/// `rb_gc_register_mark_object` asks for exactly this, and `globals::fill`
/// gets the same effect by leaking its scope. Nothing releases these, which
/// is the point: MRI's own are immortal too.
pub fn pin_forever(v: Value) {
    pin_raw(v);
}

/// The `RUBY_T_*` tag a `VALUE` carries.
///
/// Read off the handle's own `RBasic` prefix, which is the same word
/// `RB_BUILTIN_TYPE` reads -- so `rb_type` and the macro can never disagree.
/// An immediate answers from its encoding, as MRI's `rb_type` does.
///
/// # Safety
///
/// `v` must be an immediate or a live handle.
pub unsafe fn type_tag(v: Value) -> usize {
    if let Some(imm) = value::from_immediate(v) {
        return match imm {
            RubyValue::Nil => 0x11,
            RubyValue::Bool(true) => 0x12,
            RubyValue::Bool(false) => 0x13,
            RubyValue::Symbol(_) => t::SYMBOL,
            RubyValue::Float(_) => t::FLOAT,
            _ => 0x15, // T_FIXNUM
        };
    }
    if v == value::Q_UNDEF {
        return 0x16; // T_UNDEF
    }
    // SAFETY: the caller's contract.
    unsafe { deref(v) }.basic.flags.load(Ordering::Relaxed) & 0x1f
}

/// How many handles are live. The scope tests read it; nothing else should.
#[cfg(test)]
pub(super) fn live_count() -> usize {
    with_table(|t| t.entries.len())
}

/// C reads `flags` and `klass` through the unpatched `rbasic.h`, so the two
/// words have to be exactly two words with nothing before them.
/// `cext/probe/layout.c` asserts the same from the C side.
const _: () = {
    assert!(std::mem::size_of::<RBasic>() == 2 * std::mem::size_of::<Value>());
    assert!(std::mem::align_of::<Handle>() >= std::mem::align_of::<Value>());
    // The 8-alignment is what makes `RB_SPECIAL_CONST_P` right unpatched:
    // an immediate is anything with a low bit set, and a handle has none.
    assert!(std::mem::align_of::<Handle>().is_multiple_of(8));
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn a_string(s: &str) -> RubyValue {
        zeo_rt::builtins::string::str_value_in_enc(zeo_rt::encoding::UTF_8, s)
    }

    /// A CLASS handle never unpins. Every `Init_` stores `cFoo` in a C
    /// static that outlives the call, and a freed box's address gets reused
    /// -- `cMessagePack_Packer` came back as a `Factory`, and
    /// `TypedData_Wrap_Struct` wrapped the wrong class.
    ///
    /// The decision is what is on trial: `pin` needs the class registry to
    /// answer a tag at all, and a unit test installs none. The end-to-end
    /// proof is msgpack loading.
    #[test]
    fn only_a_class_handle_is_immortal() {
        assert!(is_immortal(&RubyValue::Class(zeo_abi::OBJECT_CLASS)));
        assert!(!is_immortal(&a_string("mortal")));
        assert!(!is_immortal(&RubyValue::Nil));
    }

    /// An ordinary handle DOES unpin, which is what keeps the table bounded.
    #[test]
    fn a_string_handle_goes_with_its_scope() {
        let before = live_count();
        {
            let _scope = super::super::scope::Scope::enter();
            pin(&a_string("mortal")).expect("a String has a handle");
            assert_eq!(live_count(), before + 1);
        }
        assert_eq!(live_count(), before, "a scope pop left a handle behind");
    }

    #[test]
    fn one_object_gets_one_handle() {
        let _scope = super::super::scope::Scope::enter();
        let s = a_string("hello");
        let a = pin(&s).expect("a String has a handle");
        let b = pin(&s).expect("a String has a handle");
        assert_eq!(a, b, "the same object must be the same VALUE");

        let other = a_string("hello");
        let c = pin(&other).expect("a String has a handle");
        assert_ne!(a, c, "two equal Strings are two objects");
    }

    #[test]
    fn a_handle_is_eight_aligned_and_not_an_immediate() {
        let _scope = super::super::scope::Scope::enter();
        let v = pin(&a_string("x")).expect("a String has a handle");
        assert_eq!(v % 8, 0, "{v:#x} would collide with an immediate tag");
        assert!(!value::is_special_const(v));
    }

    #[test]
    fn the_rbasic_prefix_says_the_type_and_the_frozen_bit() {
        let _scope = super::super::scope::Scope::enter();
        let v = pin(&a_string("x")).expect("a String has a handle");
        let h = unsafe { deref(v) };
        assert_eq!(h.basic.flags.load(Ordering::Relaxed) & 0x1f, t::STRING);
        assert_eq!(h.basic.flags.load(Ordering::Relaxed) & FL_FREEZE, 0);

        let frozen = a_string("y");
        frozen.freeze_value().expect("a String freezes");
        let fv = pin(&frozen).expect("a String has a handle");
        let fh = unsafe { deref(fv) };
        assert_eq!(
            fh.basic.flags.load(Ordering::Relaxed) & FL_FREEZE,
            FL_FREEZE
        );
    }

    /// A class identity and a payload address share one key space, and the
    /// only thing keeping them apart is that every payload address is
    /// 8-aligned and every class key is odd.
    #[test]
    fn a_class_key_can_never_be_a_payload_address() {
        let _scope = super::super::scope::Scope::enter();
        let v = pin(&a_string("x")).expect("a String has a handle");
        let addr = with_table(|t| {
            t.entries
                .get(&Addr(v))
                .expect("just pinned")
                .handle
                .identity
        });
        assert_eq!(addr % 8, 0);
        for cid in [0u32, 1, 7, 1000, u32::MAX >> 1] {
            assert_eq!(class_key(ClassId(cid)) % 2, 1);
        }
    }

    /// A value kind that has no handle refuses rather than answering a tag
    /// that would make `RB_TYPE_P` lie. Nil and Int are immediates and never
    /// reach a handle; the same `None` covers the kinds whose MRI tag zeo has
    /// not established.
    #[test]
    fn an_unknown_kind_refuses_a_handle() {
        let _scope = super::super::scope::Scope::enter();
        assert!(builtin_type(&RubyValue::Nil).is_none());
        assert!(builtin_type(&RubyValue::Int(0)).is_none());
    }
}
