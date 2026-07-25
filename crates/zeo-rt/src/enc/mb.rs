//! The multibyte CJK encodings: Shift_JIS / Windows-31J, EUC-JP, GBK, Big5.
//!
//! Two layers, deliberately separate:
//! - STRUCTURE (lead/trail byte ranges, sequence lengths) is zeo's own walk,
//!   pinned to CRuby's validity model by oracle probes -- a structurally
//!   valid sequence is a real CHARACTER even when it maps to no Unicode
//!   scalar (`"\x82z"` in Shift_JIS: `valid_encoding?` true, length 1,
//!   transcoding raises `UndefinedConversionError`).
//! - MAPPING (sequence <-> Unicode) comes from encoding_rs's WHATWG tables.
//!   For Shift_JIS that means CP932/Windows-31J mappings for BOTH rows -- a
//!   documented divergence from CRuby's strict-JIS table in the NEC/IBM
//!   extension rows (see COMPATIBILITY.md).

/// Which multibyte family an encoding row belongs to -- the structural walk
/// and the encoding_rs backend hang off this.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MbFamily {
    /// Shift_JIS / Windows-31J: 1-byte ASCII + halfwidth kana (0xA1-0xDF),
    /// 2-byte lead 0x81-0x9F / 0xE0-0xFC, trail 0x40-0x7E / 0x80-0xFC.
    Sjis,
    /// EUC-JP: 2-byte lead 0xA1-0xFE, SS2 (0x8E) + 1, SS3 (0x8F) + 2,
    /// every trail 0xA1-0xFE.
    EucJp,
    /// GBK/CP936: 2-byte lead 0x81-0xFE, trail 0x40-0x7E / 0x80-0xFE.
    Gbk,
    /// Big5: 2-byte lead 0xA1-0xFE, trail 0x40-0x7E / 0xA1-0xFE.
    Big5,
}

impl MbFamily {
    pub(crate) fn backend(self) -> &'static encoding_rs::Encoding {
        match self {
            MbFamily::Sjis => encoding_rs::SHIFT_JIS,
            MbFamily::EucJp => encoding_rs::EUC_JP,
            MbFamily::Gbk => encoding_rs::GBK,
            MbFamily::Big5 => encoding_rs::BIG5,
        }
    }
}

/// One structural unit of a multibyte string.
pub(crate) struct MbUnit {
    /// Byte length (1..=3).
    pub len: usize,
    /// Structurally valid (a real character, mapped or not). A `false` unit
    /// is always a single byte: a naked non-lead byte, an invalid trail's
    /// LEAD (the trail then re-enters the walk on its own), or a lead
    /// truncated by end-of-string.
    pub valid: bool,
    /// `valid == false` refinement for CRuby's three
    /// `InvalidByteSequenceError` message forms: a lead cut off by
    /// end-of-string is "incomplete"; a lead with a wrong NEXT byte quotes
    /// that byte ("followed by"); a naked invalid byte is plain.
    pub style: InvalidStyle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvalidStyle {
    Plain,
    /// The invalid trail byte that followed a valid lead.
    FollowedBy(u8),
    /// A valid lead truncated by end-of-string.
    Incomplete,
}

fn ok(len: usize) -> MbUnit {
    MbUnit {
        len,
        valid: true,
        style: InvalidStyle::Plain,
    }
}
fn bad(style: InvalidStyle) -> MbUnit {
    MbUnit {
        len: 1,
        valid: false,
        style,
    }
}

/// Classifies the sequence starting at `bytes[0]` (non-empty).
pub(crate) fn mb_unit(family: MbFamily, bytes: &[u8]) -> MbUnit {
    let b = bytes[0];
    if b < 0x80 {
        return ok(1);
    }
    // A lead expecting `need` trail bytes, each validated by `trail`.
    let follow = |need: usize, trail: &dyn Fn(u8) -> bool| -> MbUnit {
        if bytes.len() <= need {
            return bad(InvalidStyle::Incomplete);
        }
        for &t in &bytes[1..=need] {
            if !trail(t) {
                return bad(InvalidStyle::FollowedBy(t));
            }
        }
        ok(need + 1)
    };
    match family {
        MbFamily::Sjis => match b {
            0xA1..=0xDF => ok(1), // halfwidth kana
            0x81..=0x9F | 0xE0..=0xFC => follow(1, &|t| matches!(t, 0x40..=0x7E | 0x80..=0xFC)),
            _ => bad(InvalidStyle::Plain),
        },
        MbFamily::EucJp => {
            let t = |t: u8| (0xA1..=0xFE).contains(&t);
            match b {
                0x8E => follow(1, &t),
                0x8F => follow(2, &t),
                0xA1..=0xFE => follow(1, &t),
                _ => bad(InvalidStyle::Plain),
            }
        }
        MbFamily::Gbk => match b {
            0x81..=0xFE => follow(1, &|t| matches!(t, 0x40..=0x7E | 0x80..=0xFE)),
            _ => bad(InvalidStyle::Plain),
        },
        MbFamily::Big5 => match b {
            0xA1..=0xFE => follow(1, &|t| matches!(t, 0x40..=0x7E | 0xA1..=0xFE)),
            _ => bad(InvalidStyle::Plain),
        },
    }
}

/// The byte range of every structural unit, with its validity -- the
/// multibyte analogue of the UTF-8 walk in `StrBuf::char_ranges`.
pub(crate) fn mb_ranges(family: MbFamily, bytes: &[u8]) -> Vec<(std::ops::Range<usize>, bool)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let unit = mb_unit(family, &bytes[i..]);
        out.push((i..i + unit.len, unit.valid));
        i += unit.len;
    }
    out
}

/// Decodes ONE structurally valid sequence through the encoding_rs backend:
/// `Some(c)` for a mapped character, `None` for a valid-but-unmapped one
/// (`:undef` territory).
pub(crate) fn mb_decode_seq(family: MbFamily, seq: &[u8]) -> Option<char> {
    // `decode_without_bom_handling`: plain `decode` sniffs for BOMs and
    // could switch encodings on adversarial byte patterns.
    let (text, had_errors) = family.backend().decode_without_bom_handling(seq);
    if had_errors {
        return None;
    }
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        // A halfwidth-kana or similar decoding to more than one scalar
        // doesn't occur for these families; treat it as unmapped if it ever
        // does.
        _ => None,
    }
}

/// Encodes one scalar into the family's bytes, `None` when unrepresentable.
pub(crate) fn mb_encode_char(family: MbFamily, c: char) -> Option<Vec<u8>> {
    let mut buf = [0u8; 4];
    let s: &str = c.encode_utf8(&mut buf);
    let (bytes, _, had_errors) = family.backend().encode(s);
    if had_errors {
        None
    } else {
        Some(bytes.into_owned())
    }
}

/// Why `mb_codepoint_bytes` refused -- maps to CRuby's two `RangeError`
/// messages (`N out of char range` vs `invalid codepoint 0xNNNN in ENC`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MbCodepointError {
    OutOfRange,
    InvalidCodepoint,
}

/// `Integer#chr(enc)` / `String#<<(Integer)` for a multibyte encoding: the
/// codepoint IS the character's byte sequence read as a big-endian number
/// (CRuby's model -- `0x82A0.chr(Shift_JIS)` is the two bytes 82 A0).
/// Structurally invalid splits are `InvalidCodepoint`; a value too long for
/// the family (or whose lead says "1 byte" while the value needs more) is
/// `OutOfRange` -- both oracle-verified.
pub fn mb_codepoint_bytes(family: MbFamily, cp: u32) -> Result<Vec<u8>, MbCodepointError> {
    if cp <= 0x7F {
        return Ok(vec![cp as u8]);
    }
    let mut bytes: Vec<u8> = cp
        .to_be_bytes()
        .into_iter()
        .skip_while(|b| *b == 0)
        .collect();
    if bytes.is_empty() {
        bytes.push(0);
    }
    let max_len = match family {
        MbFamily::EucJp => 3,
        _ => 2,
    };
    if bytes.len() > max_len {
        return Err(MbCodepointError::OutOfRange);
    }
    let unit = mb_unit(family, &bytes);
    if unit.valid && unit.len == bytes.len() {
        Ok(bytes)
    } else if bytes.len() > 1 && bytes[0] < 0x80 {
        // The would-be lead is a 1-byte character, so the value can never
        // spell a multi-byte sequence: out of range, not merely invalid.
        Err(MbCodepointError::OutOfRange)
    } else {
        Err(MbCodepointError::InvalidCodepoint)
    }
}
