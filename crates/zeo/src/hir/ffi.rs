use super::*;

/// A C ABI type for one FFI argument or return value (the real `ffi`
/// gem's type keywords): the scalars, `:pointer`/`:string`, named `enum`s, and
/// `callback` function-pointer types -- enough for a faithful `attach_function`
/// over libc/libm and most C entry points. Each maps to a C type codegen
/// declares in the `extern "C"` block (or, for variadic calls and callbacks,
/// to the libffi marshaling it emits) and to the RubyValue↔C conversion.
/// `Clone`, not `Copy`: the `Enum` and `Callback` variants carry owned data
/// (a member table / a nested signature), resolved at parse time and embedded
/// so codegen needs no runtime type registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfiType {
    Void,
    /// `:char`/`:int8`, `:short`/`:int16`, `:int`/`:int32`, `:long_long`/`:int64`.
    Int(u8), // width in BITS: 8/16/32/64
    /// `:uchar`/`:uint8` … `:ulong_long`/`:uint64`.
    Uint(u8),
    /// `:long`/`:ssize_t` and `:ulong`/`:size_t`: eight bytes like `Int(64)`
    /// and `Uint(64)` (LP64), kept apart because the gem marshals them
    /// through `NUM2LONG`/`NUM2ULONG`, whose error text differs from the
    /// exact-width converters', and `FFI::Type::LONG` is not `INT64`.
    Long,
    Ulong,
    /// `:float` (32) / `:double` (64).
    Float(u8),
    /// `:bool` -- a C `bool`/`_Bool`.
    Bool,
    /// `:string` -- a C `const char *`. As an argument: a NUL-terminated copy
    /// of the Ruby String, valid for the call. As a return: read the C string
    /// back into a Ruby String (`nil` for a NULL pointer).
    Str,
    /// `:pointer` -- an opaque `void *`. As an argument: the raw address an
    /// `FFI::Pointer`/`MemoryPointer` (or `nil` = NULL) carries. As a return:
    /// wrap the address as an `FFI::Pointer`. See `ext::ffi`.
    Pointer,
    /// A named `enum :tag, [:sym, val, ...]`. The underlying C type is `int`.
    /// As an argument: a Symbol maps to its int (an Integer passes through). As
    /// a return: a mapped int becomes its Symbol (an unmapped int stays an
    /// Integer), exactly as the gem's `Enum` data-converter does. The `Vec`
    /// holds `(symbol_name, value)` members in declaration order.
    Enum(Vec<(String, i64)>),
    /// An `enum` whose MEMBERS only a running process can produce: ethon's
    /// `enum(:easy_code, easy_codes)` calls a method on a module it
    /// `extend`ed, gir_ffi builds its flag sets off type information read out
    /// of a shared library. The ABI never depended on the members -- an enum
    /// is an `int` either way -- so the signature is decided here and the
    /// marshaling table waits for the class body to run. The `usize` ties this
    /// type to the statement's runtime store
    /// (`zeo_rt::ffi::enum_store`), exactly as [`FfiLib::Deferred`] does for a
    /// library. See `FfiVocab::ffi_enum_slots`.
    EnumSlot(usize),
    /// A `callback :tag, [arg_types], ret_type` -- a C function-pointer type. As
    /// an argument, a Ruby Proc is marshaled into a libffi closure trampolining
    /// into the Proc, and the closure's code pointer is passed. Holds the
    /// callback's `(arg_types, return_type)` so the closure's own CIF can be
    /// built. (As a C ABI type it is a `void *`.)
    Callback(Vec<FfiType>, Box<FfiType>),
    /// An INLINE array field of a struct layout -- `layout :bytes, [:uint8, 4]`.
    /// It occupies `count` elements in place (it is not a pointer), so it is
    /// what decides every following field's offset. Reading the field yields
    /// ruby's own proxy over the same memory rather than a copy: an
    /// `FFI::Struct::InlineArray`, or an `FFI::StructLayout::CharArray` when
    /// the element is 8-bit, which is the one that also answers `to_s`.
    /// Only legal in a layout; the gem has no inline array in a function
    /// signature either.
    Array(Box<FfiType>, usize),
    /// A struct passed or stored BY VALUE -- `.by_value` in a signature, or
    /// a bare struct name in a LAYOUT field (an inline nested struct).
    /// Carries the full layout: the ABI needs every field's real type, not
    /// just the byte count (a `{float, float}` classifies differently from
    /// `[u8; 8]` on SysV/AArch64).
    Struct(FfiStructLayout),
    /// A BARE `FFI::Struct` subclass name in a SIGNATURE position --
    /// ruby-ffi's `StructByReference`, whose ABI type is a POINTER
    /// (`find_type` wraps a struct class this way; `.by_value` is the
    /// by-value spelling). An argument takes a struct instance; a RETURN
    /// comes back as a plain `FFI::Pointer`, never auto-wrapped in the
    /// class (oracle-verified). Lowering degrades it to `Pointer` at each
    /// boundary, so codegen never sees it; the class path rides along for
    /// the one consumer that rejects it (a callback signature).
    StructRef(String),
    /// A POSIX integer typedef whose width GENUINELY differs between the
    /// targets zeo builds for (`mode_t` is u16 on macOS, u32 on glibc; also
    /// `dev_t`, `suseconds_t`, `clock_t`, ...). Legal in argument/return
    /// position only, where generated code spells the TARGET's own
    /// `zeo_rt::libc::<name>` and rustc (or a runtime `size_of`) supplies
    /// the real width at build time. NOT legal in a struct layout: a field
    /// width that shifts by target would silently shift every later offset
    /// -- that stays `CScalar::from_c_typedef`'s documented rejection.
    PlatformScalar(String),
    /// `:strptr` -- a `char *` RETURN read back as the gem's two-element
    /// `[String, Pointer]` (the decoded string AND the raw pointer, so the
    /// caller can still free it). Return position only.
    StrPtr,
}

impl From<zeo_abi::ffi::CScalar> for FfiType {
    fn from(s: zeo_abi::ffi::CScalar) -> FfiType {
        use zeo_abi::ffi::CScalar as S;
        match s {
            S::Void => FfiType::Void,
            S::I8 => FfiType::Int(8),
            S::I16 => FfiType::Int(16),
            S::I32 => FfiType::Int(32),
            S::I64 => FfiType::Int(64),
            S::U8 => FfiType::Uint(8),
            S::U16 => FfiType::Uint(16),
            S::U32 => FfiType::Uint(32),
            S::U64 => FfiType::Uint(64),
            S::Long => FfiType::Long,
            S::ULong => FfiType::Ulong,
            S::F32 => FfiType::Float(32),
            S::F64 => FfiType::Float(64),
            S::Bool => FfiType::Bool,
            S::Str => FfiType::Str,
            S::Pointer => FfiType::Pointer,
        }
    }
}

impl FfiType {
    /// The C ABI scalar this type marshals AS: an enum is a C `int`, a
    /// callback passes as its code pointer. `None` for the two types with no
    /// scalar representation at a call site (an inline array lives only in a
    /// struct layout; a by-value struct is its own aggregate).
    pub fn c_scalar(&self) -> Option<zeo_abi::ffi::CScalar> {
        use zeo_abi::ffi::CScalar as S;
        Some(match self {
            FfiType::Void => S::Void,
            FfiType::Int(w) => match w {
                8 => S::I8,
                16 => S::I16,
                64 => S::I64,
                _ => S::I32,
            },
            FfiType::Uint(w) => match w {
                8 => S::U8,
                16 => S::U16,
                64 => S::U64,
                _ => S::U32,
            },
            FfiType::Long => S::Long,
            FfiType::Ulong => S::ULong,
            FfiType::Float(64) => S::F64,
            FfiType::Float(_) => S::F32,
            FfiType::Bool => S::Bool,
            FfiType::Str => S::Str,
            FfiType::Pointer | FfiType::Callback(..) | FfiType::StructRef(_) => S::Pointer,
            FfiType::Enum(_) | FfiType::EnumSlot(_) => S::I32,
            FfiType::Array(..)
            | FfiType::Struct(_)
            | FfiType::PlatformScalar(_)
            | FfiType::StrPtr => return None,
        })
    }
}

/// An `FFI::Struct` subclass's computed C layout, recorded when its `layout`
/// directive lowers so later declarations can pass it by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfiStructLayout {
    /// The class name as written -- for diagnostics and a future by-value
    /// RETURN wrap (which must construct this class).
    pub class_path: String,
    /// `(field, type, byte offset)` in declaration order.
    pub fields: Vec<(String, FfiType, usize)>,
    pub size: usize,
    pub align: usize,
    pub union: bool,
}

/// Where an `attach_function`'s symbol comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfiLib {
    /// A name rustc can link at build time: `#[link(name = ..)]` plus a
    /// direct `extern "C"` call -- the zero-overhead tier.
    Static(String),
    /// Library candidates only a RUNNING process can open: a bundled `.so`
    /// path built from `__dir__`, a versioned soname, a candidate list. The
    /// call site `dlopen`s the first that opens and `dlsym`s once -- which is
    /// when and how CRuby's ffi gem binds every symbol. See
    /// `zeo_rt::ffi::FfiSymSite`.
    Runtime(Vec<String>),
    /// An `ffi_lib` whose candidates NO compile-time fold can name (an ENV
    /// read, a local, a helper call like `FFI.map_library_name`): the
    /// expressions evaluate when the class body EXECUTES, dlopen eagerly
    /// there (CRuby's require-time `LoadError`, to the statement), and the
    /// handle lands in this slot for every `attach_function` under it. The
    /// call site resolves through the same dlopen/libffi tier as `Runtime`.
    Deferred { slot: usize },
    /// No `ffi_lib` (or `FFI::CURRENT_PROCESS`): the always-linked image.
    None,
}

/// One C function a module `attach_function`'d. The synthesized wrapper
/// method's whole body IS this node -- see `codegen`'s `emit_ffi_call`, which
/// declares the `extern "C"` symbol fn-locally (with `#[link(name = ..)]`, so no
/// build-step change is needed), marshals each argument, calls it, and wraps the
/// result back into a `RubyValue`.
#[derive(Debug, Clone)]
pub struct FfiCall {
    /// The C symbol to declare and call (the `attach_function` C name, which may
    /// differ from the Ruby method name in the 4-arg rename form).
    pub symbol: String,
    /// The library the symbol lives in.
    pub lib: FfiLib,
    /// Each FIXED argument: the wrapper param to read (`LocalRead`) and its C
    /// type. For a variadic function these are only the declared leading
    /// arguments (the ones before `:varargs`).
    pub args: Vec<(NodeId, FfiType)>,
    /// The C return type -- governs the wrap back to a `RubyValue`.
    pub ret: FfiType,
    /// `Some(rest)` for a variadic function (`attach_function [.., :varargs]`),
    /// where `rest` reads the wrapper's `*rest` param -- a flat Array of
    /// alternating `type_symbol, value` pairs marshaled at runtime via libffi.
    /// `None` for an ordinary fixed-arity call.
    pub variadic: Option<NodeId>,
    /// `attach_function ..., blocking: true` -- the call runs with the GVL
    /// released, so other ruby threads keep running across a long C call. Under
    /// zeo's default parallel mode there is no GVL to release and
    /// `zeo_rt::without_gvl` is a no-op; under `ZEO_GVL=1` it is the handoff
    /// the option asks for.
    pub blocking: bool,
}
