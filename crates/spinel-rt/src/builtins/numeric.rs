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
use crate::builtins::{arity, block_or_enum, builtin_methods};
use crate::builtins::integer::{int_add, int_cmp, int_div, int_mod, int_mul, int_pow, int_sub};
use crate::builtins::rational::{
    as_ratio, rat_add, rat_cmp, rat_div, rat_mul, rat_pow, rat_sub, rat_to_f64, rational_new,
};
use crate::{RubyValue, Signal};
use num_traits::ToPrimitive;

/// One binary numeric-operator table row, shared by every numeric class
/// (`Integer`/`Float`/`Rational`/`Complex`): check arity, compute the operation
/// via `numeric::<num_fn>`, and hand the result to `num_coerce_bin`, which
/// applies `coerce`/`TypeError` against a non-numeric operand. The class isn't a
/// parameter -- `num_coerce_bin` derives the "can't be coerced into <Class>"
/// message from the receiver's own type -- so all four classes' rows were
/// byte-identical; this is the one definition they now share.
macro_rules! num_op_row {
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
pub(crate) use num_op_row;

/// The lane a VALUE occupies (`Int` covers both Integer payloads).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NumLane {
    Int,
    Rat,
    Flo,
    Cpx,
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
        int($ia:ident, $ib:ident) => $int:expr,
        rat($ra:ident, $rb:ident) => $rat:expr,
        flo($fa:ident, $fb:ident) => $flo:expr,
        cpx($ca:ident, $cb:ident) => $cpx:expr $(,)?
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
    cpx(_x, _y) => panic!("Complex has no modulo (NoMethodError in real Ruby; spike scope: raised as a panic)"),
);

tower_binop!(
    /// `**` -- the Integer lane is FALLIBLE beyond overflow: a negative
    /// exponent is a Rational result, `0 ** -n` raises ZeroDivisionError.
    num_pow,
    int(x, y) => int_pow(x, y),
    rat(x, y) => rat_pow(x, y),
    flo(x, y) => Ok(RubyValue::Float(x.powf(y))),
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

/// `a <=> b` across the tower. Outer `None` = not a numeric pair; inner
/// `None` = Ruby's `nil` (a NaN comparison, or any Complex operand --
/// complexes have no ordering).
pub fn num_cmp(a: &RubyValue, b: &RubyValue) -> Option<Option<i64>> {
    let joined = lane(a)?.max(lane(b)?);
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

/// Seeds `Float::INFINITY`/`NAN`/`EPSILON`/... and `Math::PI`/`E` into the
/// constant store -- called once from generated `main()` alongside the
/// registry install (the 15.3 `const_owners` read path resolves
/// `Float::INFINITY` to owner `FLOAT_CLASS` at compile time; only the
/// VALUES need seeding here). Values are CRuby's exactly (oracle-verified).
pub fn seed_numeric_constants() {
    use crate::const_set;
    let float = spinel_abi::FLOAT_CLASS.0;
    const_set(float, "INFINITY", RubyValue::Float(f64::INFINITY));
    const_set(float, "NAN", RubyValue::Float(f64::NAN));
    const_set(float, "EPSILON", RubyValue::Float(f64::EPSILON));
    const_set(float, "MAX", RubyValue::Float(f64::MAX));
    const_set(float, "MIN", RubyValue::Float(f64::MIN_POSITIVE));
    const_set(float, "DIG", RubyValue::Int(15));
    const_set(float, "MANT_DIG", RubyValue::Int(53));
    const_set(float, "MAX_EXP", RubyValue::Int(1024));
    const_set(float, "MIN_EXP", RubyValue::Int(-1021));
    const_set(float, "MAX_10_EXP", RubyValue::Int(308));
    const_set(float, "MIN_10_EXP", RubyValue::Int(-307));
    const_set(float, "RADIX", RubyValue::Int(2));
    // `Complex::I` -- the imaginary unit, `Complex(0, 1)`.
    const_set(
        spinel_abi::COMPLEX_CLASS.0,
        "I",
        crate::builtins::complex::complex_from_literal(RubyValue::Int(1)),
    );
    let math = spinel_abi::MATH_CLASS.0;
    const_set(math, "PI", RubyValue::Float(std::f64::consts::PI));
    const_set(math, "E", RubyValue::Float(std::f64::consts::E));
}

/// Infallible views for Complex's internal component arithmetic (whose
/// operands are numeric by construction).
pub(crate) fn num_add_or_panic(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    num_add(a, b).expect("complex components are numeric by construction")
}
pub(crate) fn num_sub_or_panic(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    num_sub(a, b).expect("complex components are numeric by construction")
}
pub(crate) fn num_mul_or_panic(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    num_mul(a, b).expect("complex components are numeric by construction")
}

// The GENERIC Numeric rows (CRuby's `Numeric` ownership: methods defined
// once, driving the receiver's own core operations) -- found via the MRO
// walk on any numeric receiver whose own class doesn't override them.
builtin_methods! {
    pub(crate) fn lookup;

    "zero?"[0] => fn zero_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(num_eq(recv, &RubyValue::Int(0)).unwrap_or(false)))
    }
    "positive?"[0] => fn positive_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(matches!(num_cmp(recv, &RubyValue::Int(0)), Some(Some(1)))))
    }
    "negative?"[0] => fn negative_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(matches!(num_cmp(recv, &RubyValue::Int(0)), Some(Some(-1)))))
    }
    "nonzero?"[0] => fn nonzero_p(recv, args, _block) {
        arity!(args, 0);
        Ok(if num_eq(recv, &RubyValue::Int(0)).unwrap_or(false) {
            RubyValue::Nil
        } else {
            recv.clone()
        })
    }
    "integer?"[0] => fn integer_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(matches!(recv, RubyValue::Int(_) | RubyValue::BigInt(_))))
    }
    "real?"[0] => fn real_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(!matches!(recv, RubyValue::Complex(_))))
    }
    "real"[0] => fn real(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "imag"[0] | "imaginary"[0] => fn imag(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(0))
    }
    "to_c"[0] => fn to_c(recv, args, _block) {
        arity!(args, 0);
        crate::builtins::complex::complex_new(recv.clone(), RubyValue::Int(0))
    }
    // A real number's cartesian view is `[self, 0]`.
    "rect"[0] | "rectangular"[0] => fn rect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Array(crate::array_new(vec![recv.clone(), RubyValue::Int(0)])))
    }
    // Polar view: magnitude `|self|`, angle `0` (non-negative) or `pi` (negative).
    "polar"[0] => fn polar(recv, args, _block) {
        arity!(args, 0);
        let magnitude =
            crate::dispatch::send_value(recv, crate::Symbol::intern("abs"), &[], None)?;
        let angle = if matches!(num_cmp(recv, &RubyValue::Int(0)), Some(Some(-1))) {
            RubyValue::Float(std::f64::consts::PI)
        } else {
            RubyValue::Int(0)
        };
        Ok(RubyValue::Array(crate::array_new(vec![magnitude, angle])))
    }
    "abs2"[0] => fn abs2(recv, args, _block) {
        arity!(args, 0);
        num_mul(recv, recv).expect("numeric receiver")
    }
    "conj"[0] | "conjugate"[0] => fn conj(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "angle"[0] | "arg"[0] | "phase"[0] => fn arg(recv, args, _block) {
        arity!(args, 0);
        // 0 for non-negative reals, pi for negative (a Float in real Ruby
        // only for the negative case; 0 stays Integer).
        Ok(if matches!(num_cmp(recv, &RubyValue::Int(0)), Some(Some(-1))) {
            RubyValue::Float(std::f64::consts::PI)
        } else {
            RubyValue::Int(0)
        })
    }
    "divmod"[1] => fn divmod(recv, args, _block) {
        arity!(args, 1);
        let q = num_div(recv, &args[0])
            .ok_or_else(|| coercion_error(recv, &args[0]))??;
        let r = num_mod(recv, &args[0])
            .ok_or_else(|| coercion_error(recv, &args[0]))??;
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
            other => other,
        };
        Ok(RubyValue::Array(crate::array_new(vec![q, r])))
    }
    "fdiv"[1] => fn fdiv(recv, args, _block) {
        arity!(args, 1);
        if lane(&args[0]).is_none() || matches!(args[0], RubyValue::Complex(_)) {
            return Err(coercion_error(recv, &args[0]));
        }
        Ok(RubyValue::Float(
            num_to_f64_unchecked(recv) / num_to_f64_unchecked(&args[0]),
        ))
    }
    "quo"[1] => fn quo(recv, args, _block) {
        arity!(args, 1);
        num_quo(recv, &args[0]).ok_or_else(|| coercion_error(recv, &args[0]))?
    }
    "remainder"[1] => fn remainder(recv, args, _block) {
        arity!(args, 1);
        // a - b*(a/b).truncate -- the truncated-division counterpart of %
        // (sign follows the DIVIDEND). BigInt's own `/` truncates.
        match (recv, &args[0]) {
            (
                RubyValue::Int(_) | RubyValue::BigInt(_),
                RubyValue::Int(_) | RubyValue::BigInt(_),
            ) => {
                if crate::builtins::integer::int_is_zero(&args[0]) {
                    return Err(crate::dispatch::raise_error(
                        "ZeroDivisionError",
                        "divided by 0".to_string(),
                    ));
                }
                let a = crate::builtins::integer::to_bigint(recv);
                let b = crate::builtins::integer::to_bigint(&args[0]);
                Ok(crate::builtins::integer::int_value(&a - &b * (&a / &b)))
            }
            _ => {
                if lane(&args[0]).is_none() {
                    return Err(coercion_error(recv, &args[0]));
                }
                let (a, b) = (num_to_f64_unchecked(recv), num_to_f64_unchecked(&args[0]));
                Ok(RubyValue::Float(a - b * (a / b).trunc()))
            }
        }
    }
    // `step(limit, step = 1)`; the blockless form returns an Enumerator
    // (Phase 17.2). Drives the tower generically, so `1.step(2.0, 0.5)`
    // works too.
    "step" => fn step(recv, args, block) {
        arity!(args, 0..=2);
        let p = block_or_enum!(recv, "step", args, block);
        // `step` accepts positional (`1.step(10, 2)`) and/or keyword
        // (`1.step(by: 2, to: 10)`) forms; a trailing Hash carries `:by`/`:to`.
        let mut positional = args;
        let mut limit: Option<RubyValue> = None;
        let mut step = RubyValue::Int(1);
        if let Some(RubyValue::Hash(h)) = args.last() {
            let by = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("by")));
            let to = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("to")));
            if !matches!(by, RubyValue::Nil) {
                step = by;
            }
            if !matches!(to, RubyValue::Nil) {
                limit = Some(to);
            }
            positional = &args[..args.len() - 1];
        }
        if let Some(l) = positional.first() {
            limit = Some(l.clone());
        }
        if let Some(s) = positional.get(1) {
            step = s.clone();
        }
        // A zero step never advances `cur`, so the loop below would spin
        // forever calling the block. CRuby rejects it up front in both the
        // block and blockless paths (`num_step_check_fix_args`, numeric.c:2888
        // and `num_step`, numeric.c:3036). The comparison is a Ruby-level
        // `==` there (`rb_equal`), so `0.0` and `Rational(0, 1)` are rejected
        // too -- hence `num_cmp` rather than a native `== 0` test.
        if matches!(num_cmp(&step, &RubyValue::Int(0)), Some(Some(0))) {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "step can't be 0".to_string(),
            ));
        }
        let descending = matches!(num_cmp(&step, &RubyValue::Int(0)), Some(Some(-1)));
        // CRuby's rule: a Float limit OR step moves the WHOLE iteration
        // into the Float domain (`1.step(2.0, 0.5)` yields 1.0 first).
        let mut cur = if matches!(limit, Some(RubyValue::Float(_)))
            || matches!(step, RubyValue::Float(_))
        {
            RubyValue::Float(num_to_f64_unchecked(recv))
        } else {
            recv.clone()
        };
        loop {
            // An absent limit (`1.step(by: 2)`) is an unbounded sequence.
            if let Some(limit) = &limit {
                match num_cmp(&cur, limit) {
                    Some(Some(c)) if (!descending && c > 0) || (descending && c < 0) => break,
                    Some(Some(_)) => {}
                    _ => break,
                }
            }
            p.call(std::slice::from_ref(&cur))?;
            cur = num_add(&cur, &step).expect("numeric step operands")?;
        }
        Ok(recv.clone())
    }
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
        let pair =
            crate::dispatch::send_value(arg, coerce, std::slice::from_ref(recv), None)?;
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
        return Err(crate::dispatch::raise_error(
            "TypeError",
            "coerce must return [x, y]".to_string(),
        ));
    }
    Err(coercion_error(recv, arg))
}

/// The coercion TypeError a generic Numeric row raises (named by the
/// RECEIVER's class, CRuby's shape).
fn coercion_error(recv: &RubyValue, arg: &RubyValue) -> Signal {
    crate::dispatch::raise_error(
        "TypeError",
        format!(
            "{} can't be coerced into {}",
            crate::builtins::class_name_of(arg),
            crate::builtins::class_name_of(recv)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::integer::int_value;
    use num_bigint::BigInt;

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
        assert!(matches!(&r, RubyValue::Rational(q) if q.num == BigInt::from(3) && q.den == BigInt::from(2)));
        // Rat + Float promotes to Float: (1/2) + 0.5 == 1.0
        let r = num_add(&rat(1, 2), &RubyValue::Float(0.5)).unwrap().unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f == 1.0));
        // Anything + Complex is Complex.
        let c = crate::builtins::complex::complex_new(RubyValue::Int(1), RubyValue::Int(2)).unwrap();
        let r = num_add(&RubyValue::Int(1), &c).unwrap().unwrap();
        assert!(matches!(r, RubyValue::Complex(_)));
    }

    #[test]
    fn int_lane_stays_exact_and_promotes_on_overflow() {
        let r = num_add(&RubyValue::Int(i64::MAX), &RubyValue::Int(1)).unwrap().unwrap();
        assert!(matches!(r, RubyValue::BigInt(_)));
        assert_eq!(num_cmp(&big("100000000000000000000"), &RubyValue::Int(5)).unwrap(), Some(1));
    }

    #[test]
    fn quo_is_rational_preserving() {
        let r = num_quo(&RubyValue::Int(1), &RubyValue::Int(3)).unwrap().unwrap();
        assert!(matches!(&r, RubyValue::Rational(q) if q.num == BigInt::from(1) && q.den == BigInt::from(3)));
        let r = num_quo(&RubyValue::Int(4), &RubyValue::Int(2)).unwrap().unwrap();
        assert!(matches!(&r, RubyValue::Rational(q) if q.den == BigInt::from(1)));
    }

    #[test]
    fn eq_and_cmp_cross_every_real_lane() {
        assert_eq!(num_eq(&RubyValue::Int(1), &RubyValue::Float(1.0)), Some(true));
        assert_eq!(num_eq(&rat(2, 1), &RubyValue::Int(2)), Some(true));
        assert_eq!(num_eq(&rat(1, 2), &RubyValue::Float(0.5)), Some(true));
        assert_eq!(num_cmp(&RubyValue::Int(2), &RubyValue::Float(1.5)).unwrap(), Some(1));
        assert_eq!(num_cmp(&rat(1, 2), &rat(2, 3)).unwrap(), Some(-1));
        assert_eq!(
            num_cmp(&RubyValue::Float(f64::NAN), &RubyValue::Int(1)).unwrap(),
            None
        );
        let c = crate::builtins::complex::complex_new(RubyValue::Int(2), RubyValue::Int(0)).unwrap();
        assert_eq!(num_eq(&c, &RubyValue::Int(2)), Some(true));
        assert_eq!(num_cmp(&c, &RubyValue::Int(2)).unwrap(), None);
    }

    #[test]
    fn rational_modulo_is_exact() {
        // Rational(7,2) % 2 == (3/2) (floored, oracle rule a - b*(a/b).floor)
        let r = num_mod(&rat(7, 2), &RubyValue::Int(2)).unwrap().unwrap();
        assert!(matches!(&r, RubyValue::Rational(q) if q.num == BigInt::from(3) && q.den == BigInt::from(2)));
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
            step(&RubyValue::Int(1), &[RubyValue::Int(10), RubyValue::Int(3)], Some(RubyValue::Proc(p))).unwrap();
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
        let r = num_div(&RubyValue::Float(1.0), &RubyValue::Int(0)).unwrap().unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f.is_infinite()));
    }
}
