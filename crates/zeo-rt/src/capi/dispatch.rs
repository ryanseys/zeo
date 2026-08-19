//! The dynamic-dispatch surface: the cached/uncached sends, the `.bss`
//! site-slot initializers, and the Rust->C call bridge every `ValueImpl::C`
//! row runs through.

use super::ValueFn;
use crate::{RubyValue, Signal, Symbol};
use std::mem::{ManuallyDrop, MaybeUninit};
use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};

/// Call a C-ABI method body from Rust dispatch: borrow `recv`/`args`, move
/// the block in (the callee consumes it -- the boundary convention), and
/// translate the status protocol back into `Result`.
pub(crate) fn call_value_fn(
    f: ValueFn,
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mut out = MaybeUninit::<RubyValue>::uninit();
    // The callee owns the block from here; `ManuallyDrop` is the move.
    let mut blk = ManuallyDrop::new(block);
    let blk_ptr = match &mut *blk {
        Some(b) => {
            super::leakcheck::created(b);
            b as *mut RubyValue
        }
        None => std::ptr::null_mut(),
    };
    let status = unsafe { f(recv, args.as_ptr(), args.len(), blk_ptr, out.as_mut_ptr()) };
    if status == STATUS_OK {
        let v = unsafe { out.assume_init() };
        super::leakcheck::consumed(&v);
        Ok(v)
    } else {
        Err(crate::signal::take_pending()
            .expect("a compiled body answered STATUS_SIGNAL with an empty pending slot"))
    }
}

/// The result-to-status inverse of [`call_value_fn`]: every fallible capi
/// entry funnels its `Result` through here.
#[inline]
pub(crate) fn status_out(r: Result<RubyValue, Signal>, out: *mut RubyValue) -> i32 {
    match r {
        Ok(v) => {
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            STATUS_OK
        }
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// The borrowed views a send entry needs from its raw arguments.
unsafe fn call_views<'a>(
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
) -> (&'a [RubyValue], Option<RubyValue>) {
    let args = if argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    };
    // The caller passes the block by MOVE; a null pointer is "no block".
    let block = if blk.is_null() {
        None
    } else {
        super::leakcheck::consumed(unsafe { &*blk });
        Some(unsafe { std::ptr::read(blk) })
    };
    (args, block)
}

/// [`crate::dispatch::send_value_cached`] -- the monomorphic-inline-cached
/// dynamic send.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_value_cached(
    site: &'static crate::dispatch::CallSite,
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let (args, block) = unsafe { call_views(argv, argc, blk) };
    status_out(
        crate::dispatch::send_value_cached(
            site,
            box_id,
            unsafe { &*recv },
            Symbol::from_u32(sym),
            args,
            block,
        ),
        out,
    )
}

/// The uncached dynamic send (implicit receiver -- no visibility barrier).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_value_in(
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let (args, block) = unsafe { call_views(argv, argc, blk) };
    status_out(
        crate::dispatch::send_value_in(
            box_id,
            unsafe { &*recv },
            Symbol::from_u32(sym),
            args,
            block,
        ),
        out,
    )
}

/// The uncached dynamic send behind the explicit-receiver visibility
/// barrier (`caller` = the call site's enclosing class; `FCALL` asks no
/// question).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_value_explicit_in(
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    caller: u32,
    out: *mut RubyValue,
) -> i32 {
    let (args, block) = unsafe { call_views(argv, argc, blk) };
    status_out(
        crate::dispatch::send_value_explicit_in(
            box_id,
            unsafe { &*recv },
            Symbol::from_u32(sym),
            args,
            block,
            caller,
        ),
        out,
    )
}

/// The keyword-send twins' shared tail: `kw` is the call site's freshly
/// built keyword Hash (BORROWED). A non-empty set is marked and appended
/// as the trailing argument (the trailing-kwargs-hash convention the
/// binder peels); a runtime-EMPTY set is dropped -- `f(**{})` passes no
/// keywords, so it must not raise on a `**nil` callee.
unsafe fn with_kw_args<R>(
    argv: *const RubyValue,
    argc: usize,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    send: impl FnOnce(&[RubyValue], Option<RubyValue>) -> R,
) -> R {
    let (args, block) = unsafe { call_views(argv, argc, blk) };
    let kw = unsafe { &*kw };
    let RubyValue::Hash(h) = kw else {
        panic!("a kw send's keyword argument must be a Hash, got {kw:?}")
    };
    if h.lock().is_empty() {
        return send(args, block);
    }
    crate::value::collections::hash_mark_kwargs(h);
    let mut full = Vec::with_capacity(argc + 1);
    full.extend_from_slice(args);
    full.push(kw.clone());
    send(&full, block)
}

/// [`zeo_rt_send_value_in`] with call-site keywords.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_value_kw_in(
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    argv: *const RubyValue,
    argc: usize,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let r = unsafe {
        with_kw_args(argv, argc, kw, blk, |args, block| {
            crate::dispatch::send_value_in(box_id, &*recv, Symbol::from_u32(sym), args, block)
        })
    };
    status_out(r, out)
}

/// [`zeo_rt_send_value_explicit_in`] with call-site keywords.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_value_explicit_kw_in(
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    argv: *const RubyValue,
    argc: usize,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    caller: u32,
    out: *mut RubyValue,
) -> i32 {
    let r = unsafe {
        with_kw_args(argv, argc, kw, blk, |args, block| {
            crate::dispatch::send_value_explicit_in(
                box_id,
                &*recv,
                Symbol::from_u32(sym),
                args,
                block,
                caller,
            )
        })
    };
    status_out(r, out)
}

/// The splat-call entries' shared tail: `args` is a runtime-built Array
/// (the call site pushed singles and splat-expanded elements). `unmark`
/// clears a kw mark off the trailing element first -- a splat-expanded
/// hash is POSITIONAL again (ruby's rule; `ruby2_keywords` is the opt-out
/// and passes 0). `kw` (null = none) then appends per the kw convention.
unsafe fn with_array_args<R>(
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    send: impl FnOnce(&[RubyValue], Option<RubyValue>) -> R,
) -> R {
    let RubyValue::Array(a) = (unsafe { &*args }) else {
        panic!("a splat send's args must be an Array")
    };
    let mut full: Vec<RubyValue> = a.lock().iter().cloned().collect();
    if unmark != 0 {
        crate::value::collections::unmark_kwargs_tail(&mut full);
    }
    if !kw.is_null() {
        let kw = unsafe { &*kw };
        let RubyValue::Hash(h) = kw else {
            panic!("a kw send's keyword argument must be a Hash, got {kw:?}")
        };
        if !h.lock().is_empty() {
            crate::value::collections::hash_mark_kwargs(h);
            full.push(kw.clone());
        }
    }
    let block = if blk.is_null() {
        None
    } else {
        super::leakcheck::consumed(unsafe { &*blk });
        Some(unsafe { std::ptr::read(blk) })
    };
    send(&full, block)
}

/// [`zeo_rt_send_value_in`] with a runtime-built argument Array (a call
/// site with a `*` splat).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_value_args_in(
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let r = unsafe {
        with_array_args(args, unmark, kw, blk, |full, block| {
            crate::dispatch::send_value_in(box_id, &*recv, Symbol::from_u32(sym), full, block)
        })
    };
    status_out(r, out)
}

/// [`zeo_rt_send_value_explicit_in`] with a runtime-built argument Array.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_value_explicit_args_in(
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    caller: u32,
    out: *mut RubyValue,
) -> i32 {
    let r = unsafe {
        with_array_args(args, unmark, kw, blk, |full, block| {
            crate::dispatch::send_value_explicit_in(
                box_id,
                &*recv,
                Symbol::from_u32(sym),
                full,
                block,
                caller,
            )
        })
    };
    status_out(r, out)
}

/// `*expr` at a call site: splat-expand `src` into the args Array being
/// built at `dst` (Ruby's `to_a` coercion; nil contributes nothing;
/// non-convertible wraps as one element -- `array_splat_into`'s rules).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_push_splat(
    dst: *const RubyValue,
    src: *const RubyValue,
) -> i32 {
    let RubyValue::Array(a) = (unsafe { &*dst }) else {
        panic!("array_push_splat's destination must be the args Array")
    };
    let mut extra = Vec::new();
    match crate::value::collections::array_splat_into(&mut extra, unsafe { &*src }) {
        Ok(()) => {
            let mut al = a.lock();
            for v in extra {
                al.push(v);
            }
            STATUS_OK
        }
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// `**expr` at a call site: coerce `src` to a Hash (Ruby's `to_hash`
/// protocol; `TypeError` otherwise) and merge its pairs into the keyword
/// hash being built at `dst` -- later keys overwrite, Ruby's merge order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_kw_splat_into(dst: *const RubyValue, src: *const RubyValue) -> i32 {
    let RubyValue::Hash(d) = (unsafe { &*dst }) else {
        panic!("kw_splat_into's destination must be the kw Hash")
    };
    match crate::rproc::to_hash_coerce(unsafe { &*src }) {
        Ok(h) => {
            let pairs: Vec<(RubyValue, RubyValue)> = h.lock().values().cloned().collect();
            for (k, v) in pairs {
                crate::value::collections::hash_set(d, k, v);
            }
            STATUS_OK
        }
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// [`crate::dispatch::send_class_cached`] -- the class-method-receiver
/// cached send (`Math.sqrt`, `File.read`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_class_cached(
    site: &'static crate::dispatch::ClassMethodSite,
    cid: u32,
    recv: *const RubyValue,
    sym: u32,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    caller: u32,
    out: *mut RubyValue,
) -> i32 {
    let (args, block) = unsafe { call_views(argv, argc, blk) };
    status_out(
        crate::dispatch::send_class_cached(
            site,
            cid,
            unsafe { &*recv },
            Symbol::from_u32(sym),
            args,
            block,
            caller,
        ),
        out,
    )
}

/// Initialize one `.bss` `CallSite` slot -- `zeo_unit_init` writes every
/// site explicitly; zero bytes are never assumed valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_callsite_init(slot: *mut crate::dispatch::CallSite, caller: u32) {
    unsafe { slot.write(crate::dispatch::CallSite::new(caller)) };
}

/// Initialize one `.bss` `ClassMethodSite` slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_classmethod_site_init(slot: *mut crate::dispatch::ClassMethodSite) {
    unsafe { slot.write(crate::dispatch::ClassMethodSite::new()) };
}
