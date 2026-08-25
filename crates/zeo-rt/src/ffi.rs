//! Marshaling helpers for compile-time FFI: convert between `RubyValue`
//! and C scalar types at an `attach_function` boundary.
//!
//! The generated program declares the `extern "C"` symbols itself (fn-locally,
//! with `#[link(name = ..)]`, so no build-step change is needed) and calls them;
//! these helpers keep the emitted per-argument/return conversions small and in
//! one place. The coercions mirror the real `ffi` gem: an integer type wants an
//! Integer, a float wants a Float (or Integer), a `:string` wants a String with
//! no interior NUL. A wrong type is a `TypeError`, exactly as the gem raises.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};

use crate::builtins::{arg_error, type_error};
use crate::dispatch::class_name;
use crate::signal::Signal;
use crate::value::RubyValue;

fn type_err(what: &str, v: &RubyValue) -> Signal {
    let got = class_name(v.class_id()).unwrap_or_else(|| "?".to_string());
    type_error!("cannot convert {got} into an FFI {what}")
}

/// A Ruby value bound to a C integer argument (`:int`/`:long`/`:uintN`/…). The
/// caller narrows the returned `i64` to the exact C width with an `as` cast.
pub fn to_i64(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Int(i) => Ok(*i),
        _ => Err(type_err("integer", v)),
    }
}

/// A Ruby value bound to a C float argument (`:float`/`:double`). An Integer is
/// accepted and widened, matching the gem.
pub fn to_f64(v: &RubyValue) -> Result<f64, Signal> {
    match v {
        RubyValue::Float(f) => Ok(*f),
        RubyValue::Int(i) => Ok(*i as f64),
        _ => Err(type_err("float", v)),
    }
}

/// A Ruby value bound to a `:bool` argument -- Ruby truthiness.
pub fn to_bool(v: &RubyValue) -> bool {
    v.truthy()
}

/// A Ruby String bound to a `:string` (`const char *`) argument: a
/// NUL-terminated copy the caller keeps alive for the duration of the call. An
/// interior NUL is an `ArgumentError`, as the gem raises.
pub fn to_cstring(v: &RubyValue) -> Result<CString, Signal> {
    match v {
        RubyValue::Str(s) => {
            let bytes = s.lock().bytes().to_vec();
            CString::new(bytes).map_err(|_| arg_error!("string contains null byte"))
        }
        _ => Err(type_err("string", v)),
    }
}

/// A Ruby value bound to a `:pointer` argument -> the raw C address it carries.
/// An `FFI::Pointer`/`MemoryPointer` yields its address; `nil` is a NULL
/// pointer, exactly as the gem accepts. Anything else is a `TypeError`.
#[cfg(feature = "ext-ffi")]
pub fn to_pointer(v: &RubyValue) -> Result<*mut c_void, Signal> {
    if let Some(addr) = crate::ext::ffi::address_of(v) {
        return Ok(addr as *mut c_void);
    }
    // An `FFI::Struct` (or anything answering `to_ptr`) passes its own memory,
    // exactly as the gem auto-converts a struct given for a `:pointer` argument.
    if let RubyValue::Object(_) = v
        && let Ok(p) = crate::dispatch::send_value(v, crate::Symbol::intern("to_ptr"), &[], None)
        && let Some(addr) = crate::ext::ffi::address_of(&p)
    {
        return Ok(addr as *mut c_void);
    }
    Err(type_err("pointer", v))
}

/// A `:pointer` C return value -> an `FFI::Pointer` wrapping the address (a NULL
/// pointer wraps to an `FFI::Pointer` whose `#null?` is true, matching the gem).
#[cfg(feature = "ext-ffi")]
pub fn from_pointer(p: *const c_void) -> RubyValue {
    crate::ext::ffi::wrap_address(p as usize)
}

/// The `FfiKind` of a platform-varying integer typedef, decided where the
/// generated program BUILDS: `platform_kind(size_of::<libc::mode_t>(),
/// libc::mode_t::MIN == 0)`. Only integer typedefs route here (see
/// `FfiType::PlatformScalar`), so `size` is 1/2/4/8.
#[cfg(feature = "ext-ffi")]
pub fn platform_kind(size: usize, unsigned: bool) -> FfiKind {
    match (size, unsigned) {
        (1, false) => FfiKind::I8,
        (2, false) => FfiKind::I16,
        (4, false) => FfiKind::I32,
        (8, false) => FfiKind::I64,
        (1, true) => FfiKind::U8,
        (2, true) => FfiKind::U16,
        (8, true) => FfiKind::U64,
        _ => FfiKind::U32,
    }
}

/// A `:strptr` return, second half: `call_fixed` already wrapped the raw
/// address as an `FFI::Pointer`; read the C string back beside it and answer
/// the gem's `[String, Pointer]` pair (`[nil, Pointer]` for NULL).
///
/// # Safety
/// The pointer must be NULL or reference a NUL-terminated string, the
/// `:strptr` contract itself.
#[cfg(feature = "ext-ffi")]
pub unsafe fn strptr_pair(v: RubyValue) -> RubyValue {
    let addr = crate::ext::ffi::address_of(&v).unwrap_or(0);
    let s = unsafe { from_cstr(addr as *const c_char) };
    RubyValue::Array(crate::array_new(vec![s, v]))
}

/// A struct returned BY VALUE -> a fresh ruby-owned `MemoryPointer` copying
/// its `len` bytes; the generated wrapper then constructs the struct's own
/// class over that backing (the gem wraps a by-value return the same way).
///
/// # Safety
/// `src..src+len` must be readable -- the generated call site passes a
/// reference to the C return value it just received.
#[cfg(feature = "ext-ffi")]
pub unsafe fn from_struct_ret(src: *const u8, len: usize) -> RubyValue {
    unsafe { crate::ext::ffi::memory_from_bytes(src, len) }
}

/// A Ruby value bound to an `enum` argument -> the underlying `int`. A Symbol
/// maps through the enum's member table; an Integer passes through unchanged
/// (the gem accepts a raw value); an unknown Symbol is an `ArgumentError`, as
/// the gem raises.
pub fn enum_to_int(v: &RubyValue, members: &[(&str, i64)]) -> Result<i64, Signal> {
    match v {
        RubyValue::Symbol(s) => {
            let name = s.name();
            members
                .iter()
                .find(|(n, _)| *n == name.as_str())
                .map(|(_, i)| *i)
                .ok_or_else(|| arg_error!("invalid enum value, :{name}"))
        }
        RubyValue::Int(i) => Ok(*i),
        _ => Err(type_err("enum", v)),
    }
}

/// The DEFERRED tier's enum tables, one per `enum` statement whose member
/// list only a running process can produce -- ethon's `enum(:easy_code,
/// easy_codes)` calls a method on a module it `extend`ed, gir_ffi builds its
/// flags off type information read from a shared library at load time.
///
/// The ABI never depended on the members: an enum is an `int` either way, so
/// the extern signature is decided at compile time and only the symbol<->
/// integer marshaling waits for this. `enum_store` fills a slot when the class
/// body executes; every signature lowered under that statement reads it back.
#[cfg(feature = "ext-ffi")]
type EnumSlots = std::sync::RwLock<std::collections::HashMap<usize, Vec<(String, i64)>>>;

#[cfg(feature = "ext-ffi")]
static ENUM_SLOTS: std::sync::OnceLock<EnumSlots> = std::sync::OnceLock::new();

#[cfg(feature = "ext-ffi")]
fn enum_slots() -> &'static EnumSlots {
    ENUM_SLOTS.get_or_init(Default::default)
}

/// `enum <tag>, <members>` with the members evaluated at RUN time, numbered by
/// the gem's rule: a member takes the next value, an Integer following one
/// sets that member's value and the counter. Nested arrays flatten, which is
/// what `enum(:easy_option, easy_options(:enum).to_a.flatten)` relies on.
#[cfg(feature = "ext-ffi")]
pub fn enum_store(slot: usize, vals: &[RubyValue]) -> Result<RubyValue, Signal> {
    fn flatten(v: &RubyValue, out: &mut Vec<RubyValue>) {
        match v {
            RubyValue::Array(a) => {
                let elems: Vec<RubyValue> = a.lock().iter().cloned().collect();
                for e in &elems {
                    flatten(e, out);
                }
            }
            other => out.push(other.clone()),
        }
    }
    let mut flat = Vec::new();
    for v in vals {
        flatten(v, &mut flat);
    }
    let mut members: Vec<(String, i64)> = Vec::new();
    let mut next = 0i64;
    let mut i = 0;
    while i < flat.len() {
        let name = match &flat[i] {
            RubyValue::Symbol(s) => s.name(),
            RubyValue::Str(s) => String::from_utf8_lossy(s.lock().bytes()).into_owned(),
            other => {
                return Err(type_error!(
                    "an enum member name must be a Symbol or String, got {}",
                    crate::builtins::class_name_of(other)
                ));
            }
        };
        i += 1;
        let value = match flat.get(i) {
            Some(RubyValue::Int(n)) => {
                i += 1;
                *n
            }
            _ => next,
        };
        members.push((name, value));
        next = value + 1;
    }
    enum_slots()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(slot, members);
    Ok(RubyValue::Nil)
}

/// [`enum_to_int`] against a slot the class body filled. An unfilled slot is
/// the gem's own "unable to resolve type" -- a signature naming an enum whose
/// statement never ran.
#[cfg(feature = "ext-ffi")]
pub fn enum_to_int_slot(slot: usize, v: &RubyValue) -> Result<i64, Signal> {
    let table = enum_slots().read().unwrap_or_else(|e| e.into_inner());
    let Some(members) = table.get(&slot) else {
        return Err(arg_error!("this enum's members were never declared"));
    };
    let borrowed: Vec<(&str, i64)> = members.iter().map(|(n, v)| (n.as_str(), *v)).collect();
    enum_to_int(v, &borrowed)
}

/// [`int_to_enum`] against a slot. An unfilled slot hands the integer back
/// unchanged, which is also what an unnamed value does.
#[cfg(feature = "ext-ffi")]
pub fn int_to_enum_slot(slot: usize, i: i64) -> RubyValue {
    let table = enum_slots().read().unwrap_or_else(|e| e.into_inner());
    let Some(members) = table.get(&slot) else {
        return RubyValue::Int(i);
    };
    members
        .iter()
        .find(|(_, v)| *v == i)
        .map(|(n, _)| RubyValue::Symbol(crate::Symbol::intern(n)))
        .unwrap_or(RubyValue::Int(i))
}

/// An `int` enum return -> its Symbol if the value is a named member, else the
/// raw Integer (the gem's `Enum#from_native` behavior).
pub fn int_to_enum(i: i64, members: &[(&str, i64)]) -> RubyValue {
    members
        .iter()
        .find(|(_, v)| *v == i)
        .map(|(n, _)| RubyValue::Symbol(crate::Symbol::intern(n)))
        .unwrap_or(RubyValue::Int(i))
}

pub fn from_i64(i: i64) -> RubyValue {
    RubyValue::Int(i)
}

pub fn from_f64(f: f64) -> RubyValue {
    RubyValue::Float(f)
}

pub fn from_bool(b: bool) -> RubyValue {
    RubyValue::Bool(b)
}

/// Read a `:string` return value (a NUL-terminated C string) back into a Ruby
/// String; a NULL pointer becomes `nil`, matching the gem.
///
/// # Safety
/// `p` must be NULL or a valid pointer to a NUL-terminated string that outlives
/// this read -- the same contract the `ffi` gem places on a `:string` return.
pub unsafe fn from_cstr(p: *const c_char) -> RubyValue {
    unsafe {
        if p.is_null() {
            return RubyValue::Nil;
        }
        let bytes = CStr::from_ptr(p).to_bytes().to_vec();
        // ASCII-8BIT, matching ruby-ffi: a `:string` return is raw C bytes and
        // the gem tags them BINARY, not the default external (oracle-verified
        // on `getenv`).
        RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT))
    }
}

// ---------------------------------------------------------------------------
// libffi-backed runtime calls: variadic functions (`attach_function [.., :varargs]`)
// and callbacks (a Ruby Proc handed to a C function-pointer argument). Both need
// a call interface built at RUNTIME (the fixed `extern "C"` emission can't
// express a runtime-shaped argument list), which is exactly what libffi does.
// ---------------------------------------------------------------------------

/// A resolved FFI scalar kind. Codegen emits one for each fixed variadic
/// argument and for a callback's argument/return types; the runtime also
/// resolves a `:type` Symbol in a varargs `(type, value)` pair to one of
/// these. It IS the compiler's scalar table (`zeo_abi::ffi::CScalar`), so
/// the two sides answer every width question from one place.
#[cfg(feature = "ext-ffi")]
pub use zeo_abi::ffi::CScalar as FfiKind;

#[cfg(feature = "ext-ffi")]
fn kind_type(k: FfiKind) -> libffi::middle::Type {
    use FfiKind::*;
    use libffi::middle::Type;
    match k {
        Void => Type::void(),
        I8 => Type::i8(),
        I16 => Type::i16(),
        I32 => Type::i32(),
        I64 => Type::i64(),
        U8 => Type::u8(),
        U16 => Type::u16(),
        U32 => Type::u32(),
        U64 => Type::u64(),
        F32 => Type::f32(),
        F64 => Type::f64(),
        // A C `_Bool` is one byte; `^`/pointer are register-sized pointers.
        Bool => Type::u8(),
        Str | Pointer => Type::pointer(),
    }
}

/// The C default argument promotions applied to a variadic argument: an
/// integer narrower than `int` widens to `int`, and `float` widens to `double`.
#[cfg(feature = "ext-ffi")]
fn promote(k: FfiKind) -> FfiKind {
    use FfiKind::*;
    match k {
        I8 | I16 | U8 | U16 | Bool => I32,
        F32 => F64,
        other => other,
    }
}

/// Map a varargs `:type` Symbol name to its kind -- the shared keyword table
/// the compile-time `ffi_type_of` in `zeo`'s `lower/ffi.rs` also reads.
/// `:void` stays an error here: it is a return type, never a value's.
#[cfg(feature = "ext-ffi")]
pub fn kind_from_symbol(name: &str) -> Result<FfiKind, Signal> {
    match FfiKind::from_keyword(name) {
        Some(FfiKind::Void) | None => Err(arg_error!("unknown FFI varargs type :{name}")),
        Some(kind) => Ok(kind),
    }
}

/// One marshaled variadic argument. It OWNS its storage (a `CString` for a
/// `:string`) so the pointer libffi reads through stays valid for the call.
#[cfg(feature = "ext-ffi")]
pub struct VaVal {
    inner: VaInner,
    _owner: Option<CString>,
}

#[cfg(feature = "ext-ffi")]
enum VaInner {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Ptr(*mut c_void),
    /// A struct passed BY VALUE through the runtime tier: `data` points at
    /// the ruby struct's own backing bytes (owned by the argument object,
    /// which the caller's frame keeps alive across the call), and `elements`
    /// rebuilds libffi's structure descriptor -- the REAL field types, so
    /// libffi classifies the aggregate exactly as rustc classifies the
    /// extern tier's `#[repr(C)]` mirror.
    Struct {
        data: *mut c_void,
        elements: &'static [FfiElem],
    },
}

/// One element of a by-value struct's libffi descriptor, const-buildable by
/// generated code (which speaks only `zeo_rt`, never libffi): inline arrays
/// arrive pre-expanded to N scalar elements, nested structs recurse.
#[cfg(feature = "ext-ffi")]
#[derive(Clone, Copy)]
pub enum FfiElem {
    Scalar(FfiKind),
    Struct(&'static [FfiElem]),
}

#[cfg(feature = "ext-ffi")]
fn elem_type(e: &FfiElem) -> libffi::middle::Type {
    match e {
        FfiElem::Scalar(k) => kind_type(*k),
        FfiElem::Struct(inner) => libffi::middle::Type::structure(inner.iter().map(elem_type)),
    }
}

#[cfg(feature = "ext-ffi")]
impl VaVal {
    fn ty(&self) -> libffi::middle::Type {
        use libffi::middle::Type;
        match self.inner {
            VaInner::I8(_) => Type::i8(),
            VaInner::I16(_) => Type::i16(),
            VaInner::I32(_) => Type::i32(),
            VaInner::I64(_) => Type::i64(),
            VaInner::U8(_) => Type::u8(),
            VaInner::U16(_) => Type::u16(),
            VaInner::U32(_) => Type::u32(),
            VaInner::U64(_) => Type::u64(),
            VaInner::F32(_) => Type::f32(),
            VaInner::F64(_) => Type::f64(),
            VaInner::Ptr(_) => Type::pointer(),
            VaInner::Struct { elements, .. } => Type::structure(elements.iter().map(elem_type)),
        }
    }

    fn arg(&self) -> libffi::middle::Arg<'_> {
        use libffi::middle::Arg;
        match &self.inner {
            VaInner::I8(v) => Arg::new(v),
            VaInner::I16(v) => Arg::new(v),
            VaInner::I32(v) => Arg::new(v),
            VaInner::I64(v) => Arg::new(v),
            VaInner::U8(v) => Arg::new(v),
            VaInner::U16(v) => Arg::new(v),
            VaInner::U32(v) => Arg::new(v),
            VaInner::U64(v) => Arg::new(v),
            VaInner::F32(v) => Arg::new(v),
            VaInner::F64(v) => Arg::new(v),
            VaInner::Ptr(v) => Arg::new(v),
            // libffi wants the slot to hold a pointer TO the value; for an
            // aggregate that IS the data pointer (the type says how many
            // bytes to read from it).
            VaInner::Struct { data, .. } => Arg::new(unsafe { &*(*data as *const u8) }),
        }
    }
}

/// Marshal a single value to a variadic argument of the given (already
/// promoted) kind.
#[cfg(feature = "ext-ffi")]
fn marshal_va(kind: FfiKind, v: &RubyValue) -> Result<VaVal, Signal> {
    use FfiKind::*;
    let plain = |inner| VaVal {
        inner,
        _owner: None,
    };
    Ok(match kind {
        I8 => plain(VaInner::I8(to_i64(v)? as i8)),
        I16 => plain(VaInner::I16(to_i64(v)? as i16)),
        I32 => plain(VaInner::I32(to_i64(v)? as i32)),
        I64 => plain(VaInner::I64(to_i64(v)?)),
        U8 => plain(VaInner::U8(to_i64(v)? as u8)),
        U16 => plain(VaInner::U16(to_i64(v)? as u16)),
        U32 => plain(VaInner::U32(to_i64(v)? as u32)),
        U64 => plain(VaInner::U64(to_i64(v)? as u64)),
        F32 => plain(VaInner::F32(to_f64(v)? as f32)),
        F64 => plain(VaInner::F64(to_f64(v)?)),
        Bool => plain(VaInner::I32(i32::from(to_bool(v)))),
        Str => {
            let c = to_cstring(v)?;
            let ptr = c.as_ptr() as *mut c_void;
            VaVal {
                inner: VaInner::Ptr(ptr),
                _owner: Some(c),
            }
        }
        // A String bound to a `:pointer` argument passes a NUL-terminated
        // copy of its bytes, exactly as the gem marshals it (how fiddle's
        // `strlen.call("...")` reaches C).
        Pointer => match v {
            RubyValue::Str(_) => {
                let c = to_cstring(v)?;
                let ptr = c.as_ptr() as *mut c_void;
                VaVal {
                    inner: VaInner::Ptr(ptr),
                    _owner: Some(c),
                }
            }
            _ => plain(VaInner::Ptr(to_pointer(v)?)),
        },
        Void => return Err(type_error!("`:void` is not a valid FFI argument type")),
    })
}

/// Marshal one FIXED argument of a variadic function -- no promotion (it matches
/// the declared prototype). Called from generated code per fixed argument.
#[cfg(feature = "ext-ffi")]
pub fn va_fixed(kind: FfiKind, v: &RubyValue) -> Result<VaVal, Signal> {
    marshal_va(kind, v)
}

/// Marshal one argument of a FIXED-signature runtime call (`FFI::Function`).
/// Identical to the fixed-variadic marshaling except `:bool`, which a fixed
/// prototype passes as the one-byte C `_Bool` rather than a promoted `int`.
#[cfg(feature = "ext-ffi")]
pub fn marshal_fixed(kind: FfiKind, v: &RubyValue) -> Result<VaVal, Signal> {
    if kind == FfiKind::Bool {
        return Ok(VaVal {
            inner: VaInner::U8(u8::from(to_bool(v))),
            _owner: None,
        });
    }
    marshal_va(kind, v)
}

/// Marshal a struct passed BY VALUE through the runtime tier: the bytes are
/// the ruby struct's own backing (`#to_ptr`, kept alive by the argument
/// object across the call), the descriptor is the declaration's real field
/// types. See `VaInner::Struct`.
#[cfg(feature = "ext-ffi")]
pub fn marshal_struct(elements: &'static [FfiElem], v: &RubyValue) -> Result<VaVal, Signal> {
    let data = to_pointer(v)?;
    Ok(VaVal {
        inner: VaInner::Struct { data, elements },
        _owner: None,
    })
}

/// Marshal one PROMOTED variadic argument from a runtime `(type, value)` pair
/// (`FFI::VariadicInvoker#call`); the compile-time varargs path resolves its
/// pairs through `va_parse_pairs` instead.
#[cfg(feature = "ext-ffi")]
pub fn va_promoted(kind: FfiKind, v: &RubyValue) -> Result<VaVal, Signal> {
    marshal_va(promote(kind), v)
}

/// Call a fixed-signature C function at `addr` with already-marshaled
/// arguments, through a runtime-built CIF. `FFI::Function#call`'s engine; the
/// compile-time `attach_function` path emits a direct `extern "C"` call and
/// never comes through here.
///
/// # Safety
/// `addr` must be a valid C function whose signature is described by the kinds
/// of `vals` and `ret`.
#[cfg(feature = "ext-ffi")]
pub unsafe fn call_fixed(
    addr: *const c_void,
    vals: Vec<VaVal>,
    ret: FfiKind,
) -> Result<RubyValue, Signal> {
    use libffi::middle::{Cif, CodePtr};
    if addr.is_null() {
        return Err(arg_error!("FFI call to a NULL function pointer"));
    }
    let cif = Cif::new(vals.iter().map(VaVal::ty), kind_type(ret));
    let args: Vec<libffi::middle::Arg> = vals.iter().map(VaVal::arg).collect();
    let code = CodePtr::from_ptr(addr);
    Ok(unsafe { call_and_wrap(&cif, code, &args, ret) })
}

/// Call a fixed-signature C function whose return is a struct passed BY
/// VALUE, through the runtime tier.
///
/// The extern tier lets rustc classify an aggregate return through a
/// `#[repr(C)]` mirror; here libffi does it, from the same real field types
/// (`elements`) the by-value ARGUMENT direction already builds. The returned
/// bytes are copied into a ruby-owned buffer and the generated wrapper views
/// them through the struct's own class, exactly as the extern tier's
/// `from_struct_ret` does.
///
/// # Safety
/// `addr` must be a valid C function whose signature is described by the
/// kinds of `vals` and by `elements`/`size`.
#[cfg(feature = "ext-ffi")]
pub unsafe fn call_struct_ret(
    addr: *const c_void,
    vals: Vec<VaVal>,
    elements: &'static [FfiElem],
    size: usize,
) -> Result<RubyValue, Signal> {
    use libffi::middle::{Cif, CodePtr, Ret, Type};
    if addr.is_null() {
        return Err(arg_error!("FFI call to a NULL function pointer"));
    }
    let ret_ty = Type::structure(elements.iter().map(elem_type));
    let cif = Cif::new(vals.iter().map(VaVal::ty), ret_ty);
    let args: Vec<libffi::middle::Arg> = vals.iter().map(VaVal::arg).collect();
    // `u64` words rather than bytes for two reasons libffi imposes: it writes
    // at least one full register even for a struct smaller than one, and the
    // buffer has to satisfy the widest alignment the aggregate's fields ask
    // for -- 8, since every scalar kind zeo builds a descriptor from is at
    // most 8 bytes wide.
    let mut buf: Vec<u64> = vec![0; size.div_ceil(8).max(1)];
    unsafe {
        cif.call_return_into(CodePtr::from_ptr(addr), &args, Ret::new(&mut buf[..]));
        Ok(from_struct_ret(buf.as_ptr().cast::<u8>(), size))
    }
}

/// A `VaVal` carrying a raw code/data pointer -- how generated code hands a
/// libffi closure's `code_ptr` (or any raw address) into `call_fixed` without
/// allocating an `FFI::Pointer` around it first.
#[cfg(feature = "ext-ffi")]
pub fn va_raw_pointer(p: *const c_void) -> VaVal {
    VaVal {
        inner: VaInner::Ptr(p as *mut c_void),
        _owner: None,
    }
}

/// One `attach_function` call site whose library is only decidable at RUN
/// time: a bundled `.so` path built from `__dir__`, a versioned soname, a
/// candidate list. Resolved once per site with `dlopen`/`dlsym` -- which is
/// when and how CRuby's ffi gem binds every symbol -- and cached on success.
/// A resolution FAILURE is not cached: it re-raises on every call, exactly as
/// retrying a failed `require` re-raises.
///
/// Emission mirrors `zeo_rt::ConstSite`: a `static` in the call-site block,
/// `const fn new`, an inner `OnceLock`.
#[cfg(feature = "ext-ffi")]
pub struct FfiSymSite {
    addr: std::sync::OnceLock<usize>,
}

#[cfg(feature = "ext-ffi")]
impl FfiSymSite {
    #[must_use]
    pub const fn new() -> FfiSymSite {
        FfiSymSite {
            addr: std::sync::OnceLock::new(),
        }
    }

    /// The symbol's address, resolving on first call. `candidates` are the
    /// `ffi_lib` alternatives IN ORDER, each tried as written and then
    /// through the gem's name manglings (`lib<x>.<ext>`, `<x>.<ext>`).
    pub fn get(&self, candidates: &[&str], sym: &str) -> Result<*const c_void, Signal> {
        if let Some(&a) = self.addr.get() {
            return Ok(a as *const c_void);
        }
        let handle = dlopen_first(candidates)?;
        let cname = std::ffi::CString::new(sym)
            .map_err(|_| arg_error!("FFI symbol name contains a null byte"))?;
        let addr = unsafe { libc::dlsym(handle, cname.as_ptr()) };
        if addr.is_null() {
            // The gem's own class for a symbol the library doesn't export.
            return Err(crate::raise_error(
                "FFI::NotFoundError",
                format!("Function '{sym}' not found in [{}]", candidates.join(", ")),
            ));
        }
        let _ = self.addr.set(addr as usize);
        Ok(addr)
    }

    /// The symbol's address from a DEFERRED library slot -- searched across
    /// the handles `ffi_lib_store` dlopened when the class body executed,
    /// in declaration order, exactly as the gem's `attach_function` searches
    /// its module's libraries. The class body always runs before its methods
    /// are callable, so an empty slot means the `ffi_lib` statement itself
    /// was skipped.
    pub fn get_slot(&self, slot: usize, sym: &str) -> Result<*const c_void, Signal> {
        if let Some(&a) = self.addr.get() {
            return Ok(a as *const c_void);
        }
        let handles = lib_slots()
            .read()
            .expect("no poisoned slot writers")
            .get(&slot)
            .cloned()
            .ok_or_else(|| {
                crate::builtins::load_error!("`ffi_lib` did not run before `{sym}` was called")
            })?;
        let cname = std::ffi::CString::new(sym)
            .map_err(|_| arg_error!("FFI symbol name contains a null byte"))?;
        for handle in handles {
            let addr = unsafe { libc::dlsym(handle as *mut c_void, cname.as_ptr()) };
            if !addr.is_null() {
                let _ = self.addr.set(addr as usize);
                return Ok(addr);
            }
        }
        Err(crate::raise_error(
            "FFI::NotFoundError",
            format!("Function '{sym}' not found"),
        ))
    }
}

/// Every symbol a Cranelift-emitted call site has already resolved, keyed
/// by the emitter-assigned site id. The rustc backend gets a `.bss`
/// [`FfiSymSite`] per site; CLIF sites carry an id and share this table,
/// the same shape `zeo_rt_regexp_lit` uses for its per-site literals. Like
/// `FfiSymSite`, only SUCCESS is cached -- a failed resolution re-raises on
/// every call.
#[cfg(feature = "ext-ffi")]
static SITE_ADDRS: std::sync::OnceLock<std::sync::RwLock<std::collections::HashMap<u32, usize>>> =
    std::sync::OnceLock::new();

#[cfg(feature = "ext-ffi")]
fn site_addrs() -> &'static std::sync::RwLock<std::collections::HashMap<u32, usize>> {
    SITE_ADDRS.get_or_init(Default::default)
}

/// A run-time `eval`'s symbol-site ids, held apart from the program's the
/// way `capi::literals::reserve_regexp_sites` holds regexp literals apart.
/// A collision here resolves a call to ANOTHER site's C function.
static NEXT_EVAL_SITE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1 << 20);

/// Reserve `n` consecutive symbol-site ids for one compiled snippet.
pub fn reserve_sym_sites(n: u32) -> u32 {
    NEXT_EVAL_SITE.fetch_add(n, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(feature = "ext-ffi")]
fn cached_site(site: u32) -> Option<*const c_void> {
    site_addrs()
        .read()
        .expect("no poisoned site readers")
        .get(&site)
        .map(|a| *a as *const c_void)
}

#[cfg(feature = "ext-ffi")]
fn cache_site(site: u32, addr: *const c_void) {
    site_addrs()
        .write()
        .expect("no poisoned site writers")
        .insert(site, addr as usize);
}

#[cfg(feature = "ext-ffi")]
fn dlsym_in(handle: *mut c_void, sym: &str) -> Result<*const c_void, Signal> {
    let cname = std::ffi::CString::new(sym)
        .map_err(|_| arg_error!("FFI symbol name contains a null byte"))?;
    Ok(unsafe { libc::dlsym(handle, cname.as_ptr()) })
}

/// Resolve one Cranelift call site's C symbol. `mode` is
/// `zeo_abi::abi::FFI_SYM_*`: the named libraries, the process image, or
/// the libraries with the process image as a fallback -- which is what a
/// build-time `#[link(name = ..)]` amounted to, since the linked library's
/// symbols are simply present in the running program.
#[cfg(feature = "ext-ffi")]
pub fn site_symbol(
    site: u32,
    candidates: &[&str],
    sym: &str,
    mode: u8,
) -> Result<*const c_void, Signal> {
    if let Some(a) = cached_site(site) {
        return Ok(a);
    }
    let mut lib_error = None;
    if mode != zeo_abi::abi::FFI_SYM_PROCESS {
        match dlopen_first(candidates) {
            Ok(handle) => {
                let addr = dlsym_in(handle, sym)?;
                if !addr.is_null() {
                    cache_site(site, addr);
                    return Ok(addr);
                }
            }
            Err(e) => lib_error = Some(e),
        }
    }
    if mode != zeo_abi::abi::FFI_SYM_LIB {
        let addr = dlsym_in(libc::RTLD_DEFAULT, sym)?;
        if !addr.is_null() {
            cache_site(site, addr);
            return Ok(addr);
        }
    }
    if let Some(e) = lib_error {
        return Err(e);
    }
    Err(crate::raise_error(
        "FFI::NotFoundError",
        if candidates.is_empty() {
            format!("Function '{sym}' not found in [current process]")
        } else {
            format!("Function '{sym}' not found in [{}]", candidates.join(", "))
        },
    ))
}

/// [`site_symbol`] for a DEFERRED library slot -- the handles
/// [`ffi_lib_store`] opened when the class body ran, searched in
/// declaration order.
#[cfg(feature = "ext-ffi")]
pub fn site_symbol_slot(site: u32, slot: usize, sym: &str) -> Result<*const c_void, Signal> {
    if let Some(a) = cached_site(site) {
        return Ok(a);
    }
    let handles = lib_slots()
        .read()
        .expect("no poisoned slot writers")
        .get(&slot)
        .cloned()
        .ok_or_else(|| {
            crate::builtins::load_error!("`ffi_lib` did not run before `{sym}` was called")
        })?;
    for handle in handles {
        let addr = dlsym_in(handle as *mut c_void, sym)?;
        if !addr.is_null() {
            cache_site(site, addr);
            return Ok(addr);
        }
    }
    Err(crate::raise_error(
        "FFI::NotFoundError",
        format!("Function '{sym}' not found"),
    ))
}

#[cfg(feature = "ext-ffi")]
impl Default for FfiSymSite {
    fn default() -> FfiSymSite {
        FfiSymSite::new()
    }
}

/// The DEFERRED tier's library handles, one Vec per `ffi_lib` statement
/// whose candidates only a running process can evaluate (an ENV read, a
/// local, a helper call). `ffi_lib_store` fills a slot when the class body
/// executes; every `attach_function` lowered under that statement reads it
/// back through [`FfiSymSite::get_slot`].
#[cfg(feature = "ext-ffi")]
static LIB_SLOTS: std::sync::OnceLock<
    std::sync::RwLock<std::collections::HashMap<usize, Vec<usize>>>,
> = std::sync::OnceLock::new();

#[cfg(feature = "ext-ffi")]
fn lib_slots() -> &'static std::sync::RwLock<std::collections::HashMap<usize, Vec<usize>>> {
    LIB_SLOTS.get_or_init(Default::default)
}

/// One evaluated `ffi_lib` argument into the value list: a SPLATTED array
/// spreads into separate values (each its own required library), exactly as
/// ruby's own splat would have passed them; anything else -- including a
/// plain Array, which lists ALTERNATIVES for one library -- rides whole.
#[cfg(feature = "ext-ffi")]
pub fn ffi_lib_spread(out: &mut Vec<RubyValue>, v: RubyValue, splatted: bool) {
    match v {
        RubyValue::Array(a) if splatted => out.extend(a.lock().iter().cloned()),
        other => out.push(other),
    }
}

/// `ffi_lib <exprs>` whose candidates resolve at RUN time, with the gem's
/// exact shape: every top-level VALUE is a separate required library (ALL
/// are opened), and an Array value lists ALTERNATIVE names for one library
/// (the first that opens wins). The dlopens are EAGER -- a library that
/// can't open raises `LoadError` here, at class-body time, exactly when
/// CRuby's own `ffi_lib` raises it. The handles land under `slot`, and a
/// symbol lookup searches them in order.
#[cfg(feature = "ext-ffi")]
pub fn ffi_lib_store(slot: usize, vals: &[RubyValue]) -> Result<RubyValue, Signal> {
    fn names_of(v: &RubyValue, out: &mut Vec<String>) -> Result<(), Signal> {
        match v {
            RubyValue::Str(s) => {
                out.push(String::from_utf8_lossy(s.lock().bytes()).into_owned());
            }
            RubyValue::Symbol(s) => out.push(s.name()),
            RubyValue::Array(a) => {
                let elems: Vec<RubyValue> = a.lock().iter().cloned().collect();
                for e in &elems {
                    names_of(e, out)?;
                }
            }
            other => {
                return Err(type_error!(
                    "ffi_lib expects a library name (String/Symbol), got {}",
                    crate::builtins::class_name_of(other)
                ));
            }
        }
        Ok(())
    }
    if vals.is_empty() {
        // The gem's own message for `ffi_lib []`.
        return Err(arg_error!("library names list must not be empty"));
    }
    let mut handles = Vec::with_capacity(vals.len());
    for v in vals {
        let mut alternatives = Vec::new();
        names_of(v, &mut alternatives)?;
        if alternatives.is_empty() {
            return Err(arg_error!("library names list must not be empty"));
        }
        let names: Vec<&str> = alternatives.iter().map(String::as_str).collect();
        handles.push(dlopen_first(&names)? as usize);
    }
    lib_slots()
        .write()
        .expect("no poisoned slot writers")
        .insert(slot, handles);
    Ok(RubyValue::Nil)
}

/// `dlopen` the first candidate that opens, trying each name as written and
/// then with the platform's `lib` prefix and shared-library suffix -- the
/// mangling `FFI.map_library_name` applies. Handles are never `dlclose`d,
/// matching `FFI::DynamicLibrary`. All candidates failing is a `LoadError`
/// carrying the accumulated `dlerror` text.
#[cfg(feature = "ext-ffi")]
fn dlopen_first(candidates: &[&str]) -> Result<*mut c_void, Signal> {
    #[cfg(target_os = "macos")]
    const DYLIB_EXT: &str = "dylib";
    #[cfg(not(target_os = "macos"))]
    const DYLIB_EXT: &str = "so";
    let mut errors = Vec::new();
    for cand in candidates {
        let mut names = vec![(*cand).to_string()];
        // `FFI::LibraryPath.wrap` answers `Platform::LIBC`/`Platform::LIBM`
        // for the bare names `c` and `m` rather than files called
        // `libc`/`libm`, and on glibc those constants are SONAMEs:
        // `libc.so` there is a linker SCRIPT, `libm.so` needs the dev
        // package, and the bare `.so` spellings below open nothing. macOS
        // never noticed -- `libc.dylib` and `libm.dylib` are real
        // libraries.
        if cfg!(all(target_os = "linux", target_env = "gnu")) {
            match *cand {
                "c" => names.push("libc.so.6".to_string()),
                "m" => names.push("libm.so.6".to_string()),
                _ => {}
            }
        }
        // A bare name (no path, no extension) gets the mangled spellings.
        if !cand.contains('/') && !cand.contains('.') {
            names.push(format!("lib{cand}.{DYLIB_EXT}"));
            names.push(format!("{cand}.{DYLIB_EXT}"));
        }
        for name in names {
            let Ok(cname) = std::ffi::CString::new(name.as_str()) else {
                continue;
            };
            let handle = unsafe { libc::dlopen(cname.as_ptr(), libc::RTLD_LAZY) };
            if !handle.is_null() {
                return Ok(handle);
            }
        }
        let err = unsafe { libc::dlerror() };
        if !err.is_null() {
            errors.push(
                unsafe { std::ffi::CStr::from_ptr(err) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    let detail = if errors.is_empty() {
        candidates.join(", ")
    } else {
        errors.join("; ")
    };
    Err(crate::builtins::load_error!(
        "Could not open library: {detail}"
    ))
}

/// Parse the trailing `*rest` of a variadic call -- a flat Array of alternating
/// `type_symbol, value` pairs -- into promoted variadic arguments.
#[cfg(feature = "ext-ffi")]
pub fn va_parse_pairs(rest: &RubyValue) -> Result<Vec<VaVal>, Signal> {
    let RubyValue::Array(a) = rest else {
        return Err(type_error!(
            "FFI varargs must be an Array of (type, value) pairs"
        ));
    };
    let elems: Vec<RubyValue> = a.lock().iter().cloned().collect();
    if !elems.len().is_multiple_of(2) {
        return Err(arg_error!(
            "FFI varargs must be (type, value) pairs, got an odd number of arguments"
        ));
    }
    let mut out = Vec::with_capacity(elems.len() / 2);
    let mut i = 0;
    while i < elems.len() {
        let RubyValue::Symbol(s) = &elems[i] else {
            return Err(type_error!("an FFI varargs type must be a Symbol"));
        };
        let kind = promote(kind_from_symbol(&s.name())?);
        out.push(marshal_va(kind, &elems[i + 1])?);
        i += 2;
    }
    Ok(out)
}

/// Call a variadic C function at `addr`. `fixed` is the number of non-variadic
/// (declared) leading arguments; `vals` is every argument (fixed then variadic),
/// already marshaled. Builds a variadic CIF (correct on ABIs like AArch64 where
/// variadic arguments are passed differently) and dispatches on the return kind.
///
/// # Safety
/// `addr` must be a valid variadic C function whose signature is described by
/// the kinds of `vals` (fixed part) and `ret`.
#[cfg(feature = "ext-ffi")]
pub unsafe fn call_variadic(
    addr: *const c_void,
    fixed: usize,
    vals: Vec<VaVal>,
    ret: FfiKind,
) -> Result<RubyValue, Signal> {
    use libffi::middle::{Cif, CodePtr};
    if addr.is_null() {
        return Err(arg_error!("FFI call to a NULL function pointer"));
    }
    let cif = Cif::new_variadic(vals.iter().map(VaVal::ty), fixed, kind_type(ret));
    let args: Vec<libffi::middle::Arg> = vals.iter().map(VaVal::arg).collect();
    let code = CodePtr::from_ptr(addr);
    Ok(unsafe { call_and_wrap(&cif, code, &args, ret) })
}

#[cfg(feature = "ext-ffi")]
unsafe fn call_and_wrap(
    cif: &libffi::middle::Cif,
    code: libffi::middle::CodePtr,
    args: &[libffi::middle::Arg],
    ret: FfiKind,
) -> RubyValue {
    use FfiKind::*;
    unsafe {
        match ret {
            Void => {
                let (): () = cif.call(code, args);
                RubyValue::Nil
            }
            I8 => RubyValue::Int(cif.call::<i8>(code, args) as i64),
            I16 => RubyValue::Int(cif.call::<i16>(code, args) as i64),
            I32 => RubyValue::Int(cif.call::<i32>(code, args) as i64),
            I64 => RubyValue::Int(cif.call::<i64>(code, args)),
            U8 => RubyValue::Int(cif.call::<u8>(code, args) as i64),
            U16 => RubyValue::Int(cif.call::<u16>(code, args) as i64),
            U32 => RubyValue::Int(cif.call::<u32>(code, args) as i64),
            U64 => RubyValue::Int(cif.call::<u64>(code, args) as i64),
            F32 => RubyValue::Float(cif.call::<f32>(code, args) as f64),
            F64 => RubyValue::Float(cif.call::<f64>(code, args)),
            Bool => RubyValue::Bool(cif.call::<u8>(code, args) != 0),
            Str => from_cstr(cif.call::<*const c_char>(code, args)),
            Pointer => from_pointer(cif.call::<*const c_void>(code, args)),
        }
    }
}

// ---- callbacks: a Ruby Proc as a C function pointer ----

#[cfg(feature = "ext-ffi")]
struct CallbackData {
    /// The Ruby Proc/lambda invoked on each C call.
    callable: RubyValue,
    /// The C argument kinds (how to read each incoming argument slot).
    arg_kinds: Vec<FfiKind>,
    /// The C return kind (how to write the Proc's result back to C).
    ret: FfiKind,
}

/// A live C function pointer trampolining into a Ruby Proc. The `libffi`
/// closure holds the executable trampoline; it MUST be dropped before the
/// boxed `CallbackData` it borrows (field order below ensures that), so the
/// handle stays alive for exactly as long as the C side may call back.
#[cfg(feature = "ext-ffi")]
pub struct CallbackHandle {
    closure: libffi::middle::Closure<'static>,
    _data: Box<CallbackData>,
}

#[cfg(feature = "ext-ffi")]
impl CallbackHandle {
    /// The C-callable function pointer for this closure.
    pub fn code_ptr(&self) -> *const c_void {
        *self.closure.code_ptr() as *const c_void
    }
}

/// Build a C-callable trampoline for a Ruby `callable` (a Proc) given the C
/// callback signature. The returned handle must be kept alive for the duration
/// of any C call that may invoke it.
#[cfg(feature = "ext-ffi")]
pub fn make_callback(
    callable: &RubyValue,
    arg_kinds: &[FfiKind],
    ret: FfiKind,
) -> Result<CallbackHandle, Signal> {
    use libffi::middle::{Cif, Closure};
    if !matches!(callable, RubyValue::Proc(_)) {
        return Err(type_error!(
            "wrong argument type {} (expected a Proc for an FFI callback)",
            class_name(callable.class_id()).unwrap_or_else(|| "?".to_string())
        ));
    }
    let data = Box::new(CallbackData {
        callable: callable.clone(),
        arg_kinds: arg_kinds.to_vec(),
        ret,
    });
    let cif = Cif::new(arg_kinds.iter().map(|k| kind_type(*k)), kind_type(ret));
    // The closure borrows `data`; we extend the borrow to 'static and guarantee
    // it by dropping `closure` before `_data` (struct field order).
    let data_ref: &'static CallbackData = unsafe { &*(data.as_ref() as *const CallbackData) };
    let closure = if matches!(ret, FfiKind::F32 | FfiKind::F64) {
        Closure::new(cif, cb_trampoline_float, data_ref)
    } else {
        Closure::new(cif, cb_trampoline_word, data_ref)
    };
    Ok(CallbackHandle {
        closure,
        _data: data,
    })
}

/// Invoke the Proc with the C arguments read out of their slots.
///
/// # Safety
/// `args` must be libffi's argument-slot array matching `data.arg_kinds`.
#[cfg(feature = "ext-ffi")]
unsafe fn invoke_callback(
    data: &CallbackData,
    args: *const *const c_void,
) -> Result<RubyValue, Signal> {
    let ruby_args: Vec<RubyValue> = data
        .arg_kinds
        .iter()
        .enumerate()
        .map(|(i, k)| unsafe { read_c_arg(*k, *args.add(i)) })
        .collect();
    // Under `ZEO_GVL=1` a callback may arrive with the GVL released -- a
    // `blocking: true` call runs its C body inside `without_gvl` -- and ruby
    // must not run without it. A no-op in the default parallel mode, and on
    // the synchronous path where the caller still holds.
    let _gvl = crate::gvl::process_gvl().hold_reentrant();
    crate::dispatch::send_value(
        &data.callable,
        crate::Symbol::intern("call"),
        &ruby_args,
        None,
    )
}

/// Trampoline for a callback whose return fits a machine word (int / uint /
/// bool / pointer / void). libffi writes the result as a full `ffi_arg`.
#[cfg(feature = "ext-ffi")]
unsafe extern "C" fn cb_trampoline_word(
    _cif: &libffi::low::ffi_cif,
    result: &mut libffi::low::ffi_arg,
    args: *const *const c_void,
    data: &CallbackData,
) {
    *result = match unsafe { invoke_callback(data, args) } {
        Ok(v) => ruby_to_word(data.ret, &v),
        Err(sig) => {
            store_callback_error(sig);
            0
        }
    };
}

/// Trampoline for a callback returning `float`/`double`.
#[cfg(feature = "ext-ffi")]
unsafe extern "C" fn cb_trampoline_float(
    _cif: &libffi::low::ffi_cif,
    result: &mut f64,
    args: *const *const c_void,
    data: &CallbackData,
) {
    *result = match unsafe { invoke_callback(data, args) } {
        Ok(v) => to_f64(&v).unwrap_or(0.0),
        Err(sig) => {
            store_callback_error(sig);
            0.0
        }
    };
}

/// Read one incoming C callback argument from its slot into a `RubyValue`.
///
/// # Safety
/// `slot` must point to a live C value of the given `kind`.
#[cfg(feature = "ext-ffi")]
unsafe fn read_c_arg(kind: FfiKind, slot: *const c_void) -> RubyValue {
    use FfiKind::*;
    unsafe {
        match kind {
            I8 => RubyValue::Int(*(slot as *const i8) as i64),
            I16 => RubyValue::Int(*(slot as *const i16) as i64),
            I32 => RubyValue::Int(*(slot as *const i32) as i64),
            I64 => RubyValue::Int(*(slot as *const i64)),
            U8 => RubyValue::Int(*(slot as *const u8) as i64),
            U16 => RubyValue::Int(*(slot as *const u16) as i64),
            U32 => RubyValue::Int(*(slot as *const u32) as i64),
            U64 => RubyValue::Int(*(slot as *const u64) as i64),
            F32 => RubyValue::Float(*(slot as *const f32) as f64),
            F64 => RubyValue::Float(*(slot as *const f64)),
            Bool => RubyValue::Bool(*(slot as *const u8) != 0),
            Str => from_cstr(*(slot as *const *const c_char)),
            Pointer => from_pointer(*(slot as *const *const c_void)),
            Void => RubyValue::Nil,
        }
    }
}

/// Convert the Proc's Ruby result to a machine-word C return value.
#[cfg(feature = "ext-ffi")]
fn ruby_to_word(kind: FfiKind, v: &RubyValue) -> libffi::low::ffi_arg {
    use FfiKind::*;
    match kind {
        Pointer | Str => to_pointer(v)
            .map(|p| p as libffi::low::ffi_arg)
            .unwrap_or(0),
        Bool => libffi::low::ffi_arg::from(to_bool(v)),
        Void => 0,
        _ => to_i64(v).unwrap_or(0) as libffi::low::ffi_arg,
    }
}

thread_local! {
    /// The first exception raised inside an FFI callback on this thread. A Ruby
    /// exception cannot unwind through the C frames that called the callback, so
    /// it is stashed here and re-raised by the caller once the C function
    /// returns (see `take_callback_error`).
    #[cfg(feature = "ext-ffi")]
    static CALLBACK_ERROR: std::cell::RefCell<Option<Signal>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "ext-ffi")]
fn store_callback_error(sig: Signal) {
    CALLBACK_ERROR.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.is_none() {
            *slot = Some(sig);
        }
    });
}

/// Re-raise the first exception a callback raised during the just-returned C
/// call, if any. Generated code calls this right after a C function that took a
/// callback argument.
#[cfg(feature = "ext-ffi")]
pub fn take_callback_error() -> Result<(), Signal> {
    CALLBACK_ERROR.with(|c| match c.borrow_mut().take() {
        Some(sig) => Err(sig),
        None => Ok(()),
    })
}
