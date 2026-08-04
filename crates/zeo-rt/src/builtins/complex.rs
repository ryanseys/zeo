//! `Complex` (CRuby complex.c) -- two components that KEEP their own
//! numeric class (`Complex(1, 2)` holds Integers and inspects as
//! `(1+2i)`; a Rational component renders `(2/25)*i` -- all
//! oracle-verified). A component is any `Numeric`, including a user
//! subclass; only a Complex is refused, because a Complex never nests.
//!
//! Integer, Float and Rational components are the NATIVE lanes, computed
//! on directly. Every other component is reached by dispatch, at the same
//! points CRuby's `f_add`/`f_abs`/`f_negative_p` helpers fall through to
//! `rb_funcall` -- which is why `Complex(0, obj).inspect` can raise out of
//! the subclass's own `<=>`.
//!
//! Internal component arithmetic uses QUO (exact rational division) with
//! CRuby's canonicalization: an exact den==1 result demotes to Integer
//! (`Complex(1, 2) / 2` has real `(1/2)` but imag `1`, not `(1/1)` --
//! oracle-verified), unlike standalone Rational arithmetic which never
//! demotes.

use crate::builtins::{inherited_row, range_error, type_error};
use crate::{RubyValue, Signal};
use num_bigint::BigInt;
use num_traits::One;
use std::sync::Arc;
use zeo_macros::ruby_class;

pub struct RComplexData {
    pub real: RubyValue,
    pub imag: RubyValue,
}

pub type RComplex = Arc<RComplexData>;

/// The component lanes this file computes on DIRECTLY. Every other
/// component routes through `send_value`, exactly where CRuby's `f_add`
/// and `f_abs` families fall through to `rb_funcall`.
fn is_native_component(v: &RubyValue) -> bool {
    matches!(
        v,
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_)
    )
}

/// A value a Complex may HOLD. CRuby's internal constructor
/// (`nucomp_s_new_internal`) checks nothing at all, so any `Numeric` is a
/// component: `Numeric#i` on a user subclass builds a real Complex, and
/// every routine that then touches the part dispatches. The one structural
/// rule is that a Complex never nests.
fn is_component(v: &RubyValue) -> bool {
    is_native_component(v)
        || (!matches!(v, RubyValue::Complex(_))
            && crate::dispatch::is_a(v.class_id(), zeo_abi::NUMERIC_CLASS))
}

fn dispatch(recv: &RubyValue, meth: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::dispatch::send_value(recv, crate::Symbol::intern(meth), args, None)
}

/// One component binary operation -- CRuby's `f_add`/`f_sub`/`f_mul`
/// family (complex.c's `binop`): the native lanes compute in place, and
/// any other component takes an ordinary dispatch, so a user `Numeric`
/// subclass supplies its own arithmetic.
fn comp_op(
    a: &RubyValue,
    b: &RubyValue,
    native: fn(&RubyValue, &RubyValue) -> Option<Result<RubyValue, Signal>>,
    op: &str,
) -> Result<RubyValue, Signal> {
    match native(a, b) {
        Some(r) => r,
        None => dispatch(a, op, std::slice::from_ref(b)),
    }
}

fn comp_add(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    comp_op(a, b, crate::builtins::numeric::num_add, "+")
}

fn comp_sub(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    comp_op(a, b, crate::builtins::numeric::num_sub, "-")
}

fn comp_mul(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    comp_op(a, b, crate::builtins::numeric::num_mul, "*")
}

/// CRuby's `f_negate`: `-x` asks the COMPONENT to negate itself. Spelling
/// it `0 - x` instead would ask the zero to coerce, which a subclass that
/// defines `-@` but no `coerce` cannot answer.
fn comp_negate(v: &RubyValue) -> Result<RubyValue, Signal> {
    match crate::builtins::numeric::num_sub(&RubyValue::Int(0), v) {
        Some(r) => r,
        None => dispatch(v, "-@", &[]),
    }
}

/// CRuby's `f_signbit` (complex.c): a Float is negative by its SIGN BIT --
/// `-0.0` counts and `NaN` never does -- and every other component answers
/// `< 0`. For a non-native component that `<` is a real dispatch, which is
/// how a `Numeric` subclass with no `<=>` reaches `Comparable#<` and
/// raises there.
fn comp_negative(v: &RubyValue) -> Result<bool, Signal> {
    use num_bigint::Sign;
    Ok(match v {
        RubyValue::Float(f) => !f.is_nan() && f.is_sign_negative(),
        RubyValue::Int(i) => *i < 0,
        RubyValue::BigInt(b) => b.sign() == Sign::Minus,
        RubyValue::Rational(r) => r.num.sign() == Sign::Minus,
        other => dispatch(other, "<", &[RubyValue::Int(0)])?.truthy(),
    })
}

/// CRuby's `f_abs`: the native lanes in place, anything else through its
/// own `#abs`.
fn comp_abs(v: &RubyValue) -> Result<RubyValue, Signal> {
    if !is_native_component(v) {
        return dispatch(v, "abs", &[]);
    }
    if comp_negative(v)? {
        comp_negate(v)
    } else {
        Ok(v.clone())
    }
}

/// CRuby's `f_zero_p`.
fn comp_zero(v: &RubyValue) -> Result<bool, Signal> {
    if is_native_component(v) {
        return Ok(crate::builtins::numeric::num_eq(v, &RubyValue::Int(0)).unwrap_or(false));
    }
    Ok(dispatch(v, "==", &[RubyValue::Int(0)])?.truthy())
}

/// A component as `f64` -- the promotion every trigonometric path takes.
/// A non-native component supplies it through its own `#to_f`.
fn comp_to_f64(v: &RubyValue) -> Result<f64, Signal> {
    if is_native_component(v) {
        return Ok(crate::builtins::numeric::num_to_f64_unchecked(v));
    }
    let f = dispatch(v, "to_f", &[])?;
    if !is_native_component(&f) {
        return Err(type_error!(
            "can't convert {} into Float",
            crate::builtins::convert_name_of(v)
        ));
    }
    Ok(crate::builtins::numeric::num_to_f64_unchecked(&f))
}

/// `Some(v.real?)` for a `Numeric`, `None` for anything else -- CRuby's
/// `k_numeric_p(x) && f_real_p(x)` pair, kept together because both
/// callers need to tell "not numeric" from "numeric but not real".
pub(crate) fn is_real_numeric(v: &RubyValue) -> Result<Option<bool>, Signal> {
    if is_native_component(v) {
        return Ok(Some(true));
    }
    if matches!(v, RubyValue::Complex(_)) {
        return Ok(Some(false));
    }
    if !crate::dispatch::is_a(v.class_id(), zeo_abi::NUMERIC_CLASS) {
        return Ok(None);
    }
    Ok(Some(dispatch(v, "real?", &[])?.truthy()))
}

/// CRuby's `nucomp_real_check` (complex.c), the guard on the PUBLIC
/// rectangular constructors. A native lane passes; a real-valued Complex
/// contributes its own real part; anything else must be a `Numeric` that
/// answers `real?` -- and that `real?` is a real dispatch, so a subclass
/// which declines is refused like any non-numeric.
pub(crate) fn real_check(v: &RubyValue) -> Result<RubyValue, Signal> {
    if is_native_component(v) {
        return Ok(v.clone());
    }
    if let RubyValue::Complex(c) = v {
        if comp_zero(&c.imag)? {
            return Ok(c.real.clone());
        }
        return Err(type_error!("not a real"));
    }
    if crate::dispatch::is_a(v.class_id(), zeo_abi::NUMERIC_CLASS)
        && dispatch(v, "real?", &[])?.truthy()
    {
        return Ok(v.clone());
    }
    Err(type_error!("not a real"))
}

/// THE Complex constructor -- CRuby's "can't convert X into Complex"
/// TypeError for a non-numeric component.
pub fn complex_new(real: RubyValue, imag: RubyValue) -> Result<RubyValue, Signal> {
    if !is_component(&real) || !is_component(&imag) {
        let bad = if is_component(&real) { &imag } else { &real };
        return Err(type_error!(
            "can't convert {} into Complex",
            crate::builtins::convert_name_of(bad)
        ));
    }
    Ok(RubyValue::Complex(Arc::new(RComplexData { real, imag })))
}

/// `String#to_c`'s lenient parse (CRuby complex.c `read_comp`, non-strict):
/// consumes the leading `<rat>`, `<rat>i`, `<rat>@<rat>` (polar), or
/// `<rat><sign><rat>i` form and ignores the junk tail; an unparseable head
/// yields `(0+0i)`. A `/0` denominator raises ZeroDivisionError, matching
/// `"1/0".to_c`.
pub(crate) fn parse_str_to_c(input: &str) -> Result<RubyValue, Signal> {
    let mut cur = Cur {
        s: input.as_bytes(),
        i: 0,
    };
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
    if comp_to_f64(&mag)? == 0.0 || comp_to_f64(&angle)? == 0.0 {
        return complex_new(mag, RubyValue::Float(0.0));
    }
    let arg = comp_to_f64(&angle)?;
    let neg = |v: &RubyValue| comp_sub(&RubyValue::Int(0), v);
    if arg == std::f64::consts::PI {
        return complex_new(neg(&mag)?, RubyValue::Float(0.0));
    }
    if arg == std::f64::consts::FRAC_PI_2 {
        return complex_new(RubyValue::Float(0.0), mag);
    }
    if arg == std::f64::consts::FRAC_PI_2 + std::f64::consts::PI {
        return complex_new(RubyValue::Float(0.0), neg(&mag)?);
    }
    let re = comp_mul(&mag, &RubyValue::Float(arg.cos()))?;
    let im = comp_mul(&mag, &RubyValue::Float(arg.sin()))?;
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
    if let RubyValue::Rational(r) = &v
        && r.den.is_one() {
            return crate::builtins::integer::int_value(r.num.clone());
        }
    v
}

/// Component division: `quo` semantics (exact rational for exact
/// operands, IEEE for float pairs), canonicalized. CRuby's `f_quo`, so a
/// non-native component answers with its own `#quo`.
fn comp_quo(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    Ok(canon(comp_op(
        a,
        b,
        crate::builtins::numeric::num_quo,
        "quo",
    )?))
}

/// A right operand that is real, so `nucomp_add` and friends take their
/// componentwise branch instead of promoting it to `(x, 0)` and running
/// the full formula. The two agree on every native component; they part
/// on a user `Numeric`, which would otherwise be asked to add or multiply
/// a zero it never sees in ruby.
fn real_operand(b: &RubyValue) -> bool {
    !matches!(b, RubyValue::Complex(_)) && is_component(b)
}

pub(crate) fn cpx_add(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    if let RubyValue::Complex(c) = a
        && real_operand(b) {
            return complex_new(comp_add(&c.real, b)?, c.imag.clone());
        }
    let ((ar, ai), (br, bi)) = (as_components(a), as_components(b));
    complex_new(comp_add(&ar, &br)?, comp_add(&ai, &bi)?)
}

pub(crate) fn cpx_sub(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    if let RubyValue::Complex(c) = a
        && real_operand(b) {
            return complex_new(comp_sub(&c.real, b)?, c.imag.clone());
        }
    let ((ar, ai), (br, bi)) = (as_components(a), as_components(b));
    complex_new(comp_sub(&ar, &br)?, comp_sub(&ai, &bi)?)
}

pub(crate) fn cpx_mul(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    if let RubyValue::Complex(c) = a
        && real_operand(b) {
            return complex_new(comp_mul(&c.real, b)?, comp_mul(&c.imag, b)?);
        }
    let ((ar, ai), (br, bi)) = (as_components(a), as_components(b));
    // (ar + ai*i)(br + bi*i) = (ar*br - ai*bi) + (ar*bi + ai*br)i
    let real = comp_sub(&comp_mul(&ar, &br)?, &comp_mul(&ai, &bi)?)?;
    let imag = comp_add(&comp_mul(&ar, &bi)?, &comp_mul(&ai, &br)?)?;
    complex_new(real, imag)
}

pub(crate) fn cpx_div(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let (ar, ai) = as_components(a);
    // Dividing by a REAL scalar divides each component directly (CRuby's
    // componentwise rule): a Float `0.0` divisor yields Infinity, an Integer
    // `0` raises ZeroDivisionError. The conjugate formula below would instead
    // turn a zero divisor into `0/0 == NaN`.
    if !matches!(b, RubyValue::Complex(_)) {
        return complex_new(comp_quo(&ar, b)?, comp_quo(&ai, b)?);
    }
    let (br, bi) = as_components(b);
    // (a / b) = (a * conj(b)) / |b|^2, componentwise via quo.
    let denom = comp_add(&comp_mul(&br, &br)?, &comp_mul(&bi, &bi)?)?;
    let real_num = comp_add(&comp_mul(&ar, &br)?, &comp_mul(&ai, &bi)?)?;
    let imag_num = comp_sub(&comp_mul(&ai, &br)?, &comp_mul(&ar, &bi)?)?;
    complex_new(comp_quo(&real_num, &denom)?, comp_quo(&imag_num, &denom)?)
}

/// True for an exactly-zero numeric component (a `0` Integer or a `0/1`
/// Rational) -- CRuby's `k_exact_zero_p`.
pub(crate) fn is_exact_zero(v: &RubyValue) -> bool {
    match v {
        RubyValue::Int(0) => true,
        RubyValue::Rational(r) => r.num == BigInt::from(0),
        _ => false,
    }
}

/// CRuby's `nucomp_expt` exponent reductions: a Complex exponent with an
/// exactly-zero imaginary part collapses to its real component, and a Rational
/// with denominator 1 collapses to its integer numerator -- both then take the
/// exact square-and-multiply path (`(2+3i) ** Complex(1,0)` stays `(2+3i)`,
/// `(2+3i) ** (2/1)` stays `(-5+12i)`).
fn normalize_pow_exponent(b: &RubyValue) -> RubyValue {
    match b {
        RubyValue::Complex(c) if is_exact_zero(&c.imag) => normalize_pow_exponent(&c.real),
        RubyValue::Rational(r) if r.den == BigInt::from(1) => {
            crate::builtins::integer::int_value(r.num.clone())
        }
        _ => b.clone(),
    }
}

/// `complex ** integer` stays exact via square-and-multiply over the exact
/// component ops; every other exponent shape takes the polar-form path
/// `z ** w == exp(w * log z)` in `f64` (CRuby's `rb_complex_pow`).
pub(crate) fn cpx_pow(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let b_owned = normalize_pow_exponent(b);
    let b = &b_owned;
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
            let (ar, ai) = as_components(a);
            let (br, bi) = as_components(b);
            let (ar, ai, br, bi) = (
                comp_to_f64(&ar)?,
                comp_to_f64(&ai)?,
                comp_to_f64(&br)?,
                comp_to_f64(&bi)?,
            );
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
    // CRuby's `nucomp_eqeq_p` compares each component with `==`, so a
    // non-native component answers for itself.
    let eq = |x: &RubyValue, y: &RubyValue| {
        crate::builtins::numeric::num_eq(x, y).unwrap_or_else(|| x.rb_eq(y))
    };
    let ((ar, ai), (br, bi)) = (as_components(a), as_components(b));
    eq(&ar, &br) && eq(&ai, &bi)
}

/// CRuby's `f_format` (complex.c): render the real part, then the
/// imaginary part's SIGN, then its ABSOLUTE VALUE, and separate the `i`
/// with `*` whenever the text built so far does not end in a digit.
///
/// That last rule reads the rendered CHARACTERS, not the component's
/// class, and it is the whole story behind `(1+(1/2)*i)` against
/// `1+1/2i` -- one and the same Rational, `*` only where inspect's paren
/// lands a `)` at the end -- and behind `0+Infinity*i`. The sign is
/// removed by asking the component for its `#abs`, not by trimming a `-`
/// off the text.
///
/// `to_s` uses the parts' `to_s` and `inspect` their `inspect`, so both
/// can dispatch, and both can raise.
pub(crate) fn cpx_format(c: &RComplexData, inspect: bool) -> Result<String, Signal> {
    let render = |v: &RubyValue| {
        if inspect {
            v.try_inspect_string()
        } else {
            v.try_display_string()
        }
    };
    let mut s = render(&c.real)?;
    s.push(if comp_negative(&c.imag)? { '-' } else { '+' });
    s.push_str(&render(&comp_abs(&c.imag)?)?);
    if !s.ends_with(|ch: char| ch.is_ascii_digit()) {
        s.push('*');
    }
    s.push('i');
    Ok(if inspect { format!("({s})") } else { s })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::COMPLEX_CLASS)
            .unwrap()
            .class
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }

    fn cpx(r: i64, i: i64) -> RubyValue {
        complex_new(RubyValue::Int(r), RubyValue::Int(i)).unwrap()
    }

    #[test]
    fn polar_class_constructor_builds_a_complex() {
        // `Complex.polar(3, 0)` == `3 * (cos 0 + i sin 0)` == (3+0i).
        let c = cmethod("polar")(
            &RubyValue::Nil,
            &[RubyValue::Int(3), RubyValue::Int(0)],
            None,
        )
        .unwrap();
        assert!(matches!(c, RubyValue::Complex(_)));
        // Zero magnitude collapses to the origin regardless of the angle.
        let z = cmethod("polar")(
            &RubyValue::Nil,
            &[RubyValue::Int(0), RubyValue::Int(5)],
            None,
        )
        .unwrap();
        assert!(z.rb_eq(&cpx(0, 0)));
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
        assert!(
            matches!(&c.real, RubyValue::Rational(q) if q.num == BigInt::from(11) && q.den == BigInt::from(25))
        );
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
        let RubyValue::Complex(c) = cpx(1, 2) else {
            panic!()
        };
        assert_eq!(cpx_format(&c, false).unwrap(), "1+2i");
        assert_eq!(cpx_format(&c, true).unwrap(), "(1+2i)");
        let RubyValue::Complex(c) = cpx(1, -2) else {
            panic!()
        };
        assert_eq!(cpx_format(&c, false).unwrap(), "1-2i");
        let r = cpx_div(&cpx(1, 2), &cpx(3, 4)).unwrap();
        let RubyValue::Complex(c) = r else { panic!() };
        assert_eq!(cpx_format(&c, true).unwrap(), "((11/25)+(2/25)*i)");
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

use crate::builtins::numeric::num_op_row;

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

fn complex_arg(c: &RComplexData) -> Result<f64, Signal> {
    Ok(comp_to_f64(&c.imag)?.atan2(comp_to_f64(&c.real)?))
}

/// `Complex#abs`, preserving CRuby's component-class rule: when one component
/// is numerically zero the magnitude is `|other|` in that component's own
/// class -- Integer only when BOTH components are Integer-classed (so
/// `Complex(0, 2).abs` -> `2`, but `Complex(2, 0.0).abs` -> `2.0`). With both
/// components non-zero it is the ordinary Float hypotenuse.
///
/// A non-native component takes `rb_complex_abs`'s own shape, which is the
/// same rule reached by dispatch: `Complex(0, obj).abs` is `obj.abs`.
fn complex_abs_value(c: &RComplexData) -> Result<RubyValue, Signal> {
    if !is_native_component(&c.real) || !is_native_component(&c.imag) {
        let (re_float, im_float) = (
            matches!(c.real, RubyValue::Float(_)),
            matches!(c.imag, RubyValue::Float(_)),
        );
        if comp_zero(&c.real)? {
            let a = comp_abs(&c.imag)?;
            return if re_float && !im_float {
                Ok(RubyValue::Float(comp_to_f64(&a)?))
            } else {
                Ok(a)
            };
        }
        if comp_zero(&c.imag)? {
            let a = comp_abs(&c.real)?;
            return if im_float && !re_float {
                Ok(RubyValue::Float(comp_to_f64(&a)?))
            } else {
                Ok(a)
            };
        }
        return Ok(RubyValue::Float(
            comp_to_f64(&c.real)?.hypot(comp_to_f64(&c.imag)?),
        ));
    }
    Ok(native_abs_value(c))
}

fn native_abs_value(c: &RComplexData) -> RubyValue {
    use crate::builtins::numeric::num_to_f64_unchecked;
    let re_zero = num_to_f64_unchecked(&c.real) == 0.0;
    let im_zero = num_to_f64_unchecked(&c.imag) == 0.0;
    if !re_zero && !im_zero {
        return RubyValue::Float(abs_f64(c));
    }
    let both_int = matches!(c.real, RubyValue::Int(_) | RubyValue::BigInt(_))
        && matches!(c.imag, RubyValue::Int(_) | RubyValue::BigInt(_));
    // The magnitude is the non-zero component's absolute value (either, when
    // both are zero -- `|0|` is `0`).
    let other = if im_zero { &c.real } else { &c.imag };
    if both_int {
        return match other {
            RubyValue::Int(i) => RubyValue::Int(i.abs()),
            RubyValue::BigInt(b) => {
                let val = (**b).clone();
                let abs = if num_to_f64_unchecked(other) < 0.0 {
                    -val
                } else {
                    val
                };
                RubyValue::BigInt(std::sync::Arc::new(abs))
            }
            _ => RubyValue::Float(num_to_f64_unchecked(other).abs()),
        };
    }
    RubyValue::Float(num_to_f64_unchecked(other).abs())
}

ruby_class! {
    Complex = zeo_abi::COMPLEX_CLASS < zeo_abi::NUMERIC_CLASS;

    // `Complex::I` -- the imaginary unit, `Complex(0, 1)`. Colocated here.
    const I = complex_from_literal(RubyValue::Int(1));

    // `Complex.rect(real, imag = 0)` / `.rectangular(...)`: the direct
    // cartesian constructor (the class-method mirror of the `Complex(...)`
    // Kernel form). `Complex.polar` is a separate gap (its CRuby type-exact
    // trig is tracked with the numeric-exactness work).
    def self."rect" | "rectangular" cfunc (_recv, arg1, arg2?) {
        let real = real_check(arg1)?;
        let imag = match arg2 {
            Some(v) => real_check(v)?,
            None => RubyValue::Int(0),
        };
        complex_new(real, imag)
    }
    // `Complex.polar(abs, arg = 0)`: the polar constructor -- `abs * (cos arg
    // + i sin arg)`, computed by the shared `complex_new_polar` the string
    // parser already uses for the `r@theta` form.
    def self."polar" cfunc (_recv, arg1, arg2?) {
        let mag = (*arg1).clone();
        let angle = arg2.cloned().unwrap_or(RubyValue::Int(0));
        complex_new_polar(mag, angle)
    }

    def "+" (recv, other) { num_op_row!(other, recv, num_add, "+") }
    def "-" (recv, other) { num_op_row!(other, recv, num_sub, "-") }
    def "*" (recv, other) { num_op_row!(other, recv, num_mul, "*") }
    def "/" (recv, other) { num_op_row!(other, recv, num_div, "/") }
    def "**" (recv, other) { num_op_row!(other, recv, num_pow, "**") }
    // CRuby's `rb_complex_uminus` negates each COMPONENT (`f_negate`), which
    // is not `0 - self`: the zero would have to coerce.
    def "-@" (recv) {
        let c = recv_complex(recv);
        complex_new(comp_negate(&c.real)?, comp_negate(&c.imag)?)
    }
    def "+@" (recv) {
        Ok(recv.clone())
    }
    def "==" (recv, other) {
        Ok(RubyValue::Bool(recv.rb_eq(other)))
    }
    def "real" (recv) {
        Ok(recv_complex(recv).real.clone())
    }
    def "imag" | "imaginary" (recv) {
        Ok(recv_complex(recv).imag.clone())
    }
    def "real?" (recv) {
        let _ = recv;
        Ok(RubyValue::Bool(false))
    }
    def "abs" | "magnitude" (recv) {
        complex_abs_value(recv_complex(recv))
    }
    def "abs2" (recv) {
        let c = recv_complex(recv);
        comp_add(
            &comp_mul(&c.real, &c.real)?,
            &comp_mul(&c.imag, &c.imag)?,
        )
    }
    def "arg" | "angle" | "phase" (recv) {
        Ok(RubyValue::Float(complex_arg(recv_complex(recv))?))
    }
    def "polar" (recv) {
        let c = recv_complex(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            complex_abs_value(c)?,
            RubyValue::Float(complex_arg(c)?),
        ])))
    }
    def "rect" | "rectangular" (recv) {
        let c = recv_complex(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            c.real.clone(),
            c.imag.clone(),
        ])))
    }
    def "conj" | "conjugate" (recv) {
        let c = recv_complex(recv);
        let neg_imag = comp_negate(&c.imag)?;
        complex_new(c.real.clone(), neg_imag)
    }
    def "to_c" (recv) {
        Ok(recv.clone())
    }
    // A Complex is finite iff both components are (only a Float component can
    // be infinite/NaN); a real Complex reports `nil` from `infinite?`.
    def "finite?" (recv) {
        let c = recv_complex(recv);
        Ok(RubyValue::Bool(
            component_finite(&c.real) && component_finite(&c.imag),
        ))
    }
    def "infinite?" (recv) {
        let c = recv_complex(recv);
        Ok(if component_finite(&c.real) && component_finite(&c.imag) {
            RubyValue::Nil
        } else {
            RubyValue::Int(1)
        })
    }
    // `coerce(other)`: lift a real numeric to `Complex(other, 0)`, pass a
    // Complex through unchanged; the result is `[coerced_other, self]`.
    def "coerce" (recv, arg) {
        let other = match arg {
            RubyValue::Complex(_) => (*arg).clone(),
            v if is_component(v) => complex_new(v.clone(), RubyValue::Int(0))?,
            other => {
                return Err(type_error!("{} can't be coerced into Complex",
                        crate::builtins::class_name_of(other)))
            }
        };
        Ok(RubyValue::Array(crate::array_new(vec![other, recv.clone()])))
    }
    // The real-projection conversions raise CRuby's RangeError unless the
    // imaginary part is an exact zero (an Integer or Rational `0`; a Float
    // `0.0` still raises).
    def "to_f" (recv) {
        real_projection(recv, "to_f")
    }
    def "to_i" | "to_int" (recv) {
        real_projection(recv, "to_i")
    }
    def "to_r" (recv) {
        real_projection(recv, "to_r")
    }
    def "rationalize"(recv, _arg?) {
        real_projection(recv, "to_r")
    }
    // `denominator` = lcm of the two components' denominators; `numerator`
    // scales both components up to that shared denominator (CRuby complex.c).
    def "denominator" (recv) {
        Ok(crate::builtins::integer::int_value(complex_denominator(recv_complex(recv))?))
    }
    def "numerator" (recv) {
        let c = recv_complex(recv);
        let cd = complex_denominator(c)?;
        let real = scale_numerator(&c.real, &cd)?;
        let imag = scale_numerator(&c.imag, &cd)?;
        complex_new(real, imag)
    }
    // `Complex#<=>`: only real-valued complexes are ordered. If self's
    // imaginary part is zero and the operand is real (a real-valued Complex or
    // a plain real Numeric), compare the real parts; otherwise nil.
    def "<=>" (recv, other) {
        let c = recv_complex(recv);
        if !is_exact_zero(&c.imag) {
            return Ok(RubyValue::Nil);
        }
        let other_real = match other {
            RubyValue::Complex(o) if is_exact_zero(&o.imag) => o.real.clone(),
            RubyValue::Complex(_) => return Ok(RubyValue::Nil),
            v if is_component(v) => v.clone(),
            _ => return Ok(RubyValue::Nil),
        };
        Ok(match c.real.rb_cmp(&other_real) {
            Some(n) => RubyValue::Int(n),
            None => RubyValue::Nil,
        })
    }
    // `Complex#fdiv(other)` -- complex division carried out in floating point,
    // so `(2+3i).fdiv(2)` is `(1.0+1.5i)` (each component divided) and a
    // Complex divisor gets the full `(ar*br+ai*bi + (ai*br-ar*bi)i)/|b|^2`.
    def "fdiv" (recv, arg) {
        if !matches!(arg, RubyValue::Complex(_)) && !is_component(arg) {
            // Routed through the GENERIC numeric machinery in CRuby, so the
            // operand reads in inspect form (`nil`), unlike `coerce`'s own
            // class-named raise above -- oracle-verified both ways.
            return Err(type_error!(
                "{} can't be coerced into Complex",
                crate::builtins::coerce_operand_name(arg)
            ));
        }
        let c = recv_complex(recv);
        let (ar, ai) = (comp_to_f64(&c.real)?, comp_to_f64(&c.imag)?);
        let (br, bi) = as_components(arg);
        let (br, bi) = (comp_to_f64(&br)?, comp_to_f64(&bi)?);
        let denom = br * br + bi * bi;
        complex_new(
            RubyValue::Float((ar * br + ai * bi) / denom),
            RubyValue::Float((ai * br - ar * bi) / denom),
        )
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "eql?"(recv, _other) { inherited_row!(kernel, "eql?", recv, __args, None) }
    def "hash"(recv) { inherited_row!(kernel, "hash", recv, __args, None) }
    def "inspect"(recv) { inherited_row!(kernel, "inspect", recv, __args, None) }
    // NOT an alias of `#inspect`: `Complex`, `Rational` and `Regexp` all
    // spell the two differently, so each goes to its own Kernel row.
    def "to_s"(recv) { inherited_row!(kernel, "to_s", recv, __args, None) }
    def "quo"(recv, _other) { inherited_row!(numeric, "quo", recv, __args, None) }
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
        return Err(range_error!(
            "can't convert {} into {}",
            recv.to_display_string(),
            conv_target(conv)
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
