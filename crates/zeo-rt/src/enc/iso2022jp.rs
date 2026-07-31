//! The ISO-2022-JP codec: JIS X 0208 behind escape-sequence mode switches
//! (`ESC $ B` in, `ESC ( B` out). The encoding is STATEFUL, which is exactly
//! why CRuby models it as a dummy encoding -- a string tagged with it has no
//! per-character structure, but it can still be transcoded, and this module
//! is that converter's two directions.
//!
//! The character repertoire rides on the EUC-JP tables: a JIS X 0208 pair
//! and its EUC-JP encoding differ only by the high bit (`euc = jis | 0x8080`),
//! so both directions translate through [`mb`]'s EucJp family.

use crate::enc::mb::{self, InvalidStyle, MbFamily};
use crate::enc::transcode::Unit;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `ESC ( B` (ASCII) -- and `ESC ( J` (JIS X 0201 Roman), which this
    /// codec reads identically, as CRuby's converter does.
    Ascii,
    /// `ESC $ B` / `ESC $ @` -- JIS X 0208 two-byte pairs.
    Kanji,
    /// `ESC ( I` -- halfwidth katakana. CRuby's DECODER rejects this escape,
    /// and so does this one; only the encoder can be asked to emit it
    /// (nkf's `-x -j`), see [`Encoder::new`].
    Kana,
}

/// Decodes ISO-2022-JP bytes into transcode units. Mode carries across
/// newlines; an unknown escape or a stray high byte is an `Invalid` unit in
/// CRuby's message shape.
pub(crate) fn decode_units(bytes: &[u8]) -> Vec<Unit> {
    let mut units = Vec::new();
    let mut mode = Mode::Ascii;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1B {
            match (bytes.get(i + 1), bytes.get(i + 2)) {
                (Some(b'('), Some(b'B' | b'J')) => {
                    mode = Mode::Ascii;
                    i += 3;
                }
                (Some(b'$'), Some(b'@' | b'B')) => {
                    mode = Mode::Kanji;
                    i += 3;
                }
                // A recognized intermediate with an unsupported final byte:
                // CRuby reports the two-byte lead `"\e("` followed by it.
                (Some(m @ (b'(' | b'$')), Some(f)) => {
                    units.push(Unit::Invalid(vec![0x1B, *m], InvalidStyle::FollowedBy(*f)));
                    i += 3;
                }
                (Some(b'(' | b'$'), None) => {
                    units.push(Unit::Invalid(bytes[i..].to_vec(), InvalidStyle::Incomplete));
                    i = bytes.len();
                }
                _ => {
                    units.push(Unit::Invalid(vec![0x1B], InvalidStyle::Plain));
                    i += 1;
                }
            }
            continue;
        }
        match mode {
            Mode::Ascii => {
                if b < 0x80 {
                    units.push(Unit::Char(b as char));
                } else {
                    units.push(Unit::Invalid(vec![b], InvalidStyle::Plain));
                }
                i += 1;
            }
            Mode::Kanji => {
                if !(0x21..=0x7E).contains(&b) {
                    units.push(Unit::Invalid(vec![b], InvalidStyle::Plain));
                    i += 1;
                    continue;
                }
                match bytes.get(i + 1) {
                    Some(t) if (0x21..=0x7E).contains(t) => {
                        let euc = [b | 0x80, t | 0x80];
                        match mb::mb_decode_seq(MbFamily::EucJp, &euc) {
                            Some(c) => units.push(Unit::Char(c)),
                            None => units.push(Unit::Unmapped(vec![b, *t])),
                        }
                        i += 2;
                    }
                    Some(t) => {
                        units.push(Unit::Invalid(vec![b], InvalidStyle::FollowedBy(*t)));
                        i += 2;
                    }
                    None => {
                        units.push(Unit::Invalid(vec![b], InvalidStyle::Incomplete));
                        i += 1;
                    }
                }
            }
            Mode::Kana => {
                // Unreachable through public decoding (the `( I` escape is
                // itself invalid), but kept total for encoder round-trips.
                if (0x21..=0x5F).contains(&b) {
                    let c = char::from_u32(0xFF61 + (b as u32 - 0x21)).expect("halfwidth range");
                    units.push(Unit::Char(c));
                } else {
                    units.push(Unit::Invalid(vec![b], InvalidStyle::Plain));
                }
                i += 1;
            }
        }
    }
    units
}

/// The stateful encoder: characters in, escape-switched bytes out. `finish`
/// must run last -- it restores ASCII mode, the `ESC ( B` every generator
/// emits at end of text.
pub(crate) struct Encoder {
    mode: Mode,
    /// Whether halfwidth katakana may be emitted as `ESC ( I` runs (nkf's
    /// `-x -j`). Off for `String#encode`, where CRuby refuses them.
    emit_halfwidth: bool,
}

impl Encoder {
    pub(crate) fn new(emit_halfwidth: bool) -> Self {
        Encoder {
            mode: Mode::Ascii,
            emit_halfwidth,
        }
    }

    fn enter(&mut self, mode: Mode, out: &mut Vec<u8>) {
        if self.mode == mode {
            return;
        }
        out.extend_from_slice(match mode {
            Mode::Ascii => b"\x1B(B",
            Mode::Kanji => b"\x1B$B",
            Mode::Kana => b"\x1B(I",
        });
        self.mode = mode;
    }

    /// Encodes one scalar, or `Err(())` when ISO-2022-JP can't represent it
    /// (the caller's `:undef` territory).
    pub(crate) fn push(&mut self, c: char, out: &mut Vec<u8>) -> Result<(), ()> {
        let cp = c as u32;
        if cp < 0x80 {
            self.enter(Mode::Ascii, out);
            out.push(cp as u8);
            return Ok(());
        }
        if (0xFF61..=0xFF9F).contains(&cp) {
            if !self.emit_halfwidth {
                return Err(());
            }
            self.enter(Mode::Kana, out);
            out.push((cp - 0xFF61 + 0x21) as u8);
            return Ok(());
        }
        match mb::mb_encode_char(MbFamily::EucJp, c) {
            // A two-byte EUC-JP character IS a JIS X 0208 pair with the high
            // bits set; anything else (the 0x8E halfwidth prefix) has no
            // ISO-2022-JP form.
            Some(euc) if euc.len() == 2 && euc[0] >= 0xA1 && euc[1] >= 0xA1 => {
                self.enter(Mode::Kanji, out);
                out.push(euc[0] & 0x7F);
                out.push(euc[1] & 0x7F);
                Ok(())
            }
            _ => Err(()),
        }
    }

    pub(crate) fn finish(&mut self, out: &mut Vec<u8>) {
        self.enter(Mode::Ascii, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(s: &str, emit_halfwidth: bool) -> Result<Vec<u8>, ()> {
        let mut enc = Encoder::new(emit_halfwidth);
        let mut out = Vec::new();
        for c in s.chars() {
            enc.push(c, &mut out)?;
        }
        enc.finish(&mut out);
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> Option<String> {
        let mut s = String::new();
        for u in decode_units(bytes) {
            match u {
                Unit::Char(c) => s.push(c),
                _ => return None,
            }
        }
        Some(s)
    }

    #[test]
    fn round_trips_kanji_with_the_oracle_bytes() {
        // "テスト" -- byte-for-byte what CRuby's converter emits.
        let jis = b"\x1B$B%F%9%H\x1B(B".to_vec();
        assert_eq!(encode("\u{30C6}\u{30B9}\u{30C8}", false), Ok(jis.clone()));
        assert_eq!(decode(&jis).as_deref(), Some("\u{30C6}\u{30B9}\u{30C8}"));
    }

    #[test]
    fn plain_ascii_needs_no_escapes() {
        assert_eq!(encode("abc", false), Ok(b"abc".to_vec()));
        assert_eq!(decode(b"abc").as_deref(), Some("abc"));
    }

    #[test]
    fn halfwidth_kana_is_refused_unless_asked_for() {
        assert_eq!(encode("\u{FF71}", false), Err(()));
        // nkf's -x -j form: ESC ( I, then the JIS X 0201 byte.
        assert_eq!(
            encode("\u{FF71}\u{FF72}", true),
            Ok(b"\x1B(I12\x1B(B".to_vec())
        );
    }

    #[test]
    fn the_kana_escape_is_invalid_on_decode_like_cruby() {
        assert!(matches!(
            decode_units(b"\x1B(I1\x1B(B").first(),
            Some(Unit::Invalid(lead, InvalidStyle::FollowedBy(b'I'))) if lead == &vec![0x1B, b'(']
        ));
    }

    #[test]
    fn roman_mode_reads_as_ascii() {
        assert_eq!(decode(b"\x1B(Jabc\x1B(B").as_deref(), Some("abc"));
    }
}
