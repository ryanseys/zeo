//! The instance + class method table every `Digest` algorithm class shares.
//! Each method reads the algorithm off its receiver (a `Digest::MD5`/`SHA1`/
//! `SHA256`/`SHA512` value, or one of those classes for the class methods), so
//! one table serves all four. It is registered here under `Digest::MD5`; the
//! other three ids alias it via the linkme entries in this module's parent.

use super::{
    algo_of_class, base64, block_length_of, digest_bubblebabble, digest_eq, digest_length_of,
    finalize, hex, in_bytes, new_digest, push_bytes, reset_buf, str,
};
use crate::builtins::arity;
use crate::encoding::ASCII_8BIT;
use crate::{RubyValue, string_from_bytes};
use zeo_macros::ruby_class;

/// The raw digest bytes of a class method's `str` argument under the receiver
/// class's algorithm.
fn class_raw(recv: &RubyValue, arg: &RubyValue) -> Result<Vec<u8>, crate::Signal> {
    Ok(algo_of_class(recv).raw(&in_bytes(arg)?))
}

ruby_class! {
    // Registered under Digest::MD5; SHA1/SHA256/SHA512 alias this same table.
    Digest = zeo_abi::DIGEST_MD5_CLASS < zeo_abi::OBJECT_CLASS;

    // -- class methods (Digest::SHA256.hexdigest(str), .new, ...) --
    def self."new"(recv, *args, &_block) {
        arity!(args, 0);
        Ok(new_digest(algo_of_class(recv)))
    }
    def self."hexdigest"(recv, *args, &_block) {
        arity!(args, 1);
        Ok(str(hex(&class_raw(recv, &args[0])?)))
    }
    def self."digest"(recv, *args, &_block) {
        arity!(args, 1);
        Ok(RubyValue::Str(string_from_bytes(class_raw(recv, &args[0])?, ASCII_8BIT)))
    }
    def self."base64digest"(recv, *args, &_block) {
        arity!(args, 1);
        Ok(str(base64(&class_raw(recv, &args[0])?)))
    }

    // -- streaming instance API --
    def "update" | "<<"(recv, *args, &_block) {
        arity!(args, 1);
        push_bytes(recv, &in_bytes(&args[0])?);
        Ok(recv.clone())
    }
    def "hexdigest" | "to_s"(recv, *args, &_block) {
        Ok(str(hex(&finalize(recv, args)?)))
    }
    def "digest"(recv, *args, &_block) {
        Ok(RubyValue::Str(string_from_bytes(finalize(recv, args)?, ASCII_8BIT)))
    }
    def "base64digest"(recv, *args, &_block) {
        Ok(str(base64(&finalize(recv, args)?)))
    }
    def "reset"(recv, *args, &_block) {
        arity!(args, 0);
        reset_buf(recv);
        Ok(recv.clone())
    }
    def "digest_length" | "length" | "size"(recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(digest_length_of(recv)))
    }
    def "block_length"(recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(block_length_of(recv)))
    }
    // `d == other`: another Digest compares by raw digest; anything else is
    // compared to `d`'s hexdigest (CRuby's `to_str` path).
    def "=="(recv, *args, &_block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(digest_eq(recv, &args[0])))
    }
    // The bubble babble of this object's current digest.
    def "bubblebabble"(recv, *args, &_block) {
        arity!(args, 0);
        Ok(str(digest_bubblebabble(recv)))
    }
}
