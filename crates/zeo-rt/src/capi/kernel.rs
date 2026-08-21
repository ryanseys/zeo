//! The `&[Value]`-shaped Kernel intrinsics the emitter calls by name.

use super::dispatch::status_out;
use crate::RubyValue;

unsafe fn arg_view<'a>(argv: *const RubyValue, argc: usize) -> &'a [RubyValue] {
    if argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    }
}

/// `Kernel#puts`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_kernel_puts(
    argv: *const RubyValue,
    argc: usize,
    out: *mut RubyValue,
) -> i32 {
    status_out(crate::kernel_puts(unsafe { arg_view(argv, argc) }), out)
}

/// `Kernel#p`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_kernel_p(
    argv: *const RubyValue,
    argc: usize,
    out: *mut RubyValue,
) -> i32 {
    status_out(crate::kernel_p(unsafe { arg_view(argv, argc) }), out)
}

/// `Kernel#print`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_kernel_print(
    argv: *const RubyValue,
    argc: usize,
    out: *mut RubyValue,
) -> i32 {
    status_out(crate::kernel_print(unsafe { arg_view(argv, argc) }), out)
}

/// Whether the flip-flop numbered `id` is currently latched on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_flip_flop_on(id: u32) -> u8 {
    u8::from(crate::flipflop::flip_flop_on(id))
}

/// Latches the flip-flop numbered `id` on or off.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_flip_flop_set(id: u32, on: u8) {
    crate::flipflop::flip_flop_set(id, on != 0);
}

/// `eval(src, binding = nil, file = nil, line = nil)` and its reflective
/// spelling. `scope` is the Binding of the CALLING scope, which the call
/// site materializes so the source can read and write the caller's locals;
/// an explicit `binding` argument wins over it. Every optional slot is
/// `nil` when absent, exactly as `eval_value_in_scope` reads them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_value_in_scope(
    src: *const RubyValue,
    scope: *const RubyValue,
    binding: *const RubyValue,
    file: *const RubyValue,
    line: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let arg = |p: *const RubyValue| match unsafe { p.as_ref() } {
        Some(v) => v.clone(),
        None => RubyValue::Nil,
    };
    status_out(
        crate::eval::eval_value_in_scope(arg(src), arg(scope), arg(binding), arg(file), arg(line)),
        out,
    )
}

/// `eval(*args)` -- the splat-bearing spelling of the site above. `args`
/// is the Array the site built; the arity check happens inside.
///
/// # Safety
/// `args` is a live Array value and `scope` a live Binding value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_value_in_scope_argv(
    args: *const RubyValue,
    scope: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let scope = match unsafe { scope.as_ref() } {
        Some(v) => v.clone(),
        None => RubyValue::Nil,
    };
    let list: Vec<RubyValue> = match unsafe { args.as_ref() } {
        Some(RubyValue::Array(a)) => a.lock().iter().cloned().collect(),
        _ => Vec::new(),
    };
    status_out(crate::eval::eval_value_in_scope_argv(&list, scope), out)
}

/// Publish this scope's block channel (and, for a method, the argument
/// list and `super` target a bare `super` written in a snippet forwards)
/// for the length of the call -- emitted by any scope that lexically
/// contains a run-time `eval`. See `zeo_rt::eval::EvalHome`.
///
/// `args` null means a block's home: it publishes the block it inherited
/// and no `super` target, because a block carries neither.
///
/// # Safety
/// `blk` is null or a live block value; `args` is null or a live Array;
/// `kw` is null or a live Hash.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_home_push(
    blk: *const RubyValue,
    args: *const RubyValue,
    unmark: u8,
    kw: *const RubyValue,
    defining: u32,
    name: u32,
) {
    let block = match unsafe { blk.as_ref() } {
        Some(v) if !v.is_nil() => Some(v.clone()),
        _ => None,
    };
    let zsuper = match unsafe { args.as_ref() } {
        Some(RubyValue::Array(a)) => {
            let mut full: Vec<RubyValue> = a.lock().iter().cloned().collect();
            if unmark != 0 {
                crate::value::collections::unmark_kwargs_tail(&mut full);
            }
            if let Some(kw @ RubyValue::Hash(h)) = unsafe { kw.as_ref() }
                && !h.lock().is_empty()
            {
                crate::value::collections::hash_mark_kwargs(h);
                full.push(kw.clone());
            }
            Some((
                full,
                zeo_abi::ClassId(defining),
                crate::Symbol::from_u32(name),
            ))
        }
        _ => None,
    };
    crate::eval::home_push(block, zsuper);
}

/// The matching pop -- on every exit path of the scope that pushed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_home_pop() {
    crate::eval::home_pop();
}

/// `block_given?` written at a snippet's own level.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_block_given() -> i8 {
    i8::from(matches!(
        crate::eval::home_block(),
        Some(RubyValue::Proc(_))
    ))
}

/// The enclosing method's block as a value -- what `&blk`, `yield`'s
/// `to_proc` and `defined?(yield)` in a snippet read.
///
/// # Safety
/// `out` is a live uninitialized value slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_home_block(out: *mut RubyValue) {
    unsafe { out.write(crate::eval::home_block().unwrap_or(RubyValue::Nil)) };
}

/// `yield args` written at a snippet's own level.
///
/// # Safety
/// `argv` covers `argc` live values; `out` is a live value slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_yield(
    argv: *const RubyValue,
    argc: usize,
    out: *mut RubyValue,
) -> i32 {
    let args = if argc == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    };
    status_out(crate::eval::home_yield(args), out)
}

/// `defined?(super)` written at a snippet's own level.
///
/// # Safety
/// `recv` is the snippet's own `self`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_super_defined(recv: *const RubyValue) -> i8 {
    match unsafe { recv.as_ref() } {
        Some(v) => i8::from(crate::eval::home_super_defined(v)),
        None => 0,
    }
}

/// A bare `super` written at a snippet's own level.
///
/// # Safety
/// `recv` is the snippet's own `self`; `out` is a live value slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_super(recv: *const RubyValue, out: *mut RubyValue) -> i32 {
    let recv = match unsafe { recv.as_ref() } {
        Some(v) => v.clone(),
        None => RubyValue::Nil,
    };
    status_out(crate::eval::home_super(&recv), out)
}

/// One statement hit -- emitted beside every `set_line` stamp of a
/// coverage-activated program, and nothing at all in one without.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cov_line(file: *const u8, len: usize, line: u32) {
    crate::ext::coverage::cov_line(unsafe { super::static_str(file, len) }, line);
}

/// A spliced file's top level is beginning: the file is reported iff
/// measurement is set up at this moment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cov_file_loaded(file: *const u8, len: usize) {
    crate::ext::coverage::cov_file_loaded(unsafe { super::static_str(file, len) });
}
