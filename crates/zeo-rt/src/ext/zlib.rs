//! `zlib` (CRuby's bundled `zlib` gem). `require "zlib"` activates the `Zlib`
//! module. The checksum functions (`crc32`, `adler32`) are implemented in Rust
//! and oracle-verified against ruby 4.0.5; the compression surface
//! (`deflate`/`inflate`/`gzip`/`gunzip`) is `flate2`-backed (its default
//! miniz_oxide output is byte-compatible with CRuby's zlib for `deflate`).
//! `Zlib.gzip` uses a fixed mtime of 0 (a documented divergence from CRuby's
//! current-time default) so its output is deterministic.

use crate::builtins::{arity, builtin_methods};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};

fn bytes_arg(v: Option<&RubyValue>) -> Result<Vec<u8>, Signal> {
    match v {
        None | Some(RubyValue::Nil) => Ok(Vec::new()),
        Some(RubyValue::Str(s)) => Ok(s.lock().bytes().to_vec()),
        Some(other) => Err(raise_error(
            "TypeError",
            format!("no implicit conversion of {} into String", crate::builtins::convert_name_of(other)),
        )),
    }
}

fn u32_arg(v: Option<&RubyValue>, default: u32) -> u32 {
    match v {
        Some(RubyValue::Int(i)) => *i as u32,
        _ => default,
    }
}

/// CRC-32 (IEEE, zlib's polynomial), continuing from `crc` (Ruby's optional
/// second argument), matching `Zlib.crc32`.
fn crc32(data: &[u8], crc: u32) -> u32 {
    let mut c = crc ^ 0xFFFF_FFFF;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    c ^ 0xFFFF_FFFF
}

/// Adler-32, continuing from `adler` (Ruby's optional second argument).
fn adler32(data: &[u8], adler: u32) -> u32 {
    const MOD: u32 = 65521;
    let mut a = adler & 0xFFFF;
    let mut b = (adler >> 16) & 0xFFFF;
    for &byte in data {
        a = (a + u32::from(byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "crc32" => fn crc32_m(_recv, args, _block) {
        arity!(args, 0..=2);
        Ok(RubyValue::Int(i64::from(crc32(&bytes_arg(args.first())?, u32_arg(args.get(1), 0)))))
    }
    "adler32" => fn adler32_m(_recv, args, _block) {
        arity!(args, 0..=2);
        Ok(RubyValue::Int(i64::from(adler32(&bytes_arg(args.first())?, u32_arg(args.get(1), 1)))))
    }

    // `Zlib.deflate(str, level = DEFAULT_COMPRESSION)` -- zlib-format
    // compressed bytes (ASCII-8BIT).
    "deflate" => fn deflate(_recv, args, _block) {
        arity!(args, 1..=2);
        let data = bytes_arg(args.first())?;
        let level = compression_level(args.get(1));
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), level);
        use std::io::Write;
        enc.write_all(&data).and_then(|_| enc.finish())
            .map(bin_str)
            .map_err(io_err)
    }
    // `Zlib.inflate(str)` -- decompress a zlib stream.
    "inflate" => fn inflate(_recv, args, _block) {
        arity!(args, 1);
        let data = bytes_arg(args.first())?;
        let mut dec = flate2::read::ZlibDecoder::new(&data[..]);
        read_all(&mut dec)
    }
    // `Zlib.gzip(str, level: ...)` -- a gzip stream. The header carries a
    // machine/time-independent mtime of 0 (documented divergence from CRuby,
    // whose default mtime is the current time), so output is deterministic.
    "gzip" => fn gzip(_recv, args, _block) {
        arity!(args, 1..=2);
        let data = bytes_arg(args.first())?;
        let level = compression_level(args.get(1));
        let mut enc = flate2::GzBuilder::new()
            .mtime(0)
            .write(Vec::new(), level);
        use std::io::Write;
        enc.write_all(&data).and_then(|_| enc.finish())
            .map(bin_str)
            .map_err(io_err)
    }
    // `Zlib.gunzip(str)` -- decompress a gzip stream.
    "gunzip" => fn gunzip(_recv, args, _block) {
        arity!(args, 1);
        let data = bytes_arg(args.first())?;
        let mut dec = flate2::read::GzDecoder::new(&data[..]);
        read_all(&mut dec)
    }
}

/// A flate2 compression level from Ruby's optional integer argument (0..9, or
/// -1 for the library default), clamped to the valid range.
fn compression_level(v: Option<&RubyValue>) -> flate2::Compression {
    match v {
        Some(RubyValue::Int(n)) if (0..=9).contains(n) => flate2::Compression::new(*n as u32),
        // -1 (DEFAULT_COMPRESSION) or anything else -> the library default.
        _ => flate2::Compression::default(),
    }
}

/// Wrap raw bytes as an ASCII-8BIT String (compressed output is binary).
fn bin_str(bytes: Vec<u8>) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT))
}

/// Read a decoder to end, mapping a corrupt stream to CRuby's `Zlib::Error`
/// family (a plain-message error here; the nested classes aren't registered).
fn read_all(r: &mut impl std::io::Read) -> Result<RubyValue, Signal> {
    let mut out = Vec::new();
    std::io::Read::read_to_end(r, &mut out)
        .map(|_| bin_str(out))
        .map_err(|_| raise_error("RuntimeError", "invalid compressed data".to_string()))
}

fn io_err(_e: std::io::Error) -> Signal {
    raise_error("RuntimeError", "zlib stream error".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(text.to_string()))
    }
    fn int(v: Result<RubyValue, Signal>) -> i64 {
        match v.unwrap() {
            RubyValue::Int(i) => i,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    #[test]
    fn checksums_match_ruby() {
        assert_eq!(int(crc32_m(&RubyValue::Nil, &[s("abc")], None)), 891568578);
        assert_eq!(int(crc32_m(&RubyValue::Nil, &[], None)), 0);
        assert_eq!(int(crc32_m(&RubyValue::Nil, &[s("abc"), RubyValue::Int(100)], None)), 2063213118);
        assert_eq!(int(adler32_m(&RubyValue::Nil, &[s("abc")], None)), 38600999);
        assert_eq!(int(adler32_m(&RubyValue::Nil, &[], None)), 1);
    }

    fn bytes(v: Result<RubyValue, Signal>) -> Vec<u8> {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().bytes().to_vec(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn deflate_matches_ruby_zlib_bytes() {
        // Ruby 4.0.5: `Zlib.deflate("hello world").bytes`.
        assert_eq!(
            bytes(deflate(&RubyValue::Nil, &[s("hello world")], None)),
            vec![120, 156, 203, 72, 205, 201, 201, 87, 40, 207, 47, 202, 73, 1, 0, 26, 11, 4, 93],
        );
    }

    #[test]
    fn deflate_inflate_and_gzip_gunzip_round_trip() {
        let text = "compress me ".repeat(20);
        let msg = s(&text);
        let comp = deflate(&RubyValue::Nil, &[msg.clone()], None).unwrap();
        assert_eq!(bytes(inflate(&RubyValue::Nil, &[comp], None)), text.as_bytes());
        let gz = gzip(&RubyValue::Nil, &[msg], None).unwrap();
        assert_eq!(bytes(gunzip(&RubyValue::Nil, &[gz], None)), text.as_bytes());
    }
}
