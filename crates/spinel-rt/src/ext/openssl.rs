//! `openssl` (CRuby's C `openssl` extension). **Partially implemented** --
//! `require "openssl"` activates the `OpenSSL` constant, and the pure-Rust
//! surface that needs no TLS/PKey backend is provided here: `OpenSSL::Random`
//! bytes and the constant-time comparison helpers. The `Cipher`/`PKey`/`SSL`
//! surface still needs an FFI or rustls backend (see docs/EXTENSIONS.md).

use crate::builtins::{arity, builtin_methods};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};

/// The bytes of a String argument, else CRuby's `no implicit conversion` error.
fn str_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    match v {
        RubyValue::Str(s) => Ok(s.lock().bytes().to_vec()),
        other => Err(raise_error(
            "TypeError",
            format!("no implicit conversion of {} into String", crate::builtins::class_name_of(other)),
        )),
    }
}

/// A constant-time byte-equality check: always visits every byte of the
/// (equal-length) inputs, so timing does not leak where a mismatch is.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Non-cryptographic entropy for `OpenSSL::Random.random_bytes` -- a
/// SystemTime-seeded xorshift64*. Documented divergence: this is NOT a CSPRNG
/// (the real extension draws from OpenSSL's RAND_bytes); it is deterministic
/// only in that it produces well-distributed bytes, adequate for the
/// require-past-and-run posture of these ext modules.
fn fill_random(buf: &mut [u8]) {
    let mut state = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15)
        | 1;
    for chunk in buf.chunks_mut(8) {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let bytes = state.wrapping_mul(0x2545_f491_4f6c_dd1d).to_le_bytes();
        for (dst, src) in chunk.iter_mut().zip(bytes.iter()) {
            *dst = *src;
        }
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `OpenSSL::Random.random_bytes(n)` -- n pseudo-random bytes (ASCII-8BIT).
    "random_bytes" => fn random_bytes(_recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(raise_error("TypeError", "no implicit conversion into Integer".to_string()));
        };
        if *n < 0 {
            return Err(raise_error("ArgumentError", "negative string size (or size too big)".to_string()));
        }
        let mut buf = vec![0u8; *n as usize];
        fill_random(&mut buf);
        Ok(RubyValue::Str(crate::string_from_bytes(buf, crate::encoding::ASCII_8BIT)))
    }
    // `OpenSSL.fixed_length_secure_compare(a, b)` -- constant-time equality;
    // raises ArgumentError when the lengths differ.
    "fixed_length_secure_compare" => fn fixed_length_secure_compare(_recv, args, _block) {
        arity!(args, 2);
        let (a, b) = (str_bytes(&args[0])?, str_bytes(&args[1])?);
        if a.len() != b.len() {
            return Err(raise_error("ArgumentError", "inputs must be of equal length".to_string()));
        }
        Ok(RubyValue::Bool(constant_time_eq(&a, &b)))
    }
    // `OpenSSL.secure_compare(a, b)` -- length-independent constant-time
    // equality (true iff the strings are equal).
    "secure_compare" => fn secure_compare(_recv, args, _block) {
        arity!(args, 2);
        let (a, b) = (str_bytes(&args[0])?, str_bytes(&args[1])?);
        Ok(RubyValue::Bool(constant_time_eq(&a, &b)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string_new;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }

    #[test]
    fn random_bytes_returns_requested_length() {
        let r = random_bytes(&RubyValue::Nil, &[RubyValue::Int(16)], None).unwrap();
        let RubyValue::Str(bytes) = r else { panic!("expected a String") };
        assert_eq!(bytes.lock().bytes().len(), 16);
    }

    #[test]
    fn secure_compare_matches_equality() {
        assert!(matches!(secure_compare(&RubyValue::Nil, &[s("abc"), s("abc")], None).unwrap(), RubyValue::Bool(true)));
        assert!(matches!(secure_compare(&RubyValue::Nil, &[s("abc"), s("abd")], None).unwrap(), RubyValue::Bool(false)));
        // Different lengths compare unequal (never raise, unlike fixed_length).
        assert!(matches!(secure_compare(&RubyValue::Nil, &[s("abc"), s("abcd")], None).unwrap(), RubyValue::Bool(false)));
    }

    #[test]
    fn fixed_length_secure_compare_on_equal_length() {
        // The unequal-length ArgumentError path needs a class registry (it
        // panics registry-less), so it is exercised by the e2e example instead.
        assert!(matches!(fixed_length_secure_compare(&RubyValue::Nil, &[s("abc"), s("abc")], None).unwrap(), RubyValue::Bool(true)));
        assert!(matches!(fixed_length_secure_compare(&RubyValue::Nil, &[s("abc"), s("abd")], None).unwrap(), RubyValue::Bool(false)));
    }
}
