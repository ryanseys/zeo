//! `Math` (CRuby math.c) -- module functions over `f64` intrinsics, plus
//! `Math::PI`/`Math::E`. Every argument coerces through the numeric tower's
//! `f64` view (Integer/Bignum/Float/Rational; anything else is CRuby's
//! TypeError), and domain violations raise real `Math::DomainError` with
//! CRuby's exact message shape.

use crate::builtins::numeric::num_to_f64_unchecked;
use crate::{RubyValue, Signal};

fn arg_f64(v: &RubyValue) -> Result<f64, Signal> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
            Ok(num_to_f64_unchecked(v))
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Float",
                crate::builtins::class_name_of(other)
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
        _ => return None,
    };
    Some(result.map(RubyValue::Float))
}

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
