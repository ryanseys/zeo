//! `openssl` (CRuby's C `openssl` extension). **Scaffolded** -- `require
//! "openssl"` activates the `OpenSSL` constant so downstream code compiles past
//! the require, but the crypto surface is not yet implemented (`todo!()`
//! markers). A real implementation would bring in the nested
//! `OpenSSL::Digest`/`HMAC`/`Cipher`/`Random`/`PKey` classes; the pure-Rust
//! `Digest`/`HMAC` parts could reuse RustCrypto (as `ext/digest.rs` does), the
//! TLS/PKey parts would need an FFI or rustls backend. See docs/EXTENSIONS.md.

use crate::builtins::builtin_methods;

builtin_methods! {
    pub(crate) fn lookup_class;

    "random_bytes" => fn random_bytes(_recv, _args, _block) { todo!("OpenSSL::Random.random_bytes -- see docs/EXTENSIONS.md") }
    "fixed_length_secure_compare" => fn fixed_length_secure_compare(_recv, _args, _block) { todo!("OpenSSL.fixed_length_secure_compare") }
    "secure_compare" => fn secure_compare(_recv, _args, _block) { todo!("OpenSSL.secure_compare") }
}
