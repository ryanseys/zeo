//! `String#inspect`: the double-quoted, escaped rendering, per encoding.

use crate::strbuf::StrBuf;
use crate::table::EncKind;
use crate::transcode::{Unit, decode_utf8};

/// `String#inspect`: the double-quoted, escaped rendering. Printable
/// characters keep Rust's own debug escaping (which the runtime already
/// relied on); a byte that isn't a character in this string's encoding --
/// broken UTF-8, or a high byte of a non-Unicode encoding -- becomes
/// `\xNN`, exactly as CRuby shows it. For an all-UTF-8, all-valid string
/// this is byte-for-byte the old `format!("{:?}", s)`.
pub fn inspect(buf: &StrBuf) -> String {
    let mut out = String::from("\"");
    match buf.encoding().kind() {
        EncKind::Utf8 => {
            let units = decode_utf8(buf.bytes());
            for i in 0..units.len() {
                match &units[i] {
                    Unit::Char(c) => {
                        // `#` is escaped only before an interpolation sigil,
                        // so the next unit's character is needed.
                        let next = match units.get(i + 1) {
                            Some(Unit::Char(nc)) => Some(*nc),
                            _ => None,
                        };
                        push_inspect_char(&mut out, *c, next, true);
                    }
                    Unit::Invalid(bytes) => {
                        for b in bytes {
                            out.push_str(&format!("\\x{b:02X}"));
                        }
                    }
                }
            }
        }
        // A non-Unicode encoding: ASCII bytes escape as usual, high bytes as
        // `\xNN` (CRuby prints Latin-1 `0xE9` as `\xE9`, not as `é`).
        EncKind::Ascii | EncKind::Latin1 | EncKind::Binary => {
            let bytes = buf.bytes();
            for i in 0..bytes.len() {
                let b = bytes[i];
                if b < 0x80 {
                    let next = bytes
                        .get(i + 1)
                        .and_then(|&nb| (nb < 0x80).then_some(nb as char));
                    push_inspect_char(&mut out, b as char, next, false);
                } else {
                    out.push_str(&format!("\\x{b:02X}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// Escapes one character for `String#inspect` (CRuby `rb_str_inspect`): the
/// named control escapes, `\uXXXX`/`\xXX` for other control bytes (Unicode vs
/// byte encoding), `\#` before an interpolation sigil, and every other
/// character verbatim. Non-ASCII characters print literally, matching CRuby
/// for the printable-character common case (a documented simplification: the
/// rare non-printable Unicode formats/separators CRuby would escape are not
/// distinguished here).
fn push_inspect_char(out: &mut String, c: char, next: Option<char>, is_utf8: bool) {
    let cp = c as u32;
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\u{07}' => out.push_str("\\a"),
        '\u{08}' => out.push_str("\\b"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\u{0B}' => out.push_str("\\v"),
        '\u{0C}' => out.push_str("\\f"),
        '\r' => out.push_str("\\r"),
        '\u{1B}' => out.push_str("\\e"),
        '#' => {
            if matches!(next, Some('{' | '$' | '@')) {
                out.push('\\');
            }
            out.push('#');
        }
        _ if cp < 0x20 || cp == 0x7F => {
            if is_utf8 {
                out.push_str(&format!("\\u{cp:04X}"));
            } else {
                out.push_str(&format!("\\x{cp:02X}"));
            }
        }
        _ => out.push(c),
    }
}
