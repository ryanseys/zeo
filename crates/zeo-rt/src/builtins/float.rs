//! `Float` (CRuby numeric.c) -- the operator/comparison rows, each driving
//! the one tower matrix in `numeric.rs` (a Float receiver joins any real
//! operand in the Float lane; `Complex` operands lift higher). Ordering
//! (`<`/`>`/...) comes from Comparable driving `<=>` -- Float's chain runs
//! `[Float, Numeric, Comparable, ...]`. The Tier A breadth
//! (`nan?`/`round(n)`/`to_r`/...) lands with stage C's generics pass.

use crate::builtins::{arg_error, inherited_row, type_error};
use crate::{RubyValue, Signal};
use zeo_macros::ruby_class;

use crate::builtins::numeric::num_op_row;

ruby_class! {
    Float = zeo_abi::FLOAT_CLASS < zeo_abi::NUMERIC_CLASS;

    // The IEEE-754 double constants, colocated here. Values are CRuby's exactly.
    const INFINITY = RubyValue::Float(f64::INFINITY);
    const NAN = RubyValue::Float(f64::NAN);
    const EPSILON = RubyValue::Float(f64::EPSILON);
    const MAX = RubyValue::Float(f64::MAX);
    const MIN = RubyValue::Float(f64::MIN_POSITIVE);
    const DIG = RubyValue::Int(15);
    const MANT_DIG = RubyValue::Int(53);
    const MAX_EXP = RubyValue::Int(1024);
    const MIN_EXP = RubyValue::Int(-1021);
    const MAX_10_EXP = RubyValue::Int(308);
    const MIN_10_EXP = RubyValue::Int(-307);
    const RADIX = RubyValue::Int(2);

    def "+" (recv, other) { num_op_row!(other, recv, num_add, "+") }
    def "-" (recv, other) { num_op_row!(other, recv, num_sub, "-") }
    def "*" (recv, other) { num_op_row!(other, recv, num_mul, "*") }
    def "/" (recv, other) { num_op_row!(other, recv, num_div, "/") }
    def "%" | "modulo" (recv, other) { num_op_row!(other, recv, num_mod, "%") }
    def "**" (recv, other) { num_op_row!(other, recv, num_pow, "**") }
    def "-@" (recv) {
        match recv {
            RubyValue::Float(f) => Ok(RubyValue::Float(-f)),
            _ => unreachable!("Float table row dispatched on a non-Float receiver"),
        }
    }
    // No `+@` row: ruby owns it on Numeric, whose body is the same `self`.
    def "<=>" (recv, other) {
        Ok(match crate::builtins::numeric::num_cmp(recv, other) {
            Some(Some(c)) => RubyValue::Int(c),
            Some(None) => RubyValue::Nil,
            // Outside the native tower: the coerce protocol decides
            // (`1.5 <=> BigDecimal("2")`), CRuby's rb_num_coerce_cmp.
            None => crate::builtins::numeric::coerce_cmp(recv, other)?,
        })
    }
    def "==" (recv, other) {
        if recv.rb_eq(other) {
            return Ok(RubyValue::Bool(true));
        }
        // A non-tower operand answers for itself (`y == x`), CRuby's
        // num_equal -- how `1.5 == BigDecimal("1.5")` holds.
        crate::builtins::numeric::reverse_eq(recv, other)
    }
    def "abs" | "magnitude" (recv) {
        Ok(RubyValue::Float(recv_f64(recv).abs()))
    }
    def "nan?" (recv) {
        Ok(RubyValue::Bool(recv_f64(recv).is_nan()))
    }
    def "finite?" (recv) {
        Ok(RubyValue::Bool(recv_f64(recv).is_finite()))
    }
    // 1 / -1 / nil, real Ruby's exact shape.
    def "infinite?" (recv) {
        let f = recv_f64(recv);
        Ok(if f == f64::INFINITY {
            RubyValue::Int(1)
        } else if f == f64::NEG_INFINITY {
            RubyValue::Int(-1)
        } else {
            RubyValue::Nil
        })
    }
    // The adjacent representable doubles toward +/-infinity.
    def "next_float" (recv) {
        Ok(RubyValue::Float(recv_f64(recv).next_up()))
    }
    def "prev_float" (recv) {
        Ok(RubyValue::Float(recv_f64(recv).next_down()))
    }
    // `coerce(other)` promotes both operands to Float (`[Float(other), self]`).
    def "coerce" (recv, arg) {
        // Ruby's `num_coerce` is literally `[Float(y), Float(x)]`, so EVERY
        // argument goes through `Float()` and every failure is its own --
        // naming the VALUE and `Float` (`can't convert nil into Float`),
        // not the "can't coerce" TypeError an arithmetic operator gives.
        let other = match crate::builtins::kernel::float_impl(std::slice::from_ref(arg))? {
            RubyValue::Float(f) => f,
            other => crate::builtins::numeric::num_to_f64_unchecked(&other),
        };
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Float(other),
            recv.clone(),
        ])))
    }
    // No `div` / `i` rows: ruby owns both on Numeric. `div` there is
    // `(self / other).floor`, which for a Float receiver runs this class's own
    // `/` and the quotient's own `floor` -- the same answer this body computed.
    def "to_f" (recv) {
        Ok(recv.clone())
    }
    def "to_i" | "to_int" (recv) {
        float_to_integer(recv_f64(recv).trunc())
    }
    // EXACT: every finite double is a dyadic rational (mantissa * 2^exp).
    def "to_r" (recv) {
        float_to_rational(recv_f64(recv))
    }
    // `rationalize([eps])` -- the SIMPLEST rational within half a ULP of this
    // double (no arg), or within `eps` (with arg). Port of CRuby's
    // `float_rationalize` (numeric.c) + `nurat_rationalize_internal`.
    def "rationalize" (recv, arg?) {
        float_rationalize(recv_f64(recv), arg)
    }
    def "numerator" (recv) {
        let f = recv_f64(recv);
        // Infinity/NaN have no rational form, so CRuby skips the conversion
        // and returns the float itself (its denominator is 1) rather than
        // raising FloatDomainError.
        if !f.is_finite() {
            return Ok(RubyValue::Float(f));
        }
        match float_to_rational(f)? {
            RubyValue::Rational(r) => Ok(crate::builtins::integer::int_value(r.num.clone())),
            other => Ok(other),
        }
    }
    def "denominator" (recv) {
        let f = recv_f64(recv);
        if !f.is_finite() {
            return Ok(RubyValue::Int(1));
        }
        match float_to_rational(f)? {
            RubyValue::Rational(r) => Ok(crate::builtins::integer::int_value(r.den.clone())),
            _ => Ok(RubyValue::Int(1)),
        }
    }
    // The rounding family: ndigits <= 0 produces an Integer, > 0 a Float.
    // CRuby's shape, compensation for compensation -- see `round_half` and
    // `float_round_family`; a naive `op(x * 10**n) / 10**n` reads the scaled
    // product's own representation error as part of the value.
    // A `half:` keyword selects the tie-break mode (:up default).
    def "round" (recv, ndigits?, **opts) {
        let mode = round_half_mode(opts)?;
        float_round_family(recv, ndigits, RoundOp::Round(mode))
    }
    def "floor" (recv, ndigits?) {
        float_round_family(recv, ndigits, RoundOp::Floor)
    }
    def "ceil" (recv, ndigits?) {
        float_round_family(recv, ndigits, RoundOp::Ceil)
    }
    // `truncate` is `floor` for a positive value and `ceil` for a negative
    // one -- CRuby's `flo_truncate`, which reads the SIGN BIT, so `-0.0`
    // takes the ceil path and answers positive zero the way ruby does.
    def "truncate" (recv, ndigits?) {
        let op = if recv_f64(recv).is_sign_negative() { RoundOp::Ceil } else { RoundOp::Floor };
        float_round_family(recv, ndigits, op)
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    // `< <= > >=` between an Integer and a Float answer FALSE for an
    // incomparable pair (a NaN) where Comparable RAISES -- CRuby hand-writes
    // these rows for exactly that. Every other operand keeps Comparable's
    // body, and with it ruby's `comparison of X with Y failed`.
    def "<"(recv, other) {
        match crate::builtins::numeric::int_float_relop(recv, other, |o| o < 0) {
            Some(b) => Ok(RubyValue::Bool(b)),
            None => inherited_row!(comparable, "<", recv, __args, None),
        }
    }
    def "<="(recv, other) {
        match crate::builtins::numeric::int_float_relop(recv, other, |o| o <= 0) {
            Some(b) => Ok(RubyValue::Bool(b)),
            None => inherited_row!(comparable, "<=", recv, __args, None),
        }
    }
    def ">"(recv, other) {
        match crate::builtins::numeric::int_float_relop(recv, other, |o| o > 0) {
            Some(b) => Ok(RubyValue::Bool(b)),
            None => inherited_row!(comparable, ">", recv, __args, None),
        }
    }
    def ">="(recv, other) {
        match crate::builtins::numeric::int_float_relop(recv, other, |o| o >= 0) {
            Some(b) => Ok(RubyValue::Bool(b)),
            None => inherited_row!(comparable, ">=", recv, __args, None),
        }
    }
    def "==="(recv, _other) { inherited_row!(kernel, "===", recv, __args, None) }
    def "eql?"(recv, _other) { inherited_row!(kernel, "eql?", recv, __args, None) }
    def "hash"(recv) { inherited_row!(kernel, "hash", recv, __args, None) }
    def "inspect"(recv) { inherited_row!(kernel, "inspect", recv, __args, None) }
    // NOT an alias of `#inspect`: `Complex`, `Rational` and `Regexp` all
    // spell the two differently, so each goes to its own Kernel row.
    def "to_s"(recv) { inherited_row!(kernel, "to_s", recv, __args, None) }
    def "angle" | "arg" | "phase"(recv) { inherited_row!(numeric, "angle", recv, __args, None) }
    def "divmod"(recv, _other) { inherited_row!(numeric, "divmod", recv, __args, None) }
    def "fdiv"(recv, _other) { inherited_row!(numeric, "fdiv", recv, __args, None) }
    def "quo"(recv, _other) { inherited_row!(numeric, "quo", recv, __args, None) }
    def "negative?"(recv) { inherited_row!(numeric, "negative?", recv, __args, None) }
    def "positive?"(recv) { inherited_row!(numeric, "positive?", recv, __args, None) }
    def "zero?"(recv) { inherited_row!(numeric, "zero?", recv, __args, None) }
}

fn recv_f64(recv: &RubyValue) -> f64 {
    match recv {
        RubyValue::Float(f) => *f,
        _ => unreachable!("Float table row dispatched on a non-Float receiver"),
    }
}

/// A finite float's integral value as an Integer (bignum-capable:
/// `1e20.to_i` works); non-finite raises real Ruby's FloatDomainError.
pub(crate) fn float_to_integer(f: f64) -> Result<RubyValue, Signal> {
    use num_traits::FromPrimitive;
    if !f.is_finite() {
        return Err(crate::builtins::float_domain_error!(
            "{}",
            RubyValue::Float(f).to_display_string()
        ));
    }
    Ok(crate::builtins::integer::int_value(
        num_bigint::BigInt::from_f64(f).expect("finite float"),
    ))
}

/// A finite double's EXACT value as `(num, den)`, `den` a power of two --
/// the dyadic decomposition `f == mantissa * 2^exp` every exact reading of a
/// Float goes through. Shared with `Time`, whose sub-second fields are
/// oracle-exact rather than recomputed in floating point (`Time.at(0.7).nsec`
/// is 699999999, because that IS what the double holds; doing the arithmetic
/// in f64 rounds it back to 700000000 and hides the very thing Ruby shows).
pub(crate) fn float_exact_parts(f: f64) -> (num_bigint::BigInt, num_bigint::BigInt) {
    use num_bigint::BigInt;
    let bits = f.to_bits();
    let sign: i64 = if bits >> 63 == 0 { 1 } else { -1 };
    let exponent = ((bits >> 52) & 0x7ff) as i64;
    let mantissa = if exponent == 0 {
        (bits & 0xf_ffff_ffff_ffff) << 1
    } else {
        (bits & 0xf_ffff_ffff_ffff) | 0x10_0000_0000_0000
    };
    let exp = exponent - 1075;
    let num = BigInt::from(sign) * BigInt::from(mantissa);
    if exp >= 0 {
        (num << exp as usize, BigInt::from(1))
    } else {
        (num, BigInt::from(1) << (-exp) as usize)
    }
}

/// `Float#to_r`'s exact dyadic decomposition: f == mantissa * 2^exp.
fn float_to_rational(f: f64) -> Result<RubyValue, Signal> {
    if !f.is_finite() {
        return Err(crate::builtins::float_domain_error!(
            "{}",
            RubyValue::Float(f).to_display_string()
        ));
    }
    let (num, den) = float_exact_parts(f);
    crate::builtins::rational::rational_new(num, den)
}

/// The `(significand, exponent)` of `|d|` such that `|d| == significand *
/// 2^exp` -- the `frexp`/`ldexp` decomposition CRuby's `float_decode_internal`
/// produces (`significand` is the raw 53-bit mantissa, sign dropped).
fn frexp_parts(ad: f64) -> (num_bigint::BigInt, i64) {
    use num_bigint::BigInt;
    let bits = ad.to_bits();
    let exp_field = ((bits >> 52) & 0x7ff) as i64;
    let mant = bits & 0xf_ffff_ffff_ffff;
    if exp_field == 0 {
        // Subnormal: no implicit leading bit, fixed exponent.
        (BigInt::from(mant), -1074)
    } else {
        (BigInt::from(mant | 0x10_0000_0000_0000), exp_field - 1075)
    }
}

/// The simplest rational `p/q` in the CLOSED interval `[a, b]` (`a <= b`, each
/// a `(num, den)` pair with `den > 0`) -- CRuby's `nurat_rationalize_internal`
/// continued-fraction (Stern-Brocot) search.
fn simplest_between(
    mut a: (num_bigint::BigInt, num_bigint::BigInt),
    mut b: (num_bigint::BigInt, num_bigint::BigInt),
) -> (num_bigint::BigInt, num_bigint::BigInt) {
    use num_bigint::BigInt;
    use num_integer::Integer as _;
    use num_traits::{One, Zero};
    // `d/n` normalized to a positive denominator.
    fn recip(n: BigInt, d: BigInt) -> (BigInt, BigInt) {
        if n.sign() == num_bigint::Sign::Minus {
            (-d, -n)
        } else {
            (d, n)
        }
    }
    let (mut p0, mut p1) = (BigInt::zero(), BigInt::one());
    let (mut q0, mut q1) = (BigInt::one(), BigInt::zero());
    loop {
        let c = a.0.div_ceil(&a.1);
        // Break once `ceil(a) < b`; the result uses this `c`.
        if &c * &b.1 < b.0 {
            return (&c * &p1 + &p0, &c * &q1 + &q0);
        }
        let k = &c - 1;
        let p2 = &k * &p1 + &p0;
        let q2 = &k * &q1 + &q0;
        let t = recip(&b.0 - &k * &b.1, b.1.clone());
        b = recip(&a.0 - &k * &a.1, a.1.clone());
        a = t;
        (p0, q0, p1, q1) = (p1, q1, p2, q2);
    }
}

/// `|eps|` as an exact `(num, den)` rational (`den > 0`), for the eps form.
fn exact_abs_rational(v: &RubyValue) -> Result<(num_bigint::BigInt, num_bigint::BigInt), Signal> {
    use num_bigint::BigInt;
    use num_traits::{One, Signed};
    Ok(match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) => {
            (crate::builtins::integer::to_bigint(v).abs(), BigInt::one())
        }
        RubyValue::Float(f) => float_exact_parts(f.abs()),
        RubyValue::Rational(r) => (r.num.abs(), r.den.clone()),
        other => {
            return Err(type_error!(
                "can't convert {} into Float",
                crate::builtins::convert_name_of(other)
            ));
        }
    })
}

/// The full `Float#rationalize` body.
fn float_rationalize(d: f64, eps: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    use num_bigint::BigInt;
    use num_traits::{One, Zero};
    if !d.is_finite() {
        return Err(crate::builtins::float_domain_error!(
            "{}",
            RubyValue::Float(d).to_display_string()
        ));
    }
    let neg = d < 0.0;
    let ad = d.abs();
    let (p, q) = match eps {
        // `[|d| - |eps|, |d| + |eps|]` over exact rationals.
        Some(e) => {
            let (fn_, fd) = float_exact_parts(ad);
            let (en, ed) = exact_abs_rational(e)?;
            // a = f - e, b = f + e over the common denominator fd*ed.
            let den = &fd * &ed;
            let lo = &fn_ * &ed - &en * &fd;
            let hi = &fn_ * &ed + &en * &fd;
            if lo == hi {
                // eps == 0: fall back to the exact dyadic value.
                float_exact_parts(ad)
            } else {
                simplest_between((lo, den.clone()), (hi, den))
            }
        }
        // Half-ULP interval `[(2f-1)/2^(1-n), (2f+1)/2^(1-n)]`.
        None => {
            let (f, n) = frexp_parts(ad);
            if f.is_zero() || n >= 0 {
                let val = if n >= 0 {
                    f << (n as usize)
                } else {
                    BigInt::zero()
                };
                (val, BigInt::one())
            } else {
                let two_f = &f * 2;
                let den = BigInt::one() << ((1 - n) as usize);
                simplest_between((&two_f - 1, den.clone()), (&two_f + 1, den))
            }
        }
    };
    crate::builtins::rational::rational_new(if neg { -p } else { p }, q)
}

use crate::builtins::integer::{HalfMode, RoundMode as IntRoundMode};

/// `frexp`'s exponent -- the `e` in `x = m * 2**e` with `0.5 <= |m| < 1`.
/// CRuby's rounding family reads it to decide whether a scale is reachable
/// at all, so the two have to agree exactly; read off the bits rather than
/// through `log2`, which is not exact at the powers of two.
fn frexp_exp(x: f64) -> i32 {
    let bits = x.to_bits();
    let raw = ((bits >> 52) & 0x7ff) as i32;
    if raw != 0 {
        return raw - 1022;
    }
    let mantissa = bits & ((1u64 << 52) - 1);
    if mantissa == 0 {
        0
    } else {
        -1010 - mantissa.leading_zeros() as i32
    }
}

/// CRuby's `float_round_overflow`: at this many digits the scaled value is
/// already an integer, so the answer is the number itself.
fn float_round_overflow(ndigits: i64, binexp: i32) -> bool {
    const FLOAT_DIG: i64 = 15 + 2; // DBL_DIG + 2
    let exp = i64::from(if binexp > 0 {
        binexp / 4
    } else {
        binexp / 3 - 1
    });
    ndigits >= FLOAT_DIG - exp
}

/// CRuby's `float_round_underflow`: the place asked for is so far above the
/// value that nothing of it survives.
fn float_round_underflow(ndigits: i64, binexp: i32) -> bool {
    let exp = i64::from(if binexp > 0 {
        binexp / 3 + 1
    } else {
        binexp / 4
    });
    ndigits < -exp
}

/// CRuby's `ACCURATE_POW10`: past this a `10**n` scale is not exact as a
/// double and the answer comes from exact rational arithmetic instead.
fn accurate_pow10(ndigits: i64) -> bool {
    ndigits < 15 // DBL_DIG
}

/// Round `x` to the nearest integer of the `1/s` grid, breaking an exact
/// `.5` tie per `mode` -- CRuby's `round_half_up`/`_down`/`_even`, verbatim.
///
/// The `s` is not decoration. Scaling first and rounding after reads the
/// SCALED product's own representation error as part of the value:
/// `1.005 * 100` is `100.49999999999999`, so the digit that should round up
/// rounds down. Each form asks whether the half-way point of the answer it
/// is about to give still lies on the near side of `x` -- `(f + 0.5) / s
/// <= x` -- and steps once when it does. That question is asked in the
/// UNSCALED domain, where the original value is exact.
fn round_half(x: f64, s: f64, mode: HalfMode) -> f64 {
    match mode {
        HalfMode::Up => round_half_up(x, s),
        HalfMode::Down => round_half_down(x, s),
        HalfMode::Even => round_half_even(x, s),
    }
}

fn round_half_up(x: f64, s: f64) -> f64 {
    let mut f = (x * s).round();
    if s == 1.0 {
        return f;
    }
    if x > 0.0 {
        if (f + 0.5) / s <= x {
            f += 1.0;
        }
    } else if (f - 0.5) / s >= x {
        f -= 1.0;
    }
    f
}

fn round_half_down(x: f64, s: f64) -> f64 {
    let mut f = (x * s).round();
    if x > 0.0 {
        if (f - 0.5) / s >= x {
            f -= 1.0;
        }
    } else if (f + 0.5) / s <= x {
        f += 1.0;
    }
    f
}

fn round_half_even(x: f64, s: f64) -> f64 {
    let u = x.trunc();
    let v = x - u;
    let us = u * s;
    let vs = v * s;
    let mut r = x;
    if x > 0.0 {
        let f = vs.floor();
        let uf = us + f;
        let d = vs - f;
        let d = if d > 0.5 {
            1.0
        } else if d == 0.5 || (uf + 0.5) / s <= x {
            uf % 2.0
        } else {
            0.0
        };
        r = f + d;
    } else if x < 0.0 {
        let f = vs.ceil();
        let uf = us + f;
        let d = f - vs;
        let d = if d > 0.5 {
            1.0
        } else if d == 0.5 || (uf - 0.5) / s >= x {
            (-uf) % 2.0
        } else {
            0.0
        };
        r = f - d;
    }
    us + r
}

/// Split a trailing `half:` keyword Hash off `round`'s arguments, returning the
/// positional slice and the selected mode (`:up` when absent).
fn round_half_mode(opts: Option<&RubyValue>) -> Result<HalfMode, Signal> {
    if let Some(RubyValue::Hash(h)) = opts {
        let mode = match crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("half"))) {
            RubyValue::Nil => HalfMode::Up,
            RubyValue::Symbol(s) => match s.name().as_str() {
                "up" => HalfMode::Up,
                "down" => HalfMode::Down,
                "even" => HalfMode::Even,
                other => {
                    return Err(arg_error!("invalid rounding mode: {other}"));
                }
            },
            other => {
                return Err(arg_error!(
                    "invalid rounding mode: {}",
                    other.to_display_string()
                ));
            }
        };
        return Ok(mode);
    }
    Ok(HalfMode::Up)
}

/// Which of the four rounding rows is asking. They share the argument
/// conversion and the `ndigits <= 0` half; the positive-digit half differs
/// per row, because CRuby compensates each one differently on purpose.
#[derive(Clone, Copy)]
enum RoundOp {
    Round(HalfMode),
    Floor,
    Ceil,
}

impl RoundOp {
    /// The row's answer for a whole number of digits (`ndigits <= 0`).
    fn to_integer_op(self) -> fn(f64) -> f64 {
        match self {
            // `round`'s own whole-number answer is the half rule's, applied
            // by the caller; this is the `flo_to_i` a NEGATIVE digit count
            // takes before it rounds at the place.
            RoundOp::Round(_) => f64::trunc,
            RoundOp::Floor => f64::floor,
            RoundOp::Ceil => f64::ceil,
        }
    }
}

fn float_round_family(
    recv: &RubyValue,
    ndigits: Option<&RubyValue>,
    op: RoundOp,
) -> Result<RubyValue, Signal> {
    let f = recv_f64(recv);
    // `NUM2INT`, which is what CRuby's rounding family reads the digit
    // count with -- so a non-numeric one is "no implicit conversion",
    // never the coercion failure an ARITHMETIC operand would give.
    let ndigits = match ndigits {
        Some(v) => crate::builtins::convert::to_index(v)?,
        None => 0,
    };
    // Zero keeps its SIGN as a Float and loses it as an Integer.
    if f == 0.0 {
        return if ndigits > 0 {
            Ok(RubyValue::Float(f))
        } else {
            float_to_integer(0.0)
        };
    }
    if ndigits > 0 {
        if !f.is_finite() {
            return Ok(RubyValue::Float(f));
        }
        let binexp = frexp_exp(f);
        // More digits than the value carries: the answer is the value.
        if float_round_overflow(ndigits, binexp) {
            return Ok(RubyValue::Float(f));
        }
        // Below the value's own magnitude. `round` collapses from either
        // side; `floor` only from above and `ceil` only from below, and each
        // answers POSITIVE zero.
        let underflows = float_round_underflow(ndigits, binexp)
            && match op {
                RoundOp::Round(_) => true,
                RoundOp::Floor => f > 0.0,
                RoundOp::Ceil => f < 0.0,
            };
        if underflows {
            return Ok(RubyValue::Float(0.0));
        }
        if !accurate_pow10(ndigits) {
            return round_by_rational(f, ndigits, op);
        }
        let s = 10f64.powi(ndigits as i32);
        let scaled = match op {
            RoundOp::Round(mode) => return Ok(RubyValue::Float(round_half(f, s, mode) / s)),
            // `floor` is the one row that compensates: `1.005 * 1000` is
            // `1004.9999999999999`, so the naive `floor(x * s) / s` loses a
            // decimal. CRuby asks whether the NEXT grid point is still at or
            // below `x` and takes it when it is.
            RoundOp::Floor => {
                let mul = (f * s).floor();
                let res = (mul + 1.0) / s;
                if res > f { mul / s } else { res }
            }
            RoundOp::Ceil => (f * s).ceil() / s,
        };
        return Ok(RubyValue::Float(scaled));
    }
    let to_int = op.to_integer_op();
    if ndigits == 0 {
        return match op {
            RoundOp::Round(mode) => float_to_integer(round_half(f, 1.0, mode)),
            _ => float_to_integer(to_int(f)),
        };
    }
    // Negative `ndigits`: CRuby converts to an Integer FIRST and rounds
    // there (`rb_int_round`/`_floor`/`_ceil`). Scaling in doubles instead
    // rounds the VALUE before the place is reached, so `1e300.floor(-3)`
    // came out with a different tail than ruby's.
    let whole = float_to_integer(to_int(f))?;
    let (mode, half) = match op {
        RoundOp::Round(m) => (IntRoundMode::HalfAway, m),
        RoundOp::Floor => (IntRoundMode::Floor, HalfMode::Up),
        RoundOp::Ceil => (IntRoundMode::Ceil, HalfMode::Up),
    };
    crate::builtins::integer::int_round_family(&whole, Some(&RubyValue::Int(ndigits)), mode, half)
}

/// The positive-digit answer over EXACT rationals, for the digit counts where
/// `10**n` is no longer an exact double (CRuby's `rb_flo_*_by_rational`).
/// `x` becomes its exact dyadic rational, the grid point is chosen there, and
/// only the last division is a float.
fn round_by_rational(x: f64, ndigits: i64, op: RoundOp) -> Result<RubyValue, Signal> {
    use num_bigint::BigInt;
    use num_traits::{Signed, Zero};
    let neg = x < 0.0;
    let (p, q) = float_exact_parts(x.abs());
    let p = if neg { -p } else { p };
    let scale = BigInt::from(10u32).pow(ndigits as u32);
    // `n / d` is `x * 10**ndigits`, exactly.
    let n = p * &scale;
    let d = q;
    let (quot, rem) = (&n / &d, &n % &d);
    // `quot` truncates toward zero, so the floor is one lower for a negative
    // remainder; every other grid point is named relative to that floor.
    let floor = if rem.is_negative() {
        &quot - 1
    } else {
        quot.clone()
    };
    let exact = rem.is_zero();
    let k = match op {
        _ if exact => floor.clone(),
        RoundOp::Floor => floor,
        RoundOp::Ceil => floor + 1,
        RoundOp::Round(mode) => {
            // Compare the fractional part against a half by doubling.
            let frac2: BigInt = (&n - &floor * &d) * 2;
            let up = match frac2.cmp(&d) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Less => false,
                std::cmp::Ordering::Equal => match mode {
                    HalfMode::Up => !neg,
                    HalfMode::Down => neg,
                    HalfMode::Even => (&floor % 2u32) != BigInt::zero(),
                },
            };
            if up { floor + 1 } else { floor }
        }
    };
    let v = crate::builtins::rational::rational_new(k, scale)?;
    Ok(RubyValue::Float(
        crate::builtins::numeric::num_to_f64_unchecked(&v),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Float rows are `ruby_class!`-generated (mangled Rust fn names), so
    /// reach them the way dispatch does -- through the registered table.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::FLOAT_CLASS)
            .expect("Float is a registered builtin table")
            .instance
            .as_ref()
            .expect("Float has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Float#{name} is defined"))
    }

    #[test]
    fn nan_compares_as_nil() {
        let r = imethod("<=>")(&RubyValue::Float(f64::NAN), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(r, RubyValue::Nil));
    }

    #[test]
    fn arithmetic_rows_join_the_float_lane() {
        let r = imethod("+")(&RubyValue::Float(1.5), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f == 2.5));
        let r = imethod("/")(&RubyValue::Float(1.0), &[RubyValue::Int(0)], None).unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f.is_infinite()));
    }

    #[test]
    fn coercion_failures_carry_cruby_shape() {
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            imethod("+")(&RubyValue::Float(1.0), &[s], None)
        }));
        assert!(r.is_err());
    }
}
