//! `Comparable`, implemented in Rust (Phase 16.2) -- the compar.c
//! architecture, exactly like `enumerable`'s enum.c one: each method drives
//! the INCLUDER's own `<=>` (through the registry, so an inherited or
//! mixed-in `<=>` resolves too), and the runtime match below is the single
//! source of truth for which names Comparable answers -- no compile-time
//! method list to keep in sync (`send`'s fallback consults this whenever a
//! name doesn't resolve on a class whose ancestors carry
//! `COMPARABLE_CLASS`).
//!
//! Real Ruby raises `ArgumentError: comparison of X with Y failed` when
//! `<=>` answers nil; this runtime has no exception-construction channel
//! (the `array_set`/`IndexError` division of labor), so that case is a
//! LOUD panic instead -- documented spike scope, not silent wrongness.

use crate::dispatch::{call_user_method, RObj};
use crate::{RubyValue, Signal};

/// The includer's own `<=>`, reduced to a sign -- `Ok(None)` is Ruby's
/// `nil` (incomparable). A missing `<=>` on a Comparable includer is a
/// loud panic (real Ruby would raise `NoMethodError` from inside
/// Comparable; same outcome, earlier and louder).
fn cmp(recv: &RObj, other: &RubyValue) -> Result<Option<i64>, Signal> {
    match call_user_method(recv, "<=>", std::slice::from_ref(other)) {
        Some(Ok(RubyValue::Int(i))) => Ok(Some(i.signum())),
        Some(Ok(_)) => Ok(None),
        Some(Err(e)) => Err(e),
        None => panic!("Comparable methods require the including class to define `<=>`"),
    }
}

/// `cmp`, with real Ruby's incomparable-operands failure applied (the
/// panic standing in for `ArgumentError` -- see module docs).
fn cmp_or_fail(recv: &RObj, other: &RubyValue, method: &str) -> Result<i64, Signal> {
    match cmp(recv, other)? {
        Some(ord) => Ok(ord),
        None => panic!(
            "Comparable#{method}: comparison failed (`<=>` returned nil; ArgumentError in real Ruby -- raised as a panic, spike scope)"
        ),
    }
}

/// The Comparable method surface -- `None` when `name` isn't a Comparable
/// method at all (the caller falls through to its NoMethodError path).
/// `Comparable#==` deliberately treats an incomparable pair as `false`
/// (real Ruby's one nil-tolerant Comparable method); every ordering
/// predicate fails loudly instead.
pub(crate) fn comparable_send(
    recv: &RObj,
    name: &str,
    args: &[RubyValue],
) -> Option<Result<RubyValue, Signal>> {
    let result = match (name, args.len()) {
        ("<", 1) => cmp_or_fail(recv, &args[0], "<").map(|o| RubyValue::Bool(o < 0)),
        ("<=", 1) => cmp_or_fail(recv, &args[0], "<=").map(|o| RubyValue::Bool(o <= 0)),
        (">", 1) => cmp_or_fail(recv, &args[0], ">").map(|o| RubyValue::Bool(o > 0)),
        (">=", 1) => cmp_or_fail(recv, &args[0], ">=").map(|o| RubyValue::Bool(o >= 0)),
        ("==", 1) => cmp(recv, &args[0]).map(|o| RubyValue::Bool(o == Some(0))),
        ("between?", 2) => (|| {
            let lo = cmp_or_fail(recv, &args[0], "between?")?;
            let hi = cmp_or_fail(recv, &args[1], "between?")?;
            Ok(RubyValue::Bool(lo >= 0 && hi <= 0))
        })(),
        ("clamp", 2) => (|| {
            if cmp_or_fail(recv, &args[0], "clamp")? < 0 {
                return Ok(args[0].clone());
            }
            if cmp_or_fail(recv, &args[1], "clamp")? > 0 {
                return Ok(args[1].clone());
            }
            Ok(RubyValue::Object(recv.clone()))
        })(),
        _ => return None,
    };
    Some(result)
}
