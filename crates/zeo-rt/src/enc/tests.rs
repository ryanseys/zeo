//! The unit suite, moved verbatim with the extraction (`use crate::enc::*` --
//! every asserted item is part of the public surface).

use crate::enc::*;

/// A quick content-driven `StrBuf` for the concat-rule tests: `bytes` under
/// `enc` (ascii-only-ness and emptiness come from the content, exactly as
/// the rule reads them).
fn buf(bytes: &[u8], enc: EncodingId) -> StrBuf {
    StrBuf::from_bytes(bytes.to_vec(), enc)
}

#[test]
fn concat_enc_keeps_equal_encodings() {
    assert_eq!(
        compat_concat_enc(&buf(b"\xFF", ASCII_8BIT), &buf(b"\xFE", ASCII_8BIT)),
        Some(ASCII_8BIT)
    );
    assert_eq!(
        compat_concat_enc(&buf(b"ab", UTF_8), &buf("é".as_bytes(), UTF_8)),
        Some(UTF_8)
    );
}

#[test]
fn concat_enc_lets_a_seven_bit_side_adopt_the_other_encoding() {
    // BINARY high bytes + ASCII-only UTF-8 stays BINARY, either side.
    assert_eq!(
        compat_concat_enc(&buf(b"\xFF", ASCII_8BIT), &buf(b"y", UTF_8)),
        Some(ASCII_8BIT)
    );
    assert_eq!(
        compat_concat_enc(&buf(b"y", UTF_8), &buf(b"\xFF", ASCII_8BIT)),
        Some(ASCII_8BIT)
    );
    // Both 7-bit: the LEFT side's encoding wins (oracle-verified:
    // `usascii + "y"` is US-ASCII, `"y" + usascii` is UTF-8).
    assert_eq!(
        compat_concat_enc(&buf(b"x", US_ASCII), &buf(b"y", UTF_8)),
        Some(US_ASCII)
    );
    assert_eq!(
        compat_concat_enc(&buf(b"y", UTF_8), &buf(b"x", US_ASCII)),
        Some(UTF_8)
    );
}

#[test]
fn concat_enc_rejects_two_differently_encoded_high_bit_strings() {
    assert_eq!(
        compat_concat_enc(&buf(b"\xFF", ASCII_8BIT), &buf("é".as_bytes(), UTF_8)),
        None
    );
    assert_eq!(
        compat_concat_enc(&buf("é".as_bytes(), UTF_8), &buf(b"\xE9", ISO_8859_1)),
        None
    );
}

#[test]
fn concat_enc_wide_encodings_mix_only_through_emptiness() {
    // Oracle-verified: `utf16 + ""` stays UTF-16LE, `empty_utf16 + "abc"`
    // is UTF-8, and a NON-empty wide side refuses even pure-ASCII content.
    let u16ab = buf(&[0x61, 0x00], UTF_16LE);
    let empty16 = buf(b"", UTF_16LE);
    assert_eq!(compat_concat_enc(&u16ab, &buf(b"", UTF_8)), Some(UTF_16LE));
    assert_eq!(
        compat_concat_enc(&empty16, &buf(b"abc", UTF_8)),
        Some(UTF_8)
    );
    assert_eq!(
        compat_concat_enc(&buf(b"abc", UTF_8), &empty16),
        Some(UTF_8)
    );
    assert_eq!(compat_concat_enc(&u16ab, &buf(b"b", UTF_8)), None);
    // A wide string's content is never `ascii_only`, empty included.
    assert!(!u16ab.ascii_only());
    assert!(!empty16.ascii_only());
}

#[test]
fn push_buf_appends_raw_bytes_and_adopts_the_negotiated_encoding() {
    // THE regression seam: BINARY 0xB5 + UTF-8 "\n" must stay the two
    // raw bytes [0xB5, 0x0A] tagged BINARY -- not promoted to
    // [0xC2, 0xB5, 0x0A] UTF-8.
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
        Err(TranscodeError::UndefinedConversion(..))
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
    // Valid UTF-8 uses standard debug-style escaping.
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

// --- Single-byte table rows (Windows-125x / ISO-8859-2/-15 / KOI8-R).
// Every expectation oracle-verified against ruby 4.0.6.

#[test]
fn single_byte_tables_map_and_round_trip() {
    // Windows-1250 0xBF is ż (U+017C); ISO-8859-15 0xA4 is € (U+20AC);
    // KOI8-R 0xC1 is а (U+0430).
    let t = WINDOWS_1250.single_byte_table();
    assert_eq!(t.decode(0xBF), Some('\u{017C}'));
    assert_eq!(t.encode('\u{017C}'), Some(0xBF));
    assert_eq!(ISO_8859_15.single_byte_table().decode(0xA4), Some('€'));
    assert_eq!(KOI8_R.single_byte_table().decode(0xC1), Some('а'));
    // ASCII half is identity everywhere.
    assert_eq!(t.decode(b'z'), Some('z'));
    assert_eq!(t.encode('z'), Some(b'z'));
}

#[test]
fn unmapped_vendor_bytes_are_valid_characters_that_refuse_transcode() {
    // 0x81 is unassigned in Windows-1252: still a VALID character...
    let s = StrBuf::from_bytes(vec![0x81], WINDOWS_1252);
    assert!(s.valid_encoding());
    assert_eq!(s.char_len(), 1);
    // ...but transcoding out refuses with CRuby's byte-quoted message,
    // naming the pivot tail when the target isn't UTF-8.
    let err = transcode(&[0x81], WINDOWS_1252, UTF_8, &Default::default(), None).unwrap_err();
    match err {
        TranscodeError::UndefinedConversion(m, _) => {
            assert_eq!(
                m,
                "\"\\x81\" to UTF-8 in conversion from Windows-1252 to UTF-8"
            );
        }
        other => panic!("wrong error: {other:?}"),
    }
    let err = transcode(&[0x81], WINDOWS_1252, KOI8_R, &Default::default(), None).unwrap_err();
    match err {
        TranscodeError::UndefinedConversion(m, _) => {
            assert_eq!(
                m,
                "\"\\x81\" to UTF-8 in conversion from Windows-1252 to UTF-8 to KOI8-R"
            );
        }
        other => panic!("wrong error: {other:?}"),
    }
}

#[test]
fn undefined_conversion_messages_match_crubys_three_shapes() {
    let msg = |bytes: &[u8], from, to| match transcode(bytes, from, to, &Default::default(), None)
        .unwrap_err()
    {
        TranscodeError::UndefinedConversion(m, _) => m,
        other => panic!("wrong error: {other:?}"),
    };
    // Direct from UTF-8, plain target name: the short form.
    assert_eq!(
        msg("\u{3042}".as_bytes(), UTF_8, KOI8_R),
        "U+3042 from UTF-8 to KOI8-R"
    );
    // Direct from UTF-8, windows target: upcased long form.
    assert_eq!(
        msg("\u{044B}".as_bytes(), UTF_8, WINDOWS_1250),
        "U+044B to WINDOWS-1250 in conversion from UTF-8 to WINDOWS-1250"
    );
    // Pivoted pair: long form, FROM side keeps its requested spelling.
    assert_eq!(
        msg(&[0xC1], KOI8_R, ISO_8859_2),
        "U+0430 to ISO-8859-2 in conversion from KOI8-R to UTF-8 to ISO-8859-2"
    );
    assert_eq!(
        msg(&[0xE9], ISO_8859_1, US_ASCII),
        "U+00E9 to US-ASCII in conversion from ISO-8859-1 to UTF-8 to US-ASCII"
    );
}

#[test]
fn windows_and_iso_case_map_through_unicode_koi8_ascii_only() {
    let up = |bytes: &[u8], enc: EncodingId| {
        StrBuf::from_bytes(bytes.to_vec(), enc)
            .upcased()
            .bytes()
            .to_vec()
    };
    // Windows-1252 é -> É; ß EXPANDS to SS; µ's uppercase (Greek Μ)
    // isn't representable, so it stays.
    assert_eq!(up(b"caf\xE9", WINDOWS_1252), b"CAF\xC9");
    assert_eq!(up(b"stra\xDFe", WINDOWS_1252), b"STRASSE");
    assert_eq!(up(b"\xB5", WINDOWS_1252), b"\xB5");
    // ÿ -> Ÿ exists in Windows-1252 (0x9F) but not Latin-1 (kept).
    assert_eq!(up(b"\xFF", WINDOWS_1252), b"\x9F");
    assert_eq!(up(b"\xFF", ISO_8859_1), b"\xFF");
    // Latin-1 ß also expands (the pre-table fold missed this).
    assert_eq!(up(b"\xDF", ISO_8859_1), b"SS");
    // ISO-8859-2 ą -> Ą.
    assert_eq!(up(b"\xB1", ISO_8859_2), b"\xA1");
    // KOI8-R: ASCII folds, Cyrillic deliberately does NOT (CRuby's rule).
    assert_eq!(up(b"\xC1z", KOI8_R), b"\xC1Z");
}

#[test]
fn single_byte_lossy_display_and_inspect() {
    let s = StrBuf::from_bytes(b"caf\xE9\x81".to_vec(), WINDOWS_1252);
    // Display decodes through the table; the unassigned byte is U+FFFD.
    assert_eq!(s.to_utf8_lossy(), "caf\u{E9}\u{FFFD}");
    // Inspect stays byte-faithful: high bytes as \xNN.
    assert_eq!(inspect(&s), "\"caf\\xE9\\x81\"");
}

// --- Multibyte CJK rows (structural walk oracle-verified).

#[test]
fn multibyte_walk_counts_characters_structurally() {
    let len = |bytes: &[u8], enc| StrBuf::from_bytes(bytes.to_vec(), enc).char_len();
    let valid = |bytes: &[u8], enc| StrBuf::from_bytes(bytes.to_vec(), enc).valid_encoding();
    // あいz in Shift_JIS: two 2-byte chars + ASCII.
    assert_eq!(len(&[0x82, 0xA0, 0x82, 0xA2, 0x7A], SHIFT_JIS), 3);
    // A structurally valid but UNMAPPED pair is one valid character.
    assert_eq!(len(&[0x82, 0x7A], SHIFT_JIS), 1);
    assert!(valid(&[0x82, 0x7A], SHIFT_JIS));
    // Truncated lead: one broken 1-byte char.
    assert_eq!(len(&[0x82], SHIFT_JIS), 1);
    assert!(!valid(&[0x82], SHIFT_JIS));
    // Invalid trail: lead and trail each count on their own.
    assert_eq!(len(&[0x82, 0x00], SHIFT_JIS), 2);
    // Halfwidth kana is 1 byte; EUC-JP SS3 is 3.
    assert_eq!(len(&[0xB1, 0xB2], SHIFT_JIS), 2);
    assert_eq!(len(&[0x8F, 0xA1, 0xA1], EUC_JP), 1);
    assert_eq!(len(&[0x8E, 0xA1], EUC_JP), 1);
    // GBK accepts 0x40 trails; Big5's lead floor is 0xA1.
    assert_eq!(len(&[0x81, 0x40], GBK), 1);
    assert!(!valid(&[0x81, 0x40], BIG5));
}

#[test]
fn multibyte_codepoint_splits_match_chr_semantics() {
    use crate::enc::mb::MbCodepointError;
    assert_eq!(
        mb_codepoint_bytes(MbFamily::Sjis, 0x82A0),
        Ok(vec![0x82, 0xA0])
    );
    assert_eq!(mb_codepoint_bytes(MbFamily::Sjis, 0xB1), Ok(vec![0xB1]));
    assert_eq!(mb_codepoint_bytes(MbFamily::Sjis, 0x41), Ok(vec![0x41]));
    assert_eq!(
        mb_codepoint_bytes(MbFamily::Sjis, 0x8200),
        Err(MbCodepointError::InvalidCodepoint)
    );
    assert_eq!(
        mb_codepoint_bytes(MbFamily::Sjis, 0x80),
        Err(MbCodepointError::InvalidCodepoint)
    );
    assert_eq!(
        mb_codepoint_bytes(MbFamily::Sjis, 0x110000),
        Err(MbCodepointError::OutOfRange)
    );
}

#[test]
fn multibyte_inspect_braces_sequences_and_cases_ascii_only() {
    // あいz -> \x{82A0}\x{82A2}z; kana byte -> \xB1 (oracle forms).
    let s = StrBuf::from_bytes(vec![0x82, 0xA0, 0x82, 0xA2, 0x7A], SHIFT_JIS);
    assert_eq!(inspect(&s), "\"\\x{82A0}\\x{82A2}z\"");
    assert_eq!(
        inspect(&StrBuf::from_bytes(vec![0xB1], SHIFT_JIS)),
        "\"\\xB1\""
    );
    // ASCII-only case fold: trail bytes in the letter range are NOT
    // touched (0x82 0x61 is one character whose trail is 'a').
    let t = StrBuf::from_bytes(vec![0x82, 0x61, 0x62], SHIFT_JIS);
    assert_eq!(t.upcased().bytes(), &[0x82, 0x61, 0x42]);
}
