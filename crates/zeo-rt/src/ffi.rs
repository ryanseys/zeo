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
    if let RubyValue::Object(_) = v {
        if let Ok(p) = crate::dispatch::send_value(v, crate::Symbol::intern("to_ptr"), &[], None) {
            if let Some(addr) = crate::ext::ffi::address_of(&p) {
                return Ok(addr as *mut c_void);
            }
        }
    }
    Err(type_err("pointer", v))
}

/// A `:pointer` C return value -> an `FFI::Pointer` wrapping the address (a NULL
/// pointer wraps to an `FFI::Pointer` whose `#null?` is true, matching the gem).
#[cfg(feature = "ext-ffi")]
pub fn from_pointer(p: *const c_void) -> RubyValue {
    crate::ext::ffi::wrap_address(p as usize)
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
        RubyValue::Str(crate::string_from_bytes(
            bytes,
            crate::encoding::default_external(),
        ))
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
/// resolves a `:type` Symbol in a varargs `(type, value)` pair to one of these.
#[cfg(feature = "ext-ffi")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FfiKind {
    Void,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Str,
    Pointer,
}

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

/// Map a varargs `:type` Symbol name to its kind, mirroring the compile-time
/// `ffi_type_of` table in `zeo`'s `lower/ffi.rs`.
#[cfg(feature = "ext-ffi")]
fn kind_from_symbol(name: &str) -> Result<FfiKind, Signal> {
    use FfiKind::*;
    Ok(match name {
        "char" | "int8" => I8,
        "short" | "int16" => I16,
        "int" | "int32" => I32,
        "long" | "long_long" | "int64" | "ssize_t" => I64,
        "uchar" | "uint8" => U8,
        "ushort" | "uint16" => U16,
        "uint" | "uint32" => U32,
        "ulong" | "ulong_long" | "uint64" | "size_t" => U64,
        "float" => F32,
        "double" => F64,
        "bool" => Bool,
        "string" => Str,
        "pointer" | "buffer_in" | "buffer_out" | "buffer_inout" => Pointer,
        other => return Err(arg_error!("unknown FFI varargs type :{other}")),
    })
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
        Pointer => plain(VaInner::Ptr(to_pointer(v)?)),
        Void => return Err(type_error!("`:void` is not a valid FFI argument type")),
    })
}

/// Marshal one FIXED argument of a variadic function -- no promotion (it matches
/// the declared prototype). Called from generated code per fixed argument.
#[cfg(feature = "ext-ffi")]
pub fn va_fixed(kind: FfiKind, v: &RubyValue) -> Result<VaVal, Signal> {
    marshal_va(kind, v)
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
