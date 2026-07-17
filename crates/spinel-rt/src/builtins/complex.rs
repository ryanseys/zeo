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

/// THE Complex constructor -- CRuby's "can't convert X into Complex"
/// TypeError for a non-numeric component.
pub fn complex_new(real: RubyValue, imag: RubyValue) -> Result<RubyValue, Signal> {
    if !is_component(&real) || !is_component(&imag) {
        let bad = if is_component(&real) { &imag } else { &real };
        return Err(crate::dispatch::raise_error(
            "TypeError",
            format!("can't convert {} into Complex", convert_name(bad)),
        ));
    }
    Ok(RubyValue::Complex(Arc::new(RComplexData { real, imag })))
}

/// The name a "can't convert X into Y" TypeError uses -- CRuby prints the
/// value for `nil`/`true`/`false`, otherwise the class name.
fn convert_name(v: &RubyValue) -> String {
    match v {
        RubyValue::Nil => "nil".to_string(),
        RubyValue::Bool(true) => "true".to_string(),
        RubyValue::Bool(false) => "false".to_string(),
        _ => crate::builtins::class_name_of(v).to_string(),
    }
}

/// `String#to_c`'s lenient parse (CRuby complex.c `read_comp`, non-strict):
/// consumes the leading `<rat>`, `<rat>i`, `<rat>@<rat>` (polar), or
/// `<rat><sign><rat>i` form and ignores the junk tail; an unparseable head
/// yields `(0+0i)`. A `/0` denominator raises ZeroDivisionError, matching
/// `"1/0".to_c`.
pub(crate) fn parse_str_to_c(input: &str) -> Result<RubyValue, Signal> {
    let mut cur = Cur { s: input.as_bytes(), i: 0 };
    cur.skip_ws();
    let mut buf = String::new();
    let sign = read_sign(&mut cur, &mut buf);

    // A bare imaginary unit: "i", "+i", "-i".
    if is_imag_unit(cur.peek()) {
        cur.bump();
        let n = if sign == b'-' { -1 } else { 1 };
        return complex_new(RubyValue::Int(0), RubyValue::Int(n));
    }
    // No leading number ("-", "@", "foo") -> real from whatever was read.
    if !read_rat_nos(&mut cur, &mut buf) {
        return complex_new(str2num(&buf)?, RubyValue::Int(0));
    }
    let num = str2num(&buf)?;

    // Pure imaginary: "3i".
    if is_imag_unit(cur.peek()) {
        cur.bump();
        return complex_new(RubyValue::Int(0), num);
    }
    // Polar: "1@2".
    if cur.peek() == b'@' {
        cur.bump();
        buf.clear();
        read_rat(&mut cur, &mut buf);
        if buf.is_empty() || !buf.as_bytes().last().unwrap().is_ascii_digit() {
            return complex_new(num, RubyValue::Int(0)); // e.g. "1@-", "10@"
        }
        return complex_new_polar(num, str2num(&buf)?);
    }
    // Rectangular: "1+2i", "5+i".
    if cur.peek() == b'+' || cur.peek() == b'-' {
        buf.clear();
        let sign2 = read_sign(&mut cur, &mut buf);
        let num2 = if is_imag_unit(cur.peek()) {
            RubyValue::Int(if sign2 == b'-' { -1 } else { 1 })
        } else if !read_rat_nos(&mut cur, &mut buf) {
            return complex_new(num, RubyValue::Int(0)); // e.g. "1+xi"
        } else {
            str2num(&buf)?
        };
        if !is_imag_unit(cur.peek()) {
            return complex_new(num, RubyValue::Int(0)); // e.g. "1+3x"
        }
        cur.bump();
        return complex_new(num, num2);
    }
    complex_new(num, RubyValue::Int(0))
}

/// A byte cursor over an ASCII numeric string; `peek()` past the end reads
/// `0` (NUL), matching the C `**s` sentinel.
struct Cur<'a> {
    s: &'a [u8],
    i: usize,
}

impl Cur<'_> {
    fn peek(&self) -> u8 {
        self.s.get(self.i).copied().unwrap_or(0)
    }
    fn bump(&mut self) {
        if self.i < self.s.len() {
            self.i += 1;
        }
    }
    fn skip_ws(&mut self) {
        while self.peek().is_ascii_whitespace() {
            self.bump();
        }
    }
}

fn is_imag_unit(c: u8) -> bool {
    matches!(c, b'i' | b'I' | b'j' | b'J')
}

fn read_sign(cur: &mut Cur, buf: &mut String) -> u8 {
    let c = cur.peek();
    if c == b'+' || c == b'-' {
        buf.push(c as char);
        cur.bump();
        return c;
    }
    b'?'
}

/// A run of decimal digits with interior `_` group separators (a `_` is
/// consumed only between two digits). Returns false if no digit is present.
fn read_digits(cur: &mut Cur, buf: &mut String) -> bool {
    if !cur.peek().is_ascii_digit() {
        return false;
    }
    loop {
        let c = cur.peek();
        if c.is_ascii_digit() {
            buf.push(c as char);
            cur.bump();
        } else if c == b'_' && cur.s.get(cur.i + 1).is_some_and(|n| n.is_ascii_digit()) {
            cur.bump();
        } else {
            break;
        }
    }
    true
}

/// A `digits[.digits][e[sign]digits]` real number; a dangling `.`/`e` with no
/// following digits is left unconsumed (its buffer char popped).
fn read_num(cur: &mut Cur, buf: &mut String) -> bool {
    if cur.peek() != b'.' && !read_digits(cur, buf) {
        return false;
    }
    if cur.peek() == b'.' {
        buf.push('.');
        cur.bump();
        if !read_digits(cur, buf) {
            buf.pop();
            return false;
        }
    }
    if matches!(cur.peek(), b'e' | b'E') {
        buf.push(cur.peek() as char);
        cur.bump();
        read_sign(cur, buf);
        if !read_digits(cur, buf) {
            buf.pop();
            return false;
        }
    }
    true
}

/// A rational body `<num>[/<den>]` (no leading sign).
fn read_rat_nos(cur: &mut Cur, buf: &mut String) -> bool {
    if !read_num(cur, buf) {
        return false;
    }
    if cur.peek() == b'/' {
        buf.push('/');
        cur.bump();
        if !read_digits(cur, buf) {
            buf.pop();
            return false;
        }
    }
    true
}

/// A signed rational `[sign]<num>[/<den>]`.
fn read_rat(cur: &mut Cur, buf: &mut String) -> bool {
    read_sign(cur, buf);
    read_rat_nos(cur, buf)
}

/// CRuby complex.c `str2num`: a `/` head is a Rational, a `.`/`e` head a
/// Float, otherwise an Integer. An empty buffer is Integer `0`.
fn str2num(s: &str) -> Result<RubyValue, Signal> {
    if s.is_empty() {
        return Ok(RubyValue::Int(0));
    }
    if let Some(slash) = s.find('/') {
        let (numer, den_str) = (&s[..slash], &s[slash + 1..]);
        let (num, mut den) = decimal_to_rat(numer);
        den *= den_str.parse::<BigInt>().unwrap_or_else(|_| BigInt::one());
        return super::rational::rational_new(num, den);
    }
    if s.contains(['.', 'e', 'E']) {
        return Ok(RubyValue::Float(s.parse::<f64>().unwrap_or(0.0)));
    }
    Ok(crate::builtins::integer::int_value(
        s.parse::<BigInt>().unwrap_or_default(),
    ))
}

/// An exact `(num, den)` for a `[sign]digits[.digits][e[sign]digits]` decimal
/// -- the numerator side of `str2num`'s rational path (`"1.5/2"` -> `3/4`).
fn decimal_to_rat(s: &str) -> (BigInt, BigInt) {
    let neg = s.starts_with('-');
    let body = s.trim_start_matches(['+', '-']);
    let (mantissa, exp_str) = body.split_once(['e', 'E']).unwrap_or((body, ""));
    let (int_part, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut num: BigInt = format!("{int_part}{frac}").parse().unwrap_or_default();
    let mut den = BigInt::one();
    let net = exp_str.parse::<i64>().unwrap_or(0) - frac.len() as i64;
    if net >= 0 {
        num *= BigInt::from(10).pow(net as u32);
    } else {
        den *= BigInt::from(10).pow((-net) as u32);
    }
    if neg {
        num = -num;
    }
    (num, den)
}

/// CRuby `f_complex_polar_real`: build `Complex` from magnitude+angle,
/// preserving the exact magnitude when the angle is a right-angle multiple
/// (`"1@0"` -> `(1+0.0i)`, `"1.0@#{PI}"` -> `(-1+0.0i)`).
fn complex_new_polar(mag: RubyValue, angle: RubyValue) -> Result<RubyValue, Signal> {
    use crate::builtins::numeric::num_to_f64_unchecked;
    let is_zero = |v: &RubyValue| num_to_f64_unchecked(v) == 0.0;
    if is_zero(&mag) || is_zero(&angle) {
        return complex_new(mag, RubyValue::Float(0.0));
    }
    let arg = num_to_f64_unchecked(&angle);
    let neg = |v: &RubyValue| num_sub_or_panic(&RubyValue::Int(0), v);
    if arg == std::f64::consts::PI {
        return complex_new(neg(&mag)?, RubyValue::Float(0.0));
    }
    if arg == std::f64::consts::FRAC_PI_2 {
        return complex_new(RubyValue::Float(0.0), mag);
    }
    if arg == std::f64::consts::FRAC_PI_2 + std::f64::consts::PI {
        return complex_new(RubyValue::Float(0.0), neg(&mag)?);
    }
    let re = num_mul_or_panic(&mag, &RubyValue::Float(arg.cos()))?;
    let im = num_mul_or_panic(&mag, &RubyValue::Float(arg.sin()))?;
    complex_new(re, im)
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
/// component ops; every other exponent shape takes the polar-form path
/// `z ** w == exp(w * log z)` in `f64` (CRuby's `rb_complex_pow`).
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
        // z^w = exp(w * log z), with log z = ln|z| + i*arg(z).
        _ => {
            let f = crate::builtins::numeric::num_to_f64_unchecked;
            let (ar, ai) = as_components(a);
            let (br, bi) = as_components(b);
            let (ar, ai, br, bi) = (f(&ar), f(&ai), f(&br), f(&bi));
            let ln_r = ar.hypot(ai).ln();
            let theta = ai.atan2(ar);
            // w * log z
            let re = br * ln_r - bi * theta;
            let im = br * theta + bi * ln_r;
            let scale = re.exp();
            complex_new(
                RubyValue::Float(scale * im.cos()),
                RubyValue::Float(scale * im.sin()),
            )
        }
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
    // A Complex is finite iff both components are (only a Float component can
    // be infinite/NaN); a real Complex reports `nil` from `infinite?`.
    "finite?" => fn finite_p(recv, args, _block) {
        arity!(args, 0);
        let c = recv_complex(recv);
        Ok(RubyValue::Bool(
            component_finite(&c.real) && component_finite(&c.imag),
        ))
    }
    "infinite?" => fn infinite_p(recv, args, _block) {
        arity!(args, 0);
        let c = recv_complex(recv);
        Ok(if component_finite(&c.real) && component_finite(&c.imag) {
            RubyValue::Nil
        } else {
            RubyValue::Int(1)
        })
    }
    // `coerce(other)`: lift a real numeric to `Complex(other, 0)`, pass a
    // Complex through unchanged; the result is `[coerced_other, self]`.
    "coerce" => fn coerce(recv, args, _block) {
        arity!(args, 1);
        let other = match &args[0] {
            RubyValue::Complex(_) => args[0].clone(),
            v if is_component(v) => complex_new(v.clone(), RubyValue::Int(0))?,
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "{} can't be coerced into Complex",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        };
        Ok(RubyValue::Array(crate::array_new(vec![other, recv.clone()])))
    }
    // The real-projection conversions raise CRuby's RangeError unless the
    // imaginary part is an exact zero (an Integer or Rational `0`; a Float
    // `0.0` still raises).
    "to_f" => fn to_f(recv, args, _block) {
        arity!(args, 0);
        real_projection(recv, "to_f")
    }
    "to_i" | "to_int" => fn to_i(recv, args, _block) {
        arity!(args, 0);
        real_projection(recv, "to_i")
    }
    "to_r" => fn to_r(recv, args, _block) {
        arity!(args, 0);
        real_projection(recv, "to_r")
    }
    "rationalize" => fn rationalize(recv, args, _block) {
        arity!(args, 0..=1);
        real_projection(recv, "to_r")
    }
    // `denominator` = lcm of the two components' denominators; `numerator`
    // scales both components up to that shared denominator (CRuby complex.c).
    "denominator" => fn denominator(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::builtins::integer::int_value(complex_denominator(recv_complex(recv))?))
    }
    "numerator" => fn numerator(recv, args, _block) {
        arity!(args, 0);
        let c = recv_complex(recv);
        let cd = complex_denominator(c)?;
        let real = scale_numerator(&c.real, &cd)?;
        let imag = scale_numerator(&c.imag, &cd)?;
        complex_new(real, imag)
    }
}

/// A component's `denominator` as a BigInt (`Integer` -> 1, `Rational` -> den).
fn component_denominator(v: &RubyValue) -> Result<BigInt, Signal> {
    let d = crate::dispatch::send_value(v, crate::Symbol::intern("denominator"), &[], None)?;
    Ok(crate::builtins::integer::to_bigint(&d))
}

/// `lcm(real.denominator, imag.denominator)` -- the complex's shared denominator.
fn complex_denominator(c: &RComplexData) -> Result<BigInt, Signal> {
    use num_integer::Integer as _;
    Ok(component_denominator(&c.real)?.lcm(&component_denominator(&c.imag)?))
}

/// A component scaled to the shared denominator: `numerator * (cd / own_den)`.
fn scale_numerator(v: &RubyValue, cd: &BigInt) -> Result<RubyValue, Signal> {
    let num = crate::dispatch::send_value(v, crate::Symbol::intern("numerator"), &[], None)?;
    let scaled = crate::builtins::integer::to_bigint(&num) * (cd / component_denominator(v)?);
    Ok(crate::builtins::integer::int_value(scaled))
}

/// A Complex component is finite unless it is a non-finite Float.
fn component_finite(v: &RubyValue) -> bool {
    !matches!(v, RubyValue::Float(f) if !f.is_finite())
}

/// True for an EXACT zero (`Integer`/`Rational` zero) -- a Float `0.0` is
/// deliberately excluded (`Complex(6, 0.0).to_i` raises, per CRuby).
fn imag_is_exact_zero(v: &RubyValue) -> bool {
    use num_traits::Zero;
    match v {
        RubyValue::Int(0) => true,
        RubyValue::BigInt(b) => b.is_zero(),
        RubyValue::Rational(r) => r.num.is_zero(),
        _ => false,
    }
}

/// `to_f`/`to_i`/`to_r` on a real-valued Complex: forward the real component
/// to its own conversion, else raise CRuby's "can't convert to X" RangeError.
fn real_projection(recv: &RubyValue, conv: &str) -> Result<RubyValue, Signal> {
    let c = recv_complex(recv);
    if !imag_is_exact_zero(&c.imag) {
        return Err(crate::dispatch::raise_error(
            "RangeError",
            format!("can't convert {} into {}", recv.to_display_string(), conv_target(conv)),
        ));
    }
    crate::dispatch::send_value(&c.real, crate::Symbol::intern(conv), &[], None)
}

/// The Ruby class named in the RangeError for each real-projection verb.
fn conv_target(conv: &str) -> &'static str {
    match conv {
        "to_f" => "Float",
        "to_i" => "Integer",
        _ => "Rational",
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
