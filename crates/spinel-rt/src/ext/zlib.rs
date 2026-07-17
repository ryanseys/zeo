//! `zlib` (CRuby's bundled `zlib` gem). `require "zlib"` activates the `Zlib`
//! module. The checksum functions (`crc32`, `adler32`) are implemented in Rust
//! and oracle-verified against ruby 4.0.5; the compression surface
//! (`deflate`/`inflate`, `Gzip*`) is `todo!()` (see docs/EXTENSIONS.md), pending
//! a `flate2`-backed implementation.

use crate::builtins::{arity, builtin_methods};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};

fn bytes_arg(v: Option<&RubyValue>) -> Result<Vec<u8>, Signal> {
    match v {
        None | Some(RubyValue::Nil) => Ok(Vec::new()),
        Some(RubyValue::Str(s)) => Ok(s.lock().bytes().to_vec()),
        Some(other) => Err(raise_error(
            "TypeError",
            format!("no implicit conversion of {} into String", crate::builtins::class_name_of(other)),
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

    // Not yet implemented (see docs/EXTENSIONS.md) -- pending a flate2 backend.
    "deflate" => fn deflate(_recv, _args, _block) { todo!("Zlib.deflate -- pending flate2") }
    "inflate" => fn inflate(_recv, _args, _block) { todo!("Zlib.inflate -- pending flate2") }
    "gzip" => fn gzip(_recv, _args, _block) { todo!("Zlib.gzip") }
    "gunzip" => fn gunzip(_recv, _args, _block) { todo!("Zlib.gunzip") }
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
}
