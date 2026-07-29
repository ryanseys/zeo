//! The gzip container (RFC 1952): the header that precedes a member's deflate
//! data and the eight-byte footer that follows it.
//!
//! Written by hand rather than taken from flate2 for two reasons. flate2's
//! pure-Rust backend does not compile `Compress::new_gzip`/`Decompress::
//! new_gzip` at all (they are `#[cfg(feature = "any_zlib")]`), so the framing
//! has to come from somewhere; and `Zlib::GzipFile`'s accessors -- `mtime`,
//! `orig_name`, `comment`, `os_code`, `level` -- need the header's FIELDS,
//! which flate2's `GzDecoder` exposes only partially and its encoder not at
//! all once a stream is under way.
//!
//! Everything here is bytes in, bytes out: no Ruby values, no `Signal`. The
//! mapping from [`Corrupt`] to `Zlib::GzipFile`'s exception classes is the
//! caller's, because the same corruption is a different class depending on
//! whether a `GzipReader` or `Zlib.gunzip` found it.

/// The two bytes every gzip member opens with.
pub(crate) const MAGIC: [u8; 2] = [0x1f, 0x8b];

/// The only compression method gzip defines.
const CM_DEFLATE: u8 = 8;

// FLG bits. `FTEXT` is advisory and ignored on read, as CRuby ignores it.
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;
const FHCRC: u8 = 0x02;

/// The OS byte zeo stamps. CRuby picks this per build platform the same way;
/// zeo targets Unix, so it is always 3 (`Zlib::OS_UNIX`).
pub(crate) const OS_CODE: u8 = 3;

/// A gzip member's header fields, as `Zlib::GzipFile`'s accessors see them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Header {
    /// The modification time, seconds since the epoch. Zero means "none set",
    /// which `GzipFile#mtime` still reports as `Time.at(0)` -- CRuby does too.
    pub mtime: u32,
    /// The extra-flags byte, which encodes only the compression level.
    pub xfl: u8,
    pub os: u8,
    pub orig_name: Option<Vec<u8>>,
    pub comment: Option<Vec<u8>>,
}

impl Default for Header {
    fn default() -> Self {
        Header {
            mtime: 0,
            xfl: 0,
            os: OS_CODE,
            orig_name: None,
            comment: None,
        }
    }
}

/// What went wrong reading a header or footer. Each variant carries CRuby's
/// own message for it, so the caller only has to choose the exception class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Corrupt {
    NotGzip,
    UnsupportedMethod,
    CrcMismatch,
    LengthMismatch,
}

impl Corrupt {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Corrupt::NotGzip => "not in gzip format",
            Corrupt::UnsupportedMethod => "unsupported compression method",
            Corrupt::CrcMismatch => "invalid compressed data -- crc error",
            Corrupt::LengthMismatch => "invalid compressed data -- length error",
        }
    }
}

/// The eight-byte footer: CRC-32 of the uncompressed bytes, then their length
/// modulo 2^32, both little-endian.
pub(crate) fn footer(crc: u32, size: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out
}

/// Check a member's footer against what was actually decompressed.
pub(crate) fn check_footer(bytes: &[u8; 8], crc: u32, size: u32) -> Result<(), Corrupt> {
    if u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != crc {
        return Err(Corrupt::CrcMismatch);
    }
    if u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) != size {
        return Err(Corrupt::LengthMismatch);
    }
    Ok(())
}

/// The XFL byte for a compression level. gzip records only the two extremes;
/// everything between is "no information".
pub(crate) fn xfl_for_level(level: u32) -> u8 {
    match level {
        9 => 2,
        1 => 4,
        _ => 0,
    }
}

/// The level a header's XFL byte implies, as `GzipFile#level` reports it. Any
/// value but the two extremes means the writer said nothing, which is
/// `Zlib::DEFAULT_COMPRESSION`.
pub(crate) fn level_from_xfl(xfl: u8) -> i64 {
    match xfl {
        2 => 9,
        4 => 1,
        _ => -1,
    }
}

impl Header {
    /// Serialize, ready to precede the member's deflate data. zeo never emits
    /// FEXTRA or FHCRC (nothing in the Ruby surface can ask for them), but
    /// [`Header::parse`] accepts both.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut flg = 0u8;
        if self.orig_name.is_some() {
            flg |= FNAME;
        }
        if self.comment.is_some() {
            flg |= FCOMMENT;
        }
        let mut out = Vec::with_capacity(10);
        out.extend_from_slice(&MAGIC);
        out.push(CM_DEFLATE);
        out.push(flg);
        out.extend_from_slice(&self.mtime.to_le_bytes());
        out.push(self.xfl);
        out.push(self.os);
        for text in [&self.orig_name, &self.comment].into_iter().flatten() {
            // A NUL would truncate the field on read, so it cannot appear
            // in one; CRuby raises on that and so does the caller here.
            out.extend_from_slice(text);
            out.push(0);
        }
        out
    }

    /// Parse a header off the front of `bytes`, answering it and how many
    /// bytes it occupied.
    ///
    /// `Ok(None)` means "not yet" -- the bytes so far are a valid PREFIX of a
    /// header and more input could complete it. That is the distinction a
    /// streaming reader needs and the reason this doesn't just return
    /// `Result<Header, _>`.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Option<(Header, usize)>, Corrupt> {
        // The magic is checked as soon as each byte arrives, so a reader over
        // a non-gzip stream fails on the first byte rather than blocking for
        // ten.
        for (i, &want) in MAGIC.iter().enumerate() {
            match bytes.get(i) {
                None => return Ok(None),
                Some(&got) if got != want => return Err(Corrupt::NotGzip),
                Some(_) => {}
            }
        }
        if bytes.len() < 10 {
            return Ok(None);
        }
        if bytes[2] != CM_DEFLATE {
            return Err(Corrupt::UnsupportedMethod);
        }
        let flg = bytes[3];
        let header = Header {
            mtime: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            xfl: bytes[8],
            os: bytes[9],
            orig_name: None,
            comment: None,
        };
        let mut at = 10;

        if flg & FEXTRA != 0 {
            let Some(len) = bytes.get(at..at + 2) else {
                return Ok(None);
            };
            let len = u16::from_le_bytes([len[0], len[1]]) as usize;
            if bytes.len() < at + 2 + len {
                return Ok(None);
            }
            at += 2 + len;
        }

        let mut header = header;
        for (bit, field) in [
            (FNAME, &mut header.orig_name),
            (FCOMMENT, &mut header.comment),
        ] {
            if flg & bit == 0 {
                continue;
            }
            match bytes[at..].iter().position(|&b| b == 0) {
                None => return Ok(None),
                Some(end) => {
                    *field = Some(bytes[at..at + end].to_vec());
                    at += end + 1;
                }
            }
        }

        if flg & FHCRC != 0 {
            // A header CRC-16 zeo never writes and does not verify -- zlib
            // itself only checks it when the caller asks for the header.
            if bytes.len() < at + 2 {
                return Ok(None);
            }
            at += 2;
        }
        Ok(Some((header, at)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_header_is_the_ten_byte_minimum() {
        let bytes = Header::default().encode();
        assert_eq!(bytes, vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3]);
        let (parsed, used) = Header::parse(&bytes).unwrap().expect("complete");
        assert_eq!(used, 10);
        assert_eq!(parsed.mtime, 0);
        assert_eq!(parsed.os, OS_CODE);
        assert!(parsed.orig_name.is_none() && parsed.comment.is_none());
    }

    #[test]
    fn name_and_comment_round_trip() {
        let header = Header {
            mtime: 1234567890,
            xfl: 2,
            os: 3,
            orig_name: Some(b"f.txt".to_vec()),
            comment: Some(b"hi".to_vec()),
        };
        let bytes = header.encode();
        // Ruby 4.0.6 writes exactly this for `w.mtime = Time.at(1234567890);
        // w.orig_name = "f.txt"; w.comment = "hi"` (its FLG is FNAME|FCOMMENT).
        assert_eq!(bytes[3], FNAME | FCOMMENT);
        assert_eq!(&bytes[4..8], &1234567890u32.to_le_bytes());
        let (parsed, used) = Header::parse(&bytes).unwrap().expect("complete");
        assert_eq!(used, bytes.len());
        assert_eq!(parsed.orig_name.as_deref(), Some(&b"f.txt"[..]));
        assert_eq!(parsed.comment.as_deref(), Some(&b"hi"[..]));
        assert_eq!(parsed.mtime, 1234567890);
    }

    /// The distinction the streaming reader is built on: a prefix is "not
    /// yet", a wrong byte is a failure, and the failure is reported from the
    /// first byte that disagrees rather than after ten arrive.
    #[test]
    fn a_prefix_is_incomplete_but_a_wrong_magic_is_fatal() {
        let full = Header {
            orig_name: Some(b"name".to_vec()),
            ..Header::default()
        }
        .encode();
        for cut in 0..full.len() {
            assert_eq!(
                Header::parse(&full[..cut]),
                Ok(None),
                "{cut} bytes is a prefix, not a header"
            );
        }
        assert_eq!(Header::parse(b"\x1f\x8c"), Err(Corrupt::NotGzip));
        assert_eq!(Header::parse(b"n"), Err(Corrupt::NotGzip));
        assert_eq!(
            Header::parse(b"\x1f\x8b\x09\0\0\0\0\0\0\x03"),
            Err(Corrupt::UnsupportedMethod)
        );
    }

    #[test]
    fn an_extra_field_is_skipped() {
        let mut bytes = vec![0x1f, 0x8b, 8, FEXTRA, 0, 0, 0, 0, 0, 3];
        bytes.extend_from_slice(&[3, 0, b'a', b'b', b'c']);
        let (_, used) = Header::parse(&bytes).unwrap().expect("complete");
        assert_eq!(used, 15);
        assert_eq!(Header::parse(&bytes[..12]), Ok(None));
    }

    #[test]
    fn the_footer_catches_both_kinds_of_corruption() {
        let good = footer(0xDEADBEEF, 42);
        let good: [u8; 8] = good.try_into().unwrap();
        assert_eq!(check_footer(&good, 0xDEADBEEF, 42), Ok(()));
        assert_eq!(check_footer(&good, 1, 42), Err(Corrupt::CrcMismatch));
        assert_eq!(
            check_footer(&good, 0xDEADBEEF, 1),
            Err(Corrupt::LengthMismatch)
        );
    }

    /// gzip records only the extremes, so the round-trip is lossy in the
    /// middle -- which is why `GzipReader#level` answers -1 for level 6.
    #[test]
    fn xfl_records_only_the_extreme_levels() {
        assert_eq!(level_from_xfl(xfl_for_level(9)), 9);
        assert_eq!(level_from_xfl(xfl_for_level(1)), 1);
        for level in [0, 2, 3, 4, 5, 6, 7, 8] {
            assert_eq!(level_from_xfl(xfl_for_level(level)), -1);
        }
    }
}
