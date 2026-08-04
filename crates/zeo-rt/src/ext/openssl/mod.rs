//! `openssl` (CRuby's `openssl` gem, a C extension over libcrypto/libssl).
//! `require "openssl"` activates the `OpenSSL` constant and this class
//! surface, backed by the official rust-openssl bindings over a VENDORED
//! OpenSSL 3.x -- the same EVP implementations CRuby's extension binds, so
//! digest/cipher/BN behavior matches by construction rather than by
//! reimplementation:
//!
//! ```text
//! OpenSSL                    versions, Random, secure compares   (this file)
//! OpenSSL::Digest            EVP message digests                 (digest.rs)
//!  └ ::MD4 .. ::SHA512       fixed-algorithm subclasses      (algo_class.rs)
//! OpenSSL::HMAC              streaming keyed MAC                   (hmac.rs)
//! OpenSSL::KDF               pbkdf2_hmac / hkdf / scrypt            (kdf.rs)
//! OpenSSL::BN                OpenSSL's BIGNUM                        (bn.rs)
//! ```
//!
//! The exception hierarchy (`OpenSSL::OpenSSLError` and the per-class
//! errors) lives in `gems/openssl/lib/openssl.rb`, the gem's Ruby half --
//! see `ext/mod.rs` for why an extension cannot define its own. The native
//! half raises them by name.
//!
//! Digest/HMAC instances buffer their accumulated message and hash it on
//! demand (the `ext/digest` model) instead of holding a live `EVP_MD_CTX`:
//! CRuby's non-destructive `#hexdigest` peeks by copying the ctx, which the
//! rust-openssl `Hasher` cannot, and re-hashing the buffer is identical in
//! result.

pub(crate) mod algo_class;
pub(crate) mod bn;
pub(crate) mod cipher;
pub(crate) mod digest;
pub(crate) mod hmac;
pub(crate) mod kdf;
pub(crate) mod random;
pub(crate) mod ssl;

use crate::builtins::arg_error;
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

/// The bytes of a String argument, through the `to_str` protocol.
pub(super) fn str_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .bytes()
        .to_vec())
}

/// Wrap raw bytes as an ASCII-8BIT String -- digests, MACs, derived keys and
/// ciphertext are all binary, as CRuby's are.
pub(crate) fn bin_str(bytes: Vec<u8>) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT))
}

pub(crate) fn str(s: String) -> RubyValue {
    RubyValue::Str(crate::string_new(s))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard RFC 4648 base64 with padding -- inlined so `ext-openssl` doesn't
/// depend on the (separately gated) `ext-base64` module.
pub(crate) fn base64(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        s.push(B64[(n >> 18) as usize & 63] as char);
        s.push(B64[(n >> 12) as usize & 63] as char);
        s.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

/// Resolve a digest-algorithm argument -- a name String (any case) or an
/// `OpenSSL::Digest` instance -- to the EVP digest and its canonical name
/// (`"sha256"` -> `"SHA256"`, the NID short name CRuby reports).
pub(crate) fn md_from_value(
    v: &RubyValue,
) -> Result<(openssl::hash::MessageDigest, String), Signal> {
    if let RubyValue::Object(o) = v
        && let Some(d) = o.as_any().downcast_ref::<digest::RDigest>() {
            return md_by_name(&d.algo_name());
        }
    let name = crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    md_by_name(&name)
}

pub(crate) fn md_by_name(name: &str) -> Result<(openssl::hash::MessageDigest, String), Signal> {
    let md = openssl::hash::MessageDigest::from_name(name)
        .ok_or_else(|| digest_error(&format!("unsupported digest algorithm: {name}")))?;
    let canonical = md
        .type_()
        .short_name()
        .map(str::to_string)
        .unwrap_or_else(|_| name.to_uppercase());
    Ok((md, canonical))
}

pub(crate) fn digest_error(msg: &str) -> Signal {
    raise_error("OpenSSL::Digest::DigestError", msg.to_string())
}

/// One keyword out of a call's trailing Hash, `None` when absent.
pub(crate) fn kw(kwargs: &Option<RubyValue>, name: &str) -> Option<RubyValue> {
    let RubyValue::Hash(h) = kwargs.as_ref()? else {
        return None;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    crate::hash_pairs(h)
        .into_iter()
        .find(|(k, _)| k.rb_eq(&key))
        .map(|(_, v)| v)
}

/// A REQUIRED keyword -- CRuby's `ArgumentError` shape when it is absent.
pub(crate) fn req_kw(kwargs: &Option<RubyValue>, name: &str) -> Result<RubyValue, Signal> {
    kw(kwargs, name).ok_or_else(|| arg_error!("missing keyword: :{}", name))
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

    // `VERSION` is the bundled GEM's version (ruby 4.0.6 ships openssl
    // 4.0.2), independent of the linked library. The library constants
    // report the vendored OpenSSL this runtime actually carries; compiled
    // against == running against, so the two version strings agree.
    const VERSION = RubyValue::Str(crate::string_new("4.0.2".to_string()));
    const OPENSSL_VERSION =
        RubyValue::Str(crate::string_new(openssl::version::version().to_string()));
    const OPENSSL_LIBRARY_VERSION =
        RubyValue::Str(crate::string_new(openssl::version::version().to_string()));
    const OPENSSL_VERSION_NUMBER = RubyValue::Int(openssl::version::number());
    const OPENSSL_FIPS = RubyValue::Bool(false);

    // `OpenSSL.fixed_length_secure_compare(a, b)` -- constant-time equality;
    // raises ArgumentError when the lengths differ.
    def self."fixed_length_secure_compare" (_recv, arg1, arg2) {
        let (a, b) = (str_bytes(arg1)?, str_bytes(arg2)?);
        if a.len() != b.len() {
            return Err(arg_error!("inputs must be of equal length"));
        }
        Ok(RubyValue::Bool(constant_time_eq(&a, &b)))
    }
    // `OpenSSL.secure_compare(a, b)` -- length-independent constant-time
    // equality (true iff the strings are equal).
    def self."secure_compare" (_recv, arg1, arg2) {
        let (a, b) = (str_bytes(arg1)?, str_bytes(arg2)?);
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

    // Error paths (unknown algorithms, missing keywords) raise registered
    // exception classes, which panics registry-less -- the golden examples
    // exercise them.
    #[test]
    fn md_by_name_canonicalizes() {
        let (_, canonical) = md_by_name("sha256").unwrap();
        assert_eq!(canonical, "SHA256");
        let (_, canonical) = md_by_name("MD5").unwrap();
        assert_eq!(canonical, "MD5");
    }
}
