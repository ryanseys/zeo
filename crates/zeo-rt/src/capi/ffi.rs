//! The FFI half of the C-ABI surface: one entry that resolves an
//! `attach_function`'s C symbol, and one that performs the call.
//!
//! The rustc backend emits a fn-local `extern "C"` block per call site and
//! lets rustc link the library, classify by-value aggregates and marshal
//! each argument inline. Cranelift can do none of that, so every tier here
//! goes through the libffi engine the rustc backend already uses for a
//! runtime-resolved library ([`crate::ffi::call_fixed`] and friends) --
//! same coercions, same error text, one indirect call slower. A direct
//! `call_indirect` on a declared C signature is the perf lever the plan
//! records; correctness does not wait for it.
//!
//! The whole signature rides in `.rodata` as a [`FfiCallC`] tree, so the
//! emitted code lowers only the argument EXPRESSIONS into a plain `argv`.
//! Nothing is half-built when a coercion raises.

use zeo_abi::abi::{
    FFI_TY_CALLBACK, FFI_TY_ENUM, FFI_TY_ENUM_SLOT, FFI_TY_STRPTR, FFI_TY_STRUCT, FfiCallC,
    FfiTypeC, STATUS_OK, STATUS_SIGNAL, Str,
};

use crate::RubyValue;
use crate::signal::Signal;

/// Park `sig` and answer the signal status -- the shape every fallible
/// entry here returns on the error edge.
fn fail(sig: Signal) -> i32 {
    crate::signal::set_pending(sig);
    STATUS_SIGNAL
}

/// Resolve one call site's C symbol address. `mode` is
/// `zeo_abi::abi::FFI_SYM_*`; `candidates` are the `ffi_lib` alternatives
/// in order (empty for the process image). The address is cached per site
/// on success only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_sym(
    site: u32,
    candidates: *const Str,
    n_candidates: usize,
    sym: *const u8,
    sym_len: usize,
    mode: u8,
    out: *mut usize,
) -> i32 {
    let sym = unsafe { super::str_slice(sym, sym_len) };
    let names: Vec<&str> = (0..n_candidates)
        .map(|i| {
            let s = unsafe { &*candidates.add(i) };
            unsafe { super::str_slice(s.ptr, s.len) }
        })
        .collect();
    match site_symbol(site, &names, sym, mode) {
        Ok(addr) => {
            unsafe { out.write(addr as usize) };
            STATUS_OK
        }
        Err(s) => fail(s),
    }
}

/// [`zeo_rt_ffi_sym`] for an `ffi_lib` whose candidates only the running
/// class body could evaluate: the handles live under `slot`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_sym_slot(
    site: u32,
    slot: usize,
    sym: *const u8,
    sym_len: usize,
    out: *mut usize,
) -> i32 {
    let sym = unsafe { super::str_slice(sym, sym_len) };
    match site_symbol_slot(site, slot, sym) {
        Ok(addr) => {
            unsafe { out.write(addr as usize) };
            STATUS_OK
        }
        Err(s) => fail(s),
    }
}

/// `ffi_lib <exprs>` whose candidates only the running class body could
/// evaluate: dlopen every value EAGERLY (an unopenable library is CRuby's
/// require-time `LoadError`, at this statement) and store the handles
/// under `slot`. `splats[i]` marks a value that arrived through a `*`, so
/// an Array spreads into separate required libraries rather than listing
/// alternatives for one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_lib_store(
    slot: usize,
    vals: *const RubyValue,
    splats: *const u8,
    n: usize,
    out: *mut RubyValue,
) -> i32 {
    let (vals, splats) = unsafe { (slice(vals, n), slice(splats, n)) };
    let mut libs: Vec<RubyValue> = Vec::with_capacity(n);
    for (v, splatted) in vals.iter().zip(splats) {
        spread(&mut libs, v.clone(), *splatted != 0);
    }
    match store_libs(slot, &libs) {
        Ok(v) => out_ok(out, v),
        Err(s) => fail(s),
    }
}

/// A DEFERRED `enum`'s member list, evaluated when the class body runs and
/// stored under `slot` for every signature lowered beneath it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_enum_store(
    slot: usize,
    vals: *const RubyValue,
    n: usize,
    out: *mut RubyValue,
) -> i32 {
    let members = unsafe { slice(vals, n) }.to_vec();
    match store_enum(slot, &members) {
        Ok(v) => out_ok(out, v),
        Err(s) => fail(s),
    }
}

/// A deferred enum's struct-FIELD halves: read an `int` back as its Symbol
/// (`get`), or a Symbol down to its `int` (`put`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_enum_field(
    slot: usize,
    put: u8,
    v: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let v = unsafe { &*v };
    let r = if put != 0 {
        enum_put(slot, v)
    } else {
        enum_get(slot, v)
    };
    match r {
        Ok(v) => out_ok(out, v),
        Err(s) => fail(s),
    }
}

unsafe fn slice<'a, T>(p: *const T, n: usize) -> &'a [T] {
    if n == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(p, n) }
}

fn out_ok(out: *mut RubyValue, v: RubyValue) -> i32 {
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
    STATUS_OK
}

/// Call the C function at `addr` with the signature `desc` describes and
/// the Ruby arguments in `argv`, and write the wrapped result to `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_invoke(
    desc: *const FfiCallC,
    addr: usize,
    argv: *const RubyValue,
    argc: usize,
    out: *mut RubyValue,
) -> i32 {
    let args: &[RubyValue] = if argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    };
    // This frame is `extern "C"`: a Rust panic reaching it aborts the
    // process ("panic in a function that cannot unwind"), which is how an
    // FFI marshaling bug used to end a program instead of raising. Caught
    // here, it is the RuntimeError the Ruby caller can rescue.
    let call = std::panic::AssertUnwindSafe(|| unsafe {
        invoke(&*desc, addr as *const std::os::raw::c_void, args)
    });
    let r = match std::panic::catch_unwind(call) {
        Ok(r) => r,
        Err(payload) => Err(crate::ffi::panic_signal("FFI call", payload)),
    };
    match r {
        Ok(v) => {
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            STATUS_OK
        }
        Err(s) => fail(s),
    }
}

#[cfg(not(feature = "ext-ffi"))]
fn site_symbol(_: u32, _: &[&str], _: &str, _: u8) -> Result<*const std::os::raw::c_void, Signal> {
    Err(no_ffi())
}

#[cfg(feature = "ext-ffi")]
use crate::ffi::site_symbol;

#[cfg(not(feature = "ext-ffi"))]
fn site_symbol_slot(_: u32, _: usize, _: &str) -> Result<*const std::os::raw::c_void, Signal> {
    Err(no_ffi())
}

#[cfg(feature = "ext-ffi")]
use crate::ffi::site_symbol_slot;

#[cfg(not(feature = "ext-ffi"))]
unsafe fn invoke(
    _desc: &FfiCallC,
    _addr: *const std::os::raw::c_void,
    _args: &[RubyValue],
) -> Result<RubyValue, Signal> {
    Err(no_ffi())
}

#[cfg(feature = "ext-ffi")]
unsafe fn invoke(
    desc: &FfiCallC,
    addr: *const std::os::raw::c_void,
    args: &[RubyValue],
) -> Result<RubyValue, Signal> {
    use crate::ffi;

    let variadic = desc.variadic != 0;
    let types: &[FfiTypeC] = if desc.n_args == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(desc.args, desc.n_args) }
    };
    // A variadic wrapper's LAST argument is the `*rest` Array; the
    // declared leading ones are `types`.
    let fixed = &args[..types.len().min(args.len())];

    let mut vals: Vec<ffi::VaVal> = Vec::with_capacity(args.len());
    // The libffi closures a callback argument builds. They must outlive the
    // call -- the C side may invoke one from inside it -- so they are held
    // here and dropped only when this function returns.
    let mut callbacks: Vec<ffi::CallbackHandle> = Vec::new();
    for (ty, v) in types.iter().zip(fixed) {
        match ty.tag {
            FFI_TY_ENUM => {
                let n = ffi::enum_to_int(v, &enum_members(ty))?;
                vals.push(ffi::marshal_fixed(ffi::FfiKind::I32, &RubyValue::Int(n))?);
            }
            FFI_TY_ENUM_SLOT => {
                let n = ffi::enum_to_int_slot(ty.slot, v)?;
                vals.push(ffi::marshal_fixed(ffi::FfiKind::I32, &RubyValue::Int(n))?);
            }
            // A Proc gets a closure built for this call; an `FFI::Function`
            // (or any pointer, nil for NULL) already IS a C function pointer
            // and passes its address, as the gem's callback converter does.
            FFI_TY_CALLBACK if matches!(v, RubyValue::Proc(_)) => {
                let kinds: Vec<ffi::FfiKind> = unsafe { sub_types(ty) }
                    .iter()
                    .map(|t| scalar_of(t.scalar))
                    .collect();
                let handle = ffi::make_callback(v, &kinds, scalar_of(ty.scalar))?;
                vals.push(ffi::va_raw_pointer(handle.code_ptr()));
                callbacks.push(handle);
            }
            FFI_TY_CALLBACK => vals.push(ffi::va_raw_pointer(ffi::to_pointer(v)?)),
            FFI_TY_STRUCT => vals.push(ffi::marshal_struct(unsafe { elements_of(ty) }, v)?),
            // `:strptr` is a RETURN type; the declaration rejects it in an
            // argument position, so the tag never reaches here.
            _ => {
                let kind = scalar_of(ty.scalar);
                vals.push(if variadic {
                    ffi::va_fixed(kind, v)?
                } else {
                    ffi::marshal_fixed(kind, v)?
                });
            }
        }
    }
    if variadic {
        let rest = args
            .last()
            .ok_or_else(|| crate::builtins::arg_error!("an FFI varargs call needs its `*rest`"))?;
        vals.extend(ffi::va_parse_pairs(rest)?);
    }

    let call = || -> Result<RubyValue, Signal> {
        if variadic {
            unsafe { ffi::call_variadic(addr, types.len(), vals, scalar_of(desc.ret.scalar)) }
        } else if desc.ret.tag == FFI_TY_STRUCT {
            let elems = unsafe { elements_of(&desc.ret) };
            unsafe { ffi::call_struct_ret(addr, vals, elems, desc.ret.size) }
        } else {
            unsafe { ffi::call_fixed(addr, vals, scalar_of(desc.ret.scalar)) }
        }
    };
    // `blocking: true` hands the GVL off across the call; a no-op in the
    // default parallel mode.
    let ret = if desc.blocking != 0 {
        crate::gvl::without_gvl(call)?
    } else {
        call()?
    };
    // An exception raised inside a callback could not unwind through C; it
    // was stashed and is re-raised now the C function has returned. EVERY
    // call asks, not only one that passed a callback: the C side keeps the
    // function pointers it was handed (`class_addMethod` an IMP, `qsort` no
    // -- an Objective-C `objc_msgSend` reaches a Ruby IMP through no
    // callback argument of its own), and the gem raises from whichever
    // attached call the callback ran under.
    ffi::take_callback_error()?;
    Ok(match (desc.ret.tag, ret) {
        (FFI_TY_ENUM, RubyValue::Int(n)) => ffi::int_to_enum(n, &enum_members(&desc.ret)),
        (FFI_TY_ENUM_SLOT, RubyValue::Int(n)) => ffi::int_to_enum_slot(desc.ret.slot, n),
        (FFI_TY_STRPTR, v) => unsafe { ffi::strptr_pair(v) },
        (_, v) => v,
    })
}

/// One type's `CScalar`. An unknown code cannot happen: the emitter writes
/// only what [`zeo_abi::ffi::CScalar::code`] produced.
#[cfg(feature = "ext-ffi")]
fn scalar_of(code: u8) -> crate::ffi::FfiKind {
    zeo_abi::ffi::CScalar::from_code(code).expect("the emitter writes only real scalar codes")
}

#[cfg(feature = "ext-ffi")]
fn enum_members(ty: &FfiTypeC) -> Vec<(&'static str, i64)> {
    (0..ty.n_members)
        .map(|i| {
            let m = unsafe { &*ty.members.add(i) };
            (
                unsafe { super::static_str(m.name.ptr, m.name.len) },
                m.value,
            )
        })
        .collect()
}

#[cfg(feature = "ext-ffi")]
unsafe fn sub_types(ty: &FfiTypeC) -> &'static [FfiTypeC] {
    if ty.n_sub == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(ty.sub, ty.n_sub) }
}

/// Every by-value struct descriptor this program has already converted,
/// keyed by its `.rodata` address. [`crate::ffi::marshal_struct`] wants a
/// `&'static [FfiElem]` (the rustc backend hands it a `const` slice), so
/// the converted list is leaked -- once per DESCRIPTOR, of which a program
/// has as many as it has by-value struct positions.
#[cfg(feature = "ext-ffi")]
static ELEMS: std::sync::OnceLock<
    std::sync::RwLock<std::collections::HashMap<usize, &'static [crate::ffi::FfiElem]>>,
> = std::sync::OnceLock::new();

#[cfg(feature = "ext-ffi")]
unsafe fn elements_of(ty: &FfiTypeC) -> &'static [crate::ffi::FfiElem] {
    use crate::ffi::FfiElem;
    let key = ty.sub as usize;
    let table = ELEMS.get_or_init(Default::default);
    if let Some(e) = table.read().expect("no poisoned elem readers").get(&key) {
        return e;
    }
    let built: Vec<FfiElem> = unsafe { sub_types(ty) }
        .iter()
        .map(|t| {
            if t.tag == FFI_TY_STRUCT {
                FfiElem::Struct(unsafe { elements_of(t) })
            } else {
                FfiElem::Scalar(scalar_of(t.scalar))
            }
        })
        .collect();
    let leaked: &'static [FfiElem] = Box::leak(built.into_boxed_slice());
    table
        .write()
        .expect("no poisoned elem writers")
        .insert(key, leaked);
    leaked
}

// The four class-body halves, each `cfg`'d the same way as `invoke`: a
// runtime built without FFI answers loudly rather than failing to link.

#[cfg(feature = "ext-ffi")]
fn spread(out: &mut Vec<RubyValue>, v: RubyValue, splatted: bool) {
    crate::ffi::ffi_lib_spread(out, v, splatted);
}

#[cfg(feature = "ext-ffi")]
fn store_libs(slot: usize, vals: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::ffi::ffi_lib_store(slot, vals)
}

#[cfg(feature = "ext-ffi")]
fn store_enum(slot: usize, vals: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::ffi::enum_store(slot, vals)
}

#[cfg(feature = "ext-ffi")]
fn enum_get(slot: usize, v: &RubyValue) -> Result<RubyValue, Signal> {
    Ok(crate::ffi::int_to_enum_slot(slot, crate::ffi::to_i64(v)?))
}

#[cfg(feature = "ext-ffi")]
fn enum_put(slot: usize, v: &RubyValue) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Int(crate::ffi::enum_to_int_slot(slot, v)?))
}

#[cfg(not(feature = "ext-ffi"))]
fn spread(out: &mut Vec<RubyValue>, v: RubyValue, _splatted: bool) {
    out.push(v);
}

#[cfg(not(feature = "ext-ffi"))]
fn store_libs(_slot: usize, _vals: &[RubyValue]) -> Result<RubyValue, Signal> {
    Err(no_ffi())
}

#[cfg(not(feature = "ext-ffi"))]
fn store_enum(_slot: usize, _vals: &[RubyValue]) -> Result<RubyValue, Signal> {
    Err(no_ffi())
}

#[cfg(not(feature = "ext-ffi"))]
fn enum_get(_slot: usize, _v: &RubyValue) -> Result<RubyValue, Signal> {
    Err(no_ffi())
}

#[cfg(not(feature = "ext-ffi"))]
fn enum_put(_slot: usize, _v: &RubyValue) -> Result<RubyValue, Signal> {
    Err(no_ffi())
}

#[cfg(not(feature = "ext-ffi"))]
fn no_ffi() -> Signal {
    crate::builtins::not_impl_error!("this runtime was built without FFI support")
}
