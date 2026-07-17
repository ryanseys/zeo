//! `Complex` (CRuby complex.c) -- two components that KEEP their own
//! numeric class (`Complex(1, 2)` holds Integers and inspects as
//! `(1+2i)`; a Rational component renders `(2/25)*i` -- all
//! oracle-verified). Components are constructor-restricted to
//! Integer|Float|Rational; a Complex component never nests.
//!
//! Internal component arithmetic uses QUO (exact rational division) with
//! CRuby's canonicalization: an exact den==1 result demotes to Integer
//! (`Complex(1, 2) / 2` has real `(1/2)` but imag `1`, not `(1/1)` --
//! oracle-verified), unlike standalone Rational arithmetic which never
//! demotes.

use crate::builtins::numeric::{num_add_or_panic, num_mul_or_panic, num_sub_or_panic};
use crate::builtins::{arity, builtin_methods};
use crate::{RubyValue, Signal};
use num_bigint::BigInt;
use num_traits::One;
use std::sync::Arc;

pub struct RComplexData {
    pub real: RubyValue,
    pub imag: RubyValue,
}

pub type RComplex = Arc<RComplexData>;

fn is_component(v: &RubyValue) -> bool {
    matches!(
        v,
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_)
    )
}

/// THE Complex constructor -- TypeError (CRuby's "not a real" shape) for a
/// non-numeric component.
pub fn complex_new(real: RubyValue, imag: RubyValue) -> Result<RubyValue, Signal> {
    if !is_component(&real) || !is_component(&imag) {
        let bad = if is_component(&real) { &imag } else { &real };
        return Err(crate::dispatch::raise_error(
            "TypeError",
            format!("not a real: {}", bad.inspect_string()),
        ));
    }
    Ok(RubyValue::Complex(Arc::new(RComplexData { real, imag })))
}

/// A `4i`/`2.0i`/`3ri` LITERAL (codegen's emission target) -- infallible:
/// the inner value is a numeric literal by syntax.
pub fn complex_from_literal(imag: RubyValue) -> RubyValue {
    complex_new(RubyValue::Int(0), imag)
        .expect("an imaginary literal's inner value is numeric by syntax")
}

/// The `(real, imag)` component view, lifting a plain numeric to
/// `(x, 0)` -- the matrix's Complex-lane promotion.
pub(crate) fn as_components(v: &RubyValue) -> (RubyValue, RubyValue) {
    match v {
        RubyValue::Complex(c) => (c.real.clone(), c.imag.clone()),
        other if is_component(other) => (other.clone(), RubyValue::Int(0)),
        other => panic!("expected a numeric, got {}", other.to_display_string()),
    }
}

/// CRuby's complex-internal canonicalization: an exact `(n/1)` demotes to
/// Integer.
fn canon(v: RubyValue) -> RubyValue {
    if let RubyValue::Rational(r) = &v {
        if r.den.is_one() {
            return crate::builtins::integer::int_value(r.num.clone());
        }
    }
    v
}

/// Component division: `quo` semantics (exact rational for exact
/// operands, IEEE for float pairs), canonicalized.
fn comp_quo(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    Ok(canon(crate::builtins::numeric::num_quo(a, b).expect(
        "complex components are numeric by construction",
    )?))
}

pub(crate) fn cpx_add(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let ((ar, ai), (br, bi)) = (as_components(a), as_components(b));
    complex_new(num_add_or_panic(&ar, &br)?, num_add_or_panic(&ai, &bi)?)
}

pub(crate) fn cpx_sub(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let ((ar, ai), (br, bi)) = (as_components(a), as_components(b));
    complex_new(num_sub_or_panic(&ar, &br)?, num_sub_or_panic(&ai, &bi)?)
}

pub(crate) fn cpx_mul(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let ((ar, ai), (br, bi)) = (as_components(a), as_components(b));
    // (ar + ai*i)(br + bi*i) = (ar*br - ai*bi) + (ar*bi + ai*br)i
    let real = num_sub_or_panic(&num_mul_or_panic(&ar, &br)?, &num_mul_or_panic(&ai, &bi)?)?;
    let imag = num_add_or_panic(&num_mul_or_panic(&ar, &bi)?, &num_mul_or_panic(&ai, &br)?)?;
    complex_new(real, imag)
}

pub(crate) fn cpx_div(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let ((ar, ai), (br, bi)) = (as_components(a), as_components(b));
    // (a / b) = (a * conj(b)) / |b|^2, componentwise via quo.
    let denom = num_add_or_panic(&num_mul_or_panic(&br, &br)?, &num_mul_or_panic(&bi, &bi)?)?;
    let real_num = num_add_or_panic(&num_mul_or_panic(&ar, &br)?, &num_mul_or_panic(&ai, &bi)?)?;
    let imag_num = num_sub_or_panic(&num_mul_or_panic(&ai, &br)?, &num_mul_or_panic(&ar, &bi)?)?;
    complex_new(comp_quo(&real_num, &denom)?, comp_quo(&imag_num, &denom)?)
}

/// `complex ** integer` stays exact via square-and-multiply over the exact
/// component ops; other exponent shapes are a documented Tier B follow-up
/// (polar-form f64), raised loudly.
pub(crate) fn cpx_pow(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    match b {
        RubyValue::Int(_) | RubyValue::BigInt(_) => {
            let e = crate::builtins::integer::to_bigint(b);
            let negative = e < BigInt::from(0);
            let mut e = e.magnitude().clone();
            let mut base = a.clone();
            let mut acc = complex_new(RubyValue::Int(1), RubyValue::Int(0))?;
            let one: num_bigint::BigUint = One::one();
            while e > num_bigint::BigUint::ZERO {
                if (&e & &one) == one {
                    acc = cpx_mul(&acc, &base)?;
                }
                base = cpx_mul(&base, &base)?;
                e >>= 1;
            }
            if negative {
                cpx_div(&complex_new(RubyValue::Int(1), RubyValue::Int(0))?, &acc)
            } else {
                Ok(acc)
            }
        }
        _ => panic!(
            "Complex ** with a non-Integer exponent isn't supported yet (polar-form result; spike scope)"
        ),
    }
}

/// Structural equality: componentwise `==` (a plain numeric compares as
/// `(x, 0)` -- `Complex(2, 0) == 2` is true, oracle-verified).
pub(crate) fn cpx_eq(a: &RubyValue, b: &RubyValue) -> bool {
    let ((ar, ai), (br, bi)) = (as_components(a), as_components(b));
    crate::builtins::numeric::num_eq(&ar, &br).unwrap_or(false)
        && crate::builtins::numeric::num_eq(&ai, &bi).unwrap_or(false)
}

/// `to_s` is `"1+2i"` / `"1.5-2.5i"`; `inspect` wraps in parens and
/// renders a Rational imag as `(2/25)*i` -- all oracle-verified. The
/// imag's own sign supplies the `-`; a Rational/positive imag gets `+`.
pub(crate) fn cpx_format(c: &RComplexData, inspect: bool) -> String {
    let render = |v: &RubyValue| {
        if inspect {
            v.inspect_string()
        } else {
            v.to_display_string()
        }
    };
    let real = render(&c.real);
    let imag = render(&c.imag);
    let (sign, imag) = match imag.strip_prefix('-') {
        Some(rest) => ("-", rest.to_string()),
        None => ("+", imag),
    };
    let star = if matches!(c.imag, RubyValue::Rational(_)) && inspect { "*" } else { "" };
    let body = format!("{real}{sign}{imag}{star}i");
    if inspect { format!("({body})") } else { body }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpx(r: i64, i: i64) -> RubyValue {
        complex_new(RubyValue::Int(r), RubyValue::Int(i)).unwrap()
    }

    #[test]
    fn multiplication_matches_the_oracle() {
        // (1+2i)(3+4i) == (-5+10i), exact Integer components.
        let r = cpx_mul(&cpx(1, 2), &cpx(3, 4)).unwrap();
        let RubyValue::Complex(c) = &r else { panic!() };
        assert!(matches!(c.real, RubyValue::Int(-5)));
        assert!(matches!(c.imag, RubyValue::Int(10)));
    }

    #[test]
    fn division_produces_canonicalized_rational_components() {
        // (1+2i)/(3+4i) == ((11/25)+(2/25)*i)
        let r = cpx_div(&cpx(1, 2), &cpx(3, 4)).unwrap();
        let RubyValue::Complex(c) = &r else { panic!() };
        assert!(matches!(&c.real, RubyValue::Rational(q) if q.num == BigInt::from(11) && q.den == BigInt::from(25)));
        // (1+2i)/2: real (1/2), imag demotes to Integer 1.
        let r = cpx_div(&cpx(1, 2), &RubyValue::Int(2)).unwrap();
        let RubyValue::Complex(c) = &r else { panic!() };
        assert!(matches!(&c.real, RubyValue::Rational(_)));
        assert!(matches!(c.imag, RubyValue::Int(1)));
    }

    #[test]
    fn pow_and_eq_match_the_oracle() {
        // (1+2i)**2 == (-3+4i); (0+1i)**3 == (0-1i)
        let r = cpx_pow(&cpx(1, 2), &RubyValue::Int(2)).unwrap();
        assert!(cpx_eq(&r, &cpx(-3, 4)));
        let r = cpx_pow(&cpx(0, 1), &RubyValue::Int(3)).unwrap();
        assert!(cpx_eq(&r, &cpx(0, -1)));
        assert!(cpx_eq(&cpx(2, 0), &RubyValue::Int(2)));
    }

    #[test]
    fn formatting_matches_the_oracle() {
        let RubyValue::Complex(c) = cpx(1, 2) else { panic!() };
        assert_eq!(cpx_format(&c, false), "1+2i");
        assert_eq!(cpx_format(&c, true), "(1+2i)");
        let RubyValue::Complex(c) = cpx(1, -2) else { panic!() };
        assert_eq!(cpx_format(&c, false), "1-2i");
        let r = cpx_div(&cpx(1, 2), &cpx(3, 4)).unwrap();
        let RubyValue::Complex(c) = r else { panic!() };
        assert_eq!(cpx_format(&c, true), "((11/25)+(2/25)*i)");
    }

    #[test]
    fn constructor_rejects_non_numerics() {
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            complex_new(RubyValue::Int(1), s)
        }));
        assert!(r.is_err()); // TypeError, panicking registry-less
    }
}

macro_rules! cpx_op_row {
    ($args:ident, $recv:ident, $num_fn:ident, $op:literal) => {{
        arity!($args, 1);
        crate::builtins::numeric::num_coerce_bin(
            $recv,
            &$args[0],
            crate::builtins::numeric::$num_fn($recv, &$args[0]),
            $op,
        )
    }};
}

fn recv_complex(recv: &RubyValue) -> &RComplexData {
    match recv {
        RubyValue::Complex(c) => c,
        _ => unreachable!("Complex table row dispatched on a non-Complex receiver"),
    }
}

fn abs_f64(c: &RComplexData) -> f64 {
    crate::builtins::numeric::num_to_f64_unchecked(&c.real)
        .hypot(crate::builtins::numeric::num_to_f64_unchecked(&c.imag))
}

builtin_methods! {
    pub(crate) fn lookup;

    "+" => fn add(recv, args, _block) { cpx_op_row!(args, recv, num_add, "+") }
    "-" => fn sub(recv, args, _block) { cpx_op_row!(args, recv, num_sub, "-") }
    "*" => fn mul(recv, args, _block) { cpx_op_row!(args, recv, num_mul, "*") }
    "/" => fn div(recv, args, _block) { cpx_op_row!(args, recv, num_div, "/") }
    "**" => fn pow(recv, args, _block) { cpx_op_row!(args, recv, num_pow, "**") }
    "-@" => fn neg(recv, args, _block) {
        arity!(args, 0);
        cpx_sub(&complex_new(RubyValue::Int(0), RubyValue::Int(0))?, recv)
    }
    "+@" => fn pos(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "==" => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    "real" => fn real(recv, args, _block) {
        arity!(args, 0);
        Ok(recv_complex(recv).real.clone())
    }
    "imag" | "imaginary" => fn imag(recv, args, _block) {
        arity!(args, 0);
        Ok(recv_complex(recv).imag.clone())
    }
    "real?" => fn real_p(recv, args, _block) {
        arity!(args, 0);
        let _ = recv;
        Ok(RubyValue::Bool(false))
    }
    "abs" | "magnitude" => fn abs(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(abs_f64(recv_complex(recv))))
    }
    "abs2" => fn abs2(recv, args, _block) {
        arity!(args, 0);
        let c = recv_complex(recv);
        num_add_or_panic(
            &num_mul_or_panic(&c.real, &c.real)?,
            &num_mul_or_panic(&c.imag, &c.imag)?,
        )
    }
    "arg" | "angle" | "phase" => fn arg(recv, args, _block) {
        arity!(args, 0);
        let c = recv_complex(recv);
        Ok(RubyValue::Float(
            crate::builtins::numeric::num_to_f64_unchecked(&c.imag)
                .atan2(crate::builtins::numeric::num_to_f64_unchecked(&c.real)),
        ))
    }
    "polar" => fn polar(recv, args, _block) {
        arity!(args, 0);
        let c = recv_complex(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Float(abs_f64(c)),
            RubyValue::Float(
                crate::builtins::numeric::num_to_f64_unchecked(&c.imag)
                    .atan2(crate::builtins::numeric::num_to_f64_unchecked(&c.real)),
            ),
        ])))
    }
    "rect" | "rectangular" => fn rect(recv, args, _block) {
        arity!(args, 0);
        let c = recv_complex(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            c.real.clone(),
            c.imag.clone(),
        ])))
    }
    "conj" | "conjugate" => fn conj(recv, args, _block) {
        arity!(args, 0);
        let c = recv_complex(recv);
        let neg_imag = crate::builtins::numeric::num_sub_or_panic(&RubyValue::Int(0), &c.imag)?;
        complex_new(c.real.clone(), neg_imag)
    }
    "to_c" => fn to_c(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Complex.rect(real, imag = 0)` / `.rectangular(...)`: the direct
    // cartesian constructor (the class-method mirror of the `Complex(...)`
    // Kernel form). `Complex.polar` is a separate gap (its CRuby type-exact
    // trig is tracked with the numeric-exactness work).
    "rect" | "rectangular" => fn rect_c(_recv, args, _block) {
        arity!(args, 1..=2);
        let real = args[0].clone();
        let imag = args.get(1).cloned().unwrap_or(RubyValue::Int(0));
        complex_new(real, imag)
    }
}
