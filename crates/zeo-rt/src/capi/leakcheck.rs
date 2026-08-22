//! `ZEO_RT_LEAKCHECK=1`: the compiled-ownership ledger. Every capi entry
//! that hands an OWNED heap value to compiled code counts it in; every
//! entry that takes one back (release, a moved-in argument, a pool push)
//! counts it out. At process end the per-tag balance must be zero -- a
//! positive row is a leak in the emitted ownership lowering, a negative
//! row a double-consume. Releases also POISON the slot's tag byte, so a
//! use-after-release through the capi aborts naming the entry instead of
//! corrupting.
//!
//! Immediates (tag < FIRST_HEAP_TAG) are never counted -- a 24-byte copy
//! owns nothing.

use crate::RubyValue;
use std::sync::atomic::{AtomicI64, Ordering};
use zeo_abi::abi::FIRST_HEAP_TAG;

const N_TAGS: usize = 64;
static LIVE: [AtomicI64; N_TAGS] = [const { AtomicI64::new(0) }; N_TAGS];
const POISON_TAG: u8 = 0xFF;

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ZEO_RT_LEAKCHECK").is_some_and(|v| !v.is_empty()))
}

fn tag_of(v: &RubyValue) -> u8 {
    // The abi_layout test pins the tag byte at offset 0.
    unsafe { *(v as *const RubyValue).cast::<u8>() }
}

/// The counter row for a tag, or `None` when the byte is not a tag at all.
///
/// A slot handed to this ledger is supposed to be a live `RubyValue`, so a
/// byte outside the tag range means the caller passed one that has been
/// released or never held a value -- the exact bug the ledger exists to
/// find. Reporting it is the whole point; indexing the table with it just
/// panics inside the diagnostic and says nothing about the program.
fn row(t: u8, who: &str) -> &'static AtomicI64 {
    if t == POISON_TAG {
        report(&format!("{who} was handed a RELEASED value slot"));
    }
    match LIVE.get(t as usize) {
        Some(row) => row,
        None => report(&format!(
            "{who} was handed a slot whose tag byte is {t}, which is not a \
             value tag -- the slot holds no live value"
        )),
    }
}

/// End the process naming what the ledger found. A `panic!` rather than a
/// bare `abort`: it cannot unwind out of an `extern "C"` frame and ends the
/// process either way, but it prints the message AND, under
/// `RUST_BACKTRACE=1`, the capi entry the bad slot came through -- which is
/// the only thing that leads back to the emitted code that produced it.
fn report(what: &str) -> ! {
    panic!("ZEO_RT_LEAKCHECK: {what}");
}

/// An owned heap value crossed INTO compiled code.
pub(crate) fn created(v: &RubyValue) {
    if !enabled() {
        return;
    }
    let t = tag_of(v);
    if t >= FIRST_HEAP_TAG {
        row(t, "created").fetch_add(1, Ordering::Relaxed);
    }
}

/// An owned heap value crossed BACK OUT of compiled code (released,
/// pooled, or moved into runtime storage).
pub(crate) fn consumed(v: &RubyValue) {
    if !enabled() {
        return;
    }
    let t = tag_of(v);
    if t >= FIRST_HEAP_TAG {
        row(t, "consumed").fetch_sub(1, Ordering::Relaxed);
    }
}

/// Abort loudly when `p`'s slot was already released.
pub(crate) fn check_not_poisoned(p: *const RubyValue, who: &str) {
    if !enabled() {
        return;
    }
    let t = unsafe { *p.cast::<u8>() };
    if t == POISON_TAG {
        report(&format!("{who} touched a released value slot"));
    }
}

/// Stamp a released slot so later touches abort.
pub(crate) fn poison(p: *mut RubyValue) {
    if !enabled() {
        return;
    }
    unsafe { p.cast::<u8>().write(POISON_TAG) };
}

/// The exit gate: print any imbalance per tag and abort. Quiet when the
/// ledger balances.
pub(crate) fn check_at_exit() {
    if !enabled() {
        return;
    }
    let mut dirty = false;
    for (tag, row) in LIVE.iter().enumerate() {
        let n = row.load(Ordering::Relaxed);
        if n != 0 {
            if !dirty {
                eprintln!("ZEO_RT_LEAKCHECK: compiled-ownership imbalance at exit:");
                dirty = true;
            }
            eprintln!("  tag {tag}: {n:+}");
        }
    }
    if dirty {
        std::process::abort();
    }
}
