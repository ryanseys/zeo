//! The unit suite, moved verbatim with the extraction (`use crate::*` --
//! every asserted item is part of the public surface).

use crate::*;

#[test]
fn concat_enc_keeps_equal_encodings() {
    assert_eq!(
        compat_concat_enc(ASCII_8BIT, false, ASCII_8BIT, false),
        Some(ASCII_8BIT)
    );
    assert_eq!(compat_concat_enc(UTF_8, true, UTF_8, false), Some(UTF_8));
}

#[test]
fn concat_enc_lets_a_seven_bit_side_adopt_the_other_encoding() {
    // BINARY high bytes + ASCII-only UTF-8 stays BINARY, either side.
    assert_eq!(
        compat_concat_enc(ASCII_8BIT, false, UTF_8, true),
        Some(ASCII_8BIT)
    );
    assert_eq!(
        compat_concat_enc(UTF_8, true, ASCII_8BIT, false),
        Some(ASCII_8BIT)
    );
    // Both 7-bit: the LEFT side's encoding wins (oracle-verified:
    // `usascii + "y"` is US-ASCII, `"y" + usascii` is UTF-8).
    assert_eq!(
        compat_concat_enc(US_ASCII, true, UTF_8, true),
        Some(US_ASCII)
    );
    assert_eq!(compat_concat_enc(UTF_8, true, US_ASCII, true), Some(UTF_8));
}

#[test]
fn concat_enc_rejects_two_differently_encoded_high_bit_strings() {
    assert_eq!(compat_concat_enc(ASCII_8BIT, false, UTF_8, false), None);
    assert_eq!(compat_concat_enc(UTF_8, false, ISO_8859_1, false), None);
}

#[test]
fn push_buf_appends_raw_bytes_and_adopts_the_negotiated_encoding() {
    // THE regression seam: BINARY 0xB5 + UTF-8 "\n" must stay the two
    // raw bytes [0xB5, 0x0A] tagged BINARY -- the old display-text path
    // promoted it to [0xC2, 0xB5, 0x0A] UTF-8.
    let mut b = StrBuf::from_bytes(vec![0xb5], ASCII_8BIT);
    b.push_buf(&StrBuf::from_utf8("\n".to_string())).unwrap();
    assert_eq!(b.bytes(), [0xb5, 0x0a]);
    assert_eq!(b.encoding(), ASCII_8BIT);
    // An ASCII-only receiver adopts a high-bit UTF-8 argument's tag.
    let mut a = StrBuf::from_utf8("x".to_string());
    a.push_buf(&StrBuf::from_utf8("é".to_string())).unwrap();
    assert_eq!(a.encoding(), UTF_8);
    assert_eq!(a.bytes(), "xé".as_bytes());
}

#[test]
fn push_buf_refuses_the_incompatible_pair_without_mutating() {
    let mut b = StrBuf::from_bytes(vec![0xb5], ASCII_8BIT);
    assert!(b.push_buf(&StrBuf::from_utf8("é".to_string())).is_err());
    // The receiver is untouched on the error path.
    assert_eq!(b.bytes(), [0xb5]);
    assert_eq!(b.encoding(), ASCII_8BIT);
}

#[test]
fn find_is_case_and_separator_insensitive() {
    assert_eq!(find("UTF-8"), Some(UTF_8));
    assert_eq!(find("utf_8"), Some(UTF_8));
    assert_eq!(find("BINARY"), Some(ASCII_8BIT));
    assert_eq!(find("ascii-8bit"), Some(ASCII_8BIT));
    assert_eq!(find("Latin-1"), Some(ISO_8859_1));
    assert_eq!(find("nope"), None);
}

#[test]
fn names_lists_canonical_then_real_aliases() {
    assert_eq!(ASCII_8BIT.names(), vec!["ASCII-8BIT", "BINARY"]);
    // UTF-8's selector aliases (external/locale/...) are filtered out.
    assert_eq!(UTF_8.names(), vec!["UTF-8", "CP65001"]);
}

#[test]
fn coderange_classifies_per_encoding() {
    assert_eq!(
        StrBuf::from_utf8("abc".into()).coderange(),
        CodeRange::SevenBit
    );
    assert_eq!(
        StrBuf::from_utf8("caf\u{e9}".into()).coderange(),
        CodeRange::Valid
    );
    // A lone 0xE9 is broken UTF-8 but a valid Latin-1 character.
    assert_eq!(
        StrBuf::from_bytes(vec![0xE9], UTF_8).coderange(),
        CodeRange::Broken
    );
    assert_eq!(
        StrBuf::from_bytes(vec![0xE9], ISO_8859_1).coderange(),
        CodeRange::Valid
    );
    // A high byte is never valid US-ASCII.
    assert_eq!(
        StrBuf::from_bytes(vec![0xE9], US_ASCII).coderange(),
        CodeRange::Broken
    );
}

#[test]
fn ascii_only_and_valid_encoding() {
    let s = StrBuf::from_utf8("abc".into());
    assert!(s.ascii_only() && s.valid_encoding());
    let s = StrBuf::from_utf8("caf\u{e9}".into());
    assert!(!s.ascii_only() && s.valid_encoding());
    let s = StrBuf::from_bytes(vec![0xC2], UTF_8); // truncated 2-byte seq
    assert!(!s.valid_encoding());
}

#[test]
fn latin1_high_byte_renders_as_its_codepoint() {
    // 0xE9 in Latin-1 is 'é' (U+00E9).
    let s = StrBuf::from_bytes(vec![0x63, 0x61, 0x66, 0xE9], ISO_8859_1);
    assert_eq!(s.to_utf8_lossy(), "caf\u{e9}");
    assert_eq!(s.char_len(), 4);
}

#[test]
fn force_encoding_keeps_bytes_reinterprets() {
    let mut s = StrBuf::from_bytes(vec![0xE9], ISO_8859_1);
    assert_eq!(s.to_utf8_lossy(), "\u{e9}");
    s.set_encoding(UTF_8);
    // Same byte, now broken UTF-8.
    assert!(!s.valid_encoding());
}

#[test]
fn char_indexing_is_encoding_aware() {
    // UTF-8: a multibyte char is one index.
    let utf = StrBuf::from_utf8("caf\u{e9}".into());
    assert_eq!(utf.char_len(), 4);
    assert_eq!(utf.char_at(3).unwrap().bytes(), "\u{e9}".as_bytes());
    // BINARY: one character per byte, kept BINARY.
    let bin = StrBuf::from_bytes("caf\u{e9}".to_string().into_bytes(), ASCII_8BIT);
    assert_eq!(bin.char_len(), 5);
    assert_eq!(bin.char_at(3).unwrap().bytes(), &[0xC3]);
    assert_eq!(bin.char_at(3).unwrap().encoding(), ASCII_8BIT);
    assert_eq!(bin.reversed().bytes(), &[0xA9, 0xC3, 0x66, 0x61, 0x63]);
    assert_eq!(bin.char_substr(1, 2).unwrap().bytes(), &[0x61, 0x66]);
}

#[test]
fn casing_is_encoding_aware() {
    assert_eq!(
        StrBuf::from_utf8("caf\u{e9}".into()).upcased().bytes(),
        "CAF\u{c9}".as_bytes()
    );
    // BINARY: only ASCII bytes fold; high bytes are untouched.
    let bin = StrBuf::from_bytes(vec![0x63, 0x61, 0x66, 0xC3, 0x89], ASCII_8BIT);
    assert_eq!(bin.upcased().bytes(), &[0x43, 0x41, 0x46, 0xC3, 0x89]);
    // Latin-1: é (0xE9) uppercases to É (0xC9).
    let latin = StrBuf::from_bytes(vec![0x63, 0x61, 0x66, 0xE9], ISO_8859_1);
    assert_eq!(latin.upcased().bytes(), &[0x43, 0x41, 0x46, 0xC9]);
    assert_eq!(latin.upcased().encoding(), ISO_8859_1);
}

#[test]
fn transcode_latin1_to_utf8_and_back() {
    let opts = TranscodeOptions::default();
    // Latin-1 "café" -> UTF-8 bytes for é (0xC3 0xA9).
    let utf8 = transcode(&[0x63, 0x61, 0x66, 0xE9], ISO_8859_1, UTF_8, &opts, None).unwrap();
    assert_eq!(utf8, "caf\u{e9}".as_bytes());
    // Round trip back.
    let latin = transcode(&utf8, UTF_8, ISO_8859_1, &opts, None).unwrap();
    assert_eq!(latin, vec![0x63, 0x61, 0x66, 0xE9]);
}

#[test]
fn transcode_undefined_raises_or_replaces() {
    // é has no US-ASCII byte.
    let strict = TranscodeOptions::default();
    assert!(matches!(
        transcode("caf\u{e9}".as_bytes(), UTF_8, US_ASCII, &strict, None),
        Err(TranscodeError::UndefinedConversion(_))
    ));
    let replace = TranscodeOptions {
        undef_replace: true,
        ..Default::default()
    };
    let out = transcode("caf\u{e9}".as_bytes(), UTF_8, US_ASCII, &replace, None).unwrap();
    assert_eq!(out, b"caf?");
}

#[test]
fn transcode_xml_text_escapes() {
    let opts = TranscodeOptions {
        xml: Some(XmlMode::Text),
        ..Default::default()
    };
    let out = transcode(b"a<b>&c", UTF_8, US_ASCII, &opts, None).unwrap();
    assert_eq!(out, b"a&lt;b&gt;&amp;c");
}

#[test]
fn inspect_escapes_invalid_and_high_bytes() {
    // Valid UTF-8: byte-identical to the old Debug quoting.
    assert_eq!(inspect(&StrBuf::from_utf8("a\nb".into())), r#""a\nb""#);
    assert_eq!(
        inspect(&StrBuf::from_utf8("caf\u{e9}".into())),
        "\"caf\u{e9}\""
    );
    // A broken UTF-8 byte renders as \xNN.
    assert_eq!(
        inspect(&StrBuf::from_bytes(vec![0x61, 0xE9, 0x62], UTF_8)),
        r#""a\xE9b""#
    );
    // A Latin-1 high byte renders as \xNN, not as its codepoint char.
    assert_eq!(
        inspect(&StrBuf::from_bytes(
            vec![0x63, 0x61, 0x66, 0xE9],
            ISO_8859_1
        )),
        r#""caf\xE9""#
    );
}

#[test]
fn cross_encoding_equality_and_hash_tag() {
    // ASCII-only strings are equal (and one hash key) across encodings.
    let ascii_utf8 = StrBuf::from_utf8("abc".into());
    let ascii_latin = StrBuf::from_bytes(b"abc".to_vec(), ISO_8859_1);
    assert_eq!(ascii_utf8, ascii_latin);
    assert_eq!(ascii_utf8.hash_key_tag(), ascii_latin.hash_key_tag());
    // High-byte strings with the same bytes but different encodings are
    // distinct.
    let hi_utf8 = StrBuf::from_utf8("caf\u{e9}".into());
    let hi_latin = StrBuf::from_bytes("caf\u{e9}".to_string().into_bytes(), ISO_8859_1);
    assert_ne!(hi_utf8, hi_latin);
    assert_ne!(hi_utf8.hash_key_tag(), hi_latin.hash_key_tag());
}

#[test]
fn transcode_fallback_consulted_first() {
    let opts = TranscodeOptions::default();
    let mut fb = |s: &str| (s == "\u{e9}").then(|| "e".to_string());
    let out = transcode(
        "caf\u{e9}".as_bytes(),
        UTF_8,
        US_ASCII,
        &opts,
        Some(&mut fb),
    )
    .unwrap();
    assert_eq!(out, b"cafe");
}
