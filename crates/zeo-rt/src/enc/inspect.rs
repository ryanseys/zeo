//! `String#inspect`: the double-quoted, escaped rendering, per encoding.

use crate::enc::strbuf::StrBuf;
use crate::enc::table::EncKind;
use crate::enc::transcode::{Unit, decode_utf8};

/// `String#inspect`: the double-quoted, escaped rendering. Printable
/// characters keep Rust's own debug escaping; a byte that isn't a character
/// in this string's encoding -- broken UTF-8, or a high byte of a
/// non-Unicode encoding -- becomes `\xNN`, exactly as CRuby shows it.
pub fn inspect(buf: &StrBuf) -> String {
    let mut out = String::from("\"");
    // A dummy encoding's string has no readable characters at all: CRuby
    // renders every byte as `\xNN`, EXCEPT the named control escapes
    // (`"a\nb".force_encoding("ISO-2022-JP")` is `"\x61\n\x62"` -- even
    // printable ASCII is hex, but `\n`/`\e` keep their mnemonics;
    // oracle-verified).
    if buf.encoding().is_dummy() {
        for b in buf.bytes() {
            match b {
                0x07 => out.push_str("\\a"),
                0x08 => out.push_str("\\b"),
                b'\t' => out.push_str("\\t"),
                b'\n' => out.push_str("\\n"),
                0x0B => out.push_str("\\v"),
                0x0C => out.push_str("\\f"),
                b'\r' => out.push_str("\\r"),
                0x1B => out.push_str("\\e"),
                _ => out.push_str(&format!("\\x{b:02X}")),
            }
        }
        out.push('"');
        return out;
    }
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
                    // `Unmapped` never occurs under UTF-8, but rendering it
                    // like `Invalid` is right anywhere it could.
                    Unit::Invalid(bytes, _) | Unit::Unmapped(bytes) => {
                        for b in bytes {
                            out.push_str(&format!("\\x{b:02X}"));
                        }
                    }
                }
            }
        }
        // A non-Unicode single-byte encoding: ASCII bytes escape as usual,
        // high bytes as `\xNN` (CRuby prints Latin-1 `0xE9` as `\xE9`, not
        // as `é` -- and Windows-1252 the same, oracle-verified).
        EncKind::Ascii | EncKind::Latin1 | EncKind::Binary | EncKind::SingleByte => {
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
        // UTF-16/32, CRuby's form: decoded characters, printable ASCII as
        // usual, everything non-ASCII as `\uXXXX` (astral: `\u{XXXXX}`),
        // broken units' bytes as `\xNN` -- oracle-verified
        // (`"ab€".encode("UTF-16LE").inspect` is `"ab€"`,
        // `"AB𝄞"` gives `"AB\u{1D11E}"`).
        EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            let w = crate::enc::wide::wide_of(buf.encoding().kind()).expect("wide kind");
            let bytes = buf.bytes();
            let ranges = crate::enc::wide::wide_ranges(w, bytes);
            for i in 0..ranges.len() {
                let (r, scalar, _) = &ranges[i];
                match scalar {
                    Some(c) if (*c as u32) < 0x80 => {
                        let next = ranges
                            .get(i + 1)
                            .and_then(|(_, s, _)| s.filter(|nc| (*nc as u32) < 0x80));
                        push_inspect_char(&mut out, *c, next, true);
                    }
                    Some(c) => {
                        let cp = *c as u32;
                        if cp > 0xFFFF {
                            out.push_str(&format!("\\u{{{cp:X}}}"));
                        } else {
                            out.push_str(&format!("\\u{cp:04X}"));
                        }
                    }
                    None => {
                        for b in &bytes[r.clone()] {
                            out.push_str(&format!("\\x{b:02X}"));
                        }
                    }
                }
            }
        }
        // A multibyte CJK encoding, CRuby's forms: a MULTI-byte character
        // shows its raw bytes brace-grouped (`\x{82A0}`), a 1-byte high
        // character (halfwidth kana) or broken byte as `\xNN`, ASCII as
        // usual -- all oracle-verified.
        EncKind::MultiByte(family) => {
            let bytes = buf.bytes();
            for (r, _) in crate::enc::mb::mb_ranges(family, bytes) {
                if r.len() > 1 {
                    out.push_str("\\x{");
                    for b in &bytes[r] {
                        out.push_str(&format!("{b:02X}"));
                    }
                    out.push('}');
                } else {
                    let b = bytes[r.start];
                    if b < 0x80 {
                        let next = bytes
                            .get(r.start + 1)
                            .and_then(|&nb| (nb < 0x80).then_some(nb as char));
                        push_inspect_char(&mut out, b as char, next, false);
                    } else {
                        out.push_str(&format!("\\x{b:02X}"));
                    }
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
