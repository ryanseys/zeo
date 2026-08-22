//! `Numeric` (CRuby numeric.c) -- the numeric-tower coercion matrix.
//! CRuby's `rb_num_coerce_bin` lives in numeric.c, and so does ours: every
//! cross-type numeric operation (dynamic rows in `integer.rs`/`float.rs`/
//! `rational.rs`/`complex.rs`, codegen's Poly runtime-checked operator
//! fallback, `rb_eq`/`rb_cmp`'s numeric arms) funnels through ONE `num_*`
//! family here, so the promotion rules exist exactly once.
//!
//! The tower, lowest to highest: Integer < Rational < Float < Complex.
//! A mixed pair computes in the HIGHER operand's lane (`Rational(1,2) +
//! 0.5` is Float; anything with a Complex is Complex; `Rational + Integer`
//! stays exact -- all oracle-verified).
//!
//! `None` = a side isn't numeric at all: the caller decides (operator rows
//! raise CRuby's coercion TypeError; `rb_eq` answers false; codegen's
//! fallback re-dispatches through `send_value`).

use crate::builtins::complex::{cpx_add, cpx_div, cpx_eq, cpx_mul, cpx_pow, cpx_sub};
use crate::builtins::integer::{
    int_add, int_cmp, int_div, int_mod, int_mul, int_pow, int_sub, to_bigint,
};
use crate::builtins::rational::{
    as_ratio, rat_add, rat_cmp, rat_div, rat_mul, rat_pow, rat_sub, rat_to_f64, rational_new,
};
use crate::builtins::{arg_error, inherited_row, type_error};
use crate::{RubyValue, Signal};
use num_traits::ToPrimitive;
use zeo_macros::ruby_class;

/// One binary numeric-operator table row, shared by every numeric class
/// (`Integer`/`Float`/`Rational`/`Complex`): compute the operation via
/// `numeric::<num_fn>` and hand the result to `num_coerce_bin`, which applies
/// `coerce`/`TypeError` against a non-numeric operand. The class isn't a
/// parameter -- `num_coerce_bin` derives the "can't be coerced into <Class>"
/// message from the receiver's own type -- so all four classes' rows were
/// byte-identical; this is the one definition they now share.
macro_rules! num_op_row {
    ($other:ident, $recv:ident, $num_fn:ident, $op:literal) => {{
        crate::builtins::numeric::num_coerce_bin(
            $recv,
            $other,
            crate::builtins::numeric::$num_fn($recv, $other),
            $op,
        )
    }};
}
pub(crate) use num_op_row;

/// The lane a VALUE occupies (`Int` covers both Integer payloads).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NumLane {
    Int,
    Rat,
    Flo,
    Cpx,
}

/// CRuby's `flo_divmod` (numeric.c): floored division and remainder computed
/// together so their signs stay consistent, including the infinite-divisor
/// edge (`mod` takes the dividend when `y` is infinite and `x` finite).
fn flo_divmod(x: f64, y: f64) -> (f64, f64) {
    let mut m = if y.is_infinite() && x.is_finite() {
        x
    } else {
        x % y
    };
    let mut div = if x.is_infinite() && y.is_finite() {
        x
    } else {
        ((x - m) / y).round()
    };
    if y * m < 0.0 {
        m += y;
        div -= 1.0;
    }
    (div, m)
}

fn lane(v: &RubyValue) -> Option<NumLane> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) => Some(NumLane::Int),
        RubyValue::Rational(_) => Some(NumLane::Rat),
        RubyValue::Float(_) => Some(NumLane::Flo),
        RubyValue::Complex(_) => Some(NumLane::Cpx),
        _ => None,
    }
}

/// `f64` view of any real numeric -- the promotion a Float lane takes.
/// Bignum -> f64 is lossy beyond 2^53, the same loss `Integer#to_f` has.
pub fn num_to_f64_unchecked(v: &RubyValue) -> f64 {
    match v {
        RubyValue::Int(i) => *i as f64,
        RubyValue::BigInt(b) => b.to_f64().unwrap_or(f64::INFINITY),
        RubyValue::Float(f) => *f,
        RubyValue::Rational(r) => rat_to_f64(r),
        other => panic!(
            "expected a numeric value, got {}",
            other.to_display_string()
        ),
    }
}

/// One tower binary operation: four lane bodies, dispatched by the HIGHER
/// operand's lane. Generates the shared `Option<Result<..>>` shape every
/// consumer expects (`None` = not a numeric pair).
macro_rules! tower_binop {
    (
        $(#[$doc:meta])*
        $name:ident,
        int($ia:ident, $ib:ident) => $int:expr_2021,
        rat($ra:ident, $rb:ident) => $rat:expr_2021,
        flo($fa:ident, $fb:ident) => $flo:expr_2021,
        cpx($ca:ident, $cb:ident) => $cpx:expr_2021 $(,)?
    ) => {
        $(#[$doc])*
        pub fn $name(a: &RubyValue, b: &RubyValue) -> Option<Result<RubyValue, Signal>> {
            let joined = lane(a)?.max(lane(b)?);
            Some(match joined {
                NumLane::Int => {
                    let ($ia, $ib) = (a, b);
                    $int
                }
                NumLane::Rat => {
                    let ($ra, $rb) = (a, b);
                    $rat
                }
                NumLane::Flo => {
                    let ($fa, $fb) = (num_to_f64_unchecked(a), num_to_f64_unchecked(b));
                    $flo
                }
                NumLane::Cpx => {
                    let ($ca, $cb) = (a, b);
                    $cpx
                }
            })
        }
    };
}

tower_binop!(
    num_add,
    int(x, y) => Ok(int_add(x, y)),
    rat(x, y) => rat_add(x, y),
    flo(x, y) => Ok(RubyValue::Float(x + y)),
    cpx(x, y) => cpx_add(x, y),
);

tower_binop!(
    num_sub,
    int(x, y) => Ok(int_sub(x, y)),
    rat(x, y) => rat_sub(x, y),
    flo(x, y) => Ok(RubyValue::Float(x - y)),
    cpx(x, y) => cpx_sub(x, y),
);

tower_binop!(
    num_mul,
    int(x, y) => Ok(int_mul(x, y)),
    rat(x, y) => rat_mul(x, y),
    flo(x, y) => Ok(RubyValue::Float(x * y)),
    cpx(x, y) => cpx_mul(x, y),
);

tower_binop!(
    /// `/`: Integer division floors (0 raises ZeroDivisionError); exact
    /// lanes stay exact; Float division is IEEE (`1.0 / 0` is Infinity).
    num_div,
    int(x, y) => {
        if crate::builtins::integer::int_is_zero(y) {
            Err(crate::dispatch::raise_error(
                "ZeroDivisionError",
                "divided by 0".to_string(),
            ))
        } else {
            Ok(int_div(x, y))
        }
    },
    rat(x, y) => rat_div(x, y),
    flo(x, y) => Ok(RubyValue::Float(x / y)),
    cpx(x, y) => cpx_div(x, y),
);

tower_binop!(
    num_mod,
    int(x, y) => {
        if crate::builtins::integer::int_is_zero(y) {
            Err(crate::dispatch::raise_error(
                "ZeroDivisionError",
                "divided by 0".to_string(),
            ))
        } else {
            Ok(int_mod(x, y))
        }
    },
    rat(x, y) => (|| {
        // a % b == a - b * (a/b).floor, exact.
        let q = rat_div(x, y)?;
        let floored = num_floor_exact(&q);
        rat_sub(x, &rat_mul(y, &floored)?)
    })(),
    flo(x, y) => {
        // Float#% by zero raises ZeroDivisionError (either an int or float
        // divisor coerces into this lane), rather than answering NaN.
        if y == 0.0 {
            Err(crate::dispatch::raise_error(
                "ZeroDivisionError",
                "divided by 0".to_string(),
            ))
        } else {
            Ok(RubyValue::Float(crate::float_mod(x, y)))
        }
    },
    // Complex has no modulo -- the same NoMethodError real Ruby raises, and
    // rescuable, where a panic was not.
    cpx(_x, _y) => Err(crate::dispatch::raise_error(
        "NoMethodError",
        "undefined method '%' for an instance of Complex".to_string(),
    )),
);

tower_binop!(
    /// `**` -- the Integer lane is FALLIBLE beyond overflow: a negative
    /// exponent is a Rational result, `0 ** -n` raises ZeroDivisionError.
    num_pow,
    int(x, y) => int_pow(x, y),
    rat(x, y) => rat_pow(x, y),
    // A negative base to a fractional (non-integer) power has no real
    // result, so ruby leaves the reals and answers a Complex. One body for
    // both callers: the emitter's typed fast path already asked
    // `float_pow_checked`, and this row raised `Math::DomainError` where
    // that promoted -- so `(-2.0) ** 0.5` answered differently depending on
    // whether the site was folded.
    flo(x, y) => crate::arith::float_pow_checked(x, y),
    cpx(x, y) => cpx_pow(x, y),
);

tower_binop!(
    /// `quo` -- rational-preserving division (`1.quo(3)` is `(1/3)`,
    /// `4.quo(2)` stays `(2/1)`); Float lanes are IEEE. Also the exact
    /// division Complex's internal component arithmetic uses.
    num_quo,
    int(x, y) => {
        let ((an, ad), (bn, bd)) = (as_ratio(x), as_ratio(y));
        rational_new(an * bd, ad * bn)
    },
    rat(x, y) => rat_div(x, y),
    flo(x, y) => Ok(RubyValue::Float(x / y)),
    cpx(x, y) => cpx_div(x, y),
);

/// Exact floor of an Integer/Rational value (helper for the Rational
/// modulo lane).
fn num_floor_exact(v: &RubyValue) -> RubyValue {
    use num_integer::Integer as _;
    let (n, d) = as_ratio(v);
    crate::builtins::integer::int_value(n.div_floor(&d))
}

/// Truncate toward zero to an exact Integer (`BigInt`'s `/` truncates).
fn num_trunc_exact(v: &RubyValue) -> RubyValue {
    let (n, d) = as_ratio(v);
    crate::builtins::integer::int_value(n / d)
}

/// Whether an arithmetic sequence walks in the Float lane: a Float in ANY
/// of the three slots moves the whole walk there, so `1.step(2.0, 0.5)`
/// yields `1.0` first (CRuby's `ruby_float_step`).
fn float_step_lane(begin: &RubyValue, end: Option<&RubyValue>, step: &RubyValue) -> bool {
    matches!(begin, RubyValue::Float(_))
        || matches!(step, RubyValue::Float(_))
        || matches!(end, Some(RubyValue::Float(_)))
}

/// How many elements a Float walk yields -- CRuby's `ruby_float_step_size`
/// (numeric.c), reproduced including its epsilon slack, which is what makes
/// `(0.0..1.0).step(0.1)` answer 11 rather than the 10 that plain division
/// rounds down to.
fn float_step_count(beg: f64, end: f64, unit: f64, excl: bool) -> f64 {
    if unit.is_infinite() {
        let one = if unit > 0.0 { beg <= end } else { beg >= end };
        return one as u8 as f64;
    }
    if unit == 0.0 {
        return f64::INFINITY;
    }
    let mut n = (end - beg) / unit;
    let err = (((beg.abs() + end.abs() + (end - beg).abs()) / unit.abs()) * f64::EPSILON).min(0.5);
    if excl {
        if n <= 0.0 {
            return 0.0;
        }
        n = if n < 1.0 { 0.0 } else { (n - err).floor() };
        let d = (n + 1.0) * unit + beg;
        if (beg < end && d < end) || (beg > end && d > end) {
            n += 1.0;
        }
    } else {
        if n < 0.0 {
            return 0.0;
        }
        n = (n + err).floor();
        let d = (n + 1.0) * unit + beg;
        if (beg < end && d <= end) || (beg > end && d >= end) {
            n += 1.0;
        }
    }
    n + 1.0
}

/// Every `step_*` helper below is reached only from a sequence whose three
/// slots the constructor already checked, so a non-numeric one is a bug here
/// rather than a Ruby-level error.
const NUMERIC_OPERANDS: &str = "an arithmetic sequence's operands are numeric";

/// A beginless sequence has nothing to walk from; CRuby reports it as the
/// coercion failure the first arithmetic would have raised.
fn need_begin(begin: &RubyValue) -> Result<(), Signal> {
    match begin {
        RubyValue::Nil => Err(type_error!("nil can't be coerced into Integer")),
        _ => Ok(()),
    }
}

/// The ONE arithmetic-sequence walk, shared by `Range#step`, `Range#%`,
/// `Numeric#step` and `Enumerator::ArithmeticSequence#each` -- so a
/// sequence's `to_a` can never disagree with the block form that built it.
///
/// The Float lane computes each element as `begin + i * step` rather than
/// running a sum, which is why `0.0.step(1.0, 0.1)` lands exactly on `1.0`;
/// every other lane (Integer, Bignum, Rational) walks by repeated `+`, so it
/// stays exact.
pub(crate) fn step_walk(
    begin: &RubyValue,
    end: Option<&RubyValue>,
    step: &RubyValue,
    exclude_end: bool,
    mut f: impl FnMut(&RubyValue) -> Result<(), Signal>,
) -> Result<(), Signal> {
    need_begin(begin)?;
    // The limit has to be COMPARABLE with the start, and ruby says so when the
    // walk RUNS rather than when it is built -- `1.step("z", 1)` answers an
    // Enumerator, and driving it raises. Without this the walk simply yielded
    // nothing.
    if let Some(e) = end
        && num_cmp(begin, e).is_none()
    {
        return Err(arg_error!(
            "comparison of {} with {} failed",
            crate::builtins::class_name_of(begin),
            crate::builtins::class_name_of(e)
        ));
    }
    if float_step_lane(begin, end, step) {
        let beg = num_to_f64_unchecked(begin);
        let unit = num_to_f64_unchecked(step);
        let fin = match end {
            Some(v) => num_to_f64_unchecked(v),
            None if unit < 0.0 => f64::NEG_INFINITY,
            None => f64::INFINITY,
        };
        let n = float_step_count(beg, fin, unit, exclude_end);
        let mut i = 0.0f64;
        while i < n {
            let d = i * unit + beg;
            // CRuby clamps the final element back onto `end`, so accumulated
            // ulps can never carry it past the endpoint the caller named.
            let d = if (unit >= 0.0 && fin < d) || (unit < 0.0 && d < fin) {
                fin
            } else {
                d
            };
            f(&RubyValue::Float(d))?;
            i += 1.0;
        }
        return Ok(());
    }
    let descending = matches!(num_cmp(step, &RubyValue::Int(0)), Some(Some(-1)));
    let mut cur = begin.clone();
    loop {
        if let Some(end) = end {
            let Some(Some(o)) = num_cmp(&cur, end) else {
                break;
            };
            if (if descending { o < 0 } else { o > 0 }) || (exclude_end && o == 0) {
                break;
            }
        }
        f(&cur)?;
        cur = num_add(&cur, step).expect("an arithmetic sequence's operands are numeric")?;
    }
    Ok(())
}

/// `(end - begin).div(step)` across the tower -- how many hops separate the
/// endpoints, floored. `Enumerator::ArithmeticSequence#last` is built on it,
/// and computes rather than walks, which is why `(1..10.5).step(2).last` is
/// the Integer `9` while the walk's own last element is the Float `9.0`.
pub(crate) fn step_hops(
    begin: &RubyValue,
    end: &RubyValue,
    step: &RubyValue,
) -> Result<RubyValue, Signal> {
    let span = num_sub(end, begin).expect(NUMERIC_OPERANDS)?;
    if float_step_lane(begin, Some(end), step) {
        let q = num_to_f64_unchecked(&span) / num_to_f64_unchecked(step);
        return crate::builtins::float::float_to_integer(q.floor());
    }
    Ok(num_floor_exact(
        &num_quo(&span, step).expect(NUMERIC_OPERANDS)?,
    ))
}

/// The i-th element of the same walk: `begin + i * step`. What
/// `Enumerator::ArithmeticSequence#last` builds each of its answers from.
pub(crate) fn step_nth(
    begin: &RubyValue,
    step: &RubyValue,
    i: &RubyValue,
) -> Result<RubyValue, Signal> {
    let offset = num_mul(i, step).expect(NUMERIC_OPERANDS)?;
    num_add(begin, &offset).expect(NUMERIC_OPERANDS)
}

/// How many elements the same walk yields, WITHOUT walking -- CRuby computes
/// `Enumerator::ArithmeticSequence#size` from the quadruple, which is what
/// lets an endless sequence answer `Float::INFINITY` at all.
pub(crate) fn step_size(
    begin: &RubyValue,
    end: Option<&RubyValue>,
    step: &RubyValue,
    exclude_end: bool,
) -> Result<RubyValue, Signal> {
    need_begin(begin)?;
    let Some(end) = end else {
        return Ok(RubyValue::Float(f64::INFINITY));
    };
    if float_step_lane(begin, Some(end), step) {
        let n = float_step_count(
            num_to_f64_unchecked(begin),
            num_to_f64_unchecked(end),
            num_to_f64_unchecked(step),
            exclude_end,
        );
        return Ok(if n.is_finite() {
            RubyValue::Int(n as i64)
        } else {
            RubyValue::Float(n)
        });
    }
    let descending = matches!(num_cmp(step, &RubyValue::Int(0)), Some(Some(-1)));
    let Some(Some(o)) = num_cmp(begin, end) else {
        return Ok(RubyValue::Int(0));
    };
    if if descending { o < 0 } else { o > 0 } {
        return Ok(RubyValue::Int(0));
    }
    // The hop count, then the first element on top -- unless an exclusive end
    // lands exactly where the last hop would have.
    let hops = step_hops(begin, end, step)?;
    if exclude_end {
        let last = step_nth(begin, step, &hops)?;
        if matches!(num_cmp(&last, end), Some(Some(0))) {
            return Ok(hops);
        }
    }
    num_add(&hops, &RubyValue::Int(1)).expect(NUMERIC_OPERANDS)
}

/// `a <=> b` across the tower. Outer `None` = not a numeric pair; inner
/// `None` = Ruby's `nil` (a NaN comparison, or any Complex operand --
/// complexes have no ordering).
pub fn num_cmp(a: &RubyValue, b: &RubyValue) -> Option<Option<i64>> {
    let joined = lane(a)?.max(lane(b)?);
    if let Some((i, y, flip)) = integer_float_pair(a, b) {
        return Some(integer_float_cmp(i, y).map(|o| if flip { -o } else { o }));
    }
    Some(match joined {
        NumLane::Int => Some(int_cmp(a, b)),
        NumLane::Rat => Some(rat_cmp(a, b)),
        NumLane::Flo => num_to_f64_unchecked(a)
            .partial_cmp(&num_to_f64_unchecked(b))
            .map(|o| o as i64),
        NumLane::Cpx => None,
    })
}

/// `a == b` across the tower (`1 == 1.0`, `Rational(2,1) == 2`,
/// `Complex(2,0) == 2` -- all true, oracle-verified). `None` = not a
/// numeric pair (the caller's non-numeric equality applies).
pub fn num_eq(a: &RubyValue, b: &RubyValue) -> Option<bool> {
    let joined = lane(a)?.max(lane(b)?);
    Some(match joined {
        NumLane::Cpx => cpx_eq(a, b),
        _ => matches!(num_cmp(a, b)?, Some(0)),
    })
}

/// `<`, `<=`, `>`, `>=` with an `Integer` or `Float` receiver -- CRuby's
/// `fix_lt`, `big_op` and `flo_lt`, which read `<=>`'s answer and map a NIL
/// one to FALSE rather than raising. A NaN on either side is that nil, so
/// `1 < Float::NAN` is false, not `ArgumentError`.
///
/// A `Rational` operand belongs here too, because CRuby reaches it through
/// `rb_num_coerce_relop`, whose coercion makes the pair two Floats and then
/// asks `Float#<` -- which answers false for a NaN and never nil. Note the
/// asymmetry that follows and is real: `Float::NAN < Rational(1,2)` is
/// false while `Rational(1,2) < Float::NAN` RAISES, because a Rational
/// receiver keeps Comparable's row.
///
/// `None` means the operand is something else (a Complex, a String) and the
/// caller falls through to `Comparable`, where ruby's
/// `comparison of X with Y failed` still lives.
pub(crate) fn int_float_relop(
    recv: &RubyValue,
    other: &RubyValue,
    want: fn(i64) -> bool,
) -> Option<bool> {
    let coercible = |v: &RubyValue| {
        matches!(
            v,
            RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_)
        )
    };
    if !coercible(recv) || !coercible(other) {
        return None;
    }
    Some(num_cmp(recv, other)?.is_some_and(want))
}

/// An `Integer`/`Float` pair in either order, as `(integer, float, flipped)`.
fn integer_float_pair<'a>(
    a: &'a RubyValue,
    b: &'a RubyValue,
) -> Option<(&'a RubyValue, f64, bool)> {
    match (a, b) {
        (i @ (RubyValue::Int(_) | RubyValue::BigInt(_)), RubyValue::Float(y)) => {
            Some((i, *y, false))
        }
        (RubyValue::Float(y), i @ (RubyValue::Int(_) | RubyValue::BigInt(_))) => {
            Some((i, *y, true))
        }
        _ => None,
    }
}

/// `Integer <=> Float`, CRuby's `rb_integer_float_cmp`: the comparison is
/// EXACT, decided against the float's own integer part with the fraction as
/// the tie-break.
///
/// Comparing the pair as doubles instead answers `10**100 == 1.0e100` TRUE.
/// They are different numbers; the double simply cannot tell, and the
/// silent wrong answer is the dangerous direction.
fn integer_float_cmp(i: &RubyValue, y: f64) -> Option<i64> {
    use num_traits::FromPrimitive;
    if y.is_nan() {
        return None;
    }
    if y.is_infinite() {
        return Some(if y > 0.0 { -1 } else { 1 });
    }
    // Below 2**53 an `i64` is exact as a double, so the plain comparison
    // already IS the exact one -- and this is the pair nearly every program
    // writes.
    if let RubyValue::Int(n) = i
        && n.unsigned_abs() < (1u64 << 53)
    {
        return (*n as f64).partial_cmp(&y).map(|o| o as i64);
    }
    let yi = y.trunc();
    let yf = y - yi;
    let whole = num_bigint::BigInt::from_f64(yi)?;
    let rel = to_bigint(i).cmp(&whole) as i64;
    if yf == 0.0 || rel != 0 {
        return Some(rel);
    }
    Some(if yf < 0.0 { 1 } else { -1 })
}

// The GENERIC Numeric rows (CRuby's `Numeric` ownership: methods defined
// once, driving the receiver's own core operations) -- found via the MRO
// walk on any numeric receiver whose own class doesn't override them.
ruby_class! {
    Numeric = zeo_abi::NUMERIC_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    def "zero?" (recv) {
        Ok(RubyValue::Bool(num_eq(recv, &RubyValue::Int(0)).unwrap_or(false)))
    }
    def "positive?" (recv) {
        Ok(RubyValue::Bool(matches!(num_cmp(recv, &RubyValue::Int(0)), Some(Some(1)))))
    }
    def "negative?" (recv) {
        Ok(RubyValue::Bool(matches!(num_cmp(recv, &RubyValue::Int(0)), Some(Some(-1)))))
    }
    def "nonzero?" (recv) {
        Ok(if num_eq(recv, &RubyValue::Int(0)).unwrap_or(false) {
            RubyValue::Nil
        } else {
            recv.clone()
        })
    }
    def "integer?" (recv) {
        Ok(RubyValue::Bool(matches!(recv, RubyValue::Int(_) | RubyValue::BigInt(_))))
    }
    def "real?" (recv) {
        Ok(RubyValue::Bool(!matches!(recv, RubyValue::Complex(_))))
    }
    def "real" (recv) {
        Ok(recv.clone())
    }
    def "imag" | "imaginary" (_recv) {
        Ok(RubyValue::Int(0))
    }
    def "to_c" (recv) {
        crate::builtins::complex::complex_new(recv.clone(), RubyValue::Int(0))
    }
    // A real number's cartesian view is `[self, 0]`.
    def "rect" | "rectangular" (recv) {
        Ok(RubyValue::Array(crate::array_new(vec![recv.clone(), RubyValue::Int(0)])))
    }
    // Polar view: magnitude `|self|`, angle `0` (non-negative) or `pi` (negative).
    def "polar" (recv) {
        let magnitude =
            crate::dispatch::send_value(recv, crate::Symbol::intern("abs"), &[], None)?;
        let angle = if points_negative(recv) {
            RubyValue::Float(std::f64::consts::PI)
        } else {
            RubyValue::Int(0)
        };
        Ok(RubyValue::Array(crate::array_new(vec![magnitude, angle])))
    }
    def "abs2" (recv) {
        num_mul(recv, recv).expect("numeric receiver")
    }
    def "conj" | "conjugate" (recv) {
        Ok(recv.clone())
    }
    def "angle" | "arg" | "phase" (recv) {
        // 0 for non-negative reals, pi for negative (a Float in real Ruby
        // only for the negative case; 0 stays Integer). A NaN is its own
        // argument, which is CRuby's first test.
        if matches!(recv, RubyValue::Float(f) if f.is_nan()) {
            return Ok(recv.clone());
        }
        Ok(if points_negative(recv) {
            RubyValue::Float(std::f64::consts::PI)
        } else {
            RubyValue::Int(0)
        })
    }
    def "divmod" (recv, arg) {
        // Float lane: CRuby's coupled `flo_divmod` adjusts the quotient and the
        // remainder together at a sign boundary, so `7.0.divmod(-Infinity)` is
        // `[-1, -Infinity]`, not `[0, -Infinity]` -- an independent floor of
        // `7.0 / -Infinity` (== -0.0) would answer 0.
        if matches!(lane(recv), Some(NumLane::Flo)) || matches!(lane(arg), Some(NumLane::Flo)) {
            if lane(recv).is_none() || lane(arg).is_none() {
                return Err(coercion_error(recv, arg));
            }
            let x = num_to_f64_unchecked(recv);
            let y = num_to_f64_unchecked(arg);
            if y == 0.0 {
                return Err(crate::dispatch::raise_error(
                    "ZeroDivisionError",
                    "divided by 0".to_string(),
                ));
            }
            let (div, m) = flo_divmod(x, y);
            if !div.is_finite() {
                let msg = if div.is_nan() { "NaN" } else if div > 0.0 { "Infinity" } else { "-Infinity" };
                return Err(crate::dispatch::raise_error("FloatDomainError", msg.to_string()));
            }
            use num_traits::FromPrimitive;
            let q = crate::builtins::integer::int_value(
                num_bigint::BigInt::from_f64(div).expect("finite float"),
            );
            return Ok(RubyValue::Array(crate::array_new(vec![q, RubyValue::Float(m)])));
        }
        let q = num_div(recv, arg)
            .ok_or_else(|| coercion_error(recv, arg))??;
        let r = num_mod(recv, arg)
            .ok_or_else(|| coercion_error(recv, arg))??;
        // The QUOTIENT converts to Integer even on the Float lane
        // (`7.divmod(2.5)` is `[2, 2.0]` -- oracle-verified; CRuby's
        // flo_divmod floors then rb_dbl2ival's the div half).
        let q = match q {
            RubyValue::Float(f) if f.is_finite() => {
                use num_traits::FromPrimitive;
                crate::builtins::integer::int_value(
                    num_bigint::BigInt::from_f64(f.floor()).expect("finite float"),
                )
            }
            // A non-finite quotient (NaN/Infinity dividend or divisor) can't
            // floor to an Integer -- CRuby's flo_divmod raises FloatDomainError
            // named for the offending value, before the (mrb_int)floor cast.
            RubyValue::Float(f) => {
                let msg = if f.is_nan() {
                    "NaN"
                } else if f > 0.0 {
                    "Infinity"
                } else {
                    "-Infinity"
                };
                return Err(crate::dispatch::raise_error("FloatDomainError", msg.to_string()));
            }
            // A Rational quotient floors to an Integer too (CRuby's divmod
            // quotient is always an Integer): `(7/2).divmod(1/3)` is
            // `[10, (1/6)]`, not `[(21/2), (1/6)]`.
            RubyValue::Rational(_) => num_floor_exact(&q),
            other => other,
        };
        Ok(RubyValue::Array(crate::array_new(vec![q, r])))
    }
    def "fdiv" (recv, arg) {
        if lane(arg).is_none() || matches!( *arg, RubyValue::Complex(_)) {
            return Err(coercion_error(recv, arg));
        }
        Ok(RubyValue::Float(
            num_to_f64_unchecked(recv) / num_to_f64_unchecked(arg),
        ))
    }
    def "quo" (recv, arg) {
        num_quo(recv, arg).ok_or_else(|| coercion_error(recv, arg))?
    }
    def "remainder" (recv, arg) {
        // a - b*(a/b).truncate -- the truncated-division counterpart of %
        // (sign follows the DIVIDEND). BigInt's own `/` truncates.
        match (recv, arg) {
            (
                RubyValue::Int(_) | RubyValue::BigInt(_),
                RubyValue::Int(_) | RubyValue::BigInt(_),
            ) => {
                if crate::builtins::integer::int_is_zero(arg) {
                    return Err(crate::dispatch::raise_error(
                        "ZeroDivisionError",
                        "divided by 0".to_string(),
                    ));
                }
                let a = crate::builtins::integer::to_bigint(recv);
                let b = crate::builtins::integer::to_bigint(arg);
                Ok(crate::builtins::integer::int_value(&a - &b * (&a / &b)))
            }
            _ => {
                if lane(arg).is_none() {
                    return Err(coercion_error(recv, arg));
                }
                // Exact on the Rational lane (no Float operand): a - b*(a/b).truncate,
                // so `(7/2).remainder(1/3)` is `(1/6)`, not a Float.
                let hi = lane(recv).zip(lane(arg)).map(|(a, b)| a.max(b));
                if hi == Some(NumLane::Rat) {
                    let coerce = || coercion_error(recv, arg);
                    let q = num_div(recv, arg).ok_or_else(coerce)??;
                    let t = num_trunc_exact(&q);
                    let bt = num_mul(arg, &t).ok_or_else(coerce)??;
                    return num_sub(recv, &bt).ok_or_else(coerce)?;
                }
                let (a, b) = (num_to_f64_unchecked(recv), num_to_f64_unchecked(arg));
                // A zero divisor is ZeroDivisionError on this lane as well:
                // `flo_remainder` reaches `rb_num_zerodiv` rather than letting
                // the IEEE division answer NaN.
                if b == 0.0 {
                    return Err(crate::dispatch::raise_error(
                        "ZeroDivisionError",
                        "divided by 0".to_string(),
                    ));
                }
                Ok(RubyValue::Float(a - b * (a / b).trunc()))
            }
        }
    }
    // ---- The defaults a `class Temp < Numeric` inherits ----
    //
    // Everything above computes on zeo's own numeric lanes, which a user
    // subclass is not one of. These nineteen are CRuby's own generic
    // implementations: they know nothing but `<=>`, `-`, `/`, `to_f`, `to_i`,
    // `to_r` and `coerce`, and reach them by SEND, so they work for a subclass
    // that defines only those. Every concrete numeric class carries its own
    // faster row for each, which the ancestor walk finds first.

    // `[other, self]` when the two are the same class, else both as Floats --
    // the fallback CRuby's `rb_num_coerce_bin` leans on when a subclass
    // defines no `coerce` of its own.
    def "coerce" (recv, other) {
        if recv.class_id() == other.class_id() {
            return Ok(RubyValue::Array(crate::array_new(vec![other.clone(), recv.clone()])));
        }
        let pair = vec![to_float(other)?, to_float(recv)?];
        Ok(RubyValue::Array(crate::array_new(pair)))
    }
    def "+@" (recv) {
        Ok(recv.clone())
    }
    // `zero, x = self.coerce(0); zero - x` -- NOT `0 - self`, which would ask
    // the subclass to accept an Integer on the left.
    def "-@" (recv) {
        let pair = send(recv, "coerce", &[RubyValue::Int(0)])?;
        let (zero, x) = coerced_pair(&pair)?;
        send(&zero, "-", &[x])
    }
    def "abs" | "magnitude" (recv) {
        if is_negative(recv)? {
            return send(recv, "-@", &[]);
        }
        Ok(recv.clone())
    }
    // `(self / other).floor`, so the subclass's own `/` decides the lane and
    // the quotient's own `floor` decides the rounding.
    def "div" (recv, other) {
        // The zero check comes FIRST, as `num_div`'s `rb_num_zerodiv` does.
        // Without it a Float receiver divided to Infinity and then died in
        // `floor` with FloatDomainError, where ruby says ZeroDivisionError.
        if crate::dispatch::send_value(other, crate::Symbol::intern("=="), &[RubyValue::Int(0)], None)?.truthy() {
            return Err(crate::dispatch::raise_error(
                "ZeroDivisionError",
                "divided by 0".to_string(),
            ));
        }
        let q = send(recv, "/", std::slice::from_ref(other))?;
        send(&q, "floor", &[])
    }
    // `self - other * self.div(other)` -- floored division's remainder, whose
    // sign follows the DIVISOR.
    def "modulo" | "%" (recv, other) {
        let q = send(recv, "div", std::slice::from_ref(other))?;
        let whole = send(other, "*", &[q])?;
        send(recv, "-", &[whole])
    }
    // The rounding family all defer to Float, which is what CRuby does
    // (`num_round` is `flo_round(rb_Float(num))`) -- so a subclass needs only
    // `to_f` to get all four.
    def "round" (recv, *args) {
        float_row(recv, "round", args)
    }
    def "floor" (recv, *args) {
        float_row(recv, "floor", args)
    }
    def "ceil" (recv, *args) {
        float_row(recv, "ceil", args)
    }
    def "truncate" (recv, *args) {
        float_row(recv, "truncate", args)
    }
    def "to_int" (recv) {
        send(recv, "to_i", &[])
    }
    // `Complex(0, self)`. CRuby refuses a receiver that is not real, which is
    // why `Complex#i` does not exist -- see the undef list in `bootstrap`.
    def "i" (recv) {
        crate::builtins::complex::complex_new(RubyValue::Int(0), recv.clone())
    }
    def "numerator" (recv) {
        let r = send(recv, "to_r", &[])?;
        send(&r, "numerator", &[])
    }
    def "denominator" (recv) {
        let r = send(recv, "to_r", &[])?;
        send(&r, "denominator", &[])
    }
    // Only `Float` has values that are neither, so the generic answers are
    // fixed -- `Float` overrides both.
    def "finite?" (_recv) {
        Ok(RubyValue::Bool(true))
    }
    def "infinite?" (_recv) {
        Ok(RubyValue::Nil)
    }
    // A Numeric is meant to be immutable and interchangeable with any equal
    // value, so CRuby refuses to give one a singleton class at all.
    //
    // Present and correct when CALLED, but nothing calls it yet: zeo has no
    // `singleton_method_added` hook, so `def n.foo` on a Numeric still
    // succeeds. `tests/gaps/numeric_subclass_limits.rb` records that.
    def "singleton_method_added" (recv, name) {
        let name = match name {
            RubyValue::Symbol(s) => s.name_str().to_string(),
            other => crate::builtins::convert::to_rstr(other)?
                .lock()
                .to_utf8_lossy()
                .into_owned(),
        };
        let class = crate::dispatch::class_name(recv.class_id()).unwrap_or_default();
        Err(type_error!("can't define singleton method \"{name}\" for {class}"))
    }

    // `step(limit, step = 1)`; the blockless form answers an
    // `Enumerator::ArithmeticSequence`. Drives the tower generically, so
    // `1.step(2.0, 0.5)` works too.
    def "step" (recv, to?, by?, **opts, &block) {
        // `step` accepts positional (`1.step(10, 2)`) and/or keyword
        // (`1.step(by: 2, to: 10)`) forms.
        let mut limit: Option<RubyValue> = None;
        let mut step = RubyValue::Int(1);
        if let Some(RubyValue::Hash(h)) = opts {
            let by = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("by")));
            let to = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("to")));
            if !matches!(by, RubyValue::Nil) {
                step = by;
            }
            if !matches!(to, RubyValue::Nil) {
                limit = Some(to);
            }
        }
        // An explicit `nil` limit is UNBOUNDED, exactly as an absent one is
        // -- `1.step(nil) { break :x }` is ruby's endless form. Reading it
        // as a bound made the first comparison fail on a NilClass the
        // caller never wrote.
        if let Some(l) = to
            && !matches!(l, RubyValue::Nil)
        {
            limit = Some(l.clone());
        }
        if let Some(s) = by {
            step = s.clone();
        }
        // A zero step never advances `cur`, so the loop below would spin
        // forever calling the block. CRuby rejects it up front in both the
        // block and blockless paths (`num_step_check_fix_args`, numeric.c:2888
        // and `num_step`, numeric.c:3036). The comparison is a Ruby-level
        // `==` there (`rb_equal`), so `0.0` and `Rational(0, 1)` are rejected
        // too -- hence `num_cmp` rather than a native `== 0` test.
        if matches!(num_cmp(&step, &RubyValue::Int(0)), Some(Some(0))) {
            return Err(arg_error!("step can't be 0"));
        }
        // An absent limit (`1.step(by: 2)`) is an unbounded sequence, and a
        // Numeric one is never exclusive.
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(crate::builtins::enumerator::arith_seq_of(
                recv,
                "step",
                __args,
                recv.clone(),
                limit.unwrap_or(RubyValue::Nil),
                step,
                false,
            ));
        };
        step_walk(recv, limit.as_ref(), &step, false, |v| {
            p.call(std::slice::from_ref(v))?;
            Ok(())
        })?;
        Ok(recv.clone())
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "<=>"(recv, _other) { inherited_row!(kernel, "<=>", recv, __args, None) }
    def "clone" cfunc (recv, *_args) { inherited_row!(kernel, "clone", recv, __args, None) }
    def "dup"(recv) { inherited_row!(kernel, "dup", recv, __args, None) }
    def "eql?"(recv, _other) { inherited_row!(kernel, "eql?", recv, __args, None) }
}

/// CRuby's `rb_num_coerce_bin` for a binary numeric operator: given the
/// native-tower result (`Some` when both operands fit the built-in numeric
/// types), pass it through; otherwise fall back to the `coerce` protocol --
/// ask `arg` to `coerce(recv)`, then apply `op` to the returned `[a, b]`
/// pair. This is what lets a user numeric type (a `Money`, a `Vector`) join
/// arithmetic with a built-in: `5 + Money.new(...)` becomes
/// `Money.new(...).coerce(5) => [a, b]; a + b`. A non-numeric operand that
/// doesn't answer `coerce` raises the ordinary coercion TypeError.
pub(crate) fn num_coerce_bin(
    recv: &RubyValue,
    arg: &RubyValue,
    computed: Option<Result<RubyValue, Signal>>,
    op: &str,
) -> Result<RubyValue, Signal> {
    if let Some(r) = computed {
        return r;
    }
    let coerce = crate::Symbol::intern("coerce");
    if crate::dispatch::responds_to(arg.class_id(), coerce, false) {
        let pair = crate::dispatch::send_value(arg, coerce, std::slice::from_ref(recv), None)?;
        if let RubyValue::Array(a) = &pair {
            let items: Vec<RubyValue> = a.lock().iter().cloned().collect();
            if items.len() == 2 {
                return crate::dispatch::send_value(
                    &items[0],
                    crate::Symbol::intern(op),
                    std::slice::from_ref(&items[1]),
                    None,
                );
            }
        }
        // `coerce` must answer a 2-element Array (CRuby's exact TypeError).
        return Err(type_error!("coerce must return [x, y]"));
    }
    Err(coercion_error(recv, arg))
}

/// CRuby's `rb_num_coerce_cmp` tail: a `<=>` whose operand is outside the
/// native tower asks it to `coerce` and compares the returned pair; an
/// operand with no `coerce` is simply incomparable (nil). What lets
/// `1.5 <=> BigDecimal("2")` (and through Comparable, `1.5 < bd`) work.
pub(crate) fn coerce_cmp(recv: &RubyValue, arg: &RubyValue) -> Result<RubyValue, Signal> {
    let coerce = crate::Symbol::intern("coerce");
    if !matches!(arg, RubyValue::Object(_))
        || !crate::dispatch::responds_to(arg.class_id(), coerce, false)
    {
        return Ok(RubyValue::Nil);
    }
    let pair = crate::dispatch::send_value(arg, coerce, std::slice::from_ref(recv), None)?;
    if let RubyValue::Array(a) = &pair {
        let items: Vec<RubyValue> = a.lock().iter().cloned().collect();
        if items.len() == 2 {
            return crate::dispatch::send_value(
                &items[0],
                crate::Symbol::intern("<=>"),
                std::slice::from_ref(&items[1]),
                None,
            );
        }
    }
    Ok(RubyValue::Nil)
}

/// CRuby's `num_equal` tail for `Integer#==`/`Float#==`: equality with an
/// object outside the tower is THE OPERAND's question (`y == x`), which is
/// how `1.5 == BigDecimal("1.5")` answers true.
pub(crate) fn reverse_eq(recv: &RubyValue, arg: &RubyValue) -> Result<RubyValue, Signal> {
    if !matches!(arg, RubyValue::Object(_)) {
        return Ok(RubyValue::Bool(false));
    }
    let r = crate::dispatch::send_value(
        arg,
        crate::Symbol::intern("=="),
        std::slice::from_ref(recv),
        None,
    )?;
    Ok(RubyValue::Bool(r.truthy()))
}

/// The coercion TypeError a generic Numeric row raises (named by the
/// RECEIVER's class, CRuby's shape; the ARGUMENT reads per
/// `coerce_operand_name`'s special-constant rule).
/// Call `name` on `recv` through ordinary dispatch. Every generic `Numeric`
/// row goes through this rather than a `num_*` helper: the receiver may be a
/// user subclass, whose `-`/`/`/`to_f` are the only things that know what it
/// means.
fn send(recv: &RubyValue, name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::dispatch::send_value(recv, crate::Symbol::intern(name), args, None)
}

/// `rb_Float` -- what the rounding family and the fallback `coerce` convert
/// through.
fn to_float(v: &RubyValue) -> Result<RubyValue, Signal> {
    crate::builtins::kernel::float_impl(std::slice::from_ref(v))
}

/// Run `Float`'s own row: `Float(self).round(*args)` and its three siblings,
/// which is exactly how CRuby defines the generic ones.
fn float_row(recv: &RubyValue, name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    send(&to_float(recv)?, name, args)
}

/// `self < 0`, asked through `<=>` so a subclass needs nothing else.
/// Which half of the plane `recv` points into, for `angle`/`arg`/`phase` and
/// `polar`. A Float is judged by its SIGN BIT, not by `<=> 0`: `-0.0` compares
/// equal to zero and yet its argument is pi, which is what CRuby's `float_arg`
/// answers (it reaches `f_signbit`, not a comparison).
fn points_negative(recv: &RubyValue) -> bool {
    match recv {
        RubyValue::Float(f) => f.is_sign_negative(),
        other => matches!(num_cmp(other, &RubyValue::Int(0)), Some(Some(-1))),
    }
}

fn is_negative(recv: &RubyValue) -> Result<bool, Signal> {
    Ok(matches!(
        send(recv, "<=>", &[RubyValue::Int(0)])?,
        RubyValue::Int(n) if n < 0
    ))
}

/// Split what `coerce` answered into its two halves, rejecting anything that
/// is not a two-element Array -- CRuby's own check, and the message it uses.
fn coerced_pair(pair: &RubyValue) -> Result<(RubyValue, RubyValue), Signal> {
    if let RubyValue::Array(a) = pair {
        let a = a.lock();
        if a.len() == 2 {
            return Ok((a[0].clone(), a[1].clone()));
        }
    }
    Err(type_error!("coerce must return [x, y]"))
}

fn coercion_error(recv: &RubyValue, arg: &RubyValue) -> Signal {
    type_error!(
        "{} can't be coerced into {}",
        crate::builtins::coerce_operand_name(arg),
        crate::builtins::class_name_of(recv)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::integer::int_value;
    use num_bigint::BigInt;

    /// The Numeric rows are `ruby_class!`-generated (mangled Rust fn names), so
    /// reach them the way dispatch does -- through the registered table.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::NUMERIC_CLASS)
            .expect("Numeric is a registered builtin table")
            .instance
            .as_ref()
            .expect("Numeric has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Numeric#{name} is defined"))
    }
    fn divmod(
        recv: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        imethod("divmod")(recv, args, block)
    }
    fn remainder(
        recv: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        imethod("remainder")(recv, args, block)
    }
    fn step(
        recv: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        imethod("step")(recv, args, block)
    }

    fn big(s: &str) -> RubyValue {
        int_value(s.parse::<BigInt>().unwrap())
    }

    fn rat(n: i64, d: i64) -> RubyValue {
        rational_new(BigInt::from(n), BigInt::from(d)).unwrap()
    }

    #[test]
    fn num_coerce_bin_passes_a_native_result_through_untouched() {
        // When the native tower already handled the operands, `num_coerce_bin`
        // returns that result without consulting the coerce protocol (which
        // would need a registry). The retry path is covered by the e2e suite.
        let native = num_add(&RubyValue::Int(2), &RubyValue::Int(3));
        let r = num_coerce_bin(&RubyValue::Int(2), &RubyValue::Int(3), native, "+").unwrap();
        assert!(matches!(r, RubyValue::Int(5)));
    }

    #[test]
    fn lanes_join_upward() {
        // Int + Rat stays exact: (1/2) + 1 == (3/2)
        let r = num_add(&rat(1, 2), &RubyValue::Int(1)).unwrap().unwrap();
        assert!(
            matches!(&r, RubyValue::Rational(q) if q.num == BigInt::from(3) && q.den == BigInt::from(2))
        );
        // Rat + Float promotes to Float: (1/2) + 0.5 == 1.0
        let r = num_add(&rat(1, 2), &RubyValue::Float(0.5))
            .unwrap()
            .unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f == 1.0));
        // Anything + Complex is Complex.
        let c =
            crate::builtins::complex::complex_new(RubyValue::Int(1), RubyValue::Int(2)).unwrap();
        let r = num_add(&RubyValue::Int(1), &c).unwrap().unwrap();
        assert!(matches!(r, RubyValue::Complex(_)));
    }

    #[test]
    fn int_lane_stays_exact_and_promotes_on_overflow() {
        let r = num_add(&RubyValue::Int(i64::MAX), &RubyValue::Int(1))
            .unwrap()
            .unwrap();
        assert!(matches!(r, RubyValue::BigInt(_)));
        assert_eq!(
            num_cmp(&big("100000000000000000000"), &RubyValue::Int(5)).unwrap(),
            Some(1)
        );
    }

    #[test]
    fn quo_is_rational_preserving() {
        let r = num_quo(&RubyValue::Int(1), &RubyValue::Int(3))
            .unwrap()
            .unwrap();
        assert!(
            matches!(&r, RubyValue::Rational(q) if q.num == BigInt::from(1) && q.den == BigInt::from(3))
        );
        let r = num_quo(&RubyValue::Int(4), &RubyValue::Int(2))
            .unwrap()
            .unwrap();
        assert!(matches!(&r, RubyValue::Rational(q) if q.den == BigInt::from(1)));
    }

    #[test]
    fn eq_and_cmp_cross_every_real_lane() {
        assert_eq!(
            num_eq(&RubyValue::Int(1), &RubyValue::Float(1.0)),
            Some(true)
        );
        assert_eq!(num_eq(&rat(2, 1), &RubyValue::Int(2)), Some(true));
        assert_eq!(num_eq(&rat(1, 2), &RubyValue::Float(0.5)), Some(true));
        assert_eq!(
            num_cmp(&RubyValue::Int(2), &RubyValue::Float(1.5)).unwrap(),
            Some(1)
        );
        assert_eq!(num_cmp(&rat(1, 2), &rat(2, 3)).unwrap(), Some(-1));
        assert_eq!(
            num_cmp(&RubyValue::Float(f64::NAN), &RubyValue::Int(1)).unwrap(),
            None
        );
        let c =
            crate::builtins::complex::complex_new(RubyValue::Int(2), RubyValue::Int(0)).unwrap();
        assert_eq!(num_eq(&c, &RubyValue::Int(2)), Some(true));
        assert_eq!(num_cmp(&c, &RubyValue::Int(2)).unwrap(), None);
    }

    #[test]
    fn rational_modulo_is_exact() {
        // Rational(7,2) % 2 == (3/2) (floored, oracle rule a - b*(a/b).floor)
        let r = num_mod(&rat(7, 2), &RubyValue::Int(2)).unwrap().unwrap();
        assert!(
            matches!(&r, RubyValue::Rational(q) if q.num == BigInt::from(3) && q.den == BigInt::from(2))
        );
    }

    #[test]
    fn non_numeric_sides_return_none() {
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        assert!(num_add(&RubyValue::Int(1), &s).is_none());
        assert!(num_eq(&s, &RubyValue::Int(1)).is_none());
    }

    #[test]
    fn generic_rows_drive_the_tower() {
        let r = divmod(&RubyValue::Int(7), &[RubyValue::Float(2.5)], None).unwrap();
        assert_eq!(r.inspect_string(), "[2, 2.0]");
        let r = remainder(&RubyValue::Int(-7), &[RubyValue::Int(3)], None).unwrap();
        assert!(matches!(r, RubyValue::Int(-1)));
        let mut seen = Vec::new();
        {
            let cell = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
            let c2 = std::sync::Arc::clone(&cell);
            let p: crate::RProc = crate::RProc::new(move |args: &[RubyValue]| {
                c2.lock().push(args[0].inspect_string());
                Ok(RubyValue::Nil)
            });
            step(
                &RubyValue::Int(1),
                &[RubyValue::Int(10), RubyValue::Int(3)],
                Some(RubyValue::Proc(p)),
            )
            .unwrap();
            seen.extend(cell.lock().iter().cloned());
        }
        assert_eq!(seen, vec!["1", "4", "7", "10"]);
    }

    #[test]
    fn zero_division_raises_on_exact_lanes_only() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            num_div(&RubyValue::Int(1), &RubyValue::Int(0))
        }));
        assert!(r.is_err());
        let r = num_div(&RubyValue::Float(1.0), &RubyValue::Int(0))
            .unwrap()
            .unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f.is_infinite()));
    }
}
