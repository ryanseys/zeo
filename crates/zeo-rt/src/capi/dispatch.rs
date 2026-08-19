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
