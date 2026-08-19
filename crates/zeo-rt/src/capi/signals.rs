//! The status-protocol signal surface: reading/re-arming the pending
//! slot, the `ensure` save/restore bracket, rescue matching, `$!`, proc
//! homes, svar scopes, and the raise channels.

use crate::signal::{set_pending, take_pending, with_pending};
use crate::{RubyValue, Signal};
use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL, SignalKind};

/// `Signal` -> the wire kind.
fn kind_of(sig: &Signal) -> SignalKind {
    match sig {
        Signal::Break(_) => SignalKind::Break,
        Signal::Next(_) => SignalKind::Next,
        Signal::Redo => SignalKind::Redo,
        Signal::Retry => SignalKind::Retry,
        Signal::Return(_) => SignalKind::Return,
        Signal::Raise(_) => SignalKind::Raise,
        Signal::Throw(_) => SignalKind::Throw,
        Signal::Terminate => SignalKind::Terminate,
    }
}

/// The pending signal's kind without consuming it (`None` = empty slot).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_signal_kind() -> u8 {
    with_pending(|p| p.as_ref().map_or(SignalKind::None, kind_of)) as u8
}

/// Consume the pending signal's PAYLOAD: for `Break`/`Next`/`Return`/
/// `Raise` the value moves into `out` and the slot empties; `Redo`/
/// `Retry`/`Throw`/`Terminate` leave the slot untouched (they keep
/// propagating whole) and just answer the kind, as does an empty slot
/// (`None`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_signal_take(out: *mut RubyValue) -> u8 {
    let kind = with_pending(|p| p.as_ref().map_or(SignalKind::None, kind_of));
    match kind {
        SignalKind::Break | SignalKind::Next | SignalKind::Return | SignalKind::Raise => {
            let v = match take_pending() {
                Some(Signal::Break(v) | Signal::Next(v) | Signal::Return(v) | Signal::Raise(v)) => {
                    v
                }
                Some(Signal::Redo | Signal::Retry | Signal::Throw(_) | Signal::Terminate)
                | None => unreachable!("pending slot changed between peek and take"),
            };
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
        }
        SignalKind::None
        | SignalKind::Redo
        | SignalKind::Retry
        | SignalKind::Throw
        | SignalKind::Terminate => {}
    }
    kind as u8
}

/// Re-arm the pending slot with `kind` (+ the payload moved from `v` for
/// the payload-carrying kinds; `v` may be null otherwise). `Throw` is
/// never re-armed -- `zeo_rt_signal_take` never took it apart.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_signal_set(kind: u8, v: *mut RubyValue) {
    let payload = || {
        super::leakcheck::consumed(unsafe { &*v });
        unsafe { std::ptr::read(v) }
    };
    let sig = match kind {
        k if k == SignalKind::Break as u8 => Signal::Break(payload()),
        k if k == SignalKind::Next as u8 => Signal::Next(payload()),
        k if k == SignalKind::Return as u8 => Signal::Return(payload()),
        k if k == SignalKind::Raise as u8 => Signal::Raise(payload()),
        k if k == SignalKind::Redo as u8 => Signal::Redo,
        k if k == SignalKind::Retry as u8 => Signal::Retry,
        k if k == SignalKind::Terminate as u8 => Signal::Terminate,
        other => unreachable!("zeo_rt_signal_set: kind {other} is not re-armable"),
    };
    set_pending(sig);
}

/// An in-flight signal parked aside while an `ensure` body runs -- opaque
/// to compiled code, which only shuttles the pointer between
/// [`zeo_rt_signal_save`] and [`zeo_rt_signal_restore`]/
/// [`zeo_rt_signal_drop`].
pub struct SavedSignal(Option<Signal>);

/// Move the pending signal (whole -- `Throw` and all) into a saved box for
/// the `ensure` bracket.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_signal_save() -> *mut SavedSignal {
    Box::into_raw(Box::new(SavedSignal(take_pending())))
}

/// Re-arm the saved signal after a normally-finishing `ensure` body. The
/// slot must be empty (an `ensure` body that itself signaled calls
/// [`zeo_rt_signal_drop`] instead -- CRuby's "the ensure's own signal
/// wins").
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_signal_restore(saved: *mut SavedSignal) {
    let saved = unsafe { Box::from_raw(saved) };
    if let Some(sig) = saved.0 {
        set_pending(sig);
    }
}

/// Discard a saved signal (the `ensure` body signaled on its own).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_signal_drop(saved: *mut SavedSignal) {
    drop(unsafe { Box::from_raw(saved) });
}

/// `$!` for a propagating `ensure` body: pushes the saved raise (if the
/// saved signal IS a raise) and answers whether it pushed -- pass that to
/// [`zeo_rt_propagating_leave`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_propagating_enter(saved: *const SavedSignal) -> i8 {
    match unsafe { &(*saved).0 } {
        Some(sig) => crate::handling::propagating_enter_raw(sig) as i8,
        None => 0,
    }
}

/// The pop matching [`zeo_rt_propagating_enter`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_propagating_leave(pushed: i8) {
    crate::handling::propagating_leave_raw(pushed != 0);
}

/// `rescue` clause matching against STATIC class ids: does `exc` match any
/// of the `n` ids at `classes`?
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_rescue_matches(
    exc: *const RubyValue,
    classes: *const u32,
    n: usize,
) -> i8 {
    let exc = unsafe { &*exc };
    let ids = unsafe { std::slice::from_raw_parts(classes, n) };
    ids.iter()
        .any(|&cid| crate::dispatch::is_a_value(exc, zeo_abi::ClassId(cid))) as i8
}

/// `rescue` matching against a DYNAMIC class list (splatted expressions,
/// which may not be classes at all -- `TypeError` channel inside). Writes
/// the match into `*matched_out` on `STATUS_OK`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_rescue_matches_any(
    exc: *const RubyValue,
    list: *const RubyValue,
    matched_out: *mut i8,
) -> i32 {
    match crate::dispatch::rescue_matches_any(unsafe { &*exc }, unsafe { &*list }) {
        Ok(m) => {
            unsafe { matched_out.write(m as i8) };
            STATUS_OK
        }
        Err(sig) => {
            set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// Enter a method activation that a non-lambda `Proc` may capture.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_home_push() {
    crate::signal::home_push();
}

/// Leave it (marks the home dead for any captured `Proc`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_home_pop() {
    crate::signal::home_pop();
}

/// Whether the in-flight `Signal::Return` targets the innermost
/// activation -- the method landing's catch test, asked BEFORE its
/// `zeo_rt_home_pop`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_return_targets_here() -> i8 {
    crate::signal::return_targets_here() as i8
}

/// `$!` push on entry to a `rescue` clause's body (borrows `exc`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_handling_push(exc: *const RubyValue) {
    crate::handling::push_handling(unsafe { (*exc).clone() });
}

/// `$!` pop on any exit from a `rescue` clause's body.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_handling_pop() {
    crate::handling::pop_handling();
}

/// Enter a fresh `$~` svar scope (a method prologue whose scope mentions
/// an svar).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_svar_scope_push() {
    crate::lastmatch::svar_scope_push_raw();
}

/// The pop matching [`zeo_rt_svar_scope_push`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_svar_scope_pop() {
    crate::lastmatch::svar_scope_pop_raw();
}

/// THE runtime raise channel: raise class `cid` with `msg` -- builds the
/// exception (cause + backtrace attached), parks the `Signal::Raise`,
/// answers `STATUS_SIGNAL` so the caller's `brif` lands directly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_raise_error(cid: u32, msg: *const u8, msg_len: usize) -> i32 {
    let name = crate::dispatch::class_name(zeo_abi::ClassId(cid))
        .unwrap_or_else(|| panic!("zeo_rt_raise_error: unregistered class id {cid}"));
    let text = unsafe { super::str_slice(msg, msg_len) }.to_string();
    set_pending(crate::dispatch::raise_error(&name, text));
    STATUS_SIGNAL
}

/// The `ArgumentError` arity raise: `max == usize::MAX` spells an
/// open-ended `"min+"`, `min == max` the fixed count, else `"min..max"`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_wrong_arity(given: usize, min: usize, max: usize) -> i32 {
    let sig = if min == max {
        crate::dispatch::arity_error(given, min)
    } else if max == usize::MAX {
        crate::dispatch::wrong_arity(given, &format!("{min}+"))
    } else {
        crate::dispatch::wrong_arity(given, &format!("{min}..{max}"))
    };
    set_pending(sig);
    STATUS_SIGNAL
}

/// `SystemExit`'s status if `exc` is one: writes it and answers 1, else 0
/// -- the top-level exit-status match.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_system_exit_status(
    exc: *const RubyValue,
    status_out: *mut i32,
) -> i8 {
    match crate::system_exit_status(unsafe { &*exc }) {
        Some(status) => {
            unsafe { status_out.write(status) };
            1
        }
        None => 0,
    }
}

/// CRuby-shaped uncaught-exception report to stderr.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_report_uncaught(exc: *const RubyValue) {
    crate::report_uncaught(unsafe { &*exc });
}
