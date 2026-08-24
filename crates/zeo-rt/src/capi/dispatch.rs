//! The dynamic-dispatch surface: the cached/uncached sends, the `.bss`
//! site-slot initializers, and the Rust->C call bridge every `ValueImpl::C`
//! row runs through.

use super::ValueFn;
use crate::{ClassId, RubyValue, Signal, Symbol};
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

/// The `alias new old` KEYWORD: it installs on the frame's DEFAULT
/// DEFINEE (a `*_eval`/`instance_exec` may have re-homed the block), not
/// on `self` -- `Module#alias_method` is the call form that dispatches to
/// its receiver.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_alias_in_default_definee(
    cref: *const RubyValue,
    slf: *const RubyValue,
    new: *const RubyValue,
    old: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    status_out(
        crate::dispatch::alias_in_default_definee(
            unsafe { &*cref },
            unsafe { &*slf },
            unsafe { &*new }.clone(),
            unsafe { &*old }.clone(),
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

/// A VCALL -- a bare identifier ruby could have read as a local
/// (`foo`, never `foo()` or `self.foo`). Same lookup as
/// [`zeo_rt_send_value_in`]; only the MISS differs, and it is the whole
/// point: ruby says "undefined local variable or method" and raises
/// NameError, not NoMethodError.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_value_vcall_in(
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    out: *mut RubyValue,
) -> i32 {
    status_out(
        crate::dispatch::send_value_vcall_in(box_id, unsafe { &*recv }, Symbol::from_u32(sym)),
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
pub(super) unsafe fn with_array_args<R>(
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

/// A builtin reopen's body ran ABOVE its own `class Foo ... end`, so the row
/// it REPLACED is what ruby answers with. Call that row.
///
/// Not `super`: `super` resumes PAST the reopened class, and a name the
/// builtin owns outright (`Array#size`) has nothing above it -- ruby raises
/// "no superclass method" for a `super` written there, and rightly. The row
/// being replaced sits ON one of these ancestors, so the walk starts at the
/// receiver's own class and takes the first NATIVE table hit.
///
/// A name the builtin never had answers ruby's plain `undefined method`: the
/// reopen has not run, so nothing defines it yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_native_row_call(
    recv: *const RubyValue,
    sym: u32,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let recv = unsafe { &*recv };
    let name = Symbol::from_u32(sym);
    let n = name.name_str();
    let (args, block) = unsafe { call_views(argv, argc, blk) };
    let row = crate::dispatch::ancestors_of_value(recv.class_id())
        .iter()
        .find_map(|&anc| {
            crate::builtins::class_table(anc)
                .and_then(|lookup| lookup(n))
                .map(|f| (anc, f))
        });
    let r = match row {
        Some((anc, f)) => {
            crate::dispatch::with_c_frame(crate::dispatch::c_frame_label(anc, name, '#'), || {
                f(recv, args, block)
            })
        }
        None => Err(crate::dispatch::raise_method_missing(
            recv,
            n,
            args,
            crate::dispatch::MissingReason::NoEntry,
        )),
    };
    status_out(r, out)
}

/// Has anything been defined at run time? The compile-time inlined
/// accessor asks before it may stand in for the ordinary dispatch: a
/// `define_method` of the same name installs a row the fold cannot see, so
/// the fold is only sound while the overlay is dormant.
///
/// Emitted ONLY at a site whose name `Compiler::may_be_patched_at_runtime`
/// answers for, so a program that redefines nothing pays nothing.
#[unsafe(no_mangle)]
pub extern "C" fn zeo_rt_is_live() -> i8 {
    i8::from(crate::runtime_meta::is_live())
}

/// May a fused iteration splice stand in for `recv_class`'s own iterator
/// method? False once anything has redefined it (or opened an overlay the
/// splice cannot see), which is what makes the fast arm's guard a runtime
/// question rather than a compile-time one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_iter_inline_ok_for(box_id: u32, recv_class: u32) -> i8 {
    i8::from(crate::runtime_meta::iter_inline_ok_for(
        box_id,
        ClassId(recv_class),
    ))
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
    // `**nil` is ruby's "pass no keywords" spelling -- a no-op, never a
    // conversion. Only the literal `nil` qualifies; anything else that is
    // not a Hash still has to answer `to_hash` or raise.
    if matches!(unsafe { &*src }, RubyValue::Nil) {
        return STATUS_OK;
    }
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

/// `case`/`when`: does `pattern === subject`? Dispatches the candidate's
/// own `===` (a user class overriding it is the whole point of `case`).
/// `*hit` gets 0/1 on success; a raising `===` propagates.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_case_eq(
    pattern: *const RubyValue,
    subject: *const RubyValue,
    hit: *mut i8,
) -> i32 {
    match crate::value::case_eq(unsafe { &*pattern }, unsafe { &*subject }) {
        Ok(b) => {
            unsafe { hit.write(i8::from(b)) };
            STATUS_OK
        }
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// `K.method(:name)` captured BEFORE every own `def self.name` in the
/// document: CRuby resolves at capture time, so the Method binds the
/// INHERITED entry. The COMPILER proves the position and emits this
/// instead of the by-name capture, which would find the later override
/// and recurse forever.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_method_capture_inherited(
    recv: *const RubyValue,
    name_arg: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    status_out(
        crate::builtins::method::method_capture_inherited(unsafe { &*recv }, unsafe { &*name_arg }),
        out,
    )
}

/// `when *candidates`: the splat form -- `===` over every element of the
/// (to_a-coerced) candidate list, short-circuiting on the first hit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_case_eq_any(
    candidates: *const RubyValue,
    subject: *const RubyValue,
    hit: *mut i8,
) -> i32 {
    match crate::value::case_eq_any(unsafe { &*candidates }, unsafe { &*subject }) {
        Ok(b) => {
            unsafe { hit.write(i8::from(b)) };
            STATUS_OK
        }
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// An instance-method `super`: resume the MRO walk after `defining_class`
/// in the receiver's live ancestry (`send_super_from`). Args ride the
/// splat-call Array convention ([`with_array_args`]): `unmark` clears a
/// splat-expanded trailing hash's kw mark (rustc passes it only when a
/// splat/`*rest` actually forwarded), `kw` appends marked-if-non-empty,
/// `blk` moves.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_super_from_args(
    recv: *const RubyValue,
    defining_class: u32,
    sym: u32,
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let r = unsafe {
        with_array_args(args, unmark, kw, blk, |full, block| {
            crate::dispatch::send_super_from(
                &*recv,
                zeo_abi::ClassId(defining_class),
                Symbol::from_u32(sym),
                full,
                block,
            )
        })
    };
    status_out(r, out)
}

/// `super` from a RUNTIME-installed method body, whose defining class is
/// minted at run time: the runtime reads it (and the method name) off the
/// method-frame stack pushed when the body was entered. Same argument
/// convention as [`zeo_rt_send_super_from_args`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_super_dynamic_args(
    recv: *const RubyValue,
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let r = unsafe {
        with_array_args(args, unmark, kw, blk, |full, block| {
            crate::runtime_meta::send_super_dynamic(&*recv, full, block)
        })
    };
    status_out(r, out)
}

/// A CLASS-method `super` whose target the compile-time singleton-chain
/// walk could not name: the runtime resumes the receiver class's
/// singleton chain after `defining_class`. Same argument convention as
/// [`zeo_rt_send_super_from_args`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_send_super_class_from_args(
    recv_class: u32,
    defining_class: u32,
    sym: u32,
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let r = unsafe {
        with_array_args(args, unmark, kw, blk, |full, block| {
            crate::dispatch::send_super_class_from(
                zeo_abi::ClassId(recv_class),
                zeo_abi::ClassId(defining_class),
                Symbol::from_u32(sym),
                full,
                block,
            )
        })
    };
    status_out(r, out)
}

/// A CLASS-method `super` the emitter DID resolve against the singleton
/// chain (sibling-extend order is compile-time knowledge the runtime
/// registry does not record): dispatch the named row directly --
/// `module_instance` picks the extended module's instance-method bridge
/// over a `def self.x` row.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_call_singleton_super_target_args(
    target: u32,
    module_instance: u8,
    recv_class: u32,
    sym: u32,
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let r = unsafe {
        with_array_args(args, unmark, kw, blk, |full, block| {
            crate::dispatch::call_singleton_super_target(
                zeo_abi::ClassId(target),
                module_instance != 0,
                zeo_abi::ClassId(recv_class),
                Symbol::from_u32(sym),
                full,
                block,
            )
        })
    };
    status_out(r, out)
}

/// `super` from a value-builtin subclass into the inherited builtin
/// (`value_super`: `initialize` re-seats the payload, anything else runs
/// the root builtin against it). Same argument convention as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_value_super_args(
    recv: *const RubyValue,
    name: *const u8,
    len: usize,
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let mname = unsafe { super::str_slice(name, len) };
    let r = unsafe {
        with_array_args(args, unmark, kw, blk, |full, block| {
            crate::value_super(&*recv, mname, full, block)
        })
    };
    status_out(r, out)
}

/// `defined?(recv.m)` / `defined?(m)`: does the receiver answer `m`
/// (respond_to_missing? consulted; its raise SWALLOWED -- rustc's
/// `unwrap_or(false)`)? Infallible by design.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_defined_method(
    recv: *const RubyValue,
    sym: u32,
    include_all: u8,
    hit: *mut i8,
) {
    let ok = crate::dispatch::responds_to_or_missing(
        unsafe { &*recv },
        Symbol::from_u32(sym),
        include_all != 0,
    )
    .unwrap_or(false);
    unsafe { hit.write(i8::from(ok)) };
}

/// `defined?(super)`: a target exists past `defining_class` on the
/// receiver's chain.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_super_defined(
    recv: *const RubyValue,
    defining_class: u32,
    sym: u32,
) -> i8 {
    i8::from(crate::dispatch::super_defined(
        unsafe { &*recv },
        zeo_abi::ClassId(defining_class),
        Symbol::from_u32(sym),
    ))
}

/// A BARE `super` in a scope with no compile-time method -- the raise, chosen
/// by whether this body became a method at run time. Always fails.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_bare_super_outside_a_method() -> i32 {
    crate::signal::set_pending(crate::runtime_meta::bare_super_outside_a_method());
    zeo_abi::abi::STATUS_SIGNAL
}

/// `defined?(super)` for a body whose owner only the RUN TIME knows -- a
/// `define_method` block, whose home is the method frame it entered under.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_super_defined_dynamic(recv: *const RubyValue) -> i8 {
    i8::from(crate::runtime_meta::super_defined_dynamic(unsafe {
        &*recv
    }))
}

/// `defined?(@iv)`: set on self RIGHT NOW -- behind CRuby's Ractor guard
/// (which can raise, hence the status).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_defined_ivar(
    recv: *const RubyValue,
    name: *const u8,
    len: usize,
    hit: *mut i8,
) -> i32 {
    use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};
    let recv = unsafe { &*recv };
    if let Err(sig) = crate::ractor::ivar_isolation_check(recv) {
        crate::signal::set_pending(sig);
        return STATUS_SIGNAL;
    }
    let name = unsafe { super::str_slice(name, len) };
    unsafe { hit.write(i8::from(crate::dispatch::ivar_defined(recv, name))) };
    STATUS_OK
}

/// `defined?(@@x)` / `defined?($g)` / `defined?(Scope::N)` /
/// `const_is_private` -- the four infallible table probes, one entry each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_defined_cvar(owner: u32, name: *const u8, len: usize) -> i8 {
    i8::from(crate::cvars::cvar_defined(owner, unsafe {
        super::str_slice(name, len)
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_defined_gvar(box_id: u32, name: *const u8, len: usize) -> i8 {
    i8::from(crate::globals::global_defined(box_id, unsafe {
        super::str_slice(name, len)
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_defined_const_in(scope: u32, name: *const u8, len: usize) -> i8 {
    i8::from(crate::builtins::rmodule::const_defined_in(
        zeo_abi::ClassId(scope),
        unsafe { super::str_slice(name, len) },
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_const_private(scope: u32, name: *const u8, len: usize) -> i8 {
    i8::from(crate::constants::const_is_private(scope, unsafe {
        super::str_slice(name, len)
    }))
}

/// The `(target, holder, singleton)` candidates a refined call site bakes,
/// as a flat `u32` triple array in `.rodata`.
unsafe fn refine_candidates(ids: *const u32, n: usize) -> Vec<(ClassId, ClassId, bool)> {
    if n == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(ids, n * 3) }
        .chunks_exact(3)
        .map(|c| (ClassId(c[0]), ClassId(c[1]), c[2] != 0))
        .collect()
}

/// A call at a site some `using` covers, on a name one of the covering
/// refinements defines. Whether the refinement APPLIES depends on the
/// receiver's runtime class, so the whole call goes through one entry that
/// tries the refined bodies and then falls back to an ordinary send --
/// exactly what the rustc backend's `refined_send_in` does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_refined_send_in(
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    argv: *const RubyValue,
    argc: usize,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    ids: *const u32,
    n_ids: usize,
    explicit: u8,
    out: *mut RubyValue,
) -> i32 {
    let cands = unsafe { refine_candidates(ids, n_ids) };
    let call = |args: &[RubyValue], block: Option<RubyValue>| {
        crate::dispatch::refined_send_in(
            box_id,
            unsafe { &*recv },
            Symbol::from_u32(sym),
            args,
            block,
            &cands,
            explicit != 0,
        )
    };
    let r = unsafe {
        if kw.is_null() {
            let (args, block) = call_views(argv, argc, blk);
            call(args, block)
        } else {
            with_kw_args(argv, argc, kw, blk, call)
        }
    };
    status_out(r, out)
}

/// [`zeo_rt_refined_send_in`] with a runtime-built argument Array.
///
/// A splat's element count is only known at run time, so the site builds an
/// Array and hands it over whole. `unmark` is `ruby2_keywords`' opt-out, as
/// on every other args-Array entry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_refined_send_args_in(
    box_id: u32,
    recv: *const RubyValue,
    sym: u32,
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    blk: *mut RubyValue,
    ids: *const u32,
    n_ids: usize,
    explicit: u8,
    out: *mut RubyValue,
) -> i32 {
    let cands = unsafe { refine_candidates(ids, n_ids) };
    let r = unsafe {
        with_array_args(args, unmark, kw, blk, |full, block| {
            crate::dispatch::refined_send_in(
                box_id,
                &*recv,
                Symbol::from_u32(sym),
                full,
                block,
                &cands,
                explicit != 0,
            )
        })
    };
    status_out(r, out)
}

/// `send`/`public_send`/`respond_to?`/`method` at a site some `using`
/// covers. Two questions are open and neither is decidable at compile
/// time: which of the entries the receiver's chain actually resolves to
/// (any class may define its own), and whether a refinement answers the
/// name it was handed. `entry` is the written one, as
/// `zeo_abi::abi::REFLECT_*`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_reflect_dispatch_in(
    box_id: u32,
    recv: *const RubyValue,
    entry: u8,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    ids: *const u32,
    n_ids: usize,
    out: *mut RubyValue,
) -> i32 {
    use crate::dispatch::Reflect;
    let entry = match entry {
        zeo_abi::abi::REFLECT_SEND => Reflect::Send,
        zeo_abi::abi::REFLECT_PUBLIC_SEND => Reflect::PublicSend,
        zeo_abi::abi::REFLECT_RESPOND_TO => Reflect::RespondTo,
        zeo_abi::abi::REFLECT_METHOD => Reflect::Method,
        other => panic!("reflect_dispatch_in: unknown entry {other}"),
    };
    let cands = unsafe { refine_candidates(ids, n_ids) };
    let (args, block) = unsafe { call_views(argv, argc, blk) };
    let r =
        crate::dispatch::reflect_dispatch_in(box_id, unsafe { &*recv }, entry, args, block, &cands);
    status_out(r, out)
}

/// [`zeo_rt_reflect_dispatch_in`] for a SNIPPET, where the candidates
/// arrive as activation slots rather than resolved class ids.
///
/// `send`/`public_send`/`respond_to?`/`method` at a site some `using`
/// covers. Two questions are open and neither is decidable at compile
/// time: which of the entries the receiver's chain actually resolves to
/// (any class may define its own), and whether a refinement answers the
/// name it was handed. `entry` is the written one, as
/// `zeo_abi::abi::REFLECT_*`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_reflect_dispatch(
    box_id: u32,
    recv: *const RubyValue,
    entry: u8,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    slots: *const u32,
    n_slots: usize,
    out: *mut RubyValue,
) -> i32 {
    use crate::dispatch::Reflect;
    let entry = match entry {
        zeo_abi::abi::REFLECT_SEND => Reflect::Send,
        zeo_abi::abi::REFLECT_PUBLIC_SEND => Reflect::PublicSend,
        zeo_abi::abi::REFLECT_RESPOND_TO => Reflect::RespondTo,
        zeo_abi::abi::REFLECT_METHOD => Reflect::Method,
        other => panic!("reflect_dispatch_in: unknown entry {other}"),
    };
    let slots = if n_slots == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(slots, n_slots) }
    };
    let cands = crate::eval::using_candidates(slots);
    let (args, block) = unsafe { call_views(argv, argc, blk) };
    let r =
        crate::dispatch::reflect_dispatch_in(box_id, unsafe { &*recv }, entry, args, block, &cands);
    status_out(r, out)
}

#[cfg(test)]
mod native_row_tests {
    use crate::RubyValue;
    use crate::symbol::Symbol;

    /// The forward finds the row on the receiver's OWN class -- the position a
    /// `super` written there would resume PAST. `Array#size` is the case: ruby
    /// has no `size` above `Array`, so a `super` raises where this must not.
    #[test]
    fn the_forward_reaches_the_receivers_own_native_row() {
        let recv = RubyValue::Array(crate::array_new(vec![RubyValue::Int(1), RubyValue::Int(2)]));
        let mut out = RubyValue::Nil;
        let st = unsafe {
            super::zeo_rt_native_row_call(
                &raw const recv,
                Symbol::intern("size").to_u32(),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                &raw mut out,
            )
        };
        assert_eq!(st, zeo_abi::abi::STATUS_OK);
        assert!(matches!(out, RubyValue::Int(2)));
    }

    /// A name the builtin never had has no row to forward to, and the walk
    /// says so -- the shim then raises ruby's plain `undefined method`.
    ///
    /// The raise itself is not asserted here: building an exception needs a
    /// registered program, which a `zeo-rt` unit test has no way to install.
    /// `tests/a_reopened_builtin_answers_from_its_own_line.rb` pins the
    /// message.
    #[test]
    fn a_name_the_builtin_never_had_has_no_row() {
        let recv = RubyValue::Array(crate::array_new(Vec::new()));
        let name = Symbol::intern("brand_new_name");
        assert!(
            crate::dispatch::ancestors_of_value(recv.class_id())
                .iter()
                .all(|&anc| crate::builtins::class_table(anc)
                    .and_then(|lookup| lookup(name.name_str()))
                    .is_none())
        );
    }
}

#[cfg(test)]
mod is_live_tests {
    use crate::RubyValue;
    use crate::symbol::Symbol;

    /// The gate the inlined accessor reads: false while nothing has been
    /// defined at run time, true afterwards, and it never goes back.
    ///
    /// `GATES` is process-global, so this asserts the transition rather than
    /// an absolute value -- nextest gives each test its own process, which is
    /// what makes the `false` half meaningful.
    #[test]
    fn is_live_answers_the_overlay_gate() {
        assert_eq!(super::zeo_rt_is_live(), 0);
        let body = crate::RProc::new(|_args| Ok(RubyValue::Nil));
        crate::runtime_meta::runtime_define_method(
            zeo_abi::OBJECT_CLASS,
            Symbol::intern("a_method_defined_at_run_time"),
            body,
        )
        .expect("Object accepts a run-time definition");
        assert_eq!(super::zeo_rt_is_live(), 1);
    }
}
