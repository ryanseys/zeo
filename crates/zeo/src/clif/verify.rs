//! The ownership check: every lowering that
//! CREATES an owned value increments `Fx::owned_created` at its emission
//! site, and every consumption (move into a slot, hand-off to the release
//! pool) increments `owned_consumed`. The two must balance per function --
//! an imbalance is an internal compiler error naming the function, caught
//! at compile time rather than as a leak at run time.
//!
//! This is a static per-site ledger, not a CFG dataflow pass: with the
//! frame-pool design every consumption is emitted
//! at the same site as its creation (both sides of a branch count their
//! own sites), so site-balance is exactly the property that holds when no
//! lowering forgets an owned value. A dataflow version becomes necessary
//! only with control-flow shapes that break the one-site-per-value rule.

use super::ctx::Fx;
use crate::diagnostics::clif::{CResult, CodegenError};

pub(crate) fn check(fx: &Fx, what: &str) -> CResult<()> {
    if fx.owned_created != fx.owned_consumed {
        return Err(CodegenError::internal(format!(
            "ICE: ownership imbalance lowering {what}: {} owned values created, {} consumed",
            fx.owned_created, fx.owned_consumed,
        )));
    }
    Ok(())
}
