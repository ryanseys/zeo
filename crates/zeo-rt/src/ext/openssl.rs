//! `openssl` (CRuby's C `openssl` extension). **Partially implemented** --
//! `require "openssl"` activates the `OpenSSL` constant, and the pure-Rust
//! surface that needs no TLS/PKey backend is provided here: `OpenSSL::Random`
//! bytes and the constant-time comparison helpers. The `Cipher`/`PKey`/`SSL`
//! surface still needs an FFI or rustls backend (see docs/EXTENSIONS.md).

use crate::builtins::{arg_error, arity};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

/// The bytes of a String argument, through the `to_str` protocol.
fn str_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .bytes()
        .to_vec())
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
    // Gvl-released: the OS entropy pool can block right after boot.
    crate::gvl::without_gvl(|| getrandom::fill(buf))
        .map_err(|e| raise_error("OpenSSL::Random::RandomError", format!("RAND_bytes: {e}")))
}

ruby_module! {
    OpenSSL = zeo_abi::OPENSSL_MODULE;

    // `random_bytes` belongs to the `OpenSSL::Random` module and the two
    // compares to `OpenSSL` itself; zeo collapses both onto OPENSSL_MODULE and
    // dispatches them as class methods, so all three migrate as `def self.`.
    // `OpenSSL::Random.random_bytes(n)` -- n cryptographically random bytes
    // (ASCII-8BIT), drawn from the OS CSPRNG.
    def self."random_bytes" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        let n = &crate::builtins::convert::to_index(&args[0])?;
        if *n < 0 {
            return Err(arg_error!("negative string size (or size too big)"));
        }
        let mut buf = vec![0u8; *n as usize];
        fill_random(&mut buf)?;
        Ok(RubyValue::Str(crate::string_from_bytes(buf, crate::encoding::ASCII_8BIT)))
    }
    // `OpenSSL.fixed_length_secure_compare(a, b)` -- constant-time equality;
    // raises ArgumentError when the lengths differ.
    def self."fixed_length_secure_compare" arity 2 (_recv, args, _block) {
        arity!(args, 2);
        let (a, b) = (str_bytes(&args[0])?, str_bytes(&args[1])?);
        if a.len() != b.len() {
            return Err(arg_error!("inputs must be of equal length"));
        }
        Ok(RubyValue::Bool(constant_time_eq(&a, &b)))
    }
    // `OpenSSL.secure_compare(a, b)` -- length-independent constant-time
    // equality (true iff the strings are equal).
    def self."secure_compare" arity 2 (_recv, args, _block) {
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
    /// `OpenSSL`'s `ruby_module!`-generated functions have mangled Rust idents,
    /// so the tests call them through the registered class-method `lookup`.
    fn f(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::OPENSSL_MODULE)
            .expect("OpenSSL is a registered builtin table")
            .class
            .as_ref()
            .expect("OpenSSL has class methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("OpenSSL.{name} is defined"))
    }

    #[test]
    fn random_bytes_returns_requested_length() {
        let r = f("random_bytes")(&RubyValue::Nil, &[RubyValue::Int(16)], None).unwrap();
        let RubyValue::Str(bytes) = r else {
            panic!("expected a String")
        };
        assert_eq!(bytes.lock().bytes().len(), 16);
    }

    #[test]
    fn secure_compare_matches_equality() {
        assert!(matches!(
            f("secure_compare")(&RubyValue::Nil, &[s("abc"), s("abc")], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            f("secure_compare")(&RubyValue::Nil, &[s("abc"), s("abd")], None).unwrap(),
            RubyValue::Bool(false)
        ));
        // Different lengths compare unequal (never raise, unlike fixed_length).
        assert!(matches!(
            f("secure_compare")(&RubyValue::Nil, &[s("abc"), s("abcd")], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    #[test]
    fn fixed_length_secure_compare_on_equal_length() {
        // The unequal-length ArgumentError path needs a class registry (it
        // panics registry-less), so it is exercised by the e2e example instead.
        assert!(matches!(
            f("fixed_length_secure_compare")(&RubyValue::Nil, &[s("abc"), s("abc")], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            f("fixed_length_secure_compare")(&RubyValue::Nil, &[s("abc"), s("abd")], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }
}
