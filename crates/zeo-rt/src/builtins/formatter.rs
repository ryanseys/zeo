//! `Random::Formatter` (CRuby's `random/formatter.rb`) -- the mixin that turns a
//! raw source of random bytes into formatted values: `hex`, `base64`,
//! `urlsafe_base64`, `uuid`, `random_number`, `random_bytes`, `alphanumeric`.
//!
//! Like `Comparable` drives the receiver's own `<=>`, every method here draws
//! its entropy by re-dispatching `gen_random(n)` to the RECEIVER through full
//! dynamic dispatch (`send_value`). That is what lets ONE native impl serve
//! every host: `SecureRandom` and rubygems' vendored `Gem::SecureRandom` both
//! `extend Random::Formatter` and supply their own `gen_random`/`bytes` leaf
//! (which reads OS entropy via `Random.urandom`); a `Random` instance that
//! `include`s it supplies its PRNG stream. The module never generates bytes
//! itself.
//!
//! Dispatched like any other builtin module: `class_table`/`class_arity_table`
//! map `RANDOM_FORMATTER_MODULE` to this table's generated `lookup`/
//! `lookup_arity`, and `class_table_names` exposes the names so `extend`
//! can enumerate them.

use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

use crate::builtins::{arg_error, type_error};
use crate::dispatch::send_value;
use crate::encoding::ASCII_8BIT;
use crate::{RubyValue, Signal, Symbol, string_new};
use zeo_macros::ruby_module;

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `n` raw bytes from the receiver's own source -- `self.gen_random(n)`. The
/// one re-dispatch point; the host module (`SecureRandom`, ...) decides where
/// the bytes come from. A non-String answer is the host's bug, surfaced as a
/// `TypeError` rather than a panic.
fn entropy(recv: &RubyValue, n: i64) -> Result<Vec<u8>, Signal> {
    if n < 0 {
        return Err(arg_error!("negative string size (or size too big)"));
    }
    let v = send_value(
        recv,
        Symbol::intern("gen_random"),
        &[RubyValue::Int(n)],
        None,
    )?;
    let RubyValue::Str(_) = v else {
        return Err(type_error!(
            "gen_random must return a String of {n} bytes, got {}",
            crate::builtins::class_name_of(&v)
        ));
    };
    Ok(crate::builtins::convert::to_rstr(&v)?
        .lock()
        .bytes()
        .to_vec())
}

/// The byte-count argument shared by `random_bytes`/`hex`/`base64`/... : a
/// missing or `nil` count means the CRuby default (16); anything else runs the
/// `to_int` protocol. Negative sizes raise, matching `Random#bytes`.
fn count(n: Option<&RubyValue>, default: i64) -> Result<i64, Signal> {
    match n {
        None | Some(RubyValue::Nil) => Ok(default),
        Some(v) => {
            let n = crate::builtins::convert::to_index(v)?;
            if n < 0 {
                return Err(arg_error!("negative string size (or size too big)"));
            }
            Ok(n)
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
    }
    out
}

/// RFC 4648 base64 (what `Array#pack("m0")` produces) over `alphabet`; padding
/// with `=` when `pad`.
fn base64_encode(data: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(alphabet[((n >> 18) & 63) as usize] as char);
        out.push(alphabet[((n >> 12) & 63) as usize] as char);
        match chunk.len() {
            1 if pad => out.push('='),
            1 => {}
            _ => out.push(alphabet[((n >> 6) & 63) as usize] as char),
        }
        match chunk.len() {
            3 => out.push(alphabet[(n & 63) as usize] as char),
            _ if pad => out.push('='),
            _ => {}
        }
    }
    out
}

/// `bits` low bits assembled big-endian from `ceil(bits/8)` entropy bytes.
fn rand_bits(recv: &RubyValue, bits: u32) -> Result<u64, Signal> {
    let nbytes = bits.div_ceil(8) as i64;
    let bytes = entropy(recv, nbytes)?;
    let mut v: u64 = 0;
    for b in &bytes {
        v = (v << 8) | (*b as u64);
    }
    if bits < 64 {
        v &= (1u64 << bits) - 1;
    }
    Ok(v)
}

/// Uniform integer in `[0, n)` for `n > 0` by rejection sampling -- draw just
/// enough bits and retry on the rare over-range draw (no modulo bias).
fn rand_int_below(recv: &RubyValue, n: u64) -> Result<u64, Signal> {
    if n == 1 {
        return Ok(0);
    }
    let bits = 64 - (n - 1).leading_zeros();
    loop {
        let v = rand_bits(recv, bits)?;
        if v < n {
            return Ok(v);
        }
    }
}

/// `rand_int_below` for a bignum bound: same rejection sampling in arbitrary
/// precision.
fn rand_bigint_below(recv: &RubyValue, n: &BigInt) -> Result<BigInt, Signal> {
    let bits = n.bits();
    let nbytes = bits.div_ceil(8).max(1) as i64;
    // Mask the top byte down to the exact bit width so rejection converges fast.
    let top_mask: u8 = {
        let rem = (bits % 8) as u32;
        if rem == 0 { 0xff } else { (1u8 << rem) - 1 }
    };
    loop {
        let mut bytes = entropy(recv, nbytes)?;
        if let Some(first) = bytes.first_mut() {
            *first &= top_mask;
        }
        let v = BigInt::from_bytes_be(Sign::Plus, &bytes);
        if &v < n {
            return Ok(v);
        }
    }
}

/// A 53-bit random float in `[0.0, 1.0)` -- CRuby's `genrand_real` precision.
fn rand_float_unit(recv: &RubyValue) -> Result<f64, Signal> {
    let v = rand_bits(recv, 64)?;
    Ok((v >> 11) as f64 / (1u64 << 53) as f64)
}

/// Draw from a `source` array of 1-character strings, `n` characters long
/// (`Random::Formatter#choose`, the engine behind `alphanumeric`).
fn choose(recv: &RubyValue, source: &[RubyValue], n: i64) -> Result<RubyValue, Signal> {
    if source.is_empty() {
        return Err(arg_error!("source array must not be empty"));
    }
    let mut out = String::new();
    for _ in 0..n.max(0) {
        let idx = rand_int_below(recv, source.len() as u64)? as usize;
        out.push_str(
            &crate::builtins::convert::to_rstr(&source[idx])?
                .lock()
                .to_utf8_lossy(),
        );
    }
    Ok(RubyValue::Str(string_new(out)))
}

/// The default `alphanumeric` alphabet: `[*'A'..'Z', *'a'..'z', *'0'..'9']`.
fn default_alnum() -> Vec<RubyValue> {
    let mut chars = Vec::with_capacity(62);
    for range in [b'A'..=b'Z', b'a'..=b'z', b'0'..=b'9'] {
        for c in range {
            chars.push(RubyValue::Str(string_new((c as char).to_string())));
        }
    }
    chars
}

ruby_module! {
    Formatter = zeo_abi::RANDOM_FORMATTER_MODULE;

    // A default `gen_random` for a host that supplied only `bytes` -- bridges
    // back to it. A host defining its own `gen_random` (SecureRandom) shadows
    // this, so it is normally unused; it exists so `entropy`'s `gen_random`
    // send resolves for a `bytes`-only host too.
    def "gen_random"(recv, arg) {
        send_value(recv, Symbol::intern("bytes"), std::slice::from_ref(arg), None)
    }
    // `random_bytes(n = 16)` -- n raw bytes (ASCII-8BIT).
    def "random_bytes" params "n = nil"(recv, n?) {
        let bytes = entropy(recv, count(n, 16)?)?;
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, ASCII_8BIT)))
    }
    // `hex(n = 16)` -- 2n lowercase hex chars.
    def "hex" params "n = nil"(recv, n?) {
        let bytes = entropy(recv, count(n, 16)?)?;
        Ok(RubyValue::Str(string_new(hex_encode(&bytes))))
    }
    // `base64(n = 16)` -- RFC 4648 base64, padded.
    def "base64" params "n = nil"(recv, n?) {
        let bytes = entropy(recv, count(n, 16)?)?;
        Ok(RubyValue::Str(string_new(base64_encode(&bytes, STD, true))))
    }
    // `urlsafe_base64(n = 16, padding = false)` -- URL/filename-safe alphabet;
    // padding stripped unless the second argument is truthy.
    def "urlsafe_base64" params "n = nil, padding = nil"(recv, n?, padding?) {
        let bytes = entropy(recv, count(n, 16)?)?;
        let padding = matches!(padding, Some(v) if v.truthy());
        Ok(RubyValue::Str(string_new(base64_encode(&bytes, URL, padding))))
    }
    // `uuid` / `uuid_v4` -- a random RFC 9562 version-4 UUID.
    def "uuid"(recv) {
        uuid_v4(recv)
    }
    def "uuid_v4"(recv) {
        uuid_v4(recv)
    }
    // `random_number(n = 0)` -- an integer in `[0, n)` for a positive Integer,
    // a float in `[0.0, n)` for a positive Float, a value inside a Range, and a
    // float in `[0.0, 1.0)` for `0`/absent/non-positive (CRuby's fallback).
    // `rand` is the SAME method under a second name here, which is why a
    // `SecureRandom.rand` works. `Random::Base` defines its own `rand` below
    // this module in the MRO, so a real generator's `rand` is unaffected.
    def "random_number" | "rand"(recv, n?) {
        match n {
            None | Some(RubyValue::Nil) => Ok(RubyValue::Float(rand_float_unit(recv)?)),
            Some(RubyValue::Int(n)) if *n > 0 => {
                Ok(RubyValue::Int(rand_int_below(recv, *n as u64)? as i64))
            }
            Some(RubyValue::BigInt(n)) if n.sign() == Sign::Plus => {
                Ok(crate::builtins::integer::int_value(rand_bigint_below(recv, n)?))
            }
            Some(RubyValue::Float(f)) if *f > 0.0 => {
                Ok(RubyValue::Float(rand_float_unit(recv)? * *f))
            }
            Some(v @ RubyValue::Range(..)) => random_in_range(recv, v),
            _ => Ok(RubyValue::Float(rand_float_unit(recv)?)),
        }
    }
    // `alphanumeric(n = 16, chars: [A-Za-z0-9])`.
    def "alphanumeric" params "n = nil, chars: nil"(recv, n?, **opts) {
        let chars = chars_kwarg(opts);
        let n = count(n, 16)?;
        choose(recv, &chars, n)
    }
    // `choose(source, n)` -- public in CRuby's formatter.
    def "choose"(recv, arg1, arg2) {
        let RubyValue::Array(a) = arg1 else {
            return Err(type_error!("no implicit conversion into Array"));
        };
        let source = a.lock().to_vec();
        let n = crate::builtins::convert::to_index(arg2)?;
        choose(recv, &source, n)
    }
}

fn uuid_v4(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let mut b = entropy(recv, 16)?;
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant 1
    let h = hex_encode(&b);
    let s = format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    );
    Ok(RubyValue::Str(string_new(s)))
}

/// `random_number(range)` for an Integer or Float range -- draws inside it,
/// honoring `exclude_end?`. A begin-less or end-less range is an invalid
/// argument (there is no finite span to sample).
fn random_in_range(recv: &RubyValue, range: &RubyValue) -> Result<RubyValue, Signal> {
    let RubyValue::Range(__rg) = range else {
        return Err(type_error!("expected a Range"));
    };
    let (begin, end, excl) = __rg.parts();
    let invalid = || arg_error!("invalid argument - {}", range.inspect_string());
    match (begin, end) {
        (Some(RubyValue::Int(lo)), Some(RubyValue::Int(hi))) => {
            let span = hi - lo + if excl { 0 } else { 1 };
            if span <= 0 {
                return Err(invalid());
            }
            Ok(RubyValue::Int(
                lo + rand_int_below(recv, span as u64)? as i64,
            ))
        }
        (Some(b), Some(e)) => {
            let lo = to_f(b)?;
            let hi = to_f(e)?;
            if hi < lo || (excl && hi == lo) {
                return Err(invalid());
            }
            Ok(RubyValue::Float(lo + rand_float_unit(recv)? * (hi - lo)))
        }
        _ => Err(invalid()),
    }
}

fn to_f(v: &RubyValue) -> Result<f64, Signal> {
    match v {
        RubyValue::Int(i) => Ok(*i as f64),
        RubyValue::Float(f) => Ok(*f),
        RubyValue::BigInt(b) => Ok(b.to_f64().unwrap_or(f64::INFINITY)),
        _ => Err(type_error!("no implicit conversion into Float")),
    }
}

/// Split a trailing `chars:` keyword hash off `alphanumeric`'s args, returning
/// the positional args and the chosen alphabet.
fn chars_kwarg(opts: Option<&RubyValue>) -> Vec<RubyValue> {
    if let Some(RubyValue::Hash(h)) = opts {
        let chars = crate::collections::hash_get(h, &RubyValue::Symbol(Symbol::intern("chars")));
        if let RubyValue::Array(a) = chars {
            return a.lock().to_vec();
        }
    }
    default_alnum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encode_pads_each_byte_to_two_lowercase_nibbles() {
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_encode(&[0x00, 0xff, 0x10]), "00ff10");
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn base64_std_matches_rfc4648_vectors() {
        // The canonical RFC 4648 §10 progression (what `Array#pack("m0")` emits).
        let cases = [
            (&b""[..], ""),
            (&b"f"[..], "Zg=="),
            (&b"fo"[..], "Zm8="),
            (&b"foo"[..], "Zm9v"),
            (&b"foob"[..], "Zm9vYg=="),
            (&b"fooba"[..], "Zm9vYmE="),
            (&b"foobar"[..], "Zm9vYmFy"),
        ];
        for (input, expected) in cases {
            assert_eq!(base64_encode(input, STD, true), expected, "input {input:?}");
        }
    }

    #[test]
    fn base64_urlsafe_swaps_alphabet_and_honors_padding() {
        // 0xfb 0xff -> std "+/8=" (oracle-verified); the URL alphabet maps +,/ to -,_.
        assert_eq!(base64_encode(&[0xfb, 0xff], STD, true), "+/8=");
        assert_eq!(base64_encode(&[0xfb, 0xff], URL, true), "-_8=");
        assert_eq!(base64_encode(&[0xfb, 0xff], URL, false), "-_8");
        // No high bytes: std and url agree except padding.
        assert_eq!(base64_encode(b"foob", URL, false), "Zm9vYg");
    }
}
