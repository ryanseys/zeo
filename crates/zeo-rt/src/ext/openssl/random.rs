//! `OpenSSL::Random` -- CSPRNG bytes. Its `RandomError` lives in the gem's
//! Ruby half, nested here by reopening this module.

use super::{bin_str, fill_random};
use crate::builtins::{arg_error, arity};
use zeo_macros::ruby_module;

ruby_module! {
    Random = zeo_abi::OPENSSL_RANDOM_MODULE;

    // `OpenSSL::Random.random_bytes(n)` -- n cryptographically random bytes
    // (ASCII-8BIT), drawn from the OS CSPRNG.
    def self."random_bytes" arity 1 (_recv, *args, &_block) {
        arity!(args, 1);
        let n = &crate::builtins::convert::to_index(&args[0])?;
        if *n < 0 {
            return Err(arg_error!("negative string size (or size too big)"));
        }
        let mut buf = vec![0u8; *n as usize];
        fill_random(&mut buf)?;
        Ok(bin_str(buf))
    }
}

#[cfg(test)]
mod tests {
    use crate::RubyValue;

    #[test]
    fn random_bytes_returns_requested_length() {
        let tbl = crate::builtins::registered_table(zeo_abi::OPENSSL_RANDOM_MODULE)
            .expect("OpenSSL::Random is a registered builtin table")
            .class
            .as_ref()
            .expect("OpenSSL::Random has module functions");
        let f = (tbl.lookup)("random_bytes").expect("random_bytes is defined");
        let r = f(&RubyValue::Nil, &[RubyValue::Int(16)], None).unwrap();
        let RubyValue::Str(bytes) = r else {
            panic!("expected a String")
        };
        assert_eq!(bytes.lock().bytes().len(), 16);
    }
}
