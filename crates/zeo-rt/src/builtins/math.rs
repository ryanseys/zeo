//! `Math` (CRuby math.c) -- module functions over `f64` intrinsics, plus
//! `Math::PI`/`Math::E`. Every argument coerces through the numeric tower's
//! `f64` view (Integer/Bignum/Float/Rational; anything else is CRuby's
//! TypeError), and domain violations raise real `Math::DomainError` with
//! CRuby's exact message shape.

use crate::builtins::numeric::num_to_f64_unchecked;
use crate::builtins::{arg_error, type_error};
use crate::collections::array_new;
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

// System libm -- the same C math library CRuby's own `Math` methods call, so
// `gamma`/`lgamma`/`erf`/`erfc`/`frexp`/`ldexp` match the oracle bit-for-bit.
// No new crate dependency: these come with the always-linked math library, the
// same way the `libc` crate's time/process calls are used elsewhere in the
// runtime. (`sinh`/`cosh`/`tanh`/`asinh`/`acosh`/`atanh` use Rust's own `f64`.)
unsafe extern "C" {
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
        other => Err(type_error!(
            "can't convert {} into Float",
            crate::builtins::convert_name_of(other)
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
    arg_error!("wrong number of arguments (given {given}, expected {expected})")
}

/// A one-argument libm call: coerce the single argument to `f64` and apply `f`.
/// A wrong argument count is CRuby's ArgumentError.
fn unary(args: &[RubyValue], f: impl Fn(f64) -> f64) -> Result<f64, Signal> {
    if args.len() != 1 {
        return Err(arg_error!(
            "wrong number of arguments (given {}, expected 1)",
            args.len()
        ));
    }
    Ok(f(arg_f64(&args[0])?))
}

/// [`unary`] wrapped as a `Float` result -- the common case.
fn plain(args: &[RubyValue], f: impl Fn(f64) -> f64) -> Result<RubyValue, Signal> {
    unary(args, f).map(RubyValue::Float)
}

/// [`unary`] with a NaN-result domain check: `sqrt`/`asin`/`log2`/... of an
/// out-of-domain argument is a `Math::DomainError` rather than a NaN Float.
fn checked(args: &[RubyValue], f: impl Fn(f64) -> f64, name: &str) -> Result<RubyValue, Signal> {
    let r = unary(args, f)?;
    if r.is_nan() {
        Err(domain_error(name))
    } else {
        Ok(RubyValue::Float(r))
    }
}

ruby_module! {
    Math = zeo_abi::MATH_CLASS;

    const PI = RubyValue::Float(std::f64::consts::PI);
    const E = RubyValue::Float(std::f64::consts::E);

    // Every function is a `module_function`: reachable as `Math.sqrt(x)` AND,
    // after `include Math`, as a private `sqrt(x)`. Every argument coerces
    // through the numeric tower's `f64` view; domain violations raise
    // `Math::DomainError` with CRuby's exact message.
    module_function def "sqrt"(_recv, args, _block) { checked(args, f64::sqrt, "sqrt") }
    module_function def "cbrt"(_recv, args, _block) { plain(args, f64::cbrt) }
    module_function def "sin"(_recv, args, _block) { plain(args, f64::sin) }
    module_function def "cos"(_recv, args, _block) { plain(args, f64::cos) }
    module_function def "tan"(_recv, args, _block) { plain(args, f64::tan) }
    module_function def "asin"(_recv, args, _block) { checked(args, f64::asin, "asin") }
    module_function def "acos"(_recv, args, _block) { checked(args, f64::acos, "acos") }
    module_function def "atan"(_recv, args, _block) { plain(args, f64::atan) }
    module_function def "exp"(_recv, args, _block) { plain(args, f64::exp) }
    // `expm1`/`log1p` keep precision near zero, where `exp(x) - 1` and
    // `log(1 + x)` lose it to cancellation.
    module_function def "expm1"(_recv, args, _block) { plain(args, f64::exp_m1) }
    module_function def "log1p"(_recv, args, _block) { checked(args, f64::ln_1p, "log1p") }
    module_function def "log2"(_recv, args, _block) { checked(args, f64::log2, "log2") }
    module_function def "log10"(_recv, args, _block) { checked(args, f64::log10, "log10") }
    // `log(x)` natural; `log(x, base)` arbitrary-base.
    module_function def "log"(_recv, args, _block) {
        if args.is_empty() || args.len() > 2 {
            return Err(arg_error!(
                "wrong number of arguments (given {}, expected 1..2)",
                args.len()
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
        Ok(RubyValue::Float(r))
    }
    module_function def "atan2"(_recv, args, _block) {
        if args.len() != 2 {
            return Err(arg_error!("wrong number of arguments (given {}, expected 2)", args.len()));
        }
        Ok(RubyValue::Float(arg_f64(&args[0])?.atan2(arg_f64(&args[1])?)))
    }
    module_function def "hypot"(_recv, args, _block) {
        if args.len() != 2 {
            return Err(arg_error!("wrong number of arguments (given {}, expected 2)", args.len()));
        }
        Ok(RubyValue::Float(arg_f64(&args[0])?.hypot(arg_f64(&args[1])?)))
    }
    module_function def "sinh"(_recv, args, _block) { plain(args, f64::sinh) }
    module_function def "cosh"(_recv, args, _block) { plain(args, f64::cosh) }
    module_function def "tanh"(_recv, args, _block) { plain(args, f64::tanh) }
    module_function def "asinh"(_recv, args, _block) { plain(args, f64::asinh) }
    // `acosh(x<1)` and `atanh(|x|>1)` are NaN -> DomainError; `atanh(±1)` is
    // ±Infinity, which passes through (oracle-verified).
    module_function def "acosh"(_recv, args, _block) { checked(args, f64::acosh, "acosh") }
    module_function def "atanh"(_recv, args, _block) { checked(args, f64::atanh, "atanh") }
    module_function def "erf"(_recv, args, _block) { plain(args, |x| unsafe { erf(x) }) }
    module_function def "erfc"(_recv, args, _block) { plain(args, |x| unsafe { erfc(x) }) }
    module_function def "gamma"(_recv, args, _block) { math_gamma(args).map(RubyValue::Float) }
    module_function def "ldexp"(_recv, args, _block) {
        if args.len() != 2 {
            return Err(math_arity(args.len(), "2"));
        }
        let fraction = arg_f64(&args[0])?;
        let exponent = arg_f64(&args[1])? as i32;
        Ok(RubyValue::Float(unsafe { ldexp(fraction, exponent) }))
    }
    // `frexp`/`lgamma` answer a two-element Array, not a Float.
    module_function def "frexp"(_recv, args, _block) { math_frexp(args) }
    module_function def "lgamma"(_recv, args, _block) { math_lgamma(args) }
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
    RubyValue::Array(array_new(vec![
        RubyValue::Float(value),
        RubyValue::Int(tag),
    ]))
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
        return if d < 0.0 {
            Err(domain_error("gamma"))
        } else {
            Ok(f64::INFINITY)
        };
    }
    if d == 0.0 {
        return Ok(if d.is_sign_negative() {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
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
    1.0,
    1.0,
    2.0,
    6.0,
    24.0,
    120.0,
    720.0,
    5040.0,
    40320.0,
    362880.0,
    3628800.0,
    39916800.0,
    479001600.0,
    6227020800.0,
    87178291200.0,
    1307674368000.0,
    20922789888000.0,
    355687428096000.0,
    6402373705728000.0,
    121645100408832000.0,
    2432902008176640000.0,
    51090942171709440000.0,
    1124000727777607680000.0,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The Math functions are `ruby_module!`-generated `module_function`s
    /// (mangled Rust fn names), so reach them the way dispatch does -- through
    /// the registered instance table. `Math.sqrt` (the class side) shares the
    /// same body, verified by `expand.rs`'s module_function test.
    fn call(name: &str, args: &[RubyValue]) -> Option<Result<RubyValue, Signal>> {
        let tbl = crate::builtins::registered_table(zeo_abi::MATH_CLASS)
            .expect("Math is a registered builtin table")
            .instance
            .as_ref()
            .expect("Math has module functions");
        (tbl.lookup)(name).map(|f| f(&RubyValue::Nil, args, None))
    }

    fn f(r: Option<Result<RubyValue, Signal>>) -> f64 {
        match r.unwrap().unwrap() {
            RubyValue::Float(f) => f,
            _ => panic!("expected a Float"),
        }
    }

    #[test]
    fn functions_coerce_across_the_tower() {
        assert_eq!(f(call("sqrt", &[RubyValue::Int(4)])), 2.0);
        assert_eq!(f(call("log2", &[RubyValue::Int(8)])), 3.0);
        assert_eq!(
            f(call("hypot", &[RubyValue::Int(3), RubyValue::Int(4)])),
            5.0
        );
        let quarter = crate::builtins::rational::rational_new(1.into(), 4.into()).unwrap();
        assert_eq!(f(call("sqrt", &[quarter])), 0.5);
        assert!(call("nope", &[]).is_none());
    }

    #[test]
    fn negative_sqrt_is_a_domain_error() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            call("sqrt", &[RubyValue::Int(-1)])
        }));
        assert!(r.is_err()); // Math::DomainError, panicking registry-less
    }
}
