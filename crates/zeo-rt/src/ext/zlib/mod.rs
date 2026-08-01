//! `zlib` (CRuby's bundled `zlib` gem). `require "zlib"` activates the `Zlib`
//! module and its six stream classes, mirroring CRuby's:
//!
//! ```text
//! Zlib::ZStream        the shared counters/lifecycle      (zstream.rs)
//!  ├ Zlib::Deflate     a compressor                       (deflate.rs)
//!  └ Zlib::Inflate     a decompressor                     (inflate.rs)
//! Zlib::GzipFile       the shared gzip header/footer     (gzip_file.rs)
//!  ├ Zlib::GzipWriter  writes a gzip member to an IO   (gzip_writer.rs)
//!  └ Zlib::GzipReader  reads a gzip member from an IO  (gzip_reader.rs)
//! ```
//!
//! The checksum functions (`crc32`, `adler32`) are implemented here in Rust;
//! everything else is `flate2`-backed, driving its LOW-level `Compress`/
//! `Decompress` rather than the `ZlibEncoder` wrappers, because CRuby's surface
//! exposes the incremental stream (`Deflate#deflate(s, SYNC_FLUSH)` must hand
//! back exactly the bytes flushed so far) and a whole-buffer wrapper cannot.
//! The compression engine itself is [`codec`]; the six classes above are rows
//! over it, one file each because the DSL allows one `ruby_class!` per module.
//! The gzip framing -- header, CRC-32/ISIZE footer -- is written and parsed by
//! [`frame`] rather than by flate2, both because `Compress::new_gzip` is absent
//! from the pure-Rust backend and because `GzipFile`'s accessors need the
//! header FIELDS (`mtime`/`orig_name`/`comment`/`os_code`), not just the bytes.
//!
//! `Zlib`'s thirteen exception classes live in `gems/zlib/lib/zlib.rb`, the
//! gem's Ruby half -- see `ext/mod.rs` for why an extension cannot define its
//! own. The native half raises them by name.

pub(crate) mod codec;
pub(crate) mod deflate;
pub(crate) mod frame;
pub(crate) mod gzip;
pub(crate) mod gzip_file;
pub(crate) mod gzip_reader;
pub(crate) mod gzip_writer;
pub(crate) mod inflate;
pub(crate) mod zstream;

use crate::builtins::convert;
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

pub(super) fn bytes_arg(v: Option<&RubyValue>) -> Result<Vec<u8>, Signal> {
    match v {
        None | Some(RubyValue::Nil) => Ok(Vec::new()),
        Some(other) => Ok(convert::to_rstr(other)?.lock().bytes().to_vec()),
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
pub(crate) fn crc32(data: &[u8], crc: u32) -> u32 {
    let mut c = crc ^ 0xFFFF_FFFF;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    c ^ 0xFFFF_FFFF
}

/// Adler-32, continuing from `adler` (Ruby's optional second argument).
pub(crate) fn adler32(data: &[u8], adler: u32) -> u32 {
    const MOD: u32 = 65521;
    let mut a = adler & 0xFFFF;
    let mut b = (adler >> 16) & 0xFFFF;
    for &byte in data {
        a = (a + u32::from(byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

ruby_module! {
    Zlib = zeo_abi::ZLIB_MODULE;

    // Compression levels, as `deflateInit2`'s `level`.
    const NO_COMPRESSION = RubyValue::Int(0);
    const BEST_SPEED = RubyValue::Int(1);
    const BEST_COMPRESSION = RubyValue::Int(9);
    const DEFAULT_COMPRESSION = RubyValue::Int(-1);

    // Flush values, as `deflate`'s/`inflate`'s `flush`.
    const NO_FLUSH = RubyValue::Int(0);
    const SYNC_FLUSH = RubyValue::Int(2);
    const FULL_FLUSH = RubyValue::Int(3);
    const FINISH = RubyValue::Int(4);

    // Compression strategies. Accepted and reported back by `params`, but not
    // acted on: the pure-Rust deflate backend has no strategy knob (see
    // docs/COMPATIBILITY.md).
    const DEFAULT_STRATEGY = RubyValue::Int(0);
    const FILTERED = RubyValue::Int(1);
    const HUFFMAN_ONLY = RubyValue::Int(2);
    const RLE = RubyValue::Int(3);
    const FIXED = RubyValue::Int(4);

    // Window and memory sizing.
    const MAX_WBITS = RubyValue::Int(15);
    const DEF_MEM_LEVEL = RubyValue::Int(8);
    const MAX_MEM_LEVEL = RubyValue::Int(9);

    // `ZStream#data_type`'s answers.
    const BINARY = RubyValue::Int(0);
    const ASCII = RubyValue::Int(1);
    const TEXT = RubyValue::Int(1);
    const UNKNOWN = RubyValue::Int(2);

    // The gzip header's OS byte. `OS_CODE` is the value this build STAMPS, and
    // zeo targets Unix only -- CRuby picks it per platform the same way.
    const OS_MSDOS = RubyValue::Int(0);
    const OS_AMIGA = RubyValue::Int(1);
    const OS_VMS = RubyValue::Int(2);
    const OS_UNIX = RubyValue::Int(3);
    const OS_VMCMS = RubyValue::Int(4);
    const OS_ATARI = RubyValue::Int(5);
    const OS_OS2 = RubyValue::Int(6);
    const OS_MACOS = RubyValue::Int(7);
    const OS_ZSYSTEM = RubyValue::Int(8);
    const OS_CPM = RubyValue::Int(9);
    const OS_TOPS20 = RubyValue::Int(10);
    const OS_WIN32 = RubyValue::Int(11);
    const OS_QDOS = RubyValue::Int(12);
    const OS_RISCOS = RubyValue::Int(13);
    const OS_UNKNOWN = RubyValue::Int(255);
    const OS_CODE = RubyValue::Int(3);

    // `VERSION` is the bundled gem's, as CRuby's is. `ZLIB_VERSION` names the
    // zlib API level this implements, not a linked libz -- there isn't one
    // (see docs/COMPATIBILITY.md).
    const VERSION = RubyValue::Str(crate::string_new("3.2.3".to_string()));
    const ZLIB_VERSION = RubyValue::Str(crate::string_new("1.3.1".to_string()));

    // `crc32`/`adler32` are CRuby module_functions (usable via `include Zlib`);
    // the compression calls are plain module methods. Arities match ruby 4.0.6.
    module_function def "crc32" (_recv, arg1?, arg2?) {
        Ok(RubyValue::Int(i64::from(crc32(&bytes_arg(arg1)?, u32_arg(arg2, 0)))))
    }
    module_function def "adler32" (_recv, arg1?, arg2?) {
        Ok(RubyValue::Int(i64::from(adler32(&bytes_arg(arg1)?, u32_arg(arg2, 1)))))
    }

    // `Zlib.deflate(str, level = DEFAULT_COMPRESSION)` -- zlib-format
    // compressed bytes (ASCII-8BIT).
    def self."deflate" cfunc (_recv, string, level?) {
        codec::one_shot_deflate(&bytes_arg(Some(string))?, level_of(level), codec::Wrap::Zlib)
    }
    // `Zlib.inflate(str)` -- decompress a zlib stream.
    def self."inflate" (_recv, string) {
        codec::one_shot_inflate(&bytes_arg(Some(string))?, codec::Wrap::Zlib)
    }
    // `Zlib.gzip(str, level: nil, strategy: nil)` -- a whole gzip member.
    def self."gzip" cfunc (_recv, string, **opts) {
        let level = level_of(kw(&opts.cloned(), "level").as_ref());
        gzip::gzip_string(&bytes_arg(Some(string))?, level)
    }
    // `Zlib.gunzip(str)` -- decompress a gzip member, footer checked.
    def self."gunzip" (_recv, string) {
        gzip::gunzip_string(&bytes_arg(Some(string))?)
    }
}

/// A compression level from Ruby's optional argument: 0..9, or `nil`/-1
/// (`DEFAULT_COMPRESSION`) for the library default.
pub(super) fn level_of(v: Option<&RubyValue>) -> flate2::Compression {
    match v {
        Some(RubyValue::Int(n)) if (0..=9).contains(n) => flate2::Compression::new(*n as u32),
        Some(RubyValue::Int(n)) if *n > 9 => flate2::Compression::best(),
        _ => flate2::Compression::default(),
    }
}

/// One keyword out of a call's trailing Hash. An explicit `nil` reads as
/// absent, which is what makes `Zlib.gzip(s, level: nil)` mean "the default".
fn kw(kwargs: &Option<RubyValue>, name: &str) -> Option<RubyValue> {
    let RubyValue::Hash(h) = kwargs.as_ref()? else {
        return None;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    crate::hash_pairs(h)
        .into_iter()
        .find(|(k, _)| k.rb_eq(&key))
        .map(|(_, v)| v)
        .filter(|v| !matches!(v, RubyValue::Nil))
}

/// Wrap raw bytes as an ASCII-8BIT String -- compressed output is binary, and
/// so is everything `Inflate` produces.
pub(crate) fn bin_str(bytes: Vec<u8>) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT))
}

/// Wrap raw bytes as a TEXT String, tagged with the external encoding.
/// `GzipReader` is the one part of this extension that hands back text rather
/// than bytes -- `#read` and `#gets` are UTF-8 where `Inflate#inflate` is
/// binary, which is CRuby's split, not an accident of this implementation.
pub(crate) fn text_str(bytes: Vec<u8>) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::UTF_8))
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
    /// `Zlib`'s `ruby_module!`-generated functions have mangled Rust idents, so
    /// the tests call them through the registered module-function `lookup`.
    fn f(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::ZLIB_MODULE)
            .expect("Zlib is a registered builtin table")
            .class
            .as_ref()
            .expect("Zlib has module functions");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Zlib.{name} is defined"))
    }

    #[test]
    fn checksums_match_ruby() {
        assert_eq!(
            int(f("crc32")(&RubyValue::Nil, &[s("abc")], None)),
            891568578
        );
        assert_eq!(int(f("crc32")(&RubyValue::Nil, &[], None)), 0);
        assert_eq!(
            int(f("crc32")(
                &RubyValue::Nil,
                &[s("abc"), RubyValue::Int(100)],
                None
            )),
            2063213118
        );
        assert_eq!(
            int(f("adler32")(&RubyValue::Nil, &[s("abc")], None)),
            38600999
        );
        assert_eq!(int(f("adler32")(&RubyValue::Nil, &[], None)), 1);
    }

    fn bytes(v: Result<RubyValue, Signal>) -> Vec<u8> {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().bytes().to_vec(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn deflate_matches_ruby_zlib_bytes() {
        // Ruby 4.0.6: `Zlib.deflate("hello world").bytes`.
        assert_eq!(
            bytes(f("deflate")(&RubyValue::Nil, &[s("hello world")], None)),
            vec![
                120, 156, 203, 72, 205, 201, 201, 87, 40, 207, 47, 202, 73, 1, 0, 26, 11, 4, 93
            ],
        );
    }

    #[test]
    fn deflate_inflate_and_gzip_gunzip_round_trip() {
        let text = "compress me ".repeat(20);
        let msg = s(&text);
        let comp = f("deflate")(&RubyValue::Nil, std::slice::from_ref(&msg), None).unwrap();
        assert_eq!(
            bytes(f("inflate")(&RubyValue::Nil, &[comp], None)),
            text.as_bytes()
        );
        let gz = f("gzip")(&RubyValue::Nil, &[msg], None).unwrap();
        assert_eq!(
            bytes(f("gunzip")(&RubyValue::Nil, &[gz], None)),
            text.as_bytes()
        );
    }

    /// Every level round-trips, and `level:` -- a KEYWORD for `gzip` where it
    /// is POSITIONAL for `deflate`, CRuby's own split -- actually reaches the
    /// compressor.
    #[test]
    fn compression_levels_round_trip() {
        let text = "compress me ".repeat(50);
        for level in 0..=9 {
            let comp = f("deflate")(&RubyValue::Nil, &[s(&text), RubyValue::Int(level)], None);
            assert_eq!(
                bytes(f("inflate")(&RubyValue::Nil, &[comp.unwrap()], None)),
                text.as_bytes(),
                "level {level} should round-trip"
            );
        }
        let kwargs = RubyValue::Hash(crate::hash_new(vec![(
            RubyValue::Symbol(crate::Symbol::intern("level")),
            RubyValue::Int(9),
        )]));
        let best = bytes(f("gzip")(&RubyValue::Nil, &[s(&text), kwargs], None));
        let stored = RubyValue::Hash(crate::hash_new(vec![(
            RubyValue::Symbol(crate::Symbol::intern("level")),
            RubyValue::Int(0),
        )]));
        let stored = bytes(f("gzip")(&RubyValue::Nil, &[s(&text), stored], None));
        assert!(best.len() < stored.len(), "level: 9 should beat level: 0");
    }
}
