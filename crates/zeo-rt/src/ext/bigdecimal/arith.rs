//! BigDecimal arithmetic: exact add/sub/mult over the integer coefficient,
//! division to CRuby's precision rule, the rounding engine every mutation
//! funnels through, and the process-wide mode/limit state.

use super::value::{BD, ndigits, pow10};
use num_bigint::BigUint;
use num_traits::Zero;
use std::cmp::Ordering;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering as AO};

// ---------------------------------------------------------------------------
// Mode / limit state (process-wide, as CRuby's is)
// ---------------------------------------------------------------------------

pub(crate) const ROUND_UP: u32 = 1;
pub(crate) const ROUND_DOWN: u32 = 2;
pub(crate) const ROUND_HALF_UP: u32 = 3;
pub(crate) const ROUND_HALF_DOWN: u32 = 4;
pub(crate) const ROUND_CEILING: u32 = 5;
pub(crate) const ROUND_FLOOR: u32 = 6;
pub(crate) const ROUND_HALF_EVEN: u32 = 7;

pub(crate) const EXCEPTION_NAN: u32 = 2;
pub(crate) const EXCEPTION_INFINITY: u32 = 1;
pub(crate) const EXCEPTION_UNDERFLOW: u32 = 4;
pub(crate) const EXCEPTION_OVERFLOW: u32 = 1;
pub(crate) const EXCEPTION_ZERODIVIDE: u32 = 16;
pub(crate) const EXCEPTION_ALL: u32 = 255;

pub(crate) const DOUBLE_FIG: i64 = 16;

static ROUND_MODE: AtomicU32 = AtomicU32::new(ROUND_HALF_UP);
static EXCEPTION_FLAGS: AtomicU32 = AtomicU32::new(0);
static PREC_LIMIT: AtomicI64 = AtomicI64::new(0);

pub(crate) fn round_mode() -> u32 {
    ROUND_MODE.load(AO::Relaxed)
}
pub(crate) fn set_round_mode(m: u32) {
    ROUND_MODE.store(m, AO::Relaxed);
}
pub(crate) fn exception_flags() -> u32 {
    EXCEPTION_FLAGS.load(AO::Relaxed)
}
pub(crate) fn set_exception_flags(f: u32) {
    EXCEPTION_FLAGS.store(f, AO::Relaxed);
}
pub(crate) fn prec_limit() -> i64 {
    PREC_LIMIT.load(AO::Relaxed)
}
pub(crate) fn set_prec_limit(n: i64) {
    PREC_LIMIT.store(n, AO::Relaxed);
}

// ---------------------------------------------------------------------------
// Rounding
// ---------------------------------------------------------------------------

/// Rounds `coeff` down by `drop` digits under `mode`. `sign` picks the
/// direction for CEILING/FLOOR; `sticky` says nonzero digits exist BEYOND
/// the ones being dropped (a division remainder).
fn round_coeff(coeff: &BigUint, drop: u64, mode: u32, sign: i8, sticky: bool) -> BigUint {
    if drop == 0 {
        return coeff.clone();
    }
    let scale = pow10(drop);
    let (q, r) = num_integer::Integer::div_rem(coeff, &scale);
    if r.is_zero() && !sticky {
        return q;
    }
    let bump = match mode {
        ROUND_UP => true,
        ROUND_DOWN => false,
        ROUND_CEILING => sign > 0,
        ROUND_FLOOR => sign < 0,
        _ => {
            // The half modes compare the dropped tail to exactly one half.
            let half = &pow10(drop - 1) * 5u8;
            match r.cmp(&half) {
                Ordering::Greater => true,
                Ordering::Less => false,
                Ordering::Equal if sticky => true,
                Ordering::Equal => match mode {
                    ROUND_HALF_UP => true,
                    ROUND_HALF_DOWN => false,
                    // HALF_EVEN: up only when the kept digit is odd.
                    _ => num_integer::Integer::is_odd(&(&q % 2u8)),
                },
            }
        }
    };
    if bump { q + 1u8 } else { q }
}

/// `VpLeftRound`: round to `n` significant digits (from the most
/// significant). A carry that adds a digit ("999" -> "100") bumps the
/// exponent.
pub(crate) fn left_round(bd: &BD, mode: u32, n: i64) -> BD {
    let BD::Fin { sign, coeff, exp } = bd else { return bd.clone() };
    let l = ndigits(coeff) as i64;
    if n <= 0 || l <= n {
        return bd.clone();
    }
    let rounded = round_coeff(coeff, (l - n) as u64, mode, *sign, false);
    let new_l = ndigits(&rounded) as i64;
    BD::fin(*sign, rounded, exp + (new_l - n))
}

/// `VpMidRound`: round at `nf` digits after the decimal point (`nf` may be
/// negative -- `round(-1)` works in tens).
pub(crate) fn mid_round(bd: &BD, mode: u32, nf: i64) -> BD {
    let BD::Fin { sign, coeff, exp } = bd else { return bd.clone() };
    let l = ndigits(coeff) as i64;
    // Digits kept: everything down to 10^-nf, i.e. exp + nf of them.
    let keep = exp + nf;
    if keep >= l {
        return bd.clone();
    }
    if keep <= 0 {
        // Every digit is dropped; the value collapses to zero or to one
        // unit at the rounding position (`round(0.5)` is 1, `round(0.4)`
        // is 0). Relative to that unit the value is `0.<coeff> * 10^keep`,
        // so a half mode can only round up when `keep == 0`.
        let first = first_digit(coeff);
        let bump = match mode {
            ROUND_UP => true,
            ROUND_CEILING => *sign > 0,
            ROUND_FLOOR => *sign < 0,
            ROUND_DOWN => false,
            _ if keep < 0 || first < 5 => false,
            _ if first > 5 || !tail_is_exact_half(coeff) => true,
            // Exactly one half: HALF_UP goes up, HALF_DOWN down, and
            // HALF_EVEN keeps the (even) zero.
            _ => mode == ROUND_HALF_UP,
        };
        return if bump {
            BD::fin(*sign, BigUint::from(1u8), -nf + 1)
        } else {
            BD::Zero(*sign)
        };
    }
    let rounded = round_coeff(coeff, (l - keep) as u64, mode, *sign, false);
    if rounded.is_zero() {
        return BD::Zero(*sign);
    }
    let new_l = ndigits(&rounded) as i64;
    BD::fin(*sign, rounded, exp + (new_l - keep))
}

fn first_digit(coeff: &BigUint) -> u8 {
    coeff.to_str_radix(10).as_bytes()[0] - b'0'
}

fn tail_is_exact_half(coeff: &BigUint) -> bool {
    // "5", "50", ... -- exactly one half at the leading position.
    let s = coeff.to_str_radix(10);
    s.as_bytes()[0] == b'5' && s[1..].bytes().all(|b| b == b'0')
}

/// `VpLimitRound`: the global significant-digit limit, applied to every
/// fresh arithmetic result.
pub(crate) fn limit_round(bd: &BD) -> BD {
    let limit = prec_limit();
    if limit <= 0 {
        return bd.clone();
    }
    left_round(bd, round_mode(), limit)
}

// ---------------------------------------------------------------------------
// Exact helpers
// ---------------------------------------------------------------------------

/// `(coeff, low_exp)` -- the value as `coeff * 10^low_exp` (integer form).
fn as_int_scaled(sign: i8, coeff: &BigUint, exp: i64) -> (i8, BigUint, i64) {
    (sign, coeff.clone(), exp - ndigits(coeff) as i64)
}

fn from_int_scaled(sign: i8, coeff: BigUint, low: i64) -> BD {
    let l = ndigits(&coeff) as i64;
    BD::fin(sign, coeff, low + l)
}

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

/// Exact `a + b` (signs included), before any limit rounding.
pub(crate) fn add(a: &BD, b: &BD) -> BD {
    match (a, b) {
        (BD::NaN, _) | (_, BD::NaN) => BD::NaN,
        (BD::Inf(x), BD::Inf(y)) => {
            if x == y {
                BD::Inf(*x)
            } else {
                BD::NaN
            }
        }
        (BD::Inf(x), _) => BD::Inf(*x),
        (_, BD::Inf(y)) => BD::Inf(*y),
        (BD::Zero(x), BD::Zero(y)) => BD::Zero(if *x < 0 && *y < 0 { -1 } else { 1 }),
        (BD::Zero(_), v) => v.clone(),
        (v, BD::Zero(_)) => v.clone(),
        (BD::Fin { sign: sa, coeff: ca, exp: ea }, BD::Fin { sign: sb, coeff: cb, exp: eb }) => {
            let (sa, ca, la) = as_int_scaled(*sa, ca, *ea);
            let (sb, cb, lb) = as_int_scaled(*sb, cb, *eb);
            let low = la.min(lb);
            let ca = &ca * pow10((la - low) as u64);
            let cb = &cb * pow10((lb - low) as u64);
            if sa == sb {
                from_int_scaled(sa, ca + cb, low)
            } else {
                match ca.cmp(&cb) {
                    Ordering::Equal => BD::Zero(1),
                    Ordering::Greater => from_int_scaled(sa, ca - cb, low),
                    Ordering::Less => from_int_scaled(sb, cb - ca, low),
                }
            }
        }
    }
}

pub(crate) fn sub(a: &BD, b: &BD) -> BD {
    add(a, &b.neg())
}

/// Exact `a * b`, before any limit rounding.
pub(crate) fn mult(a: &BD, b: &BD) -> BD {
    match (a, b) {
        (BD::NaN, _) | (_, BD::NaN) => BD::NaN,
        (BD::Inf(_), BD::Zero(_)) | (BD::Zero(_), BD::Inf(_)) => BD::NaN,
        (BD::Inf(x), other) | (other, BD::Inf(x)) => BD::Inf(x * other.sign_factor()),
        (BD::Zero(x), other) | (other, BD::Zero(x)) => BD::Zero(x * other.sign_factor()),
        (BD::Fin { sign: sa, coeff: ca, exp: ea }, BD::Fin { sign: sb, coeff: cb, exp: eb }) => {
            let (sa, ca, la) = as_int_scaled(*sa, ca, *ea);
            let (sb, cb, lb) = as_int_scaled(*sb, cb, *eb);
            from_int_scaled(sa * sb, ca * cb, la + lb)
        }
    }
}

/// Division to `ix` significant digits, rounded under the current mode with
/// a TRUE sticky tail (the exact remainder decides half cases) -- what
/// CRuby's remainder-nudge approximates. Callers gate zero/special cases.
pub(crate) fn div_to(a: &BD, b: &BD, ix: i64) -> BD {
    let (BD::Fin { sign: sa, coeff: ca, exp: ea }, BD::Fin { sign: sb, coeff: cb, exp: eb }) =
        (a, b)
    else {
        unreachable!("div_to takes finite nonzero operands")
    };
    let la = ndigits(ca) as i64;
    let lb = ndigits(cb) as i64;
    // Scale the dividend so the integer quotient carries ix+1 digits.
    let shift = ix + 1 + lb - la;
    let (num, den_shift) = if shift >= 0 {
        (ca * pow10(shift as u64), 0u64)
    } else {
        (ca.clone(), (-shift) as u64)
    };
    let den = cb * pow10(den_shift);
    let (q, r) = num_integer::Integer::div_rem(&num, &den);
    let lq = ndigits(&q) as i64;
    let exp = (ea - la) - (eb - lb) - shift + lq;
    let sign = sa * sb;
    let sticky = !r.is_zero();
    // Round at ix from the quotient's own digits (lq is ix+1 or ix+2 by
    // the shift's construction; <= ix only for an exact short quotient).
    if lq > ix {
        let rounded = round_coeff(&q, (lq - ix) as u64, round_mode(), sign, sticky);
        let new_l = ndigits(&rounded) as i64;
        BD::fin(sign, rounded, exp + (new_l - ix))
    } else {
        BD::fin(sign, q, exp)
    }
}

/// The `/` result precision: `max(a.precision, b.precision) + DOUBLE_FIG`,
/// floored at `2*DOUBLE_FIG`, capped by the global limit -- straight from
/// `BigDecimal_div2`.
pub(crate) fn default_div_prec(a: &BD, b: &BD) -> i64 {
    let mut ix = a.precision().max(b.precision()) + DOUBLE_FIG;
    if ix < 2 * DOUBLE_FIG {
        ix = 2 * DOUBLE_FIG;
    }
    let limit = prec_limit();
    if limit > 0 && limit < ix {
        ix = limit;
    }
    ix
}

/// Exact truncated integer quotient of two finite nonzero values.
pub(crate) fn trunc_quotient(a: &BD, b: &BD) -> BD {
    let (BD::Fin { sign: sa, coeff: ca, exp: ea }, BD::Fin { sign: sb, coeff: cb, exp: eb }) =
        (a, b)
    else {
        unreachable!("trunc_quotient takes finite nonzero operands")
    };
    let (sa, ca, la) = as_int_scaled(*sa, ca, *ea);
    let (sb, cb, lb) = as_int_scaled(*sb, cb, *eb);
    let d = la - lb;
    let q = if d >= 0 { (ca * pow10(d as u64)) / cb } else { ca / (cb * pow10((-d) as u64)) };
    from_int_scaled(sa * sb, q, 0)
}

/// Comparison; `None` for NaN on either side.
pub(crate) fn cmp(a: &BD, b: &BD) -> Option<Ordering> {
    match (a, b) {
        (BD::NaN, _) | (_, BD::NaN) => None,
        (BD::Inf(x), BD::Inf(y)) => Some(x.cmp(y)),
        (BD::Inf(x), _) => Some(if *x > 0 { Ordering::Greater } else { Ordering::Less }),
        (_, BD::Inf(y)) => Some(if *y > 0 { Ordering::Less } else { Ordering::Greater }),
        (BD::Zero(_), BD::Zero(_)) => Some(Ordering::Equal),
        (BD::Zero(_), v) => Some(if v.sign_factor() > 0 { Ordering::Less } else { Ordering::Greater }),
        (v, BD::Zero(_)) => Some(if v.sign_factor() > 0 { Ordering::Greater } else { Ordering::Less }),
        (BD::Fin { sign: sa, coeff: ca, exp: ea }, BD::Fin { sign: sb, coeff: cb, exp: eb }) => {
            if sa != sb {
                return Some(sa.cmp(sb));
            }
            let mag = match ea.cmp(eb) {
                Ordering::Equal => {
                    let la = ndigits(ca);
                    let lb = ndigits(cb);
                    let width = la.max(lb);
                    let ca = ca * pow10(width - la);
                    let cb = cb * pow10(width - lb);
                    ca.cmp(&cb)
                }
                other => other,
            };
            Some(if *sa > 0 { mag } else { mag.reverse() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext::bigdecimal::value::parse;

    fn s(x: &str) -> BD {
        parse(x, true).expect("valid")
    }

    fn show(bd: &BD) -> String {
        crate::ext::bigdecimal::value::to_s(bd, "")
    }

    #[test]
    fn exact_add_sub_mult() {
        assert_eq!(show(&add(&s("2"), &s("3.5"))), "0.55e1");
        assert_eq!(show(&sub(&s("1"), &s("3"))), "-0.2e1");
        assert_eq!(show(&mult(&s("2"), &s("3.5"))), "0.7e1");
        assert_eq!(show(&mult(&s("-1.5"), &s("1.5"))), "-0.225e1");
        assert_eq!(add(&s("1.5"), &s("-1.5")), BD::Zero(1));
        assert_eq!(show(&add(&s("1e50"), &s("1"))), "0.100000000000000000000000000000000000000000000000001e51");
    }

    #[test]
    fn division_matches_the_oracle_precision_rule() {
        let (a, b) = (s("1"), s("3"));
        let r = div_to(&a, &b, default_div_prec(&a, &b));
        assert_eq!(show(&r), format!("0.{}e0", "3".repeat(32)));
        let (a, b) = (s("100"), s("7"));
        let r = div_to(&a, &b, default_div_prec(&a, &b));
        assert_eq!(show(&r), "0.14285714285714285714285714285714e2");
        let (a, b) = (s("1"), s("3.00000000000000000001"));
        assert_eq!(default_div_prec(&a, &b), 37);
        // An exact quotient drops its trailing zeros.
        let (a, b) = (s("1.2345"), s("3"));
        let r = div_to(&a, &b, default_div_prec(&a, &b));
        assert_eq!(show(&r), "0.4115e0");
    }

    #[test]
    fn rounding_modes() {
        let x = s("2.345");
        assert_eq!(show(&mid_round(&x, ROUND_DOWN, 2)), "0.234e1");
        assert_eq!(show(&mid_round(&x, ROUND_UP, 2)), "0.235e1");
        assert_eq!(show(&mid_round(&s("2.5"), ROUND_HALF_UP, 0)), "0.3e1");
        assert_eq!(show(&mid_round(&s("2.5"), ROUND_HALF_EVEN, 0)), "0.2e1");
        assert_eq!(show(&mid_round(&s("3.5"), ROUND_HALF_EVEN, 0)), "0.4e1");
        assert_eq!(show(&mid_round(&s("-2.5"), ROUND_CEILING, 0)), "-0.2e1");
        assert_eq!(show(&mid_round(&s("-2.5"), ROUND_FLOOR, 0)), "-0.3e1");
        assert_eq!(show(&mid_round(&s("123.45"), ROUND_HALF_UP, -1)), "0.12e3");
        assert_eq!(show(&mid_round(&s("0.994"), ROUND_HALF_UP, 2)), "0.99e0");
        assert_eq!(show(&mid_round(&s("0.996"), ROUND_HALF_UP, 2)), "0.1e1");
        assert_eq!(mid_round(&s("0.4"), ROUND_HALF_UP, 0), BD::Zero(1));
        assert_eq!(show(&mid_round(&s("0.5"), ROUND_HALF_UP, 0)), "0.1e1");
    }

    #[test]
    fn comparisons() {
        assert_eq!(cmp(&s("1.5"), &s("2")), Some(Ordering::Less));
        assert_eq!(cmp(&s("-0"), &s("0")), Some(Ordering::Equal));
        assert_eq!(cmp(&s("NaN"), &s("1")), None);
        assert_eq!(cmp(&s("Infinity"), &s("1e999")), Some(Ordering::Greater));
        assert_eq!(cmp(&s("0.15e1"), &s("1.5")), Some(Ordering::Equal));
        assert_eq!(cmp(&s("-1.5"), &s("-1.4")), Some(Ordering::Less));
    }

    #[test]
    fn trunc_quotient_is_exact() {
        assert_eq!(show(&trunc_quotient(&s("10"), &s("3"))), "0.3e1");
        assert_eq!(show(&trunc_quotient(&s("-10"), &s("3"))), "-0.3e1");
        assert_eq!(show(&trunc_quotient(&s("10.5"), &s("3"))), "0.3e1");
    }
}
