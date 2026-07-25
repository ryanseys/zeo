//! `base64` (CRuby's bundled `base64` gem) -- the first in-tree, require-gated
//! extension in the `ext/` tree. `require "base64"` activates it (a built-in
//! feature, resolved without a filesystem file, mirroring CRuby); the module's
//! functions dispatch through `class_method_table` on the `Base64` module value.
//!
//! Ported from the retired `zeo-base64` native crate. Because it now lives
//! inside `zeo-rt`, it raises proper rescuable `ArgumentError`/`TypeError`
//! (the native crate could only `panic!`, a divergence this migration retires).
//!
//! Semantics oracle-verified against ruby 4.0.5: `encode64` = RFC 2045 (a `\n`
//! every 60 chars and at the end; `""` -> `""`); `strict_encode64` = RFC 4648
//! (no newlines); `urlsafe_encode64` = `-`/`_` alphabet, padding kept;
//! `decode64` is liberal (skips non-alphabet chars, like `unpack1("m")`);
//! `strict_decode64`/`urlsafe_decode64` reject invalid input. Documented
//! divergence retained from the port: decoded non-UTF-8 bytes are lossily
//! replaced (this runtime's default `Str` is UTF-8; CRuby returns BINARY).

use crate::builtins::{arg_error, arity};
use crate::{RubyValue, Signal, string_new};
use zeo_macros::ruby_module;

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// A `String` argument, or a `TypeError` (the raise the native crate couldn't do).
fn str_arg(v: &RubyValue, _method: &str) -> Result<String, Signal> {
    // CRuby's message has no method prefix (base64 is plain Ruby over
    // `String#unpack1`/`Array#pack` -- oracle-verified).
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned())
}

/// The RAW bytes of a String argument -- what the encoders must operate on.
/// Reading through `to_utf8_lossy` would inflate every BINARY high byte
/// (`0xB3` -> `U+00B3` -> `C2 B3`), so a binary digest would encode as the
/// wrong, longer Base64. `to_str`-coerced, like CRuby's `Array#pack("m")`.
fn bytes_arg(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?.lock().bytes().to_vec())
}

ruby_module! {
    Base64 = zeo_abi::BASE64_MODULE;

    // `module_function` in CRuby's base64.rb: each is BOTH a public method on the
    // `Base64` module (`Base64.encode64`) and a private instance method reachable
    // through `include Base64` -- so both dispatch paths route here.
    module_function def "encode64" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        let data = bytes_arg(&args[0])?;
        let raw = encode(&data, STD);
        let mut out = String::with_capacity(raw.len() + raw.len() / 60 + 1);
        for chunk in raw.as_bytes().chunks(60) {
            out.push_str(std::str::from_utf8(chunk).expect("base64 output is ASCII"));
            out.push('\n');
        }
        Ok(RubyValue::Str(string_new(out)))
    }
    module_function def "strict_encode64" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        let data = bytes_arg(&args[0])?;
        Ok(RubyValue::Str(string_new(encode(&data, STD))))
    }
    module_function def "urlsafe_encode64" arity -2 (_recv, args, _block) {
        arity!(args, 1..=2);
        let data = bytes_arg(&args[0])?;
        let mut out = encode(&data, URL);
        // A `padding: false` keyword strips the trailing '=' padding.
        if let Some(RubyValue::Hash(h)) = args.get(1) {
            let pad = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("padding")));
            if matches!(pad, RubyValue::Bool(false)) {
                out = out.trim_end_matches('=').to_string();
            }
        }
        Ok(RubyValue::Str(string_new(out)))
    }
    module_function def "decode64" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        let text = str_arg(&args[0], "decode64")?;
        // Liberal: keep only alphabet/padding characters (like `unpack1("m")`).
        let cleaned: String = text
            .chars()
            .filter(|&c| c.is_ascii() && (val(STD, c as u8).is_some() || c == '='))
            .collect();
        let bytes = decode(cleaned.trim_end_matches('='), STD)
            .expect("liberal decode only sees pre-filtered alphabet chars");
        Ok(RubyValue::Str(string_new(String::from_utf8_lossy(&bytes).into_owned())))
    }
    module_function def "strict_decode64" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        let text = str_arg(&args[0], "strict_decode64")?;
        Ok(RubyValue::Str(string_new(strict_decode(&text, STD)?)))
    }
    module_function def "urlsafe_decode64" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        let mut text = str_arg(&args[0], "urlsafe_decode64")?;
        // Ruby (2.3+) accepts unpadded urlsafe input; re-pad before decoding.
        while !text.len().is_multiple_of(4) {
            text.push('=');
        }
        Ok(RubyValue::Str(string_new(strict_decode(&text, URL)?)))
    }
}

/// Strict RFC 4648: length % 4 == 0, alphabet chars only, `=` padding at the
/// end only. Raises `ArgumentError("invalid base64")` on violation (CRuby's
/// exact message -- now a rescuable raise, not the native crate's panic).
fn strict_decode(text: &str, alphabet: &[u8; 64]) -> Result<String, Signal> {
    let invalid = || arg_error!("invalid base64");
    if !text.len().is_multiple_of(4) {
        return Err(invalid());
    }
    let stripped = text.trim_end_matches('=');
    if text.len() - stripped.len() > 2 {
        return Err(invalid());
    }
    let bytes = decode(stripped, alphabet).ok_or_else(invalid)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn encode(data: &[u8], alphabet: &[u8; 64]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(alphabet[(n >> 18) as usize & 63] as char);
        out.push(alphabet[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            alphabet[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            alphabet[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decodes padding-stripped input; `None` on a non-alphabet char or an
/// impossible leftover length (a single trailing 6-bit char).
fn decode(stripped: &str, alphabet: &[u8; 64]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(stripped.len() / 4 * 3 + 2);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in stripped.bytes() {
        acc = (acc << 6) | u32::from(val(alphabet, c)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    if bits >= 6 {
        return None; // a lone trailing char can't carry a whole byte
    }
    Some(out)
}

fn val(alphabet: &[u8; 64], c: u8) -> Option<u8> {
    alphabet.iter().position(|&a| a == c).map(|i| i as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    fn out(v: Result<RubyValue, Signal>) -> String {
        match v.expect("no signal") {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }
    /// `Base64`'s `ruby_module!`-generated functions have mangled Rust idents, so
    /// the tests call them the way real dispatch does -- through the registered
    /// module-function `lookup` (the class-method bucket).
    fn f(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::BASE64_MODULE)
            .expect("Base64 is a registered builtin table")
            .class
            .as_ref()
            .expect("Base64 has module functions");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Base64.{name} is defined"))
    }

    #[test]
    fn encode_variants_match_ruby() {
        assert_eq!(out(f("encode64")(&RubyValue::Nil, &[s("hi")], None)), "aGk=\n");
        assert_eq!(out(f("encode64")(&RubyValue::Nil, &[s("")], None)), "");
        assert_eq!(
            out(f("strict_encode64")(&RubyValue::Nil, &[s("hello world!")], None)),
            "aGVsbG8gd29ybGQh"
        );
        assert_eq!(encode(&[0xFB, 0xEF, 0xBE], URL), "----");
    }

    #[test]
    fn decode_variants_match_ruby() {
        assert_eq!(out(f("decode64")(&RubyValue::Nil, &[s("YWJj\n")], None)), "abc");
        assert_eq!(
            out(f("strict_decode64")(&RubyValue::Nil, &[s("YWJj")], None)),
            "abc"
        );
        assert_eq!(
            out(f("urlsafe_decode64")(&RubyValue::Nil, &[s("YWI")], None)),
            "ab"
        );
    }
    // The `strict_decode64` raise path (now a rescuable ArgumentError, retiring
    // the native crate's panic) is covered by the e2e test -- `raise_error`
    // needs the class registry, which isn't installed in this unit tier.
}
