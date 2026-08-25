//! `Kernel`'s PRNG surface -- the process-wide Mersenne Twister behind
//! `rand` and `srand`. The `ruby_module!` rows stay in `mod.rs` and call
//! these by bare name.

use super::*;

/// The process-wide generator behind `rand`/`srand` -- MT19937, so a program
/// that `srand`s a fixed seed draws ruby's own sequence. Lazily seeded from
/// the clock on first use; `seed` is what `srand` reports back.
static PRNG: parking_lot::Mutex<Option<(crate::mt::Mt, u64)>> = parking_lot::Mutex::new(None);

/// The live generator, seeding it from the clock if nothing has yet.
pub(super) fn with_prng<T>(f: impl FnOnce(&mut crate::mt::Mt) -> T) -> T {
    let mut guard = PRNG.lock();
    let entry = guard.get_or_insert_with(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e3779b97f4a7c15);
        (
            crate::mt::Mt::from_bigint(&num_bigint::BigInt::from(now)),
            now,
        )
    });
    f(&mut entry.0)
}

/// One uniform integer in `[0, limit]` from the process-wide generator.
pub(crate) fn prng_limited(limit: u64) -> u64 {
    with_prng(|mt| mt.limited(limit))
}

/// One uniform Float in `[0, 1)` from the process-wide generator.
pub(crate) fn prng_real() -> f64 {
    with_prng(crate::mt::Mt::next_real)
}

/// `Kernel#rand`: no arg -> Float in [0, 1); positive Integer n -> Integer
/// in [0, n); Float x -> Float in [0, x).
pub(crate) fn rand_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    Ok(match args.first() {
        None | Some(RubyValue::Nil) | Some(RubyValue::Int(0)) => RubyValue::Float(prng_real()),
        Some(RubyValue::Int(n)) if *n > 0 => RubyValue::Int(prng_limited(*n as u64 - 1) as i64),
        // A negative bound draws from `[0, |n|)` (a non-negative Integer).
        Some(RubyValue::Int(n)) => RubyValue::Int(prng_limited(n.unsigned_abs() - 1) as i64),
        // A bignum bound (`rand(2**70)`) draws from `[0, |n|)`: assemble enough
        // random words to cover the magnitude, then reduce mod |n|.
        Some(RubyValue::BigInt(n)) => {
            use num_bigint::{BigInt, Sign};
            let n: &BigInt = n;
            let magnitude = if n.sign() == Sign::Minus {
                -n
            } else {
                n.clone()
            };
            let _ = Sign::Plus;
            crate::builtins::integer::int_value(with_prng(|mt| mt.bigint_below(&magnitude)))
        }
        Some(RubyValue::Float(x)) => {
            // A non-finite bound has no Integer image: CRuby's `dbl2ival`
            // raises FloatDomainError named for the value ("Infinity"/"NaN").
            if !x.is_finite() {
                return Err(crate::builtins::float_domain_error!(
                    "{}",
                    RubyValue::Float(*x).to_display_string()
                ));
            }
            // CRuby's `Kernel#rand` truncates a Float bound to an Integer and
            // draws an Integer from `[0, ⌊x⌋)` (`rand(3.5)` -> 0..2). A bound
            // below 1 truncates to 0, i.e. the plain `[0.0, 1.0)` Float draw.
            let n = x.trunc();
            if n >= 1.0 {
                RubyValue::Int(prng_limited(n as u64 - 1) as i64)
            } else {
                RubyValue::Float(prng_real())
            }
        }
        Some(RubyValue::Range(__rg)) => {
            let (lo, hi, exclusive) = __rg.parts();
            return kernel_rand_range(lo, hi, exclusive);
        }
        Some(other) => {
            return Err(arg_error!(
                "invalid argument - {}",
                other.to_display_string()
            ));
        }
    })
}

/// `rand(a..b)` -- an Integer range yields an Integer, a Float endpoint yields
/// a Float. An empty/reversed range answers nil (CRuby's rule, NOT an error); a
/// beginless or endless range raises Errno::EDOM.
pub(super) fn kernel_rand_range(
    lo: Option<&RubyValue>,
    hi: Option<&RubyValue>,
    exclusive: bool,
) -> Result<RubyValue, Signal> {
    let (Some(lo), Some(hi)) = (lo, hi) else {
        return Err(crate::dispatch::raise_error(
            "Errno::EDOM",
            "Numerical argument out of domain".to_string(),
        ));
    };
    match (lo, hi) {
        (RubyValue::Int(a), RubyValue::Int(b)) => {
            let span = b - a + i64::from(!exclusive);
            if span <= 0 {
                return Ok(RubyValue::Nil);
            }
            Ok(RubyValue::Int(a + prng_limited(span as u64 - 1) as i64))
        }
        _ => {
            let (Some(a), Some(b)) = (num_to_f64(lo), num_to_f64(hi)) else {
                // A Range whose endpoints aren't numeric: CRuby names the
                // Range in the generic to_int shape (oracle: `rand("a".."b")`
                // is "no implicit conversion of Range into Integer").
                return Err(type_error!("no implicit conversion of Range into Integer"));
            };
            if b < a || (b == a && exclusive) {
                return Ok(RubyValue::Nil);
            }
            // An inclusive range draws through a `[0, 1]` unit (see
            // `Random#rand`'s range arm).
            let unit = if exclusive {
                prng_real()
            } else {
                with_prng(crate::mt::Mt::next_real_inclusive)
            };
            Ok(RubyValue::Float(a + unit * (b - a)))
        }
    }
}

/// Integer/Float -> f64 (for a range endpoint); `None` otherwise.
pub(super) fn num_to_f64(v: &RubyValue) -> Option<f64> {
    match v {
        RubyValue::Int(n) => Some(*n as f64),
        RubyValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// `Kernel#srand(seed)`: reseeds, returns the PREVIOUS seed.
pub(crate) fn srand_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let new_seed = match args.first() {
        Some(RubyValue::Int(n)) => *n as u64,
        _ => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1),
    };
    let mut guard = PRNG.lock();
    let previous = guard.as_ref().map_or(0, |(_, seed)| *seed);
    *guard = Some((
        crate::mt::Mt::from_bigint(&num_bigint::BigInt::from(new_seed)),
        new_seed,
    ));
    Ok(crate::builtins::integer::int_value(
        num_bigint::BigInt::from(previous),
    ))
}
