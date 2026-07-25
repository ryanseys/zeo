//! The wide Unicode encodings: UTF-16LE/BE and UTF-32LE/BE. NOT
//! ASCII-compatible -- their strings are never `SevenBit`, and concatenation
//! with any other encoding is refused unless one side is empty.

use crate::enc::mb::InvalidStyle;

/// Which wide layout -- carried inside `EncKind::{Utf16,Utf32}`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Wide {
    pub width: usize, // 2 or 4
    pub be: bool,
}

/// One decoded unit: `scalar` is `Some` iff `valid`.
pub(crate) struct WideUnit {
    pub len: usize,
    pub scalar: Option<char>,
    pub style: InvalidStyle,
}

fn read16(bytes: &[u8], be: bool) -> u16 {
    if be {
        u16::from_be_bytes([bytes[0], bytes[1]])
    } else {
        u16::from_le_bytes([bytes[0], bytes[1]])
    }
}

/// Classifies the unit starting at `bytes[0]` (non-empty).
pub(crate) fn wide_unit(w: Wide, bytes: &[u8]) -> WideUnit {
    let bad = |len: usize, style: InvalidStyle| WideUnit {
        len,
        scalar: None,
        style,
    };
    if bytes.len() < w.width {
        // A trailing fragment shorter than one code unit.
        return bad(bytes.len(), InvalidStyle::Incomplete);
    }
    if w.width == 4 {
        let raw = if w.be {
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        } else {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        };
        return match char::from_u32(raw) {
            Some(c) => WideUnit {
                len: 4,
                scalar: Some(c),
                style: InvalidStyle::Plain,
            },
            None => bad(4, InvalidStyle::Plain),
        };
    }
    let u = read16(bytes, w.be);
    match u {
        // A high surrogate needs a low one: missing at end-of-string is
        // CRuby's "incomplete" form, a wrong next unit quotes its first
        // byte ("followed by") -- both oracle-verified.
        0xD800..=0xDBFF => {
            if bytes.len() < 4 {
                return bad(2, InvalidStyle::Incomplete);
            }
            let lo = read16(&bytes[2..], w.be);
            if (0xDC00..=0xDFFF).contains(&lo) {
                let cp = 0x10000 + (((u as u32) - 0xD800) << 10) + ((lo as u32) - 0xDC00);
                WideUnit {
                    len: 4,
                    scalar: char::from_u32(cp),
                    style: InvalidStyle::Plain,
                }
            } else {
                bad(2, InvalidStyle::FollowedBy(bytes[2]))
            }
        }
        // A lone low surrogate is plainly invalid.
        0xDC00..=0xDFFF => bad(2, InvalidStyle::Plain),
        _ => WideUnit {
            len: 2,
            scalar: char::from_u32(u as u32),
            style: InvalidStyle::Plain,
        },
    }
}

/// Every unit's byte range + decoded scalar, in order.
pub(crate) fn wide_ranges(
    w: Wide,
    bytes: &[u8],
) -> Vec<(std::ops::Range<usize>, Option<char>, InvalidStyle)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let unit = wide_unit(w, &bytes[i..]);
        out.push((i..i + unit.len, unit.scalar, unit.style));
        i += unit.len;
    }
    out
}

/// Encodes one scalar into the layout's bytes (always representable --
/// these are full Unicode encodings).
pub(crate) fn wide_encode_char(w: Wide, c: char) -> Vec<u8> {
    let mut out = Vec::with_capacity(w.width);
    if w.width == 4 {
        let raw = c as u32;
        out.extend_from_slice(&if w.be {
            raw.to_be_bytes()
        } else {
            raw.to_le_bytes()
        });
    } else {
        let mut units = [0u16; 2];
        for u in c.encode_utf16(&mut units) {
            out.extend_from_slice(&if w.be {
                u.to_be_bytes()
            } else {
                u.to_le_bytes()
            });
        }
    }
    out
}

/// The `Wide` layout of a `Utf16`/`Utf32` kind, `None` for every other.
pub(crate) fn wide_of(kind: crate::enc::table::EncKind) -> Option<Wide> {
    match kind {
        crate::enc::table::EncKind::Utf16 { be } => Some(Wide { width: 2, be }),
        crate::enc::table::EncKind::Utf32 { be } => Some(Wide { width: 4, be }),
        _ => None,
    }
}
