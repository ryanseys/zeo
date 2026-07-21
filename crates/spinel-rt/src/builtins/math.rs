//! `Math` (CRuby math.c) -- module functions over `f64` intrinsics, plus
//! `Math::PI`/`Math::E`. Every argument coerces through the numeric tower's
//! `f64` view (Integer/Bignum/Float/Rational; anything else is CRuby's
//! TypeError), and domain violations raise real `Math::DomainError` with
//! CRuby's exact message shape.

use crate::builtins::numeric::num_to_f64_unchecked;
use crate::collections::array_new;
use crate::{RubyValue, Signal};

// System libm -- the same C math library CRuby's own `Math` methods call, so
// `gamma`/`lgamma`/`erf`/`erfc`/`frexp`/`ldexp` match the oracle bit-for-bit.
// No new crate dependency: these come with the always-linked math library, the
// same way the `libc` crate's time/process calls are used elsewhere in the
// runtime. (`sinh`/`cosh`/`tanh`/`asinh`/`acosh`/`atanh` use Rust's own `f64`.)
extern "C" {
    fn tgamma(x: f64) -> f64;
    fn lgamma_r(x: f64, sign: *mut i32) -> f64;
    fn erf(x: f64) -> f64;
    fn erfc(x: f64) -> f64;
    fn frexp(x: f64, exp: *mut i32) -> f64;
    fn ldexp(x: f64, exp: i32) -> f64;
}

fn arg_f64(v: &RubyValue) -> Result<f64, Signal> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
            Ok(num_to_f64_unchecked(v))
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Float",
                crate::builtins::convert_name_of(other)
            ),
        )),
    }
}

fn domain_error(fn_name: &str) -> Signal {
    crate::dispatch::raise_error(
        "Math::DomainError",
        format!("Numerical argument is out of domain - {fn_name}"),
    )
}

fn math_arity(given: usize, expected: &str) -> Signal {
    crate::dispatch::raise_error(
        "ArgumentError",
        format!("wrong number of arguments (given {given}, expected {expected})"),
    )
}

/// Every Math module function -- the reflection surface for
/// `Math.instance_methods` / `include Math` name enumeration. Must mirror
/// `math_call`'s match arms (the single dispatch source of truth).
pub(crate) const NAMES: &[&str] = &[
    "sqrt", "cbrt", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh",
    "tanh", "asinh", "acosh", "atanh", "exp", "log2", "log10", "log", "atan2",
    "hypot", "frexp", "ldexp", "gamma", "lgamma", "erf", "erfc",
];

/// One Math module function -- `None` when `name` isn't one (the caller
/// falls to its NoMethodError path). Arity is validated per function.
pub fn math_call(name: &str, args: &[RubyValue]) -> Option<Result<RubyValue, Signal>> {
    fn unary(
        args: &[RubyValue],
        f: impl Fn(f64) -> f64,
    ) -> Result<f64, Signal> {
        if args.len() != 1 {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("wrong number of arguments (given {}, expected 1)", args.len()),
            ));
        }
        Ok(f(arg_f64(&args[0])?))
    }
    // `frexp`/`lgamma` return a two-element Array, so they can't flow through
    // the float-mapping tail (`Some(result.map(RubyValue::Float))`) below.
    match name {
        "frexp" => return Some(math_frexp(args)),
        "lgamma" => return Some(math_lgamma(args)),
        _ => {}
    }
    let result = match name {
        "sqrt" => unary(args, f64::sqrt).and_then(|r| {
            if r.is_nan() {
                Err(domain_error("sqrt"))
            } else {
                Ok(r)
            }
        }),
        "cbrt" => unary(args, f64::cbrt),
        "sin" => unary(args, f64::sin),
        "cos" => unary(args, f64::cos),
        "tan" => unary(args, f64::tan),
        "asin" => unary(args, f64::asin).and_then(|r| {
            if r.is_nan() {
                Err(domain_error("asin"))
            } else {
                Ok(r)
            }
        }),
        "acos" => unary(args, f64::acos).and_then(|r| {
            if r.is_nan() {
                Err(domain_error("acos"))
            } else {
                Ok(r)
            }
        }),
        "atan" => unary(args, f64::atan),
        "exp" => unary(args, f64::exp),
        "log2" => unary(args, f64::log2).and_then(|r| {
            if r.is_nan() {
                Err(domain_error("log2"))
            } else {
                Ok(r)
            }
        }),
        "log10" => unary(args, f64::log10).and_then(|r| {
            if r.is_nan() {
                Err(domain_error("log10"))
            } else {
                Ok(r)
            }
        }),
        // `log(x)` natural; `log(x, base)` arbitrary-base.
        "log" => (|| {
            if args.is_empty() || args.len() > 2 {
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    format!("wrong number of arguments (given {}, expected 1..2)", args.len()),
                ));
            }
            let x = arg_f64(&args[0])?;
            let r = match args.get(1) {
                Some(base) => x.log(arg_f64(base)?),
                None => x.ln(),
            };
            if r.is_nan() {
                return Err(domain_error("log"));
            }
            Ok(r)
        })(),
        "atan2" | "hypot" => (|| {
            if args.len() != 2 {
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    format!("wrong number of arguments (given {}, expected 2)", args.len()),
                ));
            }
            let (a, b) = (arg_f64(&args[0])?, arg_f64(&args[1])?);
            Ok(if name == "atan2" { a.atan2(b) } else { a.hypot(b) })
        })(),
        "sinh" => unary(args, f64::sinh),
        "cosh" => unary(args, f64::cosh),
        "tanh" => unary(args, f64::tanh),
        "asinh" => unary(args, f64::asinh),
        // `acosh(x<1)` and `atanh(|x|>1)` are NaN -> DomainError; `atanh(±1)`
        // is ±Infinity, which passes through (oracle-verified).
        "acosh" => unary(args, f64::acosh).and_then(|r| {
            if r.is_nan() {
                Err(domain_error("acosh"))
            } else {
                Ok(r)
            }
        }),
        "atanh" => unary(args, f64::atanh).and_then(|r| {
            if r.is_nan() {
                Err(domain_error("atanh"))
            } else {
                Ok(r)
            }
        }),
        "erf" => unary(args, |x| unsafe { erf(x) }),
        "erfc" => unary(args, |x| unsafe { erfc(x) }),
        "gamma" => math_gamma(args),
        "ldexp" => (|| {
            if args.len() != 2 {
                return Err(math_arity(args.len(), "2"));
            }
            let fraction = arg_f64(&args[0])?;
            let exponent = arg_f64(&args[1])? as i32;
            Ok(unsafe { ldexp(fraction, exponent) })
        })(),
        _ => return None,
    };
    Some(result.map(RubyValue::Float))
}

/// `Math.frexp(x) -> [fraction, exponent]` with `x == fraction * 2**exponent`
/// and `0.5 <= |fraction| < 1` (`[0.0, 0]` for `x == 0`) -- the C `frexp`.
fn math_frexp(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if args.len() != 1 {
        return Err(math_arity(args.len(), "1"));
    }
    let x = arg_f64(&args[0])?;
    let mut exponent: i32 = 0;
    let fraction = unsafe { frexp(x, &mut exponent) };
    Ok(pair(fraction, i64::from(exponent)))
}

/// A `[float, integer]` result pair (for `frexp`/`lgamma`).
fn pair(value: f64, tag: i64) -> RubyValue {
    RubyValue::Array(array_new(vec![RubyValue::Float(value), RubyValue::Int(tag)]))
}

/// `Math.lgamma(x) -> [log(|gamma(x)|), sign]` (`sign` is -1 or 1), matching
/// CRuby's `math.c`: `+inf`/`+0.0` -> `[Infinity, 1]`, `-0.0` -> `[Infinity, -1]`,
/// `-inf` -> DomainError; otherwise the system `lgamma_r`.
fn math_lgamma(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if args.len() != 1 {
        return Err(math_arity(args.len(), "1"));
    }
    let d = arg_f64(&args[0])?;
    if d.is_infinite() {
        return if d < 0.0 {
            Err(domain_error("lgamma"))
        } else {
            Ok(pair(f64::INFINITY, 1))
        };
    }
    if d == 0.0 {
        let sign = if d.is_sign_negative() { -1 } else { 1 };
        return Ok(pair(f64::INFINITY, sign));
    }
    let mut sign: i32 = 1;
    let value = unsafe { lgamma_r(d, &mut sign) };
    Ok(pair(value, i64::from(sign)))
}

/// `Math.gamma(x)`, matching CRuby's `math.c`: `+inf` -> Infinity, `-inf` ->
/// DomainError, `±0.0` -> `±Infinity`, negative integers -> DomainError, small
/// positive integers via the exact factorial table (so e.g. `gamma(20)` is
/// bit-identical to Ruby's, not a rounded `tgamma`), else the system `tgamma`.
fn math_gamma(args: &[RubyValue]) -> Result<f64, Signal> {
    if args.len() != 1 {
        return Err(math_arity(args.len(), "1"));
    }
    let d = arg_f64(&args[0])?;
    if d.is_infinite() {
        return if d < 0.0 { Err(domain_error("gamma")) } else { Ok(f64::INFINITY) };
    }
    if d == 0.0 {
        return Ok(if d.is_sign_negative() { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    if d == d.floor() {
        // Integer: negative is a pole -> DomainError; small positive -> exact (n-1)!.
        if d < 0.0 {
            return Err(domain_error("gamma"));
        }
        if (1.0..=FACT_TABLE.len() as f64).contains(&d) {
            return Ok(FACT_TABLE[d as usize - 1]);
        }
    }
    Ok(unsafe { tgamma(d) })
}

/// `(n-1)!` for `gamma(n)`, `n` in `1..=23` -- CRuby's `math.c` `fact_table`
/// verbatim, so gamma of small positive integers is bit-identical to Ruby's.
/// (`23!` needs a 56-bit mantissa, so CRuby stops here and falls to `tgamma`.)
const FACT_TABLE: [f64; 23] = [
    1.0, 1.0, 2.0, 6.0, 24.0, 120.0, 720.0, 5040.0, 40320.0, 362880.0, 3628800.0,
    39916800.0, 479001600.0, 6227020800.0, 87178291200.0, 1307674368000.0,
    20922789888000.0, 355687428096000.0, 6402373705728000.0, 121645100408832000.0,
    2432902008176640000.0, 51090942171709440000.0, 1124000727777607680000.0,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn f(r: Option<Result<RubyValue, Signal>>) -> f64 {
        match r.unwrap().unwrap() {
            RubyValue::Float(f) => f,
            _ => panic!("expected a Float"),
        }
    }

    #[test]
    fn functions_coerce_across_the_tower() {
        assert_eq!(f(math_call("sqrt", &[RubyValue::Int(4)])), 2.0);
        assert_eq!(f(math_call("log2", &[RubyValue::Int(8)])), 3.0);
        assert_eq!(f(math_call("hypot", &[RubyValue::Int(3), RubyValue::Int(4)])), 5.0);
        let quarter = crate::builtins::rational::rational_new(1.into(), 4.into()).unwrap();
        assert_eq!(f(math_call("sqrt", &[quarter])), 0.5);
        assert!(math_call("nope", &[]).is_none());
    }

    #[test]
    fn negative_sqrt_is_a_domain_error() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            math_call("sqrt", &[RubyValue::Int(-1)])
        }));
        assert!(r.is_err()); // Math::DomainError, panicking registry-less
    }
}
