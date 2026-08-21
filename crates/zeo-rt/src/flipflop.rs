//! The latches behind Ruby's flip-flop operator (`cond1..cond2` used as a
//! condition). Each syntactic flip-flop in the program owns one slot, indexed
//! by the id the compiler minted for it; the generated code reads and sets its
//! own slot around evaluating the two operands.
//!
//! THREAD-LOCAL, matching `lastmatch`: two threads running the same loop must
//! not share a latch. CRuby stores the state on the enclosing iseq's local
//! table, so it is frame-local there -- the same divergence `lastmatch`
//! documents, and with the same practical reach: a flip-flop inside a method
//! called from two places shares one latch here where CRuby would give the
//! two calls separate ones. Every ordinary use (a flip-flop in a loop, which
//! is the entire point of the operator) is unaffected.

use std::cell::RefCell;

thread_local! {
    static LATCHES: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
}

/// Where a run-time `eval`'s ids begin. A program's are dense from zero and
/// minted by ONE compile; a snippet is compiled by a fresh compiler that
/// starts counting at zero again, so the two spaces would otherwise share
/// latches. No program has a million flip-flop sites, and the latch vector
/// is sparse (it grows to the highest id ever used, one byte each).
const EVAL_BASE: u32 = 1 << 20;

static NEXT_EVAL_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(EVAL_BASE);

/// Reserve `n` consecutive latch ids for one compiled `eval` snippet and
/// answer the first. Called once per snippet, at compile time.
pub fn reserve(n: u32) -> u32 {
    NEXT_EVAL_ID.fetch_add(n, std::sync::atomic::Ordering::Relaxed)
}

/// Whether the flip-flop numbered `id` is currently on.
pub fn flip_flop_on(id: u32) -> bool {
    LATCHES.with(|l| l.borrow().get(id as usize).copied().unwrap_or(false))
}

/// Turns the flip-flop numbered `id` on or off.
pub fn flip_flop_set(id: u32, on: bool) {
    LATCHES.with(|l| {
        let mut l = l.borrow_mut();
        if l.len() <= id as usize {
            l.resize(id as usize + 1, false);
        }
        l[id as usize] = on;
    });
}
