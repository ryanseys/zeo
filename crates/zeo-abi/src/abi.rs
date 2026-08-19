//! The Cranelift-backend value ABI: the layout contract generated code and
//! the runtime share.
//!
//! `zeo-rt` makes `RubyValue` `#[repr(C, u8)]` with the explicit
//! discriminants in [`ValueTag`]; its `abi_layout` test asserts every
//! constant here against the real types, so the two sides cannot drift.
//! Generated code reads/writes ONLY the tag byte and the `Int`/`Float`/
//! `Bool`/`Symbol`/`Class` payloads inline; every heap payload is opaque
//! (retain/release/inspect via runtime calls).
//!
//! Numbering rule: the six no-`Arc` payloads (a 24-byte copy needs no
//! retain/release) sit below [`FIRST_HEAP_TAG`]; every `Arc`-backed payload
//! sits at or above it. Within each band, numbers follow the enum's source
//! order. APPEND-ONLY, like `ClassId`s.

/// `size_of::<RubyValue>()` -- and of the 24-byte value slots generated
/// code allocates.
pub const VALUE_SIZE: usize = 24;
/// `align_of::<RubyValue>()`.
pub const VALUE_ALIGN: usize = 8;
/// The tag byte's offset inside a value.
pub const TAG_OFFSET: usize = 0;
/// Every payload's offset: `repr(C, u8)` is tag + union, so `Int`'s `i64`,
/// `Float`'s `f64`, `Bool`'s `i8` and `Symbol`/`Class`'s `u32` all start
/// here.
pub const PAYLOAD_OFFSET: usize = 8;

/// `RubyValue`'s explicit discriminants. Immediates (no `Arc` payload)
/// below 16, heap variants from 16.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueTag {
    Nil = 0,
    Bool = 1,
    Int = 2,
    Float = 3,
    Symbol = 4,
    Class = 5,
    BigInt = 16,
    Rational = 17,
    Complex = 18,
    Str = 19,
    Array = 20,
    Hash = 21,
    Range = 22,
    Object = 23,
    Proc = 24,
    Regexp = 25,
    MatchData = 26,
    Fiber = 27,
    Enumerator = 28,
    Yielder = 29,
    Thread = 30,
    Mutex = 31,
    Queue = 32,
    Ractor = 33,
}

/// `tag < FIRST_HEAP_TAG` => the value is a plain 24-byte memcpy, no
/// retain/release; `>=` => the payload holds an `Arc` the runtime must
/// retain/release.
pub const FIRST_HEAP_TAG: u8 = 16;

/// A compiled function returned normally; `out` holds the value.
pub const STATUS_OK: i32 = 0;
/// A signal is pending in the per-fiber slot; `out` is untouched.
pub const STATUS_SIGNAL: i32 = 1;

/// What the pending-signal slot holds -- `Signal`'s variants by number,
/// as `zeo_rt_signal_kind`/`zeo_rt_signal_take` answer them. `None` = the
/// slot is empty. `Terminate` is the fiber/enumerator teardown signal
/// (never native unwinding).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalKind {
    None = 0,
    Break = 1,
    Next = 2,
    Redo = 3,
    Retry = 4,
    Return = 5,
    Raise = 6,
    Throw = 7,
    Terminate = 8,
}

/// `size_of` of the runtime's `.bss` inline-cache site structs -- what the
/// emitter sizes each opaque site slot to (initialised by `zeo_unit_init`,
/// never assumed zero-valid). All align 8. Asserted by `zeo-rt`'s
/// `abi_layout` test.
pub const CALLSITE_SIZE: usize = 48;
pub const DYNCALLER_SITE_SIZE: usize = 48;
pub const CLASSMETHOD_SITE_SIZE: usize = 40;
pub const CONST_SITE_SIZE: usize = 40;
pub const CIVAR_SITE_SIZE: usize = 40;
pub const REGEXP_SITE_SIZE: usize = 16;
pub const FFISYM_SITE_SIZE: usize = 16;
/// The common alignment of every site struct above.
pub const SITE_ALIGN: usize = 8;
