//! `Integer` (CRuby numeric.c + bignum.c) -- ONE Ruby class, two payloads:
//! `RubyValue::Int(i64)` for the fixnum range, `RubyValue::BigInt` beyond
//! it (Phase 17.1's full-bignum decision). Every operation here takes
//! `&RubyValue` pairs whose Integer-ness the caller has proven (statically
//! via `TyKind::Int`, or dynamically via the table keying) -- the
//! `#[inline]` small-small fast half costs what the old raw-`i64` helpers
//! did, with a `#[cold]` bignum half behind it.
//!
//! The ONE construction invariant: `int_value` demotes every `BigInt`
//! result that fits back into `Int`, so a big payload never aliases a
//! fixnum value (equality/hashing/matching stay canonical).

use crate::builtins::{arity, builtin_methods};
use crate::{RubyValue, Signal};
use num_bigint::BigInt;
use num_integer::Integer as _;
use num_traits::{Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::sync::Arc;

/// THE Integer constructor: demotes to `Int` whenever the value fits i64.
pub fn int_value(n: BigInt) -> RubyValue {
    match n.to_i64() {
        Some(i) => RubyValue::Int(i),
        None => RubyValue::BigInt(Arc::new(n)),
    }
}

/// A prism integer literal beyond i64 -- `(negative, LSB-first u32 digits)`
/// is exactly `ruby_prism::Integer::to_u32_digits`'s shape.
pub fn int_from_u32_digits(negative: bool, digits: &[u32]) -> RubyValue {
    let sign = if negative {
        num_bigint::Sign::Minus
    } else {
        num_bigint::Sign::Plus
    };
    int_value(BigInt::from_slice(sign, digits))
}

/// The `BigInt` view of a proven-Integer value (the cold half's working
/// representation).
pub(crate) fn to_bigint(v: &RubyValue) -> BigInt {
    match v {
        RubyValue::Int(i) => BigInt::from(*i),
        RubyValue::BigInt(b) => (**b).clone(),
        other => panic!("expected an Integer, got {}", other.to_display_string()),
    }
}

pub fn int_is_zero(v: &RubyValue) -> bool {
    match v {
        RubyValue::Int(i) => *i == 0,
        // The demotion invariant: a BigInt is never in i64 range, so never 0.
        RubyValue::BigInt(_) => false,
        other => panic!("expected an Integer, got {}", other.to_display_string()),
    }
}

/// Total ordering across both payloads -- by the demotion invariant a
/// `BigInt` is always outside i64 range, so its SIGN alone orders it
/// against any `Int`.
fn int_ord(a: &RubyValue, b: &RubyValue) -> Ordering {
    match (a, b) {
        (RubyValue::Int(x), RubyValue::Int(y)) => x.cmp(y),
        (RubyValue::BigInt(x), RubyValue::BigInt(y)) => x.cmp(y),
        (RubyValue::Int(_), RubyValue::BigInt(y)) => {
            if y.is_positive() {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (RubyValue::BigInt(x), RubyValue::Int(_)) => {
            if x.is_positive() {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        _ => panic!("expected Integer operands"),
    }
}

macro_rules! int_binop {
    (
        $(#[$doc:meta])*
        $name:ident, $slow:ident, $checked:ident, $op:tt
    ) => {
        $(#[$doc])*
        #[inline]
        pub fn $name(a: &RubyValue, b: &RubyValue) -> RubyValue {
            if let (RubyValue::Int(x), RubyValue::Int(y)) = (a, b) {
                if let Some(r) = x.$checked(*y) {
                    return RubyValue::Int(r);
                }
            }
            $slow(a, b)
        }
        #[cold]
        fn $slow(a: &RubyValue, b: &RubyValue) -> RubyValue {
            int_value(to_bigint(a) $op to_bigint(b))
        }
    };
}

int_binop!(
    /// `a + b`: checked i64 fast half, bignum promotion on overflow.
    int_add, int_add_slow, checked_add, +
);
int_binop!(int_sub, int_sub_slow, checked_sub, -);
int_binop!(int_mul, int_mul_slow, checked_mul, *);

/// Ruby's `Integer#/` floors toward negative infinity (`-7 / 2 == -4`).
/// PANICS on a zero divisor -- codegen's static path pre-guards with a
/// `ZeroDivisionError` raise, and the dynamic rows check via
/// `num_div`/`int_is_zero` first.
#[inline]
pub fn int_div(a: &RubyValue, b: &RubyValue) -> RubyValue {
    if let (RubyValue::Int(x), RubyValue::Int(y)) = (a, b) {
        if *y == 0 {
            panic!("divided by 0");
        }
        // i64::MIN / -1 is the one overflowing case -- fall through to it.
        if !(*x == i64::MIN && *y == -1) {
            return RubyValue::Int(x.div_floor(y));
        }
    }
    int_div_slow(a, b)
}
#[cold]
fn int_div_slow(a: &RubyValue, b: &RubyValue) -> RubyValue {
    let d = to_bigint(b);
    if d.is_zero() {
        panic!("divided by 0");
    }
    int_value(to_bigint(a).div_floor(&d))
}

/// Ruby's `Integer#%` takes the sign of the divisor (floored modulo). Same
/// zero-divisor contract as `int_div`.
#[inline]
pub fn int_mod(a: &RubyValue, b: &RubyValue) -> RubyValue {
    if let (RubyValue::Int(x), RubyValue::Int(y)) = (a, b) {
        if *y == 0 {
            panic!("divided by 0");
        }
        if !(*x == i64::MIN && *y == -1) {
            return RubyValue::Int(x.mod_floor(y));
        }
    }
    int_mod_slow(a, b)
}
#[cold]
fn int_mod_slow(a: &RubyValue, b: &RubyValue) -> RubyValue {
    let d = to_bigint(b);
    if d.is_zero() {
        panic!("divided by 0");
    }
    int_value(to_bigint(a).mod_floor(&d))
}

/// `a ** b` -- fallible: a NEGATIVE exponent is a Rational result
/// (`2 ** -2 == (1/4)`, oracle-verified), `0 ** -n` raises
/// ZeroDivisionError, and an exponent beyond u32 range mirrors CRuby's
/// "may be too big" posture by answering Float::INFINITY for |a| > 1
/// (the warning itself is a documented scope-cut).
pub fn int_pow(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    // Small-small fast half.
    if let (RubyValue::Int(x), RubyValue::Int(y)) = (a, b) {
        if *y >= 0 {
            if let Ok(exp) = u32::try_from(*y) {
                if let Some(r) = x.checked_pow(exp) {
                    return Ok(RubyValue::Int(r));
                }
            }
        }
    }
    let exp = to_bigint(b);
    if exp.is_negative() {
        // a ** -n == Rational(1, a ** n).
        let base = to_bigint(a);
        if base.is_zero() {
            return Err(crate::dispatch::raise_error(
                "ZeroDivisionError",
                "divided by 0".to_string(),
            ));
        }
        let pos = int_pow(a, &int_value(-exp))?;
        return crate::builtins::rational::rational_new(BigInt::from(1), to_bigint(&pos));
    }
    let base = to_bigint(a);
    let Some(exp) = exp.to_u32() else {
        // |a| <= 1 has closed forms; anything else is CRuby's Infinity.
        return Ok(match base.to_i64() {
            Some(0) => RubyValue::Int(0),
            Some(1) => RubyValue::Int(1),
            Some(-1) => RubyValue::Int(if to_bigint(b).is_even() { 1 } else { -1 }),
            _ => RubyValue::Float(f64::INFINITY),
        });
    };
    Ok(int_value(num_traits::pow::Pow::pow(base, exp)))
}

/// Bitwise ops -- `BigInt`'s own `& | ^` already implement Ruby's
/// infinite-two's-complement semantics for negatives.
#[inline]
pub fn int_band(a: &RubyValue, b: &RubyValue) -> RubyValue {
    if let (RubyValue::Int(x), RubyValue::Int(y)) = (a, b) {
        return RubyValue::Int(x & y);
    }
    int_value(to_bigint(a) & to_bigint(b))
}
#[inline]
pub fn int_bor(a: &RubyValue, b: &RubyValue) -> RubyValue {
    if let (RubyValue::Int(x), RubyValue::Int(y)) = (a, b) {
        return RubyValue::Int(x | y);
    }
    int_value(to_bigint(a) | to_bigint(b))
}
#[inline]
pub fn int_bxor(a: &RubyValue, b: &RubyValue) -> RubyValue {
    if let (RubyValue::Int(x), RubyValue::Int(y)) = (a, b) {
        return RubyValue::Int(x ^ y);
    }
    int_value(to_bigint(a) ^ to_bigint(b))
}

/// Ruby's `Integer#~` (`~a == -(a + 1)`).
pub fn int_bnot(a: &RubyValue) -> RubyValue {
    match a {
        RubyValue::Int(x) => RubyValue::Int(!x),
        _ => int_value(!to_bigint(a)),
    }
}

pub fn int_neg(a: &RubyValue) -> RubyValue {
    match a {
        RubyValue::Int(x) => match x.checked_neg() {
            Some(r) => RubyValue::Int(r),
            None => int_value(-BigInt::from(*x)),
        },
        _ => int_value(-to_bigint(a)),
    }
}

pub fn int_pos(a: &RubyValue) -> RubyValue {
    a.clone()
}

/// `a << b` / `a >> b` -- a negative amount redirects to the opposite
/// shift (Ruby's rule); an amount beyond `u32` raises RangeError ("shift
/// width too big"). Left shifts promote through bignum freely.
pub fn int_shl(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let amount = to_bigint(b);
    if amount.is_negative() {
        return int_shr(a, &int_value(-amount));
    }
    let Some(amount) = amount.to_u32() else {
        return Err(crate::dispatch::raise_error(
            "RangeError",
            "shift width too big".to_string(),
        ));
    };
    if let RubyValue::Int(x) = a {
        if let Some(r) = x.checked_shl(amount) {
            // checked_shl wraps the AMOUNT, not the value -- verify the
            // round trip really was lossless before taking the fast path.
            if amount < 64 && (r >> amount) == *x {
                return Ok(RubyValue::Int(r));
            }
        }
    }
    Ok(int_value(to_bigint(a) << amount as usize))
}

pub fn int_shr(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let amount = to_bigint(b);
    if amount.is_negative() {
        return int_shl(a, &int_value(-amount));
    }
    let Some(amount) = amount.to_u32() else {
        // Shifting everything out: 0 for non-negative, -1 for negative
        // (arithmetic shift), real Ruby's limit behavior.
        let neg = matches!(int_ord(a, &RubyValue::Int(0)), Ordering::Less);
        return Ok(RubyValue::Int(if neg { -1 } else { 0 }));
    };
    if let RubyValue::Int(x) = a {
        if amount >= 64 {
            return Ok(RubyValue::Int(if *x < 0 { -1 } else { 0 }));
        }
        return Ok(RubyValue::Int(x >> amount));
    }
    Ok(int_value(to_bigint(a) >> amount as usize))
}

pub fn int_eq(a: &RubyValue, b: &RubyValue) -> bool {
    int_ord(a, b) == Ordering::Equal
}
pub fn int_neq(a: &RubyValue, b: &RubyValue) -> bool {
    int_ord(a, b) != Ordering::Equal
}
pub fn int_lt(a: &RubyValue, b: &RubyValue) -> bool {
    int_ord(a, b) == Ordering::Less
}
pub fn int_gt(a: &RubyValue, b: &RubyValue) -> bool {
    int_ord(a, b) == Ordering::Greater
}
pub fn int_le(a: &RubyValue, b: &RubyValue) -> bool {
    int_ord(a, b) != Ordering::Greater
}
pub fn int_ge(a: &RubyValue, b: &RubyValue) -> bool {
    int_ord(a, b) != Ordering::Less
}

/// `Integer#<=>` over proven-Integer operands: -1/0/1, never nil.
pub fn int_cmp(a: &RubyValue, b: &RubyValue) -> i64 {
    int_ord(a, b) as i64
}

/// The coercion TypeError every operator row raises for a non-numeric
/// operand -- CRuby's exact shape (`5 + "x"` -> `String can't be coerced
/// into Integer`).
fn coerce_error(arg: &RubyValue, into: &str) -> Signal {
    crate::dispatch::raise_error(
        "TypeError",
        format!(
            "{} can't be coerced into {into}",
            crate::builtins::class_name_of(arg)
        ),
    )
}

macro_rules! int_op_row {
    ($args:ident, $recv:ident, $num_fn:ident) => {{
        arity!($args, 1);
        match crate::builtins::numeric::$num_fn($recv, &$args[0]) {
            Some(r) => r,
            None => Err(coerce_error(&$args[0], "Integer")),
        }
    }};
}

builtin_methods! {
    pub(crate) fn lookup;

    "+" => fn add(recv, args, _block) { int_op_row!(args, recv, num_add) }
    "-" => fn sub(recv, args, _block) { int_op_row!(args, recv, num_sub) }
    "*" => fn mul(recv, args, _block) { int_op_row!(args, recv, num_mul) }
    "/" => fn div(recv, args, _block) { int_op_row!(args, recv, num_div) }
    "%" | "modulo" => fn modulo(recv, args, _block) { int_op_row!(args, recv, num_mod) }
    "**" => fn pow(recv, args, _block) { int_op_row!(args, recv, num_pow) }
    "&" => fn band(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_band(recv, &args[0])),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "|" => fn bor(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_bor(recv, &args[0])),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "^" => fn bxor(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_bxor(recv, &args[0])),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "<<" => fn shl(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => int_shl(recv, &args[0]),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    ">>" => fn shr(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => int_shr(recv, &args[0]),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "-@" => fn neg(recv, args, _block) {
        arity!(args, 0);
        Ok(int_neg(recv))
    }
    "+@" => fn pos(recv, args, _block) {
        arity!(args, 0);
        Ok(int_pos(recv))
    }
    "~" => fn bnot(recv, args, _block) {
        arity!(args, 0);
        Ok(int_bnot(recv))
    }
    // Numeric-tower comparison; a non-numeric argument compares as nil
    // (real Ruby: `5 <=> "a"` is nil, never an error). Comparable's
    // operators drive this row.
    "<=>" => fn spaceship(recv, args, _block) {
        arity!(args, 1);
        Ok(match crate::builtins::numeric::num_cmp(recv, &args[0]) {
            Some(Some(c)) => RubyValue::Int(c),
            _ => RubyValue::Nil,
        })
    }
    // Integer's own `==` (cross-tower: `1 == 1.0` is true) -- resolving
    // before `Comparable#==` in the chain. Non-numeric -> false.
    "==" => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    "abs" | "magnitude" => fn abs(recv, args, _block) {
        arity!(args, 0);
        Ok(match recv {
            RubyValue::Int(i) if *i >= 0 => recv.clone(),
            _ => {
                let b = to_bigint(recv);
                int_value(b.abs())
            }
        })
    }
    "even?" => fn even_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(to_bigint(recv).is_even()))
    }
    "odd?" => fn odd_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(to_bigint(recv).is_odd()))
    }
    "succ" | "next" => fn succ(recv, args, _block) {
        arity!(args, 0);
        Ok(int_add(recv, &RubyValue::Int(1)))
    }
    "pred" => fn pred(recv, args, _block) {
        arity!(args, 0);
        Ok(int_sub(recv, &RubyValue::Int(1)))
    }
    // ASCII only: our strings are UTF-8, so a 128..=255 chr would change
    // byte representation -- rejected loudly (spike scope), not silently
    // re-encoded. Out of byte range is real Ruby's RangeError.
    "chr" => fn chr(recv, args, _block) {
        arity!(args, 0);
        let RubyValue::Int(i) = recv else {
            return Err(crate::dispatch::raise_error(
                "RangeError",
                format!("{} out of char range", recv.to_display_string()),
            ));
        };
        match u8::try_from(*i) {
            Ok(b) if b < 128 => Ok(RubyValue::Str(crate::string_new(
                (b as char).to_string(),
            ))),
            Ok(_) => panic!("Integer#chr beyond ASCII isn't supported (UTF-8 strings; spike scope)"),
            Err(_) => Err(crate::dispatch::raise_error(
                "RangeError",
                format!("{i} out of char range"),
            )),
        }
    }
    "ord" | "to_i" | "to_int" => fn ord(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "to_f" => fn to_f(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(crate::builtins::numeric::num_to_f64_unchecked(recv)))
    }
    "to_r" => fn to_r(recv, args, _block) {
        arity!(args, 0);
        crate::builtins::rational::rational_new(to_bigint(recv), BigInt::from(1))
    }
    "numerator" => fn numerator(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "denominator" => fn denominator(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(1))
    }
    // `to_s(base)` / bare `to_s`; `inspect` is the same rendering.
    "to_s" | "inspect" => fn to_s(recv, args, _block) {
        arity!(args, 0..=1);
        let base = match args.first() {
            Some(RubyValue::Int(b)) if (2..=36).contains(b) => *b as u32,
            Some(RubyValue::Int(b)) => {
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    format!("invalid radix {b}"),
                ))
            }
            Some(other) => {
                return Err(coerce_error(other, "Integer"));
            }
            None => 10,
        };
        Ok(RubyValue::Str(crate::string_new(
            to_bigint(recv).to_str_radix(base),
        )))
    }
    "digits" => fn digits(recv, args, _block) {
        arity!(args, 0..=1);
        let base = match args.first() {
            Some(RubyValue::Int(b)) if *b >= 2 => BigInt::from(*b),
            Some(RubyValue::Int(b)) => {
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    format!("invalid radix {b}"),
                ))
            }
            Some(other) => return Err(coerce_error(other, "Integer")),
            None => BigInt::from(10),
        };
        let mut n = to_bigint(recv);
        if n.is_negative() {
            return Err(crate::dispatch::raise_error(
                "Math::DomainError",
                "out of domain".to_string(),
            ));
        }
        let mut out = Vec::new();
        loop {
            let (q, r) = n.div_mod_floor(&base);
            out.push(int_value(r));
            n = q;
            if n.is_zero() {
                break;
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "gcd" => fn gcd(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => {
                Ok(int_value(to_bigint(recv).gcd(&to_bigint(&args[0]))))
            }
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "lcm" => fn lcm(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => {
                Ok(int_value(to_bigint(recv).lcm(&to_bigint(&args[0]))))
            }
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "gcdlcm" => fn gcdlcm(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => {
                let (a, b) = (to_bigint(recv), to_bigint(&args[0]));
                Ok(RubyValue::Array(crate::array_new(vec![
                    int_value(a.gcd(&b)),
                    int_value(a.lcm(&b)),
                ])))
            }
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "bit_length" => fn bit_length(recv, args, _block) {
        arity!(args, 0);
        let n = to_bigint(recv);
        // CRuby: bits needed excluding the sign (negative x measures ~x).
        let measured = if n.is_negative() { !n } else { n };
        Ok(RubyValue::Int(measured.bits() as i64))
    }
    "size" => fn size(_recv, args, _block) {
        arity!(args, 0);
        // Machine-word size for fixnums; bignums report their limb bytes
        // in CRuby -- 8 is the honest fixnum answer, kept uniform here.
        Ok(RubyValue::Int(8))
    }
    // `pow(e)` == `**`; `pow(e, m)` is modular exponentiation.
    "pow" => fn pow_m(recv, args, _block) {
        arity!(args, 1..=2);
        match args.len() {
            1 => match crate::builtins::numeric::num_pow(recv, &args[0]) {
                Some(r) => r,
                None => Err(coerce_error(&args[0], "Integer")),
            },
            _ => {
                let (RubyValue::Int(_) | RubyValue::BigInt(_), RubyValue::Int(_) | RubyValue::BigInt(_)) =
                    (&args[0], &args[1])
                else {
                    return Err(crate::dispatch::raise_error(
                        "TypeError",
                        "Integer#pow() 2nd argument not allowed unless all arguments are integers".to_string(),
                    ));
                };
                let e = to_bigint(&args[0]);
                if e.is_negative() {
                    return Err(crate::dispatch::raise_error(
                        "RangeError",
                        "Integer#pow() 1st argument cannot be negative when 2nd argument specified".to_string(),
                    ));
                }
                let m = to_bigint(&args[1]);
                if m.is_zero() {
                    return Err(crate::dispatch::raise_error(
                        "ZeroDivisionError",
                        "divided by 0".to_string(),
                    ));
                }
                Ok(int_value(to_bigint(recv).modpow(&e, &m)))
            }
        }
    }
    // The rounding family: a missing/non-negative ndigits is `self`; a
    // negative ndigits rounds to a power of ten with each mode's rule.
    "round" => fn round(recv, args, _block) {
        int_round_family(recv, args, RoundMode::HalfAway)
    }
    "floor" => fn floor(recv, args, _block) {
        int_round_family(recv, args, RoundMode::Floor)
    }
    "ceil" => fn ceil(recv, args, _block) {
        int_round_family(recv, args, RoundMode::Ceil)
    }
    "truncate" => fn truncate(recv, args, _block) {
        int_round_family(recv, args, RoundMode::Trunc)
    }
    // Iteration primitives, block form (blockless Enumerator forms are
    // Phase 17.2). Counts beyond i64 are physically unrunnable -- loud.
    "times" => fn times(recv, args, block) {
        arity!(args, 0);
        let Some(RubyValue::Proc(p)) = &block else {
            panic!("Integer#times without a block isn't supported (no Enumerator; spike scope)");
        };
        let RubyValue::Int(n) = recv else {
            panic!("Integer#times receiver exceeds i64 (unrunnable iteration count)");
        };
        for i in 0..*n {
            p(&[RubyValue::Int(i)])?;
        }
        Ok(recv.clone())
    }
    "upto" => fn upto(recv, args, block) {
        arity!(args, 1);
        let Some(RubyValue::Proc(p)) = &block else {
            panic!("Integer#upto without a block isn't supported (no Enumerator; spike scope)");
        };
        let (RubyValue::Int(a), RubyValue::Int(b)) = (recv, &args[0]) else {
            panic!("Integer#upto beyond i64 isn't supported (unrunnable iteration count)");
        };
        for i in *a..=*b {
            p(&[RubyValue::Int(i)])?;
        }
        Ok(recv.clone())
    }
    "downto" => fn downto(recv, args, block) {
        arity!(args, 1);
        let Some(RubyValue::Proc(p)) = &block else {
            panic!("Integer#downto without a block isn't supported (no Enumerator; spike scope)");
        };
        let (RubyValue::Int(a), RubyValue::Int(b)) = (recv, &args[0]) else {
            panic!("Integer#downto beyond i64 isn't supported (unrunnable iteration count)");
        };
        let mut i = *a;
        while i >= *b {
            p(&[RubyValue::Int(i)])?;
            i -= 1;
        }
        Ok(recv.clone())
    }
}

/// The Integer rounding family's shared core.
enum RoundMode {
    HalfAway,
    Floor,
    Ceil,
    Trunc,
}

fn int_round_family(
    recv: &RubyValue,
    args: &[RubyValue],
    mode: RoundMode,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0..=1);
    let ndigits = match args.first() {
        Some(RubyValue::Int(n)) => *n,
        Some(other) => {
            return Err(coerce_error(other, "Integer"));
        }
        None => 0,
    };
    if ndigits >= 0 {
        return Ok(recv.clone());
    }
    let p = num_traits::pow::Pow::pow(BigInt::from(10), (-ndigits) as u32);
    let a = to_bigint(recv);
    let negative = a.is_negative();
    let m = BigInt::from(a.magnitude().clone());
    let rounded_mag = match mode {
        RoundMode::HalfAway => (&m + &p / 2) / &p * &p,
        RoundMode::Trunc => &m / &p * &p,
        // Floor/Ceil depend on the SIGN: floor of a negative rounds the
        // magnitude UP, ceil of a negative truncates it.
        RoundMode::Floor => {
            if negative {
                m.div_ceil(&p) * &p
            } else {
                &m / &p * &p
            }
        }
        RoundMode::Ceil => {
            if negative {
                &m / &p * &p
            } else {
                m.div_ceil(&p) * &p
            }
        }
    };
    Ok(int_value(if negative { -rounded_mag } else { rounded_mag }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big(s: &str) -> RubyValue {
        int_value(s.parse::<BigInt>().unwrap())
    }

    #[test]
    fn int_value_always_demotes() {
        assert!(matches!(int_value(BigInt::from(5)), RubyValue::Int(5)));
        assert!(matches!(
            int_value(BigInt::from(i64::MAX)),
            RubyValue::Int(i64::MAX)
        ));
        assert!(matches!(big("9223372036854775808"), RubyValue::BigInt(_)));
    }

    #[test]
    fn overflow_promotes_and_round_trips() {
        let r = int_add(&RubyValue::Int(i64::MAX), &RubyValue::Int(1));
        assert!(matches!(r, RubyValue::BigInt(_)));
        // ... and demotes back when the value shrinks into range.
        let back = int_sub(&r, &RubyValue::Int(1));
        assert!(matches!(back, RubyValue::Int(i64::MAX)));
    }

    #[test]
    fn floored_division_matrix_matches_ruby() {
        let d = |a: i64, b: i64| match int_div(&RubyValue::Int(a), &RubyValue::Int(b)) {
            RubyValue::Int(i) => i,
            _ => panic!(),
        };
        let m = |a: i64, b: i64| match int_mod(&RubyValue::Int(a), &RubyValue::Int(b)) {
            RubyValue::Int(i) => i,
            _ => panic!(),
        };
        assert_eq!(d(-7, 2), -4);
        assert_eq!(m(-7, 3), 2);
        assert_eq!(m(7, -3), -2);
        // The i64::MIN / -1 overflow edge takes the bignum path.
        let r = int_div(&RubyValue::Int(i64::MIN), &RubyValue::Int(-1));
        assert!(matches!(r, RubyValue::BigInt(_)));
    }

    #[test]
    fn pow_covers_rational_zero_and_huge_exponents() {
        let r = int_pow(&RubyValue::Int(2), &RubyValue::Int(10)).unwrap();
        assert!(matches!(r, RubyValue::Int(1024)));
        let r = int_pow(&RubyValue::Int(2), &RubyValue::Int(100)).unwrap();
        assert!(matches!(r, RubyValue::BigInt(_)));
        let r = int_pow(&RubyValue::Int(2), &RubyValue::Int(-2)).unwrap();
        assert!(matches!(r, RubyValue::Rational(_)));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            int_pow(&RubyValue::Int(0), &RubyValue::Int(-1))
        }));
        assert!(r.is_err()); // ZeroDivisionError, panicking registry-less
    }

    #[test]
    fn shifts_redirect_and_promote() {
        let r = int_shl(&RubyValue::Int(1), &RubyValue::Int(100)).unwrap();
        assert!(matches!(r, RubyValue::BigInt(_)));
        let back = int_shr(&r, &RubyValue::Int(100)).unwrap();
        assert!(matches!(back, RubyValue::Int(1)));
        let r = int_shl(&RubyValue::Int(8), &RubyValue::Int(-2)).unwrap();
        assert!(matches!(r, RubyValue::Int(2)));
        let r = int_shr(&RubyValue::Int(-1), &RubyValue::Int(1000)).unwrap();
        assert!(matches!(r, RubyValue::Int(-1)));
    }

    #[test]
    fn ordering_spans_payloads_by_the_demotion_invariant() {
        assert!(int_lt(&RubyValue::Int(5), &big("9223372036854775808")));
        assert!(int_gt(&RubyValue::Int(5), &big("-9223372036854775809")));
        assert_eq!(int_cmp(&big("9223372036854775808"), &RubyValue::Int(5)), 1);
        assert!(int_eq(&RubyValue::Int(5), &RubyValue::Int(5)));
        assert!(!int_eq(&RubyValue::Int(5), &big("9223372036854775808")));
    }

    #[test]
    fn bitwise_matches_twos_complement_semantics() {
        let r = int_band(&RubyValue::Int(-1), &big("18446744073709551616"));
        // -1 & 2^64 == 2^64
        assert!(int_eq(&r, &big("18446744073709551616")));
        assert!(matches!(int_bnot(&RubyValue::Int(0)), RubyValue::Int(-1)));
    }
}
