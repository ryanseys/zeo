//! The instance + class method table every `Digest` algorithm class shares.
//! Each method reads the algorithm off its receiver (a `Digest::MD5`/`SHA1`/
//! `SHA256`/`SHA512` value, or one of those classes for the class methods), so
//! one table serves all four. It is registered here under `Digest::MD5`; the
//! other three ids alias it via the linkme entries in this module's parent.

use super::{
    algo_of_class, base64, block_length_of, digest_bubblebabble, digest_eq, digest_length_of,
    finalize, hex, in_bytes, new_digest, push_bytes, reset_buf, str,
};
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
    // `Digest::SHA2.new(bits)` picks the width; every other class takes no
    // argument and its own algorithm.
    def self."new" cfunc (recv, bits?) {
        let algo = match (bits, recv) {
            (Some(v), RubyValue::Class(cid)) if *cid == zeo_abi::DIGEST_SHA2_CLASS => {
                match crate::builtins::convert::to_index(v)? {
                    // 224 is NOT accepted -- probed: ruby's `SHA2` offers
                    // 256, 384 and 512 only.
                    256 => super::Algo::Sha256,
                    384 => super::Algo::Sha384,
                    512 => super::Algo::Sha512,
                    other => {
                        return Err(crate::builtins::arg_error!(
                            "unsupported bit length: {other}"
                        ));
                    }
                }
            }
            _ => algo_of_class(recv),
        };
        Ok(new_digest(algo))
    }
    // `.file(path)` -- a digest of the file's CONTENT, answering the
    // instance so `.hexdigest` reads off it.
    def self."file" (recv, path) {
        let path = crate::builtins::convert::to_rstr(path)?
            .lock()
            .to_utf8_lossy()
            .into_owned();
        let bytes = std::fs::read(&path)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "rb_sysopen", &path))?;
        let d = new_digest(algo_of_class(recv));
        crate::dispatch::send_value(
            &d,
            crate::Symbol::intern("update"),
            &[RubyValue::Str(string_from_bytes(bytes, ASCII_8BIT))],
            None,
        )?;
        Ok(d)
    }
    // `#<Digest::MD5: <hexdigest>>` -- CRuby embeds the CURRENT digest, so
    // it changes as the instance is fed. `SHA2` names its width in the class
    // half instead of a space (`#<Digest::SHA2:384 ...>`), because the class
    // alone does not say which width it is.
    def "inspect" (recv) {
        let algo = super::algo_of_instance(recv);
        let hex = hex(&super::digest_of_instance(recv));
        let name = crate::dispatch::class_name(recv.class_id()).unwrap_or_default();
        Ok(str(match recv.class_id() == zeo_abi::DIGEST_SHA2_CLASS {
            true => format!("#<{name}:{} {hex}>", algo.digest_length() * 8),
            false => format!("#<{name}: {hex}>"),
        }))
    }
    def self."hexdigest" cfunc (recv, arg, *_rest) {
        Ok(str(hex(&class_raw(recv, arg)?)))
    }
    def self."digest" cfunc (recv, arg, *_rest) {
        Ok(RubyValue::Str(string_from_bytes(class_raw(recv, arg)?, ASCII_8BIT)))
    }
    def self."base64digest" (recv, arg, *_rest) {
        Ok(str(base64(&class_raw(recv, arg)?)))
    }

    // -- streaming instance API --
    def "update" | "<<"(recv, other) {
        push_bytes(recv, &in_bytes(other)?);
        Ok(recv.clone())
    }
    def "hexdigest" | "to_s" arity 0 (recv, data?) {
        Ok(str(hex(&finalize(recv, data)?)))
    }
    def "digest"(recv, data?) {
        Ok(RubyValue::Str(string_from_bytes(finalize(recv, data)?, ASCII_8BIT)))
    }
    def "base64digest"(recv, data?) {
        Ok(str(base64(&finalize(recv, data)?)))
    }
    def "reset"(recv) {
        reset_buf(recv);
        Ok(recv.clone())
    }
    def "digest_length" | "length" | "size"(recv) {
        Ok(RubyValue::Int(digest_length_of(recv)))
    }
    def "block_length"(recv) {
        Ok(RubyValue::Int(block_length_of(recv)))
    }
    // `d == other`: another Digest compares by raw digest; anything else is
    // compared to `d`'s hexdigest (CRuby's `to_str` path).
    def "=="(recv, other) {
        Ok(RubyValue::Bool(digest_eq(recv, other)))
    }
    // The bubble babble of this object's current digest.
    def "bubblebabble"(recv) {
        Ok(str(digest_bubblebabble(recv)))
    }
}
