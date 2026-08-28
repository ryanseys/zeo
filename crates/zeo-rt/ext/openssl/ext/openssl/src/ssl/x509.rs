//! `OpenSSL::X509` -- the verify-result codes a `verify_result` reader or a
//! verify callback compares against. The classes under this module live in
//! `store.rs` and `cert.rs`.

use crate::RubyValue;
use zeo_macros::ruby_module;

ruby_module! {
    X509 = zeo_abi::OPENSSL_X509_MODULE;

    const V_OK = RubyValue::Int(0);
    const V_ERR_UNABLE_TO_GET_ISSUER_CERT = RubyValue::Int(2);
    const V_ERR_CERT_SIGNATURE_FAILURE = RubyValue::Int(7);
    const V_ERR_CERT_NOT_YET_VALID = RubyValue::Int(9);
    const V_ERR_CERT_HAS_EXPIRED = RubyValue::Int(10);
    const V_ERR_DEPTH_ZERO_SELF_SIGNED_CERT = RubyValue::Int(18);
    const V_ERR_SELF_SIGNED_CERT_IN_CHAIN = RubyValue::Int(19);
    const V_ERR_UNABLE_TO_GET_ISSUER_CERT_LOCALLY = RubyValue::Int(20);
    const V_ERR_CERT_UNTRUSTED = RubyValue::Int(27);
    const V_ERR_HOSTNAME_MISMATCH = RubyValue::Int(62);
}
