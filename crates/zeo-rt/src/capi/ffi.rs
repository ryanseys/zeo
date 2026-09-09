//! The FFI half of the C-ABI surface: the entries that resolve an
//! `attach_function`'s C symbol, the two call tiers' rows, and the
//! class-body halves of a deferred `ffi_lib`/`enum`.
//!
//! The DIRECT tier's rows convert one argument or wrap one result each; the
//! emitted code does the call itself on the declared C signature. The
//! LIBFFI tier is one entry, [`zeo_rt_ffi_invoke`], for the shapes
//! Cranelift cannot express (varargs, by-value aggregates, callbacks,
//! enums): the whole signature rides in `.rodata` as a [`FfiCallC`] tree,
//! so the emitted code lowers only the argument EXPRESSIONS into a plain
//! `argv`, and nothing is half-built when a coercion raises. Both tiers
//! share every coercion and every error text ([`crate::ffi`]).

use zeo_abi::abi::{
    FFI_TY_CALLBACK, FFI_TY_ENUM, FFI_TY_ENUM_SLOT, FFI_TY_STRPTR, FFI_TY_STRUCT, FfiCallC,
    FfiTypeC, STATUS_OK, STATUS_SIGNAL, Str,
};

use zeo_abi::ffi::CScalar;

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
    // process ("panic in a function that cannot unwind"), so an FFI
    // marshaling bug would end the program instead of raising. Caught
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

// --- The direct tier's marshaling rows -------------------------------------
//
// A fixed-signature `attach_function` whose every position is a plain C
// scalar is called by the emitted code itself (`call_indirect` on the
// declared C signature). These rows are the halves the emitted code cannot
// do inline: each Ruby argument's conversion, which carries ruby-ffi's own
// range checks and error texts, and each heap-allocating result's wrap.
// Every one is `catch_unwind`-guarded the way `zeo_rt_ffi_invoke` is: a
// marshaling bug is a RuntimeError, not an abort.

/// One C integer argument (or a `_Bool`, as 0/1) of `kind`, as the u64 bit
/// pattern the emitted code narrows to the declared width.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_to_int(kind: u8, v: *const RubyValue, out: *mut u64) -> i32 {
    let v = unsafe { &*v };
    match guarded(|| direct_to_int(scalar_of(kind), v)) {
        Ok(bits) => {
            unsafe { out.write(bits) };
            STATUS_OK
        }
        Err(s) => fail(s),
    }
}

/// One C floating-point argument, as an `f64` (the emitted code demotes a
/// `float`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_to_f64(v: *const RubyValue, out: *mut f64) -> i32 {
    let v = unsafe { &*v };
    match guarded(|| crate::ffi::to_f64(v)) {
        Ok(f) => {
            unsafe { out.write(f) };
            STATUS_OK
        }
        Err(s) => fail(s),
    }
}

/// One `:string` or `:pointer` argument, as the address C receives. A
/// String makes a NUL-terminated COPY whose owner is written to `tmp`, an
/// OWNED value the emitted code pools so the copy outlives the call; every
/// other pointer leaves `tmp` nil.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_to_ptr(
    kind: u8,
    v: *const RubyValue,
    tmp: *mut RubyValue,
    out: *mut usize,
) -> i32 {
    let v = unsafe { &*v };
    match guarded(|| direct_to_ptr(scalar_of(kind), v)) {
        Ok((addr, owner)) => {
            super::leakcheck::created(&owner);
            unsafe {
                tmp.write(owner);
                out.write(addr);
            }
            STATUS_OK
        }
        Err(s) => fail(s),
    }
}

/// An unsigned 64-bit C result -- past `i64::MAX` a Bignum, which the
/// emitted code cannot build inline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_from_uint(bits: u64, out: *mut RubyValue) {
    let v = direct_from_uint(bits);
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A `:pointer` C result as an `FFI::Pointer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_from_ptr(p: usize, out: *mut RubyValue) {
    let v = direct_from_ptr(p);
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A `:string` C result read back as a binary String (`nil` for NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_from_cstr(p: usize, out: *mut RubyValue) {
    let v = unsafe { crate::ffi::from_cstr(p as *const std::os::raw::c_char) };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// After the C function returned: re-raise an exception a callback stashed
/// while C had the stack -- the same question `zeo_rt_ffi_invoke` asks
/// after every call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ffi_after_call() -> i32 {
    match after_call() {
        Ok(()) => STATUS_OK,
        Err(s) => fail(s),
    }
}

/// Run one marshaling step with a Rust panic turned into the RuntimeError
/// the Ruby caller can rescue, as `zeo_rt_ffi_invoke` does for its whole
/// body.
fn guarded<T>(f: impl FnOnce() -> Result<T, Signal>) -> Result<T, Signal> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => Err(crate::ffi::panic_signal("FFI call", payload)),
    }
}

#[cfg(feature = "ext-ffi")]
fn direct_to_int(kind: CScalar, v: &RubyValue) -> Result<u64, Signal> {
    if kind == CScalar::Bool {
        return Ok(u64::from(crate::ffi::to_bool(v)));
    }
    crate::ffi::to_c_int(kind, v)
}

#[cfg(feature = "ext-ffi")]
fn direct_to_ptr(kind: CScalar, v: &RubyValue) -> Result<(usize, RubyValue), Signal> {
    // A String reaches C as a NUL-terminated copy for `:string` AND for
    // `:pointer` -- the gem's own rule (`marshal_va` keeps it too).
    if kind == CScalar::Str || matches!(v, RubyValue::Str(_)) {
        let c = crate::ffi::to_cstring(v)?;
        let owner = crate::string_from_bytes(c.into_bytes_with_nul(), crate::encoding::ASCII_8BIT);
        // The copy's buffer moves for nobody: the owner is reachable only
        // through the emitted code's temp slot, which never mutates it.
        let addr = owner.lock().bytes().as_ptr() as usize;
        return Ok((addr, RubyValue::Str(owner)));
    }
    Ok((crate::ffi::to_pointer(v)? as usize, RubyValue::Nil))
}

#[cfg(feature = "ext-ffi")]
fn direct_from_uint(bits: u64) -> RubyValue {
    crate::ffi::from_c_uint(bits)
}

#[cfg(feature = "ext-ffi")]
fn direct_from_ptr(p: usize) -> RubyValue {
    crate::ffi::from_pointer(p as *const std::os::raw::c_void)
}

#[cfg(feature = "ext-ffi")]
fn after_call() -> Result<(), Signal> {
    crate::ffi::take_callback_error()
}

#[cfg(not(feature = "ext-ffi"))]
fn direct_to_int(_kind: CScalar, _v: &RubyValue) -> Result<u64, Signal> {
    Err(no_ffi())
}

#[cfg(not(feature = "ext-ffi"))]
fn direct_to_ptr(_kind: CScalar, _v: &RubyValue) -> Result<(usize, RubyValue), Signal> {
    Err(no_ffi())
}

#[cfg(not(feature = "ext-ffi"))]
fn direct_from_uint(bits: u64) -> RubyValue {
    RubyValue::Int(bits as i64)
}

#[cfg(not(feature = "ext-ffi"))]
fn direct_from_ptr(_p: usize) -> RubyValue {
    RubyValue::Nil
}

#[cfg(not(feature = "ext-ffi"))]
fn after_call() -> Result<(), Signal> {
    Ok(())
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
fn scalar_of(code: u8) -> CScalar {
    CScalar::from_code(code).expect("the emitter writes only real scalar codes")
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
/// `&'static [FfiElem]`, so
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
