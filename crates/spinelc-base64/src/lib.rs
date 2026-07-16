//! The native half of the `base64` package (Phase 14.3) -- plain Rust free
//! functions with the native-package ABI (`fn(RubyValue, ...) ->
//! Result<RubyValue, Signal>`, one `RubyValue` per declared `native_func`
//! parameter), matching the declarations in
//! `packages/base64/lib/base64.rb`. Linked into a compiled program only
//! when that program actually `require "base64"`s (see
//! `spinelc::build::build_binary_with_deps`). No external base64 crate --
//! the transforms are ~40 lines, and this crate doubles as the worked
//! example of how little a native package needs.
//!
//! Semantics oracle-verified against real ruby's bundled `base64` gem
//! (4.0.5): `encode64` = RFC 2045 (a `\n` after every 60 encoded chars AND
//! at the end; `""` encodes to `""` with no newline); `strict_encode64` =
//! RFC 4648 (no newlines); `urlsafe_encode64` = `-`/`_` alphabet, padding
//! kept (ruby's `padding: false` option is out of spike scope);
//! `decode64` is liberal (skips whitespace/invalid chars, exactly ruby's
//! `unpack1("m")`); `strict_decode64`/`urlsafe_decode64` reject invalid
//! input. Two documented divergences: invalid strict input PANICS with
//! ruby's message text ("invalid base64") instead of raising a rescuable
//! `ArgumentError` (native crates can't construct exception objects -- the
//! same spinel-rt division of labor), and decoded bytes that aren't valid
//! UTF-8 are lossily replaced (this runtime's `Str` is a Rust `String`;
//! real ruby returns a BINARY-encoded string).

use spinel_rt::{string_new, RubyValue, Signal};

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn encode64(s: RubyValue) -> Result<RubyValue, Signal> {
    let data = str_arg(&s, "encode64");
    let raw = encode(data.as_bytes(), STD);
    let mut out = String::with_capacity(raw.len() + raw.len() / 60 + 1);
    for chunk in raw.as_bytes().chunks(60) {
        out.push_str(std::str::from_utf8(chunk).expect("base64 output is ASCII"));
        out.push('\n');
    }
    Ok(RubyValue::Str(string_new(out)))
}

pub fn strict_encode64(s: RubyValue) -> Result<RubyValue, Signal> {
    let data = str_arg(&s, "strict_encode64");
    Ok(RubyValue::Str(string_new(encode(data.as_bytes(), STD))))
}

pub fn urlsafe_encode64(s: RubyValue) -> Result<RubyValue, Signal> {
    let data = str_arg(&s, "urlsafe_encode64");
    Ok(RubyValue::Str(string_new(encode(data.as_bytes(), URL))))
}

pub fn decode64(s: RubyValue) -> Result<RubyValue, Signal> {
    let text = str_arg(&s, "decode64");
    // Liberal: keep only alphabet/padding characters, like ruby's
    // `unpack1("m")`.
    let cleaned: String = text
        .chars()
        .filter(|&c| c.is_ascii() && (val(STD, c as u8).is_some() || c == '='))
        .collect();
    let bytes = decode(cleaned.trim_end_matches('='), STD)
        .expect("liberal decode only sees pre-filtered alphabet chars");
    Ok(RubyValue::Str(string_new(
        String::from_utf8_lossy(&bytes).into_owned(),
    )))
}

pub fn strict_decode64(s: RubyValue) -> Result<RubyValue, Signal> {
    let text = str_arg(&s, "strict_decode64");
    Ok(RubyValue::Str(string_new(strict_decode(&text, STD))))
}

pub fn urlsafe_decode64(s: RubyValue) -> Result<RubyValue, Signal> {
    let text = str_arg(&s, "urlsafe_decode64");
    // Ruby (2.3+) accepts unpadded urlsafe input; re-pad before the strict
    // decode.
    let mut padded = text;
    while !padded.len().is_multiple_of(4) {
        padded.push('=');
    }
    Ok(RubyValue::Str(string_new(strict_decode(&padded, URL))))
}

/// Strict RFC 4648: length % 4 == 0, alphabet chars only, `=` padding only
/// at the very end. Panics with ruby's own message text on violation --
/// see the module docs' documented divergence.
fn strict_decode(text: &str, alphabet: &[u8; 64]) -> String {
    if !text.len().is_multiple_of(4) {
        panic!("invalid base64");
    }
    let stripped = text.trim_end_matches('=');
    if text.len() - stripped.len() > 2 {
        panic!("invalid base64");
    }
    let bytes = decode(stripped, alphabet).unwrap_or_else(|| panic!("invalid base64"));
    String::from_utf8_lossy(&bytes).into_owned()
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

/// Decodes padding-stripped input; `None` on any non-alphabet character or
/// an impossible leftover length (a single trailing 6-bit char).
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

fn str_arg(v: &RubyValue, method: &str) -> String {
    let RubyValue::Str(s) = v else {
        panic!("Base64.{method}: no implicit conversion into String (TypeError; spike scope: raised as a panic)");
    };
    s.lock().to_utf8_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }

    fn out(v: Result<RubyValue, Signal>) -> String {
        match v.expect("no signal") {
            RubyValue::Str(s) => s.lock().to_string(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    // Expected values are real ruby's own outputs (oracle-verified).
    #[test]
    fn encode64_matches_ruby_line_wrapping() {
        assert_eq!(out(encode64(s("hi"))), "aGk=\n");
        assert_eq!(out(encode64(s(""))), "");
        assert_eq!(
            out(encode64(s(&"a".repeat(70)))),
            "YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFh\nYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYQ==\n"
        );
    }

    #[test]
    fn strict_and_urlsafe_encode_match_ruby() {
        assert_eq!(
            out(strict_encode64(s(&"a".repeat(70)))),
            "YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYQ=="
        );
        // Bytes whose 6-bit groups exercise the -/_ tail of the alphabet
        // (ruby: Base64.urlsafe_encode64("\xFB\xEF\xBE") == "----").
        assert_eq!(encode(&[0xFB, 0xEF, 0xBE], URL), "----");
        assert_eq!(out(urlsafe_encode64(s("ab"))), "YWI=");
    }

    #[test]
    fn decode_variants_match_ruby() {
        assert_eq!(out(decode64(s("YWJj\n"))), "abc");
        assert_eq!(out(decode64(s("aGVsbG8gd29ybGQh"))), "hello world!");
        assert_eq!(out(strict_decode64(s("YWJj"))), "abc");
        assert_eq!(out(urlsafe_decode64(s("YWI="))), "ab");
        assert_eq!(out(urlsafe_decode64(s("YWI"))), "ab"); // unpadded accepted
    }

    #[test]
    #[should_panic(expected = "invalid base64")]
    fn strict_decode_rejects_embedded_whitespace_like_ruby() {
        let _ = strict_decode64(s("YWJj\n"));
    }
}
