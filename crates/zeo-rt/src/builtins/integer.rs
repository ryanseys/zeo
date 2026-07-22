//! `Integer` (CRuby numeric.c + bignum.c) -- ONE Ruby class, two payloads:
//! `RubyValue::Int(i64)` for the fixnum range, `RubyValue::BigInt` beyond
//! it (the full-bignum decision). Every operation here takes
//! `&RubyValue` pairs whose Integer-ness the caller has proven (statically
//! via `TyKind::Int`, or dynamically via the table keying) -- the
//! `#[inline]` small-small fast half costs what the old raw-`i64` helpers
//! did, with a `#[cold]` bignum half behind it.
//!
//! The ONE construction invariant: `int_value` demotes every `BigInt`
//! result that fits back into `Int`, so a big payload never aliases a
//! fixnum value (equality/hashing/matching stay canonical).

use crate::builtins::{arg_error, arity, block_or_enum, builtin_methods, range_error, type_error};
use crate::{RubyValue, Signal};
use num_bigint::BigInt;
use num_integer::Integer as _;
use num_traits::cast::FromPrimitive;
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

/// Encodes a codepoint in `enc` for `Integer#chr(encoding)`: UTF-8 as a
/// multibyte sequence (0..=0x10FFFF, surrogates excluded), a byte encoding as
/// a single byte within its range. `None` when the codepoint is out of range.
fn encode_codepoint(cp: i64, enc: crate::encoding::EncodingId) -> Option<Vec<u8>> {
    use crate::encoding::EncKind;
    if cp < 0 {
        return None;
    }
    match enc.kind() {
        EncKind::Utf8 => {
            let c = char::from_u32(u32::try_from(cp).ok()?)?;
            Some(c.to_string().into_bytes())
        }
        EncKind::Ascii => (cp <= 0x7F).then(|| vec![cp as u8]),
        EncKind::Latin1 | EncKind::Binary => (cp <= 0xFF).then(|| vec![cp as u8]),
    }
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
/// An Integer mask argument (`allbits?`/`anybits?`/`nobits?`) as a `BigInt`.
/// A non-Integer argument is CRuby's coercion TypeError.
fn int_mask_arg(v: &RubyValue) -> Result<BigInt, Signal> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(to_bigint(v)),
        other => Err(coerce_error(other, "Integer")),
    }
}

/// `Integer#[]`: extracts bit(s) from `recv`'s two's-complement (infinite for
/// negatives) representation. Resolves the (shift, width) window from the
/// single-index / `start, len` / range forms, then answers
/// `(recv >> shift) & ((1 << width) - 1)`; `width = None` (an endless range)
/// answers the whole `recv >> shift`.
fn int_bit_ref(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    use num_traits::One;
    let val = to_bigint(recv);
    let (shift, width): (BigInt, Option<BigInt>) = if args.len() == 2 {
        (to_bigint(&args[0]), Some(to_bigint(&args[1])))
    } else if let RubyValue::Range(begin, end, exclusive) = &args[0] {
        let Some(b) = begin else {
            return Err(arg_error!(
                "The beginless range for Integer#[] results in infinity"
            ));
        };
        let start = to_bigint(b);
        match end {
            None => (start, None),
            Some(e) => {
                let last = to_bigint(e);
                let mut w = &last - &start;
                if !*exclusive {
                    w += 1;
                }
                (start, Some(w))
            }
        }
    } else {
        (to_bigint(&args[0]), Some(BigInt::one()))
    };

    // A NEGATIVE start shifts the other way (`n[-k, len]` == `(n << k)[0, len]`);
    // a non-negative start shifts right. (Absurdly-large left shifts, which
    // don't fit `usize`, are clamped rather than allowed to OOM.)
    let shifted = if shift.is_negative() {
        &val << (-&shift).to_usize().unwrap_or(0)
    } else {
        match shift.to_usize() {
            Some(s) => &val >> s,
            None => {
                // Shifted right past every bit: all-ones for a negative endless
                // range, else 0.
                return Ok(RubyValue::Int(if val.is_negative() && width.is_none() {
                    -1
                } else {
                    0
                }));
            }
        }
    };
    // Width `None` (endless range) or NEGATIVE keeps the whole shifted value;
    // a POSITIVE width masks to that many low bits; ZERO selects nothing --
    // all CRuby's rules (`5[2, -1] == 1`, `5[2, 0] == 0`).
    match width {
        None => Ok(int_value(shifted)),
        Some(w) if w.is_negative() => Ok(int_value(shifted)),
        Some(w) if w.is_positive() => {
            let bits = w.to_usize().unwrap_or(usize::MAX);
            let mask = (BigInt::one() << bits) - 1;
            Ok(int_value(shifted & mask))
        }
        Some(_) => Ok(RubyValue::Int(0)),
    }
}

pub fn int_shl(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let amount = to_bigint(b);
    if amount.is_negative() {
        return int_shr(a, &int_value(-amount));
    }
    let Some(amount) = amount.to_u32() else {
        return Err(range_error!("shift width too big"));
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
    type_error!(
        "{} can't be coerced into {into}",
        crate::builtins::class_name_of(arg)
    )
}

use crate::builtins::numeric::num_op_row;

builtin_methods! {
    pub(crate) fn lookup;

    "+"[1] => fn add(recv, args, _block) { num_op_row!(args, recv, num_add, "+") }
    "-"[1] => fn sub(recv, args, _block) { num_op_row!(args, recv, num_sub, "-") }
    "*"[1] => fn mul(recv, args, _block) { num_op_row!(args, recv, num_mul, "*") }
    "/"[1] => fn div(recv, args, _block) { num_op_row!(args, recv, num_div, "/") }
    "%"[1] | "modulo"[1] => fn modulo(recv, args, _block) { num_op_row!(args, recv, num_mod, "%") }
    "**"[1] => fn pow(recv, args, _block) { num_op_row!(args, recv, num_pow, "**") }
    "&"[1] => fn band(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_band(recv, &args[0])),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "|"[1] => fn bor(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_bor(recv, &args[0])),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "^"[1] => fn bxor(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_bxor(recv, &args[0])),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "<<"[1] => fn shl(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => int_shl(recv, &args[0]),
            // A Float count is truncated toward zero (CRuby's `to_int`).
            RubyValue::Float(f) => int_shl(recv, &RubyValue::Int(f.trunc() as i64)),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    ">>"[1] => fn shr(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => int_shr(recv, &args[0]),
            RubyValue::Float(f) => int_shr(recv, &RubyValue::Int(f.trunc() as i64)),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "-@"[0] => fn neg(recv, args, _block) {
        arity!(args, 0);
        Ok(int_neg(recv))
    }
    "+@"[0] => fn pos(recv, args, _block) {
        arity!(args, 0);
        Ok(int_pos(recv))
    }
    "~"[0] => fn bnot(recv, args, _block) {
        arity!(args, 0);
        Ok(int_bnot(recv))
    }
    // `Integer#[]` -- bit reference, LSB = index 0, two's-complement sign
    // extension for negatives. `n[i]` is a single bit; `n[start, len]` and
    // `n[range]` extract a `len`-bit field. A beginless range is an
    // ArgumentError (its width is infinite), matching CRuby.
    "[]" => fn bit_ref(recv, args, _block) {
        arity!(args, 1..=2);
        int_bit_ref(recv, args)
    }
    // Bit-mask predicates: `allbits?` (every mask bit set), `anybits?` (at
    // least one), `nobits?` (none). All via `self & mask` over the BigInt
    // two's-complement view, so they work for fixnums and bignums alike.
    "allbits?"[1] => fn allbits(recv, args, _block) {
        arity!(args, 1);
        let mask = int_mask_arg(&args[0])?;
        Ok(RubyValue::Bool((to_bigint(recv) & &mask) == mask))
    }
    "anybits?"[1] => fn anybits(recv, args, _block) {
        arity!(args, 1);
        let mask = int_mask_arg(&args[0])?;
        Ok(RubyValue::Bool((to_bigint(recv) & mask) != BigInt::from(0)))
    }
    "nobits?"[1] => fn nobits(recv, args, _block) {
        arity!(args, 1);
        let mask = int_mask_arg(&args[0])?;
        Ok(RubyValue::Bool((to_bigint(recv) & mask) == BigInt::from(0)))
    }
    // Every Integer is finite and never infinite (the Float predicates,
    // answered here so the numeric protocol is uniform).
    "finite?"[0] => fn finite_p(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(true))
    }
    "infinite?"[0] => fn infinite_p(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Nil)
    }
    // `n.i` is the pure-imaginary Complex `0 + n*i`.
    "i"[0] => fn imaginary(recv, args, _block) {
        arity!(args, 0);
        crate::builtins::complex::complex_new(RubyValue::Int(0), recv.clone())
    }
    // Ceiling division: the smallest integer >= self/other. `-floor(-a / b)`
    // gives the exact result for either sign.
    "ceildiv"[1] => fn ceildiv(recv, args, _block) {
        arity!(args, 1);
        // `ceildiv(other)` == `-((-self).div(other))`; a Float/Rational divisor
        // rides the floored-division tower, still answering an Integer.
        if matches!(&args[0], RubyValue::Float(_) | RubyValue::Rational(_)) {
            let neg_self = int_neg(recv);
            let floored = crate::dispatch::send_value(
                &neg_self,
                crate::Symbol::intern("div"),
                std::slice::from_ref(&args[0]),
                None,
            )?;
            return Ok(int_neg(&floored));
        }
        let b = int_mask_arg(&args[0])?;
        if b.is_zero() {
            return Err(crate::dispatch::raise_error(
                "ZeroDivisionError",
                "divided by 0".to_string(),
            ));
        }
        let a = to_bigint(recv);
        Ok(int_value(-((-a).div_floor(&b))))
    }
    // `coerce(other)`: the numeric-protocol pair. A Float partner promotes
    // both to Float; an Integer partner keeps both integral. Answered as a
    // two-element Array (`other`-first, CRuby's order).
    "coerce"[1] => fn coerce(recv, args, _block) {
        arity!(args, 1);
        let pair = match &args[0] {
            RubyValue::Float(f) => vec![
                RubyValue::Float(*f),
                RubyValue::Float(to_bigint(recv).to_f64().unwrap_or(f64::NAN)),
            ],
            RubyValue::Int(_) | RubyValue::BigInt(_) => vec![args[0].clone(), recv.clone()],
            other => {
                return Err(type_error!("can't coerce {} into Integer", crate::builtins::class_name_of(other)))
            }
        };
        Ok(RubyValue::Array(crate::array_new(pair)))
    }
    // Numeric-tower comparison; a non-numeric argument compares as nil
    // (real Ruby: `5 <=> "a"` is nil, never an error). Comparable's
    // operators drive this row.
    "<=>"[1] => fn spaceship(recv, args, _block) {
        arity!(args, 1);
        Ok(match crate::builtins::numeric::num_cmp(recv, &args[0]) {
            Some(Some(c)) => RubyValue::Int(c),
            _ => RubyValue::Nil,
        })
    }
    // Integer's own `==` (cross-tower: `1 == 1.0` is true) -- resolving
    // before `Comparable#==` in the chain. Non-numeric -> false.
    "=="[1] => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    // `div` -- floored integer division (what `/` already does for Ints);
    // `fdiv` -- float division regardless of operand kinds.
    "div"[1] => fn floored_div(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(0) => Err(crate::dispatch::raise_error(
                "ZeroDivisionError",
                "divided by 0".to_string(),
            )),
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_div(recv, &args[0])),
            RubyValue::Float(f) => {
                let a = match recv {
                    RubyValue::Int(i) => *i as f64,
                    _ => return Ok(int_div(recv, &args[0])),
                };
                Ok(RubyValue::Int((a / f).floor() as i64))
            }
            // A Rational divisor: `(self / other).floor` stays exact.
            RubyValue::Rational(_) => {
                let q = crate::builtins::numeric::num_div(recv, &args[0])
                    .expect("Integer / Rational is defined")?;
                match q {
                    RubyValue::Rational(r) => Ok(int_value(r.num.div_floor(&r.den))),
                    other => Ok(other),
                }
            }
            other => Err(type_error!("{} can't be coerced into Integer", crate::builtins::class_name_of(other))),
        }
    }
    "fdiv"[1] => fn fdiv(recv, args, _block) {
        arity!(args, 1);
        let to_f = |v: &RubyValue| -> Option<f64> {
            match v {
                RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
                    Some(crate::builtins::numeric::num_to_f64_unchecked(v))
                }
                _ => None,
            }
        };
        match (to_f(recv), to_f(&args[0])) {
            (Some(a), Some(b)) => Ok(RubyValue::Float(a / b)),
            _ => Err(type_error!("{} can't be coerced into Integer", crate::builtins::class_name_of(&args[0]))),
        }
    }
    "abs"[0] | "magnitude"[0] => fn abs(recv, args, _block) {
        arity!(args, 0);
        Ok(match recv {
            RubyValue::Int(i) if *i >= 0 => recv.clone(),
            _ => {
                let b = to_bigint(recv);
                int_value(b.abs())
            }
        })
    }
    "even?"[0] => fn even_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(to_bigint(recv).is_even()))
    }
    "odd?"[0] => fn odd_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(to_bigint(recv).is_odd()))
    }
    "succ"[0] | "next"[0] => fn succ(recv, args, _block) {
        arity!(args, 0);
        Ok(int_add(recv, &RubyValue::Int(1)))
    }
    "pred"[0] => fn pred(recv, args, _block) {
        arity!(args, 0);
        Ok(int_sub(recv, &RubyValue::Int(1)))
    }
    // ASCII only: our strings are UTF-8, so a 128..=255 chr would change
    // byte representation -- rejected loudly (spike scope), not silently
    // re-encoded. Out of byte range is real Ruby's RangeError.
    // `chr` -> the one-character String for a codepoint. No argument: a single
    // byte, US-ASCII for 0..=127 and ASCII-8BIT for 128..=255. With an
    // Encoding: the codepoint encoded in it (UTF-8 multibyte, or a single byte
    // for the byte encodings). Out-of-range is a RangeError.
    "chr" => fn chr(recv, args, _block) {
        arity!(args, 0..=1);
        let RubyValue::Int(i) = recv else {
            return Err(range_error!("{} out of char range", recv.to_display_string()));
        };
        let i = *i;
        let range_err = || {
            range_error!("{i} out of char range")
        };
        let (bytes, enc) = match args.first() {
            None => {
                if !(0..=255).contains(&i) {
                    return Err(range_err());
                }
                let enc = if i < 128 {
                    crate::encoding::US_ASCII
                } else {
                    crate::encoding::ASCII_8BIT
                };
                (vec![i as u8], enc)
            }
            Some(enc_arg) => {
                let enc = crate::builtins::encoding::arg_encoding(enc_arg)?;
                (encode_codepoint(i, enc).ok_or_else(range_err)?, enc)
            }
        };
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, enc)))
    }
    "ord"[0] | "to_i"[0] | "to_int"[0] => fn ord(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "to_f"[0] => fn to_f(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(crate::builtins::numeric::num_to_f64_unchecked(recv)))
    }
    // An Integer is already exact, so `rationalize([eps])` ignores its
    // optional precision argument and equals `to_r`.
    "to_r"[0] | "rationalize" => fn to_r(recv, args, _block) {
        arity!(args, 0..=1);
        crate::builtins::rational::rational_new(to_bigint(recv), BigInt::from(1))
    }
    "numerator"[0] => fn numerator(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "denominator"[0] => fn denominator(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(1))
    }
    // `to_s(base)` / bare `to_s`; `inspect` is the same rendering.
    "to_s" | "inspect" => fn to_s(recv, args, _block) {
        arity!(args, 0..=1);
        let base = match args.first() {
            Some(RubyValue::Int(b)) if (2..=36).contains(b) => *b as u32,
            Some(RubyValue::Int(b)) => {
                return Err(arg_error!("invalid radix {b}"))
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
            // Any Integer base is valid (a bignum base too, e.g. `255.digits(2**70)`);
            // only radix < 2 is rejected, and the error echoes the raw value.
            Some(v @ (RubyValue::Int(_) | RubyValue::BigInt(_))) => {
                let b = to_bigint(v);
                // CRuby distinguishes a NEGATIVE base ("negative radix") from
                // a 0/1 base ("invalid radix N").
                if b.is_negative() {
                    return Err(arg_error!("negative radix"));
                }
                if b < BigInt::from(2) {
                    return Err(arg_error!("invalid radix {b}"));
                }
                b
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
    "gcd"[1] => fn gcd(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => {
                Ok(int_value(to_bigint(recv).gcd(&to_bigint(&args[0]))))
            }
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "lcm"[1] => fn lcm(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => {
                Ok(int_value(to_bigint(recv).lcm(&to_bigint(&args[0]))))
            }
            other => Err(coerce_error(other, "Integer")),
        }
    }
    "gcdlcm"[1] => fn gcdlcm(recv, args, _block) {
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
    "bit_length"[0] => fn bit_length(recv, args, _block) {
        arity!(args, 0);
        let n = to_bigint(recv);
        // CRuby: bits needed excluding the sign (negative x measures ~x).
        let measured = if n.is_negative() { !n } else { n };
        Ok(RubyValue::Int(measured.bits() as i64))
    }
    "size"[0] => fn size(recv, args, _block) {
        arity!(args, 0);
        // A value in the machine-word range answers `sizeof(long)` (8 here,
        // like CRuby's `fix_size`); a bignum reports its magnitude's byte
        // width, `ceil(bit_length(|n|) / 8)` (CRuby's `BIGSIZE`).
        Ok(RubyValue::Int(match recv {
            RubyValue::BigInt(b) => (b.bits().div_ceil(8)) as i64,
            _ => 8,
        }))
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
                    return Err(type_error!("Integer#pow() 2nd argument not allowed unless all arguments are integers"));
                };
                let e = to_bigint(&args[0]);
                if e.is_negative() {
                    return Err(range_error!("Integer#pow() 1st argument cannot be negative when 2nd argument specified"));
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
    // Iteration primitives; blockless forms return Enumerators (Phase
    // 17.2). Counts beyond i64 are physically unrunnable -- loud.
    "times"[0] => fn times(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "times", args, block);
        let RubyValue::Int(n) = recv else {
            panic!("Integer#times receiver exceeds i64 (unrunnable iteration count)");
        };
        for i in 0..*n {
            p.call(&[RubyValue::Int(i)])?;
        }
        Ok(recv.clone())
    }
    "upto"[1] => fn upto(recv, args, block) {
        arity!(args, 1);
        let p = block_or_enum!(recv, "upto", args, block);
        // Fast i64 path; otherwise iterate as BigInt -- the VALUES may exceed
        // i64 even when the SPAN is small (`(2**100).upto(2**100 + 2)`).
        if let (RubyValue::Int(a), RubyValue::Int(b)) = (recv, &args[0]) {
            for i in *a..=*b {
                p.call(&[RubyValue::Int(i)])?;
            }
            return Ok(recv.clone());
        }
        // General path: a BigInt receiver and/or a Float/BigInt limit. The
        // count is yielded as integers while `current <= limit`; `rb_cmp`
        // handles the mixed comparison (a Float limit is compared, not
        // converted). Spans are assumed small even when the values are huge.
        let mut i = to_bigint(recv);
        while matches!(int_value(i.clone()).rb_cmp(&args[0]), Some(c) if c <= 0) {
            p.call(&[int_value(i.clone())])?;
            i += 1;
        }
        Ok(recv.clone())
    }
    "downto"[1] => fn downto(recv, args, block) {
        arity!(args, 1);
        let p = block_or_enum!(recv, "downto", args, block);
        if let (RubyValue::Int(a), RubyValue::Int(b)) = (recv, &args[0]) {
            let mut i = *a;
            while i >= *b {
                p.call(&[RubyValue::Int(i)])?;
                i -= 1;
            }
            return Ok(recv.clone());
        }
        let mut i = to_bigint(recv);
        while matches!(int_value(i.clone()).rb_cmp(&args[0]), Some(c) if c >= 0) {
            p.call(&[int_value(i.clone())])?;
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

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Integer.sqrt(n)` -- the exact integer square root (floor of the real
    // square root, no floating-point rounding: correct for bignums too). A
    // negative argument is a `Math::DomainError`, like CRuby.
    "sqrt" => fn isqrt(_recv, args, _block) {
        arity!(args, 1);
        let n = match &args[0] {
            RubyValue::Int(_) | RubyValue::BigInt(_) => to_bigint(&args[0]),
            RubyValue::Float(f) => {
                if !f.is_finite() {
                    return Err(crate::dispatch::raise_error(
                        "Math::DomainError",
                        "Numerical argument is out of domain - \"isqrt\"".to_string(),
                    ));
                }
                num_bigint::BigInt::from_f64(f.trunc()).unwrap_or_default()
            }
            other => match crate::builtins::convert::to_int(other)? {
                v @ (RubyValue::Int(_) | RubyValue::BigInt(_)) => to_bigint(&v),
                _ => unreachable!("to_int post-checks its answer"),
            },
        };
        if n.is_negative() {
            return Err(crate::dispatch::raise_error(
                "Math::DomainError",
                "Numerical argument is out of domain - \"isqrt\"".to_string(),
            ));
        }
        Ok(int_value(n.sqrt()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big(s: &str) -> RubyValue {
        int_value(s.parse::<BigInt>().unwrap())
    }

    #[test]
    fn integer_sqrt_is_the_exact_floor_root() {
        let sq = |v: RubyValue| isqrt(&RubyValue::Nil, &[v], None).unwrap();
        assert!(matches!(sq(RubyValue::Int(0)), RubyValue::Int(0)));
        assert!(matches!(sq(RubyValue::Int(8)), RubyValue::Int(2)));
        assert!(matches!(sq(RubyValue::Int(9)), RubyValue::Int(3)));
        assert!(matches!(sq(RubyValue::Int(15)), RubyValue::Int(3)));
        // Exact for a bignum (no float rounding): sqrt(10**20) == 10**10.
        assert!(matches!(
            sq(big("100000000000000000000")),
            RubyValue::Int(10_000_000_000)
        ));
        // A Float argument truncates to its integer part first.
        assert!(matches!(sq(RubyValue::Float(26.9)), RubyValue::Int(5)));
    }

    #[test]
    #[should_panic(expected = "Math::DomainError")]
    fn integer_sqrt_of_a_negative_is_a_domain_error() {
        // Registry-less, `raise_error` panics -- the message is the assertion.
        let _ = isqrt(&RubyValue::Nil, &[RubyValue::Int(-4)], None);
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
