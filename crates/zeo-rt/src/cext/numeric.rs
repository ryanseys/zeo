//! Integers, floats, Rational, Complex and the random source.
//!
//! # `rb_big_*` is not a Bignum-only API
//!
//! MRI's Integer is one class over two representations, and the C names still
//! carry the old split: `rb_big2dbl` is called on anything an extension
//! believes is large. zeo has the same one class over `Int` and `BigInt`, so
//! every entry here takes either -- a `rb_big2long` on a Fixnum answers,
//! rather than raising about a type the caller cannot see.
//!
//! What is NOT here is MRI's Bignum LAYOUT. `rb_big_new`, `rb_big_resize`,
//! `rb_big_pack` and `rb_big_2comp` hand out or mutate the digit array behind
//! a Bignum, and zeo's is a `num_bigint::BigInt` with a different one. Those
//! four stay refused: answering with a plausible digit array would let an
//! extension write into a buffer that is not the number's.
//!
//! `rb_integer_pack` and `rb_integer_unpack` are the SUPPORTED way to do the
//! same job -- they copy through a described format rather than exposing the
//! representation -- so both are implemented, and an extension that reaches
//! for them works.

use super::convert::{to_value, value_of};
use super::object::{cstr, send};
use super::symbol::symbol_of;
use super::value::Value;
use crate::builtins::wrong_arg_type;
use crate::{RubyValue, Signal};
use num_bigint::{BigInt, Sign};
use num_traits::{Signed, ToPrimitive, Zero};
use std::ffi::{c_char, c_double, c_int, c_long, c_void};

/// The `BigInt` a `VALUE` names, whichever representation it uses.
///
/// # Safety
///
/// `v` must be a live `VALUE`.
unsafe fn as_big(v: Value) -> Result<BigInt, Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Int(n) => Ok(BigInt::from(n)),
        RubyValue::BigInt(b) => Ok((*b).clone()),
        RubyValue::Float(f) => Ok(BigInt::from(f as i64)),
        other => Err(wrong_arg_type(&other, "Integer")),
    }
}

fn out_of_range(what: &str) -> Signal {
    crate::builtins::range_error!("{what} out of range")
}

/// An integer narrowed to a C type, or a `RangeError` naming it. Truncating
/// is what turns a gem's own overflow check into a silent wrong answer.
fn narrow<T, F>(v: &BigInt, to: F, what: &str) -> Result<T, Signal>
where
    F: Fn(&BigInt) -> Option<T>,
{
    to(v).ok_or_else(|| out_of_range(what))
}

/// `rb_cstr_to_inum`'s parse: `base == 0` means "read the prefix", and
/// `badcheck` decides whether trailing junk raises or is ignored.
/// What a parse refused, without building the exception. `raise_error` needs
/// the class registry, so the decision and the raise are kept apart -- which
/// is also what lets a unit test look at the decision.
#[derive(Debug)]
enum Bad {
    Radix(c_int),
    Value(&'static str),
}

impl Bad {
    fn signal(self, text: &str) -> Signal {
        match self {
            Bad::Radix(b) => {
                crate::builtins::arg_error!("invalid radix {b}")
            }
            Bad::Value(what) => {
                crate::builtins::arg_error!("invalid value for {what}(): \"{text}\"")
            }
        }
    }
}

fn parse_int(text: &str, base: c_int, badcheck: bool) -> Result<RubyValue, Signal> {
    parse_int_core(text, base, badcheck).map_err(|b| b.signal(text))
}

fn parse_dbl(text: &str, badcheck: bool) -> Result<c_double, Signal> {
    parse_dbl_core(text, badcheck).map_err(|b| b.signal(text))
}

fn parse_int_core(text: &str, base: c_int, badcheck: bool) -> Result<RubyValue, Bad> {
    let t = text.trim();
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (radix, digits) = match base {
        0 => match rest.get(..2).map(str::to_ascii_lowercase).as_deref() {
            Some("0x") => (16, &rest[2..]),
            Some("0b") => (2, &rest[2..]),
            Some("0o") => (8, &rest[2..]),
            _ if rest.starts_with('0') && rest.len() > 1 => (8, &rest[1..]),
            _ => (10, rest),
        },
        b if (2..=36).contains(&b) => (b as u32, rest),
        b => return Err(Bad::Radix(b)),
    };
    // Ruby lets `_` separate digits, and two in a row is invalid.
    let cleaned: String = digits.chars().filter(|c| *c != '_').collect();
    let end = cleaned
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(cleaned.len());
    if badcheck && (end != cleaned.len() || cleaned.is_empty()) {
        return Err(Bad::Value("Integer"));
    }
    let body = &cleaned[..end];
    if body.is_empty() {
        return Ok(RubyValue::Int(0));
    }
    let n = BigInt::parse_bytes(body.as_bytes(), radix).ok_or(Bad::Value("Integer"))?;
    Ok(crate::builtins::integer::int_value(if neg {
        -n
    } else {
        n
    }))
}

/// MRI's `rb_num_coerce_*` shape: ask the RIGHT operand to `coerce` the left,
/// then run the operator on the pair it answers. This is the protocol that
/// lets a user-defined Numeric appear on the right of `Integer#+`.
fn coerce_and_apply(x: Value, y: Value, op: &str, relop: bool) -> Result<Value, Signal> {
    let xv = unsafe { value_of(x) };
    let yv = unsafe { value_of(y) };
    let pair = match send(&yv, "coerce", std::slice::from_ref(&xv)) {
        Ok(RubyValue::Array(a)) if a.lock().len() == 2 => {
            let g = a.lock();
            (g[0].clone(), g[1].clone())
        }
        // A receiver with no `coerce` cannot take part. `<=>` answers nil
        // there, and every other operator raises -- which is exactly what
        // ruby does with `1 + Object.new`.
        _ => {
            if relop {
                return Ok(super::value::Q_NIL);
            }
            return Err(crate::builtins::type_error!(
                "{} can't be coerced into {}",
                crate::dispatch::class_name(yv.class_id()).unwrap_or("Object".into()),
                crate::dispatch::class_name(xv.class_id()).unwrap_or("Object".into())
            ));
        }
    };
    to_value(&send(&pair.0, op, &[pair.1])?)
}

/// `rb_integer_pack`'s flags, from `ruby/intern.h`.
mod pack {
    use std::ffi::c_int;
    pub const MSWORD_FIRST: c_int = 0x01;
    pub const LSWORD_FIRST: c_int = 0x02;
    pub const MSBYTE_FIRST: c_int = 0x10;
    pub const LSBYTE_FIRST: c_int = 0x20;
    pub const NATIVE_BYTE_ORDER: c_int = 0x40;
    pub const TWOCOMP: c_int = 0x100;
    pub const NEGATIVE: c_int = 0x200;
    /// `rb_integer_pack` sets this when the value did not fit.
    pub const OVERFLOW: c_int = 0x400;
}

/// Are the words most-significant first? MRI's default when neither bit is
/// set is LSWORD_FIRST, and setting BOTH is a caller bug rather than a
/// preference -- so `None` refuses rather than resolving to one of them.
///
/// A predicate rather than a `Result`: building the `ArgumentError` needs the
/// class registry, and this is the half a unit test can look at.
fn msword_first(flags: c_int) -> Option<bool> {
    match (
        flags & pack::MSWORD_FIRST != 0,
        flags & pack::LSWORD_FIRST != 0,
    ) {
        (true, true) => None,
        (ms, _) => Some(ms),
    }
}

/// `NATIVE_BYTE_ORDER` resolves to the host's, and wins over either bit --
/// which is what it is for.
fn msbyte_first(flags: c_int) -> Option<bool> {
    if flags & pack::NATIVE_BYTE_ORDER != 0 {
        return Some(cfg!(target_endian = "big"));
    }
    match (
        flags & pack::MSBYTE_FIRST != 0,
        flags & pack::LSBYTE_FIRST != 0,
    ) {
        (true, true) => None,
        (ms, _) => Some(ms),
    }
}

fn both_orders(what: &str) -> Signal {
    crate::builtins::arg_error!("both MS{what} and LS{what} order flags are set")
}

/// The two order questions, answered together or refused together.
fn orders(flags: c_int) -> Result<(bool, bool), Signal> {
    let word = msword_first(flags).ok_or_else(|| both_orders("word"))?;
    let byte = msbyte_first(flags).ok_or_else(|| both_orders("byte"))?;
    Ok((word, byte))
}

crate::cext_fn! {
    // ---- narrowing -----------------------------------------------------

    fn rb_big2long(v: Value) -> c_long {
        narrow(&unsafe { as_big(v)? }, BigInt::to_i64, "bignum too big to convert into `long'")
            .map(|n| n as c_long)
    }

    fn rb_big2ulong(v: Value) -> usize {
        narrow(&unsafe { as_big(v)? }, BigInt::to_u64, "bignum too big to convert into `ulong'")
            .map(|n| n as usize)
    }

    fn rb_big2dbl(v: Value) -> c_double {
        // MRI answers Infinity rather than raising for a Bignum past a
        // double's range, and warns. The value is the observable part.
        Ok(unsafe { as_big(v)? }.to_f64().unwrap_or(f64::INFINITY))
    }

    fn rb_num2ulong(v: Value) -> usize {
        narrow(&unsafe { as_big(v)? }, BigInt::to_u64, "integer").map(|n| n as usize)
    }

    fn rb_num2ull(v: Value) -> u64 {
        narrow(&unsafe { as_big(v)? }, BigInt::to_u64, "integer")
    }

    fn rb_num2uint(v: Value) -> usize {
        narrow(&unsafe { as_big(v)? }, BigInt::to_u32, "integer").map(|n| n as usize)
    }

    fn rb_num2short(v: Value) -> i16 {
        narrow(&unsafe { as_big(v)? }, BigInt::to_i16, "short")
    }

    fn rb_num2ushort(v: Value) -> u16 {
        narrow(&unsafe { as_big(v)? }, BigInt::to_u16, "ushort")
    }

    /// `rb_num2fix`: the value as a Fixnum, or a `RangeError`. An extension
    /// uses it where only an immediate will do.
    fn rb_num2fix(v: Value) -> Value {
        let n = narrow(&unsafe { as_big(v)? }, BigInt::to_i64, "integer")?;
        if !super::value::fits_fixnum(n) {
            return Err(out_of_range("integer"));
        }
        Ok(super::value::fixnum(n))
    }

    /// `rb_fix2int` and friends. MRI does not type-check the receiver here
    /// -- the name says the caller already knows -- but a wrong type would
    /// read a handle as an integer, so it is checked.
    fn rb_fix2int(v: Value) -> c_long {
        narrow(&unsafe { as_big(v)? }, BigInt::to_i32, "integer").map(|n| n as c_long)
    }

    fn rb_fix2uint(v: Value) -> usize {
        narrow(&unsafe { as_big(v)? }, BigInt::to_u32, "integer").map(|n| n as usize)
    }

    fn rb_fix2short(v: Value) -> i16 {
        narrow(&unsafe { as_big(v)? }, BigInt::to_i16, "short")
    }

    fn rb_fix2ushort(v: Value) -> u16 {
        narrow(&unsafe { as_big(v)? }, BigInt::to_u16, "ushort")
    }

    // ---- widening ------------------------------------------------------

    fn rb_int2big(n: isize) -> Value {
        to_value(&RubyValue::Int(n as i64))
    }

    fn rb_dbl2big(d: c_double) -> Value {
        if !d.is_finite() {
            return Err(crate::builtins::float_domain_error!("{}", RubyValue::Float(d).to_display_string()));
        }
        to_value(&crate::builtins::integer::int_value(
            BigInt::from(d.trunc() as i128),
        ))
    }

    /// `rb_float_new_in_heap`: MRI's out-of-line half of `rb_float_new`,
    /// for a double the flonum encoding cannot hold. zeo's handle answers
    /// either, so the two entries are one function.
    fn rb_float_new_in_heap(d: c_double) -> Value {
        to_value(&RubyValue::Float(d))
    }

    fn rb_dbl_cmp(a: c_double, b: c_double) -> Value {
        // NaN compares to nothing, which is `nil` rather than an ordering.
        let out = match a.partial_cmp(&b) {
            Some(o) => RubyValue::Int(o as i64),
            None => RubyValue::Nil,
        };
        to_value(&out)
    }

    fn rb_big_sign(v: Value) -> c_int {
        // MRI answers 1 for zero and positive, 0 for negative.
        Ok(c_int::from(!unsafe { as_big(v)? }.is_negative()))
    }

    fn rb_bigzero_p(v: Value) -> c_int {
        Ok(c_int::from(unsafe { as_big(v)? }.is_zero()))
    }

    /// `rb_big_norm`: shrink a Bignum to a Fixnum when it fits. zeo's
    /// `int_value` already does that on every construction, so this is a
    /// re-canonicalization and answers the same object for a Fixnum.
    fn rb_big_norm(v: Value) -> Value {
        to_value(&crate::builtins::integer::int_value(unsafe { as_big(v)? }))
    }

    fn rb_big2str(v: Value, base: c_int) -> Value {
        to_value(&int_to_str(&unsafe { as_big(v)? }, base)?)
    }

    fn rb_fix2str(v: Value, base: c_int) -> Value {
        to_value(&int_to_str(&unsafe { as_big(v)? }, base)?)
    }

    // ---- parsing -------------------------------------------------------

    fn rb_cstr_to_inum(p: *const c_char, base: c_int, badcheck: c_int) -> Value {
        to_value(&parse_int(&unsafe { cstr(p) }, base, badcheck != 0)?)
    }

    /// `rb_cstr2inum(p, base)` is `rb_cstr_to_inum` with badcheck on.
    fn rb_cstr2inum(p: *const c_char, base: c_int) -> Value {
        to_value(&parse_int(&unsafe { cstr(p) }, base, true)?)
    }

    fn rb_str_to_inum(v: Value, base: c_int, badcheck: c_int) -> Value {
        let s = unsafe { value_of(v) };
        let RubyValue::Str(s) = &s else {
            return Err(wrong_arg_type(&s, "String"));
        };
        let text = s.lock().to_utf8_lossy().into_owned();
        to_value(&parse_int(&text, base, badcheck != 0)?)
    }

    fn rb_cstr_to_dbl(p: *const c_char, badcheck: c_int) -> c_double {
        parse_dbl(&unsafe { cstr(p) }, badcheck != 0)
    }

    fn rb_str_to_dbl(v: Value, badcheck: c_int) -> c_double {
        let s = unsafe { value_of(v) };
        let RubyValue::Str(s) = &s else {
            return Err(wrong_arg_type(&s, "String"));
        };
        let text = s.lock().to_utf8_lossy().into_owned();
        parse_dbl(&text, badcheck != 0)
    }

    // ---- coercion ------------------------------------------------------

    fn rb_num_coerce_bin(x: Value, y: Value, op: super::symbol::Id) -> Value {
        coerce_and_apply(x, y, symbol_of(op).name_str(), false)
    }

    fn rb_num_coerce_bit(x: Value, y: Value, op: super::symbol::Id) -> Value {
        coerce_and_apply(x, y, symbol_of(op).name_str(), false)
    }

    /// `rb_num_coerce_cmp`: `<=>`, which answers nil rather than raising when
    /// the two cannot be compared.
    fn rb_num_coerce_cmp(x: Value, y: Value, op: super::symbol::Id) -> Value {
        coerce_and_apply(x, y, symbol_of(op).name_str(), true)
    }

    /// `rb_num_coerce_relop`: `<`, `<=`, `>`, `>=`. An uncomparable pair is
    /// an `ArgumentError` there, not a nil.
    fn rb_num_coerce_relop(x: Value, y: Value, op: super::symbol::Id) -> Value {
        let out = coerce_and_apply(x, y, symbol_of(op).name_str(), true)?;
        if out == super::value::Q_NIL {
            let xv = unsafe { value_of(x) };
            let yv = unsafe { value_of(y) };
            return Err(crate::builtins::arg_error!("comparison of {} with {} failed",
                    crate::dispatch::class_name(xv.class_id()).unwrap_or("Object".into()),
                    yv.to_display_string()));
        }
        Ok(out)
    }

    // ---- Rational and Complex -------------------------------------------

    fn rb_Rational(num: Value, den: Value) -> Value {
        kernel_call("Rational", &[num, den])
    }

    fn rb_rational_new(num: Value, den: Value) -> Value {
        kernel_call("Rational", &[num, den])
    }

    /// `rb_rational_raw`: build without normalizing. zeo's `Rational()`
    /// always reduces, and an unreduced Rational has no representation --
    /// so this is the reduced one, which is the value every reader sees.
    fn rb_rational_raw(num: Value, den: Value) -> Value {
        kernel_call("Rational", &[num, den])
    }

    fn rb_rational_num(v: Value) -> Value {
        let r = unsafe { value_of(v) };
        to_value(&send(&r, "numerator", &[])?)
    }

    fn rb_rational_den(v: Value) -> Value {
        let r = unsafe { value_of(v) };
        to_value(&send(&r, "denominator", &[])?)
    }

    fn rb_Complex(real: Value, imag: Value) -> Value {
        kernel_call("Complex", &[real, imag])
    }

    fn rb_complex_new(real: Value, imag: Value) -> Value {
        kernel_call("Complex", &[real, imag])
    }

    fn rb_complex_raw(real: Value, imag: Value) -> Value {
        kernel_call("Complex", &[real, imag])
    }

    fn rb_dbl_complex_new(real: c_double, imag: c_double) -> Value {
        let (r, i) = (to_value(&RubyValue::Float(real))?, to_value(&RubyValue::Float(imag))?);
        kernel_call("Complex", &[r, i])
    }

    fn rb_complex_mul(a: Value, b: Value) -> Value {
        let av = unsafe { value_of(a) };
        let bv = unsafe { value_of(b) };
        to_value(&send(&av, "*", &[bv])?)
    }

    fn rb_complex_uminus(v: Value) -> Value {
        let c = unsafe { value_of(v) };
        to_value(&send(&c, "-@", &[])?)
    }

    /// `rb_complex_polar(abs, arg)` and `rb_complex_new_polar` are one
    /// function in MRI too: `Complex.polar`.
    fn rb_complex_polar(abs: Value, arg: Value) -> Value {
        complex_polar(abs, arg)
    }

    fn rb_complex_new_polar(abs: Value, arg: Value) -> Value {
        complex_polar(abs, arg)
    }

    // ---- the random source ----------------------------------------------

    /// `rb_genrand_int32`: MRI's DEFAULT Mersenne Twister, which is
    /// `Random::DEFAULT`. Answering from `Random.rand` keeps a seeded
    /// program reproducible across the C boundary.
    fn rb_genrand_int32() -> u32 {
        let limit = RubyValue::Int(1 << 32);
        match random_call("rand", &[limit])? {
            RubyValue::Int(n) => Ok(n as u32),
            other => Err(wrong_arg_type(&other, "Integer")),
        }
    }

    fn rb_genrand_real() -> c_double {
        match random_call("rand", &[])? {
            RubyValue::Float(f) => Ok(f),
            other => Err(wrong_arg_type(&other, "Float")),
        }
    }

    /// `rb_genrand_ulong_limited(limit)`: `0..=limit`, INCLUSIVE, which is
    /// the one place MRI's C random differs from `Random#rand`'s exclusive
    /// upper bound.
    fn rb_genrand_ulong_limited(limit: usize) -> usize {
        bounded(None, limit)
    }

    fn rb_random_int32(rng: Value) -> u32 {
        let limit = RubyValue::Int(1 << 32);
        match random_call_on(rng, "rand", &[limit])? {
            RubyValue::Int(n) => Ok(n as u32),
            other => Err(wrong_arg_type(&other, "Integer")),
        }
    }

    fn rb_random_real(rng: Value) -> c_double {
        match random_call_on(rng, "rand", &[])? {
            RubyValue::Float(f) => Ok(f),
            other => Err(wrong_arg_type(&other, "Float")),
        }
    }

    fn rb_random_ulong_limited(rng: Value, limit: usize) -> usize {
        bounded(Some(rng), limit)
    }

    fn rb_random_bytes(rng: Value, n: c_long) -> Value {
        let count = RubyValue::Int(n.max(0));
        to_value(&random_call_on(rng, "bytes", &[count])?)
    }

    // ---- the supported Bignum transfer ----------------------------------

    /// `rb_integer_pack(val, words, numwords, wordsize, nails, flags)`.
    ///
    /// The supported way to read an Integer's magnitude: it COPIES through a
    /// described format rather than exposing zeo's representation. Answers
    /// the sign, with `INTEGER_PACK_OVERFLOW` set when the value did not fit
    /// -- which is the bit an extension checks before trusting the words.
    ///
    /// `nails` (bits left unused per word) is refused rather than ignored:
    /// nothing in the census passes one, and silently packing densely where
    /// the caller asked for padding would be a wrong answer it cannot see.
    fn rb_integer_pack(
        val: Value,
        words: *mut c_void,
        numwords: usize,
        wordsize: usize,
        nails: usize,
        flags: c_int,
    ) -> c_int {
        if nails != 0 {
            return Err(crate::builtins::not_impl_error!("zeo's rb_integer_pack does not support nails"));
        }
        let n = unsafe { as_big(val)? };
        let sign = if n.is_negative() { -1 } else { c_int::from(!n.is_zero()) };
        let magnitude = if flags & pack::TWOCOMP != 0 && n.is_negative() {
            // Two's complement over exactly the buffer the caller gave.
            let width = numwords * wordsize * 8;
            (BigInt::from(1) << width) + &n
        } else {
            n.abs()
        };
        let mut bytes = magnitude.to_bytes_be().1;
        let want = numwords * wordsize;
        let overflowed = bytes.len() > want;
        if overflowed {
            bytes.drain(..bytes.len() - want);
        }
        while bytes.len() < want {
            bytes.insert(0, 0);
        }
        // `bytes` is most-significant-byte-first over the whole number.
        // Split it into words, then order the words and each word's bytes.
        let mut out: Vec<Vec<u8>> = bytes.chunks(wordsize.max(1)).map(<[u8]>::to_vec).collect();
        let (msword, msbyte) = orders(flags)?;
        if !msword {
            out.reverse();
        }
        if !msbyte {
            for w in &mut out {
                w.reverse();
            }
        }
        if !words.is_null() {
            let flat: Vec<u8> = out.concat();
            // SAFETY: the caller promised `numwords * wordsize` bytes.
            unsafe { std::ptr::copy_nonoverlapping(flat.as_ptr(), words.cast::<u8>(), flat.len()) };
        }
        Ok(sign | if overflowed { pack::OVERFLOW } else { 0 })
    }

    /// `rb_integer_unpack(words, numwords, wordsize, nails, flags)`: the same
    /// format, read back into an Integer.
    fn rb_integer_unpack(
        words: *const c_void,
        numwords: usize,
        wordsize: usize,
        nails: usize,
        flags: c_int,
    ) -> Value {
        if nails != 0 {
            return Err(crate::builtins::not_impl_error!("zeo's rb_integer_unpack does not support nails"));
        }
        let total = numwords * wordsize;
        if words.is_null() || total == 0 {
            return to_value(&RubyValue::Int(0));
        }
        // SAFETY: the caller promised `numwords * wordsize` readable bytes.
        let raw = unsafe { std::slice::from_raw_parts(words.cast::<u8>(), total) };
        let mut chunks: Vec<Vec<u8>> = raw.chunks(wordsize).map(<[u8]>::to_vec).collect();
        let (msword, msbyte) = orders(flags)?;
        if !msbyte {
            for w in &mut chunks {
                w.reverse();
            }
        }
        if !msword {
            chunks.reverse();
        }
        let be: Vec<u8> = chunks.concat();
        let mut n = BigInt::from_bytes_be(Sign::Plus, &be);
        if flags & pack::TWOCOMP != 0 && be.first().is_some_and(|b| b & 0x80 != 0) {
            n -= BigInt::from(1) << (total * 8);
        } else if flags & pack::NEGATIVE != 0 {
            n = -n;
        }
        to_value(&crate::builtins::integer::int_value(n))
    }
}

/// How many bits the magnitude needs. `rb_absint_numwords`'s caller sizes a
/// buffer from this, so one too few is a truncated number.
///
/// # Safety
///
/// `v` must be a live `VALUE`.
pub(super) unsafe fn abs_bits(v: Value) -> Result<usize, Signal> {
    let n = unsafe { as_big(v)? }.abs();
    Ok(if n.is_zero() { 0 } else { n.bits() as usize })
}

/// Is the magnitude a power of two? Exactly one bit set is the whole test.
///
/// # Safety
///
/// `v` must be a live `VALUE`.
pub(super) unsafe fn abs_is_power_of_two(v: Value) -> Result<bool, Signal> {
    let n = unsafe { as_big(v)? }.abs();
    if n.is_zero() {
        return Ok(false);
    }
    // `n & (n - 1) == 0` is the classic test, and `BigInt` supports both.
    Ok((&n & (&n - 1u32)).is_zero())
}

/// `Integer#to_s(base)`, through the method rather than a second formatter./// `Integer#to_s(base)`, through the method rather than a second formatter.
fn int_to_str(n: &BigInt, base: c_int) -> Result<RubyValue, Signal> {
    if !(2..=36).contains(&base) {
        return Err(crate::builtins::arg_error!("invalid radix {base}"));
    }
    let v = crate::builtins::integer::int_value(n.clone());
    send(&v, "to_s", &[RubyValue::Int(base as i64)])
}

/// `rb_cstr_to_dbl`'s parse. `badcheck` off answers `0.0` for junk, which is
/// `"abc".to_f`; on, it refuses, which is `Float("abc")`.
fn parse_dbl_core(text: &str, badcheck: bool) -> Result<c_double, Bad> {
    let t: String = text.trim().chars().filter(|c| *c != '_').collect();
    let end = t
        .char_indices()
        .find(|(i, c)| {
            !(c.is_ascii_digit()
                || *c == '.'
                || (*i == 0 && (*c == '-' || *c == '+'))
                || c.eq_ignore_ascii_case(&'e')
                || ((*c == '-' || *c == '+') && t[..*i].ends_with(['e', 'E'])))
        })
        .map_or(t.len(), |(i, _)| i);
    match t[..end].parse::<f64>() {
        Ok(d) if end == t.len() || !badcheck => Ok(d),
        _ if !badcheck => Ok(0.0),
        _ => Err(Bad::Value("Float")),
    }
}

/// A `Kernel#Rational`/`Kernel#Complex` call, which is what MRI's C entries
/// reduce to. `main` is the receiver a top-level call has.
fn kernel_call(name: &str, args: &[Value]) -> Result<Value, Signal> {
    let args: Vec<RubyValue> = args.iter().map(|a| unsafe { value_of(*a) }).collect();
    let main = crate::dispatch::main_object();
    to_value(&crate::dispatch::send_value(
        &main,
        crate::Symbol::intern(name),
        &args,
        None,
    )?)
}

fn complex_polar(abs: Value, arg: Value) -> Result<Value, Signal> {
    let cls = crate::constants::const_get(zeo_abi::OBJECT_CLASS.0, "Complex")
        .ok_or_else(|| crate::builtins::name_error!("uninitialized constant Complex"))?;
    let (a, b) = (unsafe { value_of(abs) }, unsafe { value_of(arg) });
    to_value(&send(&cls, "polar", &[a, b])?)
}

/// `Random.rand` / `Random::DEFAULT`, so a seeded program and a C extension
/// draw from one stream.
fn random_call(meth: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let cls = crate::constants::const_get(zeo_abi::OBJECT_CLASS.0, "Random")
        .ok_or_else(|| crate::builtins::name_error!("uninitialized constant Random"))?;
    send(&cls, meth, args)
}

fn random_call_on(rng: Value, meth: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let r = unsafe { value_of(rng) };
    if matches!(r, RubyValue::Nil) {
        return random_call(meth, args);
    }
    send(&r, meth, args)
}

/// `0..=limit`, inclusive. `Random#rand(n)` is exclusive, so the argument is
/// `limit + 1` -- and `limit == usize::MAX` has no such argument, which is
/// the one case that has to be drawn a different way.
fn bounded(rng: Option<Value>, limit: usize) -> Result<usize, Signal> {
    if limit == 0 {
        return Ok(0);
    }
    if limit == usize::MAX {
        // Two 32-bit draws, because `limit + 1` does not fit.
        let hi = draw(rng, 1u64 << 32)? as u64;
        let lo = draw(rng, 1u64 << 32)? as u64;
        return Ok(((hi << 32) | lo) as usize);
    }
    draw(rng, limit as u64 + 1)
}

fn draw(rng: Option<Value>, exclusive: u64) -> Result<usize, Signal> {
    let arg = RubyValue::Int(exclusive as i64);
    let out = match rng {
        Some(r) => random_call_on(r, "rand", &[arg])?,
        None => random_call("rand", &[arg])?,
    };
    match out {
        RubyValue::Int(n) => Ok(n as usize),
        other => Err(wrong_arg_type(&other, "Integer")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parse is what `Integer("0x1f", 0)` and `"0b11".to_i(0)` run, and
    /// a wrong radix prefix is a silently wrong number rather than an error.
    #[test]
    fn a_base_of_zero_reads_the_prefix() {
        let cases = [
            ("0x1f", 31i64),
            ("0b101", 5),
            ("0o17", 15),
            ("017", 15),
            ("42", 42),
            ("-42", -42),
            ("+7", 7),
            ("1_000", 1000),
        ];
        for (text, want) in cases {
            match parse_int_core(text, 0, true) {
                Ok(RubyValue::Int(n)) => assert_eq!(n, want, "{text}"),
                other => panic!("{text} parsed as {other:?}"),
            }
        }
    }

    /// `badcheck` is the whole difference between `"12abc".to_i` and
    /// `Integer("12abc")`, so both directions are pinned.
    #[test]
    fn badcheck_decides_whether_trailing_junk_is_an_error() {
        assert!(matches!(
            parse_int_core("12abc", 10, false),
            Ok(RubyValue::Int(12))
        ));
        assert!(parse_int_core("12abc", 10, true).is_err());
        assert!(matches!(
            parse_int_core("", 10, false),
            Ok(RubyValue::Int(0))
        ));
        assert!(parse_int_core("", 10, true).is_err());
    }

    #[test]
    fn a_float_parse_stops_where_the_number_does() {
        assert_eq!(parse_dbl_core("3.5", true).unwrap_or(0.0), 3.5);
        assert_eq!(parse_dbl_core("1e3", true).unwrap_or(0.0), 1000.0);
        assert_eq!(parse_dbl_core("-2.5e-2", true).unwrap_or(0.0), -0.025);
        assert_eq!(parse_dbl_core("3.5xyz", false).unwrap_or(-1.0), 3.5);
        assert!(parse_dbl_core("3.5xyz", true).is_err());
        assert_eq!(parse_dbl_core("junk", false).unwrap_or(-1.0), 0.0);
    }

    /// The byte and word ordering flags are four independent bits, and
    /// getting one backwards is a number that reads correctly on one
    /// platform and not the other.
    #[test]
    fn the_pack_order_flags_are_read_independently() {
        assert_eq!(msword_first(pack::MSWORD_FIRST), Some(true));
        assert_eq!(msword_first(pack::LSWORD_FIRST), Some(false));
        assert_eq!(
            msword_first(0),
            Some(false),
            "MRI's default is LSWORD_FIRST"
        );
        assert_eq!(msbyte_first(pack::MSBYTE_FIRST), Some(true));
        assert_eq!(msbyte_first(pack::LSBYTE_FIRST), Some(false));
        assert_eq!(
            msbyte_first(pack::NATIVE_BYTE_ORDER),
            Some(cfg!(target_endian = "big"))
        );
        // Both bits at once is a caller bug, not a default.
        assert_eq!(msword_first(pack::MSWORD_FIRST | pack::LSWORD_FIRST), None);
        assert_eq!(msbyte_first(pack::MSBYTE_FIRST | pack::LSBYTE_FIRST), None);
    }
}
