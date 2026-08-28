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
}
