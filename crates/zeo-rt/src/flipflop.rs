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
