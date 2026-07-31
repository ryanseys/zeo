//! `OpenSSL::KDF` -- key derivation: `pbkdf2_hmac`, `hkdf` and `scrypt`,
//! all keyword-driven as CRuby's are. Failures raise
//! `OpenSSL::KDF::KDFError`; an absent required keyword is CRuby's plain
//! `ArgumentError` (`"missing keyword: :hash"`).

use super::{bin_str, md_from_value, req_kw, split_kwargs, str_bytes};
use crate::builtins::{arity, convert};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

fn kdf_error(e: openssl::error::ErrorStack) -> Signal {
    raise_error("OpenSSL::KDF::KDFError", format!("{e}"))
}

fn usize_kw(kwargs: &Option<RubyValue>, name: &str) -> Result<usize, Signal> {
    let v = req_kw(kwargs, name)?;
    let n = convert::to_index(&v)?;
    if n < 0 {
        return Err(crate::builtins::arg_error!("negative length"));
    }
    Ok(n as usize)
}

ruby_module! {
    KDF = zeo_abi::OPENSSL_KDF_MODULE;

    // `KDF.pbkdf2_hmac(pass, salt:, iterations:, length:, hash:)`.
    def self."pbkdf2_hmac" arity -1 (_recv, args, _block) {
        let (positional, kwargs) = split_kwargs(args);
        arity!(positional, 1);
        let pass = str_bytes(&positional[0])?;
        let salt = str_bytes(&req_kw(&kwargs, "salt")?)?;
        let iterations = convert::to_index(&req_kw(&kwargs, "iterations")?)?;
        let length = usize_kw(&kwargs, "length")?;
        let (md, _) = md_from_value(&req_kw(&kwargs, "hash")?)?;
        let mut out = vec![0u8; length];
        openssl::pkcs5::pbkdf2_hmac(&pass, &salt, iterations as usize, md, &mut out)
            .map_err(kdf_error)?;
        Ok(bin_str(out))
    }

    // `KDF.hkdf(ikm, salt:, info:, length:, hash:)` (RFC 5869).
    def self."hkdf" arity -1 (_recv, args, _block) {
        let (positional, kwargs) = split_kwargs(args);
        arity!(positional, 1);
        let ikm = str_bytes(&positional[0])?;
        let salt = str_bytes(&req_kw(&kwargs, "salt")?)?;
        let info = str_bytes(&req_kw(&kwargs, "info")?)?;
        let length = usize_kw(&kwargs, "length")?;
        let (md, _) = md_from_value(&req_kw(&kwargs, "hash")?)?;
        // `PkeyCtx` speaks the provider-era `Md`, not the legacy
        // `MessageDigest` -- bridge by NID.
        let md = openssl::md::Md::from_nid(md.type_())
            .ok_or_else(|| crate::builtins::arg_error!("unsupported digest algorithm"))?;
        let mut ctx = openssl::pkey_ctx::PkeyCtx::new_id(openssl::pkey::Id::HKDF)
            .map_err(kdf_error)?;
        ctx.derive_init().map_err(kdf_error)?;
        ctx.set_hkdf_md(md).map_err(kdf_error)?;
        ctx.set_hkdf_key(&ikm).map_err(kdf_error)?;
        ctx.set_hkdf_salt(&salt).map_err(kdf_error)?;
        ctx.add_hkdf_info(&info).map_err(kdf_error)?;
        let mut out = vec![0u8; length];
        ctx.derive(Some(&mut out)).map_err(kdf_error)?;
        Ok(bin_str(out))
    }

    // `KDF.scrypt(pass, salt:, N:, r:, p:, length:)`. `maxmem` 0 keeps
    // OpenSSL's default ceiling, as CRuby's binding does.
    def self."scrypt" arity -1 (_recv, args, _block) {
        let (positional, kwargs) = split_kwargs(args);
        arity!(positional, 1);
        let pass = str_bytes(&positional[0])?;
        let salt = str_bytes(&req_kw(&kwargs, "salt")?)?;
        let n = convert::to_index(&req_kw(&kwargs, "N")?)? as u64;
        let r = convert::to_index(&req_kw(&kwargs, "r")?)? as u64;
        let p = convert::to_index(&req_kw(&kwargs, "p")?)? as u64;
        let length = usize_kw(&kwargs, "length")?;
        let mut out = vec![0u8; length];
        openssl::pkcs5::scrypt(&pass, &salt, n, r, p, 0, &mut out).map_err(kdf_error)?;
        Ok(bin_str(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Symbol;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(text.to_string()))
    }
    fn kwargs(pairs: &[(&str, RubyValue)]) -> RubyValue {
        RubyValue::Hash(crate::hash_new(
            pairs
                .iter()
                .map(|(k, v)| (RubyValue::Symbol(Symbol::intern(k)), v.clone()))
                .collect(),
        ))
    }
    fn f(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::OPENSSL_KDF_MODULE)
            .expect("KDF is a registered builtin table")
            .class
            .as_ref()
            .expect("KDF has module functions");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("KDF.{name} is defined"))
    }
    fn hex_out(v: Result<RubyValue, Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => super::super::hex(s.lock().bytes()),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn pbkdf2_matches_ruby() {
        // ruby 4.0.6: KDF.pbkdf2_hmac("password", salt: "salt",
        //   iterations: 1000, length: 32, hash: "SHA256").
        let kw = kwargs(&[
            ("salt", s("salt")),
            ("iterations", RubyValue::Int(1000)),
            ("length", RubyValue::Int(32)),
            ("hash", s("SHA256")),
        ]);
        assert_eq!(
            hex_out(f("pbkdf2_hmac")(
                &RubyValue::Nil,
                &[s("password"), kw],
                None
            )),
            "632c2812e46d4604102ba7618e9d6d7d2f8128f6266b4a03264d2a0460b7dcb3"
        );
    }

    #[test]
    fn hkdf_matches_ruby() {
        let kw = kwargs(&[
            ("salt", s("salt")),
            ("info", s("info")),
            ("length", RubyValue::Int(32)),
            ("hash", s("SHA256")),
        ]);
        assert_eq!(
            hex_out(f("hkdf")(&RubyValue::Nil, &[s("ikm"), kw], None)),
            "fe8f9615d2374c0d17f77d1aeaf408c2e75fe0466073d0def23c733e2f862dfd"
        );
        let kw = kwargs(&[
            ("salt", s("")),
            ("info", s("")),
            ("length", RubyValue::Int(16)),
            ("hash", s("SHA256")),
        ]);
        assert_eq!(
            hex_out(f("hkdf")(&RubyValue::Nil, &[s("ikm"), kw], None)),
            "d58e7c1c4394984eb3ca168ff7b709d4"
        );
    }

    #[test]
    fn scrypt_matches_ruby() {
        let kw = kwargs(&[
            ("salt", s("salt")),
            ("N", RubyValue::Int(1024)),
            ("r", RubyValue::Int(8)),
            ("p", RubyValue::Int(16)),
            ("length", RubyValue::Int(16)),
        ]);
        assert_eq!(
            hex_out(f("scrypt")(&RubyValue::Nil, &[s("password"), kw], None)),
            "1effd93afcf2b28964026631bf4362b0"
        );
    }
}
