//! `Integer` (CRuby numeric.c + bignum.c) -- ONE Ruby class, two payloads:
//! `RubyValue::Int(i64)` for the fixnum range, `RubyValue::BigInt` beyond
//! it (the full-bignum decision). Every operation here takes
//! `&RubyValue` pairs whose Integer-ness the caller has proven (statically
//! via `TyKind::Int`, or dynamically via the table keying) -- the
//! `#[inline]` small-small fast half is as cheap as raw `i64` arithmetic,
//! with a `#[cold]` bignum half behind it.
//!
//! The ONE construction invariant: `int_value` demotes every `BigInt`
//! result that fits back into `Int`, so a big payload never aliases a
//! fixnum value (equality/hashing/matching stay canonical).

use crate::builtins::{arg_error, block_or_enum, inherited_row, range_error, type_error};
use crate::{RubyValue, Signal};
use num_bigint::BigInt;
use num_integer::Integer as _;
use num_traits::cast::FromPrimitive;
use num_traits::{Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::sync::Arc;
use zeo_macros::ruby_class;

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
/// a single byte within its range. `Err` distinguishes CRuby's two
/// `RangeError` messages: `Ok`-shaped values that are simply too big are
/// `OutOfRange` ("N out of char range"), while a multibyte split that can't
/// spell a character is `InvalidCodepoint` ("invalid codepoint 0xNNNN in
/// ENC") -- oracle-verified.
fn encode_codepoint(
    cp: i64,
    enc: crate::encoding::EncodingId,
) -> Result<Vec<u8>, crate::encoding::MbCodepointError> {
    use crate::encoding::{EncKind, MbCodepointError};
    let out_of_range = Err(MbCodepointError::OutOfRange);
    if cp < 0 {
        return out_of_range;
    }
    match enc.kind() {
        EncKind::Utf8 => match u32::try_from(cp).ok().filter(|v| *v <= 0x10FFFF) {
            // A surrogate (0xD800..=0xDFFF) is in Unicode's range but has no
            // UTF-8 encoding: CRuby reports it as an invalid codepoint, unlike
            // a value past U+10FFFF which is simply out of range.
            Some(v) => match char::from_u32(v) {
                Some(c) => Ok(c.to_string().into_bytes()),
                None => Err(MbCodepointError::InvalidCodepoint),
            },
            None => out_of_range,
        },
        EncKind::Ascii => {
            if cp <= 0x7F {
                Ok(vec![cp as u8])
            } else {
                out_of_range
            }
        }
        // Codepoint == byte for every single-byte encoding (oracle-verified:
        // `233.chr(Encoding::Windows_1252)` is the byte 0xE9, 256 raises).
        EncKind::Latin1 | EncKind::Binary | EncKind::Registered | EncKind::SingleByte => {
            if cp <= 0xFF {
                Ok(vec![cp as u8])
            } else {
                out_of_range
            }
        }
        EncKind::MultiByte(family) => match u32::try_from(cp) {
            Ok(cp) => crate::encoding::mb_codepoint_bytes(family, cp),
            Err(_) => out_of_range,
        },
        // UTF-16/32 codepoints are Unicode SCALARS (`65.chr(UTF_16LE)` is
        // the two bytes 41 00): surrogates are invalid codepoints, values
        // past U+10FFFF out of range.
        EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            match u32::try_from(cp).ok().filter(|v| *v <= 0x10FFFF) {
                Some(v) => match char::from_u32(v) {
                    Some(c) => Ok(crate::encoding::encode_scalar(enc, c)
                        .expect("wide encodings represent every scalar")),
                    None => Err(MbCodepointError::InvalidCodepoint),
                },
                None => out_of_range,
            }
        }
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
    // A base of 1 or -1 answers an INTEGER whatever the exponent's sign --
    // CRuby tests both before it looks at the sign at all, which is why
    // `1 ** -1` is `1` where `2 ** -1` is `(1/2)`. (A BigInt is never
    // either, by the demotion invariant.)
    match a {
        RubyValue::Int(1) => return Ok(RubyValue::Int(1)),
        RubyValue::Int(-1) => {
            let even = match b {
                RubyValue::Int(y) => y % 2 == 0,
                _ => to_bigint(b).is_even(),
            };
            return Ok(RubyValue::Int(if even { 1 } else { -1 }));
        }
        _ => {}
    }
    // Small-small fast half.
    if let (RubyValue::Int(x), RubyValue::Int(y)) = (a, b)
        && *y >= 0
        && let Ok(exp) = u32::try_from(*y)
        && let Some(r) = x.checked_pow(exp)
    {
        return Ok(RubyValue::Int(r));
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
/// The `gcd`/`lcm`/`gcdlcm` argument error. Ruby checks these with
/// `rb_to_int`-shaped strictness and says plainly "not an integer" -- NOT the
/// numeric tower's "X can't be coerced into Integer", which is what an
/// arithmetic OPERATOR says about the same argument.
fn not_an_integer() -> Signal {
    type_error!("not an integer")
}

fn int_mask_arg(v: &RubyValue) -> Result<BigInt, Signal> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(to_bigint(v)),
        // The bit predicates take their mask through `rb_to_int`, not the
        // numeric tower's `coerce` -- so a Float truncates (`42.allbits?(1.5)`
        // masks with 1) and anything with a `to_int` is accepted, where the
        // coercion message rejected both.
        other => Ok(to_bigint(&crate::builtins::convert::to_int(other)?)),
    }
}

/// The limit of an `upto`/`downto` walk has to be COMPARABLE with the
/// receiver, and ruby says so before yielding anything: `1.upto("a")` is
/// `comparison of Integer with String failed`, not an empty walk. The check is
/// deliberately after the blockless-Enumerator return, so it fires when the
/// walk runs rather than when it is built.
fn guard_comparable_limit(recv: &RubyValue, limit: &RubyValue) -> Result<(), Signal> {
    if crate::builtins::numeric::num_cmp(recv, limit).is_some() {
        return Ok(());
    }
    Err(arg_error!(
        "comparison of {} with {} failed",
        crate::builtins::class_name_of(recv),
        crate::builtins::class_name_of(limit)
    ))
}

/// `Integer#[]`: extracts bit(s) from `recv`'s two's-complement (infinite for
/// negatives) representation. Resolves the (shift, width) window from the
/// single-index / `start, len` / range forms, then answers
/// `(recv >> shift) & ((1 << width) - 1)`; `width = None` (an endless range)
/// answers the whole `recv >> shift`.
fn int_bit_ref(
    recv: &RubyValue,
    index: &RubyValue,
    len: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    use num_traits::One;
    // Every INDEX goes through the conversion protocol: a non-Integer here is
    // ruby's `no implicit conversion` TypeError, and a `to_int` duck is
    // accepted. `to_bigint` panics on anything else, which took the process
    // down for `5[nil]`.
    let arg_int = |v: &RubyValue| -> Result<BigInt, Signal> {
        Ok(to_bigint(&crate::builtins::convert::to_int(v)?))
    };
    let val = to_bigint(recv);
    let (shift, width): (BigInt, Option<BigInt>) = if let Some(len) = len {
        (arg_int(index)?, Some(arg_int(len)?))
    } else if let RubyValue::Range(__rg) = index {
        let (begin, end, exclusive) = __rg.parts();
        let Some(b) = begin else {
            return Err(arg_error!(
                "The beginless range for Integer#[] results in infinity"
            ));
        };
        let start = arg_int(b)?;
        match end {
            None => (start, None),
            Some(e) => {
                let last = arg_int(e)?;
                let mut w = &last - &start;
                if !exclusive {
                    w += 1;
                }
                (start, Some(w))
            }
        }
    } else {
        (arg_int(index)?, Some(BigInt::one()))
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
    if let RubyValue::Int(x) = a
        && let Some(r) = x.checked_shl(amount)
    {
        // checked_shl wraps the AMOUNT, not the value -- verify the
        // round trip really was lossless before taking the fast path.
        if amount < 64 && (r >> amount) == *x {
            return Ok(RubyValue::Int(r));
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

ruby_class! {
    Integer = zeo_abi::INTEGER_CLASS < zeo_abi::NUMERIC_CLASS;

    // `Integer.try_convert(obj)`: `obj` if it is already an Integer, its
    // `to_int` if it defines one, else nil. Never raises for a value that
    // simply cannot convert, unlike `Integer(obj)`.
    def self."try_convert" (_recv, arg) {
        Ok(crate::builtins::convert::try_convert_value(arg, "Integer", "to_int")?
            .unwrap_or(RubyValue::Nil))
    }
    // `Integer.sqrt(n)` -- the exact integer square root (floor of the real
    // square root, no floating-point rounding: correct for bignums too). A
    // negative argument is a `Math::DomainError`, like CRuby.
    def self."sqrt"(_recv, arg) {
        let n = match arg {
            RubyValue::Int(_) | RubyValue::BigInt(_) => to_bigint(arg),
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

    def "+" (recv, other) { num_op_row!(other, recv, num_add, "+") }
    def "-" (recv, other) { num_op_row!(other, recv, num_sub, "-") }
    def "*" (recv, other) { num_op_row!(other, recv, num_mul, "*") }
    def "/" (recv, other) { num_op_row!(other, recv, num_div, "/") }
    def "%" | "modulo" (recv, other) { num_op_row!(other, recv, num_mod, "%") }
    def "**" (recv, other) { num_op_row!(other, recv, num_pow, "**") }
    def "&" (recv, other) {
        match other {
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_band(recv, other)),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    def "|" (recv, other) {
        match other {
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_bor(recv, other)),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    def "^" (recv, other) {
        match other {
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_bxor(recv, other)),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    def "<<" (recv, other) {
        match other {
            RubyValue::Int(_) | RubyValue::BigInt(_) => int_shl(recv, other),
            // A Float count is truncated toward zero (CRuby's `to_int`).
            RubyValue::Float(f) => int_shl(recv, &RubyValue::Int(f.trunc() as i64)),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    def ">>" (recv, other) {
        match other {
            RubyValue::Int(_) | RubyValue::BigInt(_) => int_shr(recv, other),
            RubyValue::Float(f) => int_shr(recv, &RubyValue::Int(f.trunc() as i64)),
            other => Err(coerce_error(other, "Integer")),
        }
    }
    def "-@" (recv) {
        Ok(int_neg(recv))
    }
    // No `+@` row: ruby owns it on Numeric, whose body is the same `self`.
    // The `+n` fast path is unaffected -- codegen emits the free `int_pos`
    // (`codegen/call/ops.rs`), never this table.
    def "~" (recv) {
        Ok(int_bnot(recv))
    }
    // `Integer#[]` -- bit reference, LSB = index 0, two's-complement sign
    // extension for negatives. `n[i]` is a single bit; `n[start, len]` and
    // `n[range]` extract a `len`-bit field. A beginless range is an
    // ArgumentError (its width is infinite), matching CRuby.
    def "[]" cfunc (recv, index, len?) {
        int_bit_ref(recv, index, len)
    }
    // Bit-mask predicates: `allbits?` (every mask bit set), `anybits?` (at
    // least one), `nobits?` (none). All via `self & mask` over the BigInt
    // two's-complement view, so they work for fixnums and bignums alike.
    def "allbits?" (recv, arg) {
        let mask = int_mask_arg(arg)?;
        Ok(RubyValue::Bool((to_bigint(recv) & &mask) == mask))
    }
    def "anybits?" (recv, arg) {
        let mask = int_mask_arg(arg)?;
        Ok(RubyValue::Bool((to_bigint(recv) & mask) != BigInt::from(0)))
    }
    def "nobits?" (recv, arg) {
        let mask = int_mask_arg(arg)?;
        Ok(RubyValue::Bool((to_bigint(recv) & mask) == BigInt::from(0)))
    }
    // No `finite?` / `infinite?` / `i` rows: ruby owns all three on Numeric,
    // and Numeric's bodies already answer true / nil / `Complex(0, self)` --
    // only `Float` overrides the first two, which it still does.
    // Ceiling division: the smallest integer >= self/other. `-floor(-a / b)`
    // gives the exact result for either sign.
    def "ceildiv" params "other" (recv, arg) {
        // `ceildiv(other)` == `-((-self).div(other))`; a Float/Rational divisor
        // rides the floored-division tower, still answering an Integer.
        if matches!(arg, RubyValue::Float(_) | RubyValue::Rational(_)) {
            let neg_self = int_neg(recv);
            let floored = crate::dispatch::send_value(
                &neg_self,
                crate::Symbol::intern("div"),
                std::slice::from_ref(arg),
                None,
            )?;
            return Ok(int_neg(&floored));
        }
        let b = int_mask_arg(arg)?;
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
    def "coerce" (recv, arg) {
        let pair = match arg {
            // An INTEGER pair stays integral (`rb_int_coerce`); every other
            // argument is ruby's `num_coerce`, which is literally
            // `[Float(y), Float(x)]` -- so the failures are `Float()`'s,
            // naming the VALUE and `Float`, and not a "can't coerce"
            // TypeError naming the class and the receiver's type.
            RubyValue::Int(_) | RubyValue::BigInt(_) => vec![(*arg).clone(), recv.clone()],
            _ => {
                let f = crate::builtins::kernel::float_impl(std::slice::from_ref(arg))?;
                vec![
                    f,
                    RubyValue::Float(to_bigint(recv).to_f64().unwrap_or(f64::NAN)),
                ]
            }
        };
        Ok(RubyValue::Array(crate::array_new(pair)))
    }
    // Numeric-tower comparison; a non-numeric argument that doesn't
    // `coerce` compares as nil (real Ruby: `5 <=> "a"` is nil, never an
    // error), while one that does (BigDecimal) compares through the
    // coerced pair. Comparable's operators drive this row.
    def "<=>" (recv, other) {
        Ok(match crate::builtins::numeric::num_cmp(recv, other) {
            Some(Some(c)) => RubyValue::Int(c),
            Some(None) => RubyValue::Nil,
            None => crate::builtins::numeric::coerce_cmp(recv, other)?,
        })
    }
    // Integer's own `==` (cross-tower: `1 == 1.0` is true) -- resolving
    // before `Comparable#==` in the chain. A non-tower operand answers for
    // itself (`y == x`, CRuby's num_equal).
    def "==" (recv, other) {
        if recv.rb_eq(other) {
            return Ok(RubyValue::Bool(true));
        }
        crate::builtins::numeric::reverse_eq(recv, other)
    }
    // `div` -- floored integer division (what `/` already does for Ints);
    // `fdiv` -- float division regardless of operand kinds.
    def "div" (recv, arg) {
        match arg {
            RubyValue::Int(0) => Err(crate::dispatch::raise_error(
                "ZeroDivisionError",
                "divided by 0".to_string(),
            )),
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(int_div(recv, arg)),
            RubyValue::Float(f) => {
                let a = match recv {
                    RubyValue::Int(i) => *i as f64,
                    _ => return Ok(int_div(recv, arg)),
                };
                Ok(RubyValue::Int((a / f).floor() as i64))
            }
            // A Rational divisor: `(self / other).floor` stays exact.
            RubyValue::Rational(_) => {
                let q = crate::builtins::numeric::num_div(recv, arg)
                    .expect("Integer / Rational is defined")?;
                match q {
                    RubyValue::Rational(r) => Ok(int_value(r.num.div_floor(&r.den))),
                    other => Ok(other),
                }
            }
            other => Err(type_error!("{} can't be coerced into Integer", crate::builtins::class_name_of(other))),
        }
    }
    def "fdiv" (recv, arg) {
        let to_f = |v: &RubyValue| -> Option<f64> {
            match v {
                RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
                    Some(crate::builtins::numeric::num_to_f64_unchecked(v))
                }
                _ => None,
            }
        };
        match (to_f(recv), to_f(arg)) {
            (Some(a), Some(b)) => Ok(RubyValue::Float(a / b)),
            _ => Err(type_error!("{} can't be coerced into Integer", crate::builtins::class_name_of(arg))),
        }
    }
    def "abs" | "magnitude" (recv) {
        Ok(match recv {
            RubyValue::Int(i) if *i >= 0 => recv.clone(),
            _ => {
                let b = to_bigint(recv);
                int_value(b.abs())
            }
        })
    }
    def "even?" (recv) {
        Ok(RubyValue::Bool(to_bigint(recv).is_even()))
    }
    def "odd?" (recv) {
        Ok(RubyValue::Bool(to_bigint(recv).is_odd()))
    }
    def "succ" | "next" (recv) {
        Ok(int_add(recv, &RubyValue::Int(1)))
    }
    def "pred" (recv) {
        Ok(int_sub(recv, &RubyValue::Int(1)))
    }
    // `chr` -> the one-character String for a codepoint. No argument: a single
    // byte, US-ASCII for 0..=127 and ASCII-8BIT for 128..=255. With an
    // Encoding: the codepoint encoded in it (UTF-8 multibyte, or a single byte
    // for the byte encodings). Out-of-range is a RangeError.
    def "chr"(recv, arg?) {
        let RubyValue::Int(i) = recv else {
            return Err(range_error!("{} out of char range", recv.to_display_string()));
        };
        let i = *i;
        let range_err = || {
            range_error!("{i} out of char range")
        };
        let (bytes, enc) = match arg {
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
                let bytes = encode_codepoint(i, enc).map_err(|e| match e {
                    crate::encoding::MbCodepointError::OutOfRange => range_err(),
                    crate::encoding::MbCodepointError::InvalidCodepoint => {
                        range_error!("invalid codepoint 0x{i:X} in {}", enc.name())
                    }
                })?;
                (bytes, enc)
            }
        };
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, enc)))
    }
    def "ord" | "to_i" | "to_int" (recv) {
        Ok(recv.clone())
    }
    def "to_f" (recv) {
        Ok(RubyValue::Float(crate::builtins::numeric::num_to_f64_unchecked(recv)))
    }
    // An Integer is already exact, so `rationalize([eps])` ignores its
    // optional precision argument and equals `to_r`.
    def "to_r" arity 0 | "rationalize"(recv, _arg?) {
        crate::builtins::rational::rational_new(to_bigint(recv), BigInt::from(1))
    }
    def "numerator" (recv) {
        Ok(recv.clone())
    }
    def "denominator" (_recv) {
        Ok(RubyValue::Int(1))
    }
    // `to_s(base)` / bare `to_s`; `inspect` is the same rendering.
    def "to_s" | "inspect"(recv, arg?) {
        let base = match arg {
            Some(RubyValue::Int(b)) if (2..=36).contains(b) => *b as u32,
            Some(RubyValue::Int(b)) => {
                return Err(arg_error!("invalid radix {b}"))
            }
            // `rb_num2long` reads the radix, so a non-numeric one is "no
            // implicit conversion", never an arithmetic coercion failure.
            Some(other) => {
                let b = crate::builtins::convert::to_index(other)?;
                if !(2..=36).contains(&b) {
                    return Err(arg_error!("invalid radix {b}"));
                }
                b as u32
            }
            None => 10,
        };
        Ok(RubyValue::Str(crate::string_new(
            to_bigint(recv).to_str_radix(base),
        )))
    }
    def "digits"(recv, arg?) {
        let base = match arg {
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
            // `rb_to_int` reads this base, where `to_s` reads its radix
            // with `rb_num2long` -- the two spell the nil message
            // differently, so each uses the conversion CRuby uses.
            Some(other) => match crate::builtins::convert::to_int(other)? {
                RubyValue::Int(n) => BigInt::from(n),
                RubyValue::BigInt(b) => (*b).clone(),
                _ => unreachable!("to_int post-checks its answer"),
            },
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
    def "gcd" (recv, arg) {
        match arg {
            RubyValue::Int(_) | RubyValue::BigInt(_) => {
                Ok(int_value(to_bigint(recv).gcd(&to_bigint(arg))))
            }
            _other => Err(not_an_integer()),
        }
    }
    def "lcm" (recv, arg) {
        match arg {
            RubyValue::Int(_) | RubyValue::BigInt(_) => {
                Ok(int_value(to_bigint(recv).lcm(&to_bigint(arg))))
            }
            _other => Err(not_an_integer()),
        }
    }
    def "gcdlcm" (recv, arg) {
        match arg {
            RubyValue::Int(_) | RubyValue::BigInt(_) => {
                let (a, b) = (to_bigint(recv), to_bigint(arg));
                Ok(RubyValue::Array(crate::array_new(vec![
                    int_value(a.gcd(&b)),
                    int_value(a.lcm(&b)),
                ])))
            }
            _other => Err(not_an_integer()),
        }
    }
    def "bit_length" (recv) {
        let n = to_bigint(recv);
        // CRuby: bits needed excluding the sign (negative x measures ~x).
        let measured = if n.is_negative() { !n } else { n };
        Ok(RubyValue::Int(measured.bits() as i64))
    }
    def "size" (recv) {
        // A value in the machine-word range answers `sizeof(long)` (8 here,
        // like CRuby's `fix_size`); a bignum reports its magnitude's byte
        // width, `ceil(bit_length(|n|) / 8)` (CRuby's `BIGSIZE`).
        Ok(RubyValue::Int(match recv {
            RubyValue::BigInt(b) => (b.bits().div_ceil(8)) as i64,
            _ => 8,
        }))
    }
    // `pow(e)` == `**`; `pow(e, m)` is modular exponentiation.
    def "pow" cfunc (recv, exponent, modulo?) {
        match modulo {
            None => match crate::builtins::numeric::num_pow(recv, exponent) {
                Some(r) => r,
                None => Err(coerce_error(exponent, "Integer")),
            },
            Some(modulo) => {
                let (RubyValue::Int(_) | RubyValue::BigInt(_), RubyValue::Int(_) | RubyValue::BigInt(_)) =
                    (exponent, modulo)
                else {
                    return Err(type_error!("Integer#pow() 2nd argument not allowed unless all arguments are integers"));
                };
                let e = to_bigint(exponent);
                if e.is_negative() {
                    return Err(range_error!("Integer#pow() 1st argument cannot be negative when 2nd argument specified"));
                }
                let m = to_bigint(modulo);
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
    // A `half:` keyword selects the tie-break mode; floor/ceil/truncate have
    // no tie to break, so they take none.
    def "round"(recv, ndigits?, **opts) {
        int_round_family(recv, ndigits, RoundMode::HalfAway, half_kwarg(opts)?)
    }
    def "floor"(recv, ndigits?) {
        int_round_family(recv, ndigits, RoundMode::Floor, HalfMode::Up)
    }
    def "ceil"(recv, ndigits?) {
        int_round_family(recv, ndigits, RoundMode::Ceil, HalfMode::Up)
    }
    def "truncate"(recv, ndigits?) {
        int_round_family(recv, ndigits, RoundMode::Trunc, HalfMode::Up)
    }
    // Iteration primitives; blockless forms return Enumerators (Phase
    // 17.2). Counts beyond i64 are physically unrunnable -- loud.
    def "times" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        let RubyValue::Int(n) = recv else {
            panic!("Integer#times receiver exceeds i64 (unrunnable iteration count)");
        };
        for i in 0..*n {
            p.call(&[RubyValue::Int(i)])?;
        }
        Ok(recv.clone())
    }
    def "upto"(recv, limit, &block) {
        let p = block_or_enum!(recv, __args, block);
        guard_comparable_limit(recv, limit)?;
        // Fast i64 path; otherwise iterate as BigInt -- the VALUES may exceed
        // i64 even when the SPAN is small (`(2**100).upto(2**100 + 2)`).
        if let (RubyValue::Int(a), RubyValue::Int(b)) = (recv, limit) {
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
        while matches!(int_value(i.clone()).rb_cmp(limit), Some(c) if c <= 0) {
            p.call(&[int_value(i.clone())])?;
            i += 1;
        }
        Ok(recv.clone())
    }
    def "downto"(recv, limit, &block) {
        let p = block_or_enum!(recv, __args, block);
        guard_comparable_limit(recv, limit)?;
        if let (RubyValue::Int(a), RubyValue::Int(b)) = (recv, limit) {
            let mut i = *a;
            while i >= *b {
                p.call(&[RubyValue::Int(i)])?;
                // A `checked_sub`: a `b` of `i64::MIN` must stop after
                // yielding it, not wrap to `i64::MAX` and spin.
                match i.checked_sub(1) {
                    Some(v) => i = v,
                    None => break,
                }
            }
            return Ok(recv.clone());
        }
        let mut i = to_bigint(recv);
        while matches!(int_value(i.clone()).rb_cmp(limit), Some(c) if c >= 0) {
            p.call(&[int_value(i.clone())])?;
            i -= 1;
        }
        Ok(recv.clone())
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
    def "divmod"(recv, _other) { inherited_row!(numeric, "divmod", recv, __args, None) }
    def "remainder"(recv, _other) { inherited_row!(numeric, "remainder", recv, __args, None) }
    def "integer?"(recv) { inherited_row!(numeric, "integer?", recv, __args, None) }
    def "zero?"(recv) { inherited_row!(numeric, "zero?", recv, __args, None) }
}

/// The Integer rounding family's shared core.
pub(crate) enum RoundMode {
    HalfAway,
    Floor,
    Ceil,
    Trunc,
}

/// The `half:` option for `Integer#round`/`Float#round` at an exact `.5`
/// boundary: `:up` (away from zero, the default), `:down` (toward zero), or
/// `:even` (banker's rounding).
#[derive(Clone, Copy)]
pub(crate) enum HalfMode {
    Up,
    Down,
    Even,
}

/// Splits an optional trailing keyword Hash (`half:`) off the positional args,
/// returning the positionals and the parsed `HalfMode` (default `Up`).
fn half_kwarg(opts: Option<&RubyValue>) -> Result<HalfMode, Signal> {
    let Some(RubyValue::Hash(h)) = opts else {
        return Ok(HalfMode::Up);
    };
    let pairs = crate::hash_pairs(h);
    let half_key = RubyValue::Symbol(crate::Symbol::intern("half"));
    // Only peel it off as keywords if every key is the recognized `half:`; a
    // stray positional Hash keeps falling through to the coercion error.
    if pairs.is_empty() || !pairs.iter().all(|(k, _)| k.rb_eq(&half_key)) {
        return Ok(HalfMode::Up);
    }
    let mode = match crate::hash_get(h, &half_key) {
        RubyValue::Symbol(s) => match s.name().as_str() {
            "up" => HalfMode::Up,
            "down" => HalfMode::Down,
            "even" => HalfMode::Even,
            other => return Err(arg_error!("invalid rounding mode: {other}")),
        },
        RubyValue::Nil => HalfMode::Up,
        other => {
            return Err(arg_error!(
                "invalid rounding mode: {}",
                other.to_display_string()
            ));
        }
    };
    Ok(mode)
}

/// Rounds magnitude `m` down to a multiple of `p`, resolving an exact half by
/// `half`.
fn round_half_mag(m: &BigInt, p: &BigInt, half: HalfMode) -> BigInt {
    use std::cmp::Ordering;
    let r = m % p;
    let base = m - &r;
    match (&r * BigInt::from(2)).cmp(p) {
        Ordering::Less => base,
        Ordering::Greater => base + p,
        Ordering::Equal => match half {
            HalfMode::Up => base + p,
            HalfMode::Down => base,
            HalfMode::Even => {
                if (&base / p).is_even() {
                    base
                } else {
                    base + p
                }
            }
        },
    }
}

pub(crate) fn int_round_family(
    recv: &RubyValue,
    ndigits: Option<&RubyValue>,
    mode: RoundMode,
    half: HalfMode,
) -> Result<RubyValue, Signal> {
    let ndigits = match ndigits {
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
        RoundMode::HalfAway => round_half_mag(&m, &p, half),
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

    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::INTEGER_CLASS)
            .unwrap()
            .class
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }

    fn big(s: &str) -> RubyValue {
        int_value(s.parse::<BigInt>().unwrap())
    }

    #[test]
    fn integer_sqrt_is_the_exact_floor_root() {
        let sq = |v: RubyValue| cmethod("sqrt")(&RubyValue::Nil, &[v], None).unwrap();
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
        let _ = cmethod("sqrt")(&RubyValue::Nil, &[RubyValue::Int(-4)], None);
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
