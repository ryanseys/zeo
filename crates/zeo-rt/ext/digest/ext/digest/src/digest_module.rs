//! The `Digest` framework module itself (`Digest.hexencode`,
//! `Digest.bubblebabble`) -- the encoding helpers that take raw bytes and do no
//! hashing. The algorithm classes (`Digest::MD5` etc.) live in `algorithm.rs`.

use super::{bubble_babble, hex, in_bytes, str};
use zeo_macros::ruby_module;

ruby_module! {
    Digest = zeo_abi::DIGEST_MODULE;

    // `Digest.hexencode(str)` -- the lowercase hex of the raw bytes (no hashing).
    def self."hexencode"(_recv, arg) {
        Ok(str(hex(&in_bytes(arg)?)))
    }
    // `Digest.bubblebabble(str)` -- the bubble babble of the raw bytes.
    def self."bubblebabble"(_recv, arg) {
        Ok(str(bubble_babble(&in_bytes(arg)?)))
    }
    // `require "digest"` defines `Digest` alone, so an algorithm class
    // arrives at its FIRST REFERENCE, through this hook.
    def self."const_missing"(_recv, arg) {
        load_algorithm(arg)
    }
}

/// The file an algorithm's class name lives in, and the class it reveals.
/// Ruby writes this hook in `digest.rb`; it is a native row here as well
/// because a COMPUTED `require "digest"` runs no ruby half, and the hook has
/// to be there either way.
fn load_algorithm(name: &crate::RubyValue) -> Result<crate::RubyValue, crate::Signal> {
    let leaf = match name {
        crate::RubyValue::Symbol(s) => s.name(),
        other => other.inspect_string(),
    };
    let (feature, id) = match leaf.as_str() {
        "MD5" => ("digest/md5", zeo_abi::DIGEST_MD5_CLASS),
        "SHA1" => ("digest/sha1", zeo_abi::DIGEST_SHA1_CLASS),
        "SHA2" => ("digest/sha2", zeo_abi::DIGEST_SHA2_CLASS),
        "SHA256" => ("digest/sha2", zeo_abi::DIGEST_SHA256_CLASS),
        "SHA384" => ("digest/sha2", zeo_abi::DIGEST_SHA384_CLASS),
        "SHA512" => ("digest/sha2", zeo_abi::DIGEST_SHA512_CLASS),
        // Ruby reports the library it looked for, not the constant alone.
        other => {
            let lib = format!("digest/{}", other.to_lowercase());
            return Err(crate::builtins::load_error!(
                "library not found for class Digest::{other} -- {lib}"
            ));
        }
    };
    crate::features::load_builtin_feature(feature, 0);
    Ok(crate::RubyValue::Class(id))
}
