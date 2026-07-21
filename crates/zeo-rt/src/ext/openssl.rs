//! `openssl` (CRuby's C `openssl` extension). **Partially implemented** --
//! `require "openssl"` activates the `OpenSSL` constant, and the pure-Rust
//! surface that needs no TLS/PKey backend is provided here: `OpenSSL::Random`
//! bytes and the constant-time comparison helpers. The `Cipher`/`PKey`/`SSL`
//! surface still needs an FFI or rustls backend (see docs/EXTENSIONS.md).

use crate::builtins::{arg_error, arity, builtin_methods, type_error};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};

/// The bytes of a String argument, else CRuby's `no implicit conversion` error.
fn str_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    match v {
        RubyValue::Str(s) => Ok(s.lock().bytes().to_vec()),
        other => Err(type_error!(
            "no implicit conversion of {} into String",
            crate::builtins::convert_name_of(other)
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

/// OS entropy for `OpenSSL::Random.random_bytes` -- the real extension draws
/// from OpenSSL's RAND_bytes, so this must be a genuine CSPRNG, not a seeded
/// generator. `OpenSSL::Random::RandomError` is the CRuby failure surface.
fn fill_random(buf: &mut [u8]) -> Result<(), Signal> {
    getrandom::fill(buf)
        .map_err(|e| raise_error("OpenSSL::Random::RandomError", format!("RAND_bytes: {e}")))
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `OpenSSL::Random.random_bytes(n)` -- n cryptographically random bytes
    // (ASCII-8BIT), drawn from the OS CSPRNG.
    "random_bytes" => fn random_bytes(_recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(type_error!("no implicit conversion into Integer"));
        };
        if *n < 0 {
            return Err(arg_error!("negative string size (or size too big)"));
        }
        let mut buf = vec![0u8; *n as usize];
        fill_random(&mut buf)?;
        Ok(RubyValue::Str(crate::string_from_bytes(buf, crate::encoding::ASCII_8BIT)))
    }
    // `OpenSSL.fixed_length_secure_compare(a, b)` -- constant-time equality;
    // raises ArgumentError when the lengths differ.
    "fixed_length_secure_compare" => fn fixed_length_secure_compare(_recv, args, _block) {
        arity!(args, 2);
        let (a, b) = (str_bytes(&args[0])?, str_bytes(&args[1])?);
        if a.len() != b.len() {
            return Err(arg_error!("inputs must be of equal length"));
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
        let RubyValue::Str(bytes) = r else {
            panic!("expected a String")
        };
        assert_eq!(bytes.lock().bytes().len(), 16);
    }

    #[test]
    fn secure_compare_matches_equality() {
        assert!(matches!(
            secure_compare(&RubyValue::Nil, &[s("abc"), s("abc")], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            secure_compare(&RubyValue::Nil, &[s("abc"), s("abd")], None).unwrap(),
            RubyValue::Bool(false)
        ));
        // Different lengths compare unequal (never raise, unlike fixed_length).
        assert!(matches!(
            secure_compare(&RubyValue::Nil, &[s("abc"), s("abcd")], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    #[test]
    fn fixed_length_secure_compare_on_equal_length() {
        // The unequal-length ArgumentError path needs a class registry (it
        // panics registry-less), so it is exercised by the e2e example instead.
        assert!(matches!(
            fixed_length_secure_compare(&RubyValue::Nil, &[s("abc"), s("abc")], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            fixed_length_secure_compare(&RubyValue::Nil, &[s("abc"), s("abd")], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }
}
