//! `nkf` -- the `NKF` module (Network Kanji Filter, Japanese text encoding
//! conversion), an in-tree require-gated extension rebuilt over the
//! runtime's own encoding engine (`crate::enc`) rather than the nkf C
//! library.
//!
//! What `nkf` fundamentally does -- decode the input bytes under a detected
//! or declared Japanese encoding, apply text passes (MIME-encoded-word
//! decoding, halfwidth->fullwidth katakana folding, `-Z` fullwidth->ASCII,
//! newline rewrites), re-encode -- is implemented faithfully for the
//! conversion-relevant option subset listed on `parse_opts`. Everything
//! else the real nkf option grammar accepts is deliberately NOT pretended
//! at: unknown flags are ignored the way nkf itself ignores them, and the
//! divergences (MIME encoding `-M`, fold/`-f`, the `guess` heuristic's
//! exact answers on junk bytes) are catalogued in `docs/COMPATIBILITY.md`.
//!
//! The `Kconv` wrapper module and the `String#tojis`/`#toeuc`/... patches
//! are the gem's Ruby half (`ext/nkf/lib/kconv.rb`, vendored upstream).

use crate::RubyValue;
use crate::builtins::encoding::encoding_value;
use crate::builtins::{arg_error, convert};
use crate::enc::{self, EncodingId, Unit};
use zeo_macros::ruby_module;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Newline {
    Unix,
    Windows,
    Mac,
}

struct Opts {
    input: Option<EncodingId>,
    output: Option<EncodingId>,
    /// MIME encoded-word decoding -- ON by default, `-m0` turns it off.
    mime_decode: bool,
    /// Halfwidth->fullwidth katakana folding -- ON by default, `-x` keeps
    /// halfwidth kana as they are.
    fold_kana: bool,
    /// `-Z`..`-Z2`: fullwidth alphanumerics/symbols to ASCII, plus the
    /// ideographic space to one (`-Z1`) or two (`-Z2`) spaces.
    z_mode: Option<u8>,
    newline: Option<Newline>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            input: None,
            output: None,
            mime_decode: true,
            fold_kana: true,
            z_mode: None,
            newline: None,
        }
    }
}

/// Parses nkf's option string: whitespace-separated tokens, each either a
/// long `--ic=`/`--oc=` or a `-` run of single-letter flags. The subset:
/// `-j/-e/-s/-w[8|16|32][B|L][0]` outputs, `-J/-E/-S/-W...` inputs,
/// `--ic=`/`--oc=` by encoding name, `-m[BQN0]`, `-x`/`-X`, `-Z[0-2]`,
/// `-L[uwm]`. Anything else is ignored, as nkf ignores what it doesn't
/// know (`--oc=bogus` included -- oracle-verified).
fn parse_opts(opt: &str) -> Opts {
    let mut o = Opts::default();
    for tok in opt.split_whitespace() {
        if let Some(name) = tok.strip_prefix("--ic=") {
            o.input = enc::find(name);
            continue;
        }
        if let Some(name) = tok.strip_prefix("--oc=") {
            o.output = enc::find(name);
            continue;
        }
        let Some(flags) = tok.strip_prefix('-') else {
            continue;
        };
        if flags.starts_with('-') {
            continue;
        }
        let mut it = flags.chars().peekable();
        while let Some(f) = it.next() {
            match f {
                'j' => o.output = Some(enc::ISO_2022_JP),
                'e' => o.output = Some(enc::EUC_JP),
                's' => o.output = Some(enc::SHIFT_JIS),
                'w' => o.output = Some(utf_variant(&mut it)),
                'J' => o.input = Some(enc::ISO_2022_JP),
                'E' => o.input = Some(enc::EUC_JP),
                'S' => o.input = Some(enc::SHIFT_JIS),
                'W' => o.input = Some(utf_variant(&mut it)),
                'm' => match it.peek() {
                    Some('0') => {
                        it.next();
                        o.mime_decode = false;
                    }
                    Some('B' | 'Q' | 'N' | 'S') => {
                        it.next();
                        o.mime_decode = true;
                    }
                    _ => o.mime_decode = true,
                },
                'x' => o.fold_kana = false,
                'X' => o.fold_kana = true,
                'Z' => {
                    o.z_mode = Some(match it.peek() {
                        Some(d @ '0'..='9') => {
                            let d = *d as u8 - b'0';
                            it.next();
                            d
                        }
                        _ => 0,
                    });
                }
                'L' => {
                    o.newline = match it.next() {
                        Some('u') => Some(Newline::Unix),
                        Some('w') => Some(Newline::Windows),
                        Some('m') => Some(Newline::Mac),
                        _ => None,
                    };
                }
                _ => {}
            }
        }
    }
    o
}

/// The `w`/`W` tail: `8` is UTF-8; `16`/`32` pick the wide pair, `B` (the
/// default) or `L` the endianness, and a trailing `0` (no BOM -- these
/// outputs never carry one anyway) is consumed.
fn utf_variant(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> EncodingId {
    let mut width = String::new();
    while let Some(d @ '0'..='9') = it.peek() {
        // A lone `0` right after `w` is the BOM suppressor, not a width.
        if width.is_empty() && *d == '0' {
            it.next();
            return enc::UTF_8;
        }
        width.push(*d);
        it.next();
    }
    let le = match it.peek() {
        Some('B') => {
            it.next();
            false
        }
        Some('L') => {
            it.next();
            true
        }
        _ => false,
    };
    if let Some('0') = it.peek() {
        it.next();
    }
    match (width.as_str(), le) {
        ("16", false) => enc::UTF_16BE,
        ("16", true) => enc::UTF_16LE,
        ("32", false) => enc::UTF_32BE,
        ("32", true) => enc::UTF_32LE,
        _ => enc::UTF_8,
    }
}

/// `NKF.guess`'s detection, documented as a HEURISTIC: the clear-cut cases
/// (an ISO-2022 escape, pure ASCII, a BOM, text valid in exactly one of
/// UTF-8/EUC-JP/Shift_JIS) answer as CRuby's nkf does; ambiguous junk may
/// answer differently (nkf scores partial matches, this does not).
fn guess_id(bytes: &[u8]) -> EncodingId {
    if bytes
        .windows(2)
        .any(|w| w[0] == 0x1B && (w[1] == b'$' || w[1] == b'('))
    {
        return enc::ISO_2022_JP;
    }
    if bytes.iter().all(|b| *b < 0x80) {
        return enc::US_ASCII;
    }
    if bytes.starts_with(b"\x00\x00\xFE\xFF") || bytes.starts_with(b"\xFF\xFE\x00\x00") {
        return enc::UTF_32;
    }
    if bytes.starts_with(b"\xFE\xFF") || bytes.starts_with(b"\xFF\xFE") {
        return enc::UTF_16;
    }
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        return enc::UTF_8;
    }
    for id in [enc::UTF_8, enc::EUC_JP, enc::SHIFT_JIS] {
        if valid_in(bytes, id) {
            return id;
        }
    }
    enc::ASCII_8BIT
}

fn valid_in(bytes: &[u8], id: EncodingId) -> bool {
    crate::string_from_bytes(bytes.to_vec(), id)
        .lock()
        .valid_encoding()
}

/// Decodes `bytes` under `from` into text, LOSSILY: bytes the encoding
/// can't read (or map to Unicode) are dropped. Real nkf's handling of
/// broken input is stream-state dependent and not promised here.
fn decode_lossy(bytes: &[u8], from: EncodingId) -> String {
    enc::decode(bytes, from)
        .into_iter()
        .filter_map(|u| match u {
            Unit::Char(c) => Some(c),
            Unit::Invalid(..) | Unit::Unmapped(_) => None,
        })
        .collect()
}

/// Decodes RFC 2047 encoded words (`=?charset?B|Q?payload?=`). Whitespace
/// between two adjacent encoded words is swallowed (oracle-verified);
/// a token that doesn't parse, or names an unknown charset, stays verbatim.
fn mime_decode(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("=?") {
        match parse_encoded_word(&rest[start..]) {
            Some((decoded, consumed)) => {
                out.push_str(&rest[..start]);
                out.push_str(&decoded);
                rest = &rest[start + consumed..];
                let trimmed = rest.trim_start_matches([' ', '\t', '\r', '\n']);
                if trimmed.starts_with("=?") && parse_encoded_word(trimmed).is_some() {
                    rest = trimmed;
                }
            }
            None => {
                out.push_str(&rest[..start + 2]);
                rest = &rest[start + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One encoded word at the start of `s`: the decoded text and how many
/// bytes of `s` it spanned.
fn parse_encoded_word(s: &str) -> Option<(String, usize)> {
    let body = s.strip_prefix("=?")?;
    let q1 = body.find('?')?;
    let charset = &body[..q1];
    let mut it = body[q1 + 1..].chars();
    let scheme = it.next()?.to_ascii_uppercase();
    if it.next() != Some('?') {
        return None;
    }
    let payload_start = q1 + 3;
    let end = body[payload_start..].find("?=")?;
    let payload = &body[payload_start..payload_start + end];
    let raw = match scheme {
        'B' => base64_decode(payload)?,
        'Q' => q_decode(payload),
        _ => return None,
    };
    let from = enc::find(charset)?;
    Some((decode_lossy(&raw, from), 2 + payload_start + end + 2))
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in s.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            '=' => break,
            '\r' | '\n' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn q_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'_' => {
                out.push(b' ');
                i += 1;
            }
            b'=' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'=');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// The fullwidth forms of U+FF61..U+FF9F, in codepoint order.
const FULLWIDTH: &[char] = &[
    '。', '「', '」', '、', '・', 'ヲ', 'ァ', 'ィ', 'ゥ', 'ェ', 'ォ', 'ャ', 'ュ', 'ョ', 'ッ', 'ー',
    'ア', 'イ', 'ウ', 'エ', 'オ', 'カ', 'キ', 'ク', 'ケ', 'コ', 'サ', 'シ', 'ス', 'セ', 'ソ', 'タ',
    'チ', 'ツ', 'テ', 'ト', 'ナ', 'ニ', 'ヌ', 'ネ', 'ノ', 'ハ', 'ヒ', 'フ', 'ヘ', 'ホ', 'マ', 'ミ',
    'ム', 'メ', 'モ', 'ヤ', 'ユ', 'ヨ', 'ラ', 'リ', 'ル', 'レ', 'ロ', 'ワ', 'ン', '゛', '゜',
];

/// Halfwidth->fullwidth katakana folding (nkf's default), including the
/// voiced-mark combinations: `ｶ + ﾞ` is one `ガ`, `ﾊ + ﾟ` one `パ`, `ｳ + ﾞ`
/// is `ヴ`; a mark with no combinable base stays standalone (`ｱﾞ` is
/// `ア` + `゛` -- oracle-verified).
fn fold_halfwidth(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        let cp = c as u32;
        if !(0xFF61..=0xFF9F).contains(&cp) {
            out.push(c);
            continue;
        }
        let full = FULLWIDTH[(cp - 0xFF61) as usize];
        let mark = chars.peek().map(|n| *n as u32);
        if mark == Some(0xFF9E) && combines_with_dakuten(full) {
            chars.next();
            out.push(
                char::from_u32(if full == 'ウ' {
                    0x30F4
                } else {
                    full as u32 + 1
                })
                .expect("kana"),
            );
        } else if mark == Some(0xFF9F) && ('ハ'..='ホ').contains(&full) {
            chars.next();
            out.push(char::from_u32(full as u32 + 2).expect("kana"));
        } else {
            out.push(full);
        }
    }
    out
}

fn combines_with_dakuten(c: char) -> bool {
    "ウカキクケコサシスセソタチツテトハヒフヘホ".contains(c)
}

/// `-Z`: fullwidth ASCII forms (U+FF01..U+FF5D -- CRuby's nkf leaves `～`
/// alone) to their ASCII characters; `-Z1`/`-Z2` also turn the ideographic
/// space into one/two spaces.
fn z_convert(s: &str, mode: u8) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let cp = c as u32;
        if (0xFF01..=0xFF5D).contains(&cp) {
            out.push(char::from_u32(cp - 0xFEE0).expect("ASCII range"));
        } else if c == '\u{3000}' && (1..=2).contains(&mode) {
            for _ in 0..mode {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn convert_newlines(s: &str, mode: Newline) -> String {
    let unix = s.replace("\r\n", "\n").replace('\r', "\n");
    match mode {
        Newline::Unix => unix,
        Newline::Windows => unix.replace('\n', "\r\n"),
        Newline::Mac => unix.replace('\n', "\r"),
    }
}

/// Re-encodes the processed text into the output encoding: the stateful
/// ISO-2022-JP encoder (halfwidth kana allowed -- nkf's `-x -j` emits real
/// `ESC ( I` runs), or the engine's transcoder with nkf's `?` for anything
/// unrepresentable.
fn encode_out(s: &str, to: EncodingId) -> Vec<u8> {
    if to == enc::ISO_2022_JP {
        let mut jis = enc::iso2022jp::Encoder::new(true);
        let mut out = Vec::with_capacity(s.len());
        for c in s.chars() {
            if jis.push(c, &mut out).is_err() {
                let _ = jis.push('?', &mut out);
            }
        }
        jis.finish(&mut out);
        return out;
    }
    let opts = enc::TranscodeOptions {
        undef_replace: true,
        invalid_replace: true,
        replace: Some("?".to_string()),
        ..Default::default()
    };
    enc::transcode(s.as_bytes(), enc::UTF_8, to, &opts, None)
        .expect("replace options make transcoding total")
}

ruby_module! {
    NKF = zeo_abi::NKF_MODULE;

    // The classic extension's identity constants, as the real nkf gem
    // reports them.
    const VERSION = str_value("2.1.5 (2018-12-15)");
    const NKF_VERSION = str_value("2.1.5");
    const NKF_RELEASE_DATE = str_value("2018-12-15");
    const GEM_VERSION = str_value("0.3.0");

    // The encoding constants `Kconv`/`NKF` callers pass around. AUTO/
    // NOCONV/UNKNOWN are nil in CRuby's nkf too.
    const AUTO = RubyValue::Nil;
    const NOCONV = RubyValue::Nil;
    const UNKNOWN = RubyValue::Nil;
    const ASCII = encoding_value(enc::US_ASCII);
    const BINARY = encoding_value(enc::ASCII_8BIT);
    const JIS = encoding_value(enc::ISO_2022_JP);
    const EUC = encoding_value(enc::EUC_JP);
    const SJIS = encoding_value(enc::SHIFT_JIS);
    const UTF8 = encoding_value(enc::UTF_8);
    const UTF16 = encoding_value(enc::UTF_16BE);
    const UTF32 = encoding_value(enc::UTF_32BE);

    // `NKF.nkf(opt, str)`: the filter. The option string picks the output
    // (mandatory) and optionally the input encoding; without `-J/-E/-S/-W`
    // or `--ic=` the input is guessed per `guess`.
    def self."nkf" (_recv, arg1, arg2) {
        let opt = convert::to_rstr(arg1)?.lock().to_utf8_lossy().into_owned();
        let o = parse_opts(&opt);
        let Some(out_id) = o.output else {
            return Err(arg_error!("no output encoding given"));
        };
        let bytes = convert::to_rstr(arg2)?.lock().bytes().to_vec();
        let in_id = o.input.unwrap_or_else(|| guess_id(&bytes));
        let mut text = decode_lossy(&bytes, in_id);
        if o.mime_decode {
            text = mime_decode(&text);
        }
        if o.fold_kana {
            text = fold_halfwidth(&text);
        }
        if let Some(z) = o.z_mode {
            text = z_convert(&text, z);
        }
        if let Some(nl) = o.newline {
            text = convert_newlines(&text, nl);
        }
        Ok(RubyValue::Str(crate::string_from_bytes(encode_out(&text, out_id), out_id)))
    }

    // `NKF.guess(str)`: the detected Encoding object (see `guess_id`).
    def self."guess" (_recv, arg) {
        let bytes = convert::to_rstr(arg)?.lock().bytes().to_vec();
        Ok(encoding_value(guess_id(&bytes)))
    }
}

fn str_value(s: &str) -> RubyValue {
    RubyValue::Str(crate::string_new(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_the_clear_cut_cases() {
        assert_eq!(guess_id(b"hello"), enc::US_ASCII);
        assert_eq!(guess_id(b"\x1B$B%F%9%H\x1B(B"), enc::ISO_2022_JP);
        assert_eq!(guess_id("テスト".as_bytes()), enc::UTF_8);
        assert_eq!(guess_id(b"\xA4\xA2"), enc::EUC_JP);
        assert_eq!(guess_id(b"\x83A"), enc::SHIFT_JIS);
        assert_eq!(guess_id(b"\xFF\xFEa\x00"), enc::UTF_16);
        assert_eq!(guess_id(b"\x00\x00\xFE\xFF"), enc::UTF_32);
    }

    #[test]
    fn options_pick_encodings_and_passes() {
        let o = parse_opts("-S -w");
        assert_eq!(o.input, Some(enc::SHIFT_JIS));
        assert_eq!(o.output, Some(enc::UTF_8));
        let o = parse_opts("-w16L0");
        assert_eq!(o.output, Some(enc::UTF_16LE));
        let o = parse_opts("--ic=Shift_JIS --oc=UTF-16");
        assert_eq!(o.input, Some(enc::SHIFT_JIS));
        assert_eq!(o.output, Some(enc::UTF_16));
        let o = parse_opts("-jm");
        assert_eq!(o.output, Some(enc::ISO_2022_JP));
        assert!(o.mime_decode);
        let o = parse_opts("-wx -m0 -Z2 -Lw");
        assert!(!o.fold_kana);
        assert!(!o.mime_decode);
        assert_eq!(o.z_mode, Some(2));
        assert!(o.newline == Some(Newline::Windows));
    }

    #[test]
    fn folds_halfwidth_katakana_with_voiced_marks() {
        assert_eq!(fold_halfwidth("\u{FF76}\u{FF9E}"), "ガ");
        assert_eq!(fold_halfwidth("\u{FF8A}\u{FF9F}"), "パ");
        assert_eq!(fold_halfwidth("\u{FF73}\u{FF9E}"), "ヴ");
        assert_eq!(fold_halfwidth("\u{FF71}\u{FF9E}"), "ア゛");
        assert_eq!(fold_halfwidth("\u{FF61}\u{FF70}"), "。ー");
    }

    #[test]
    fn decodes_mime_words_and_joins_adjacent_ones() {
        assert_eq!(mime_decode("=?UTF-8?B?44OG44K5?="), "テス");
        assert_eq!(mime_decode("=?UTF-8?B?44OG?= =?UTF-8?B?44K5?="), "テス");
        assert_eq!(mime_decode("a =?utf-8?q?te=20st_x?= b"), "a te st x b");
        assert_eq!(mime_decode("=?bogus?B?x?="), "=?bogus?B?x?=");
    }

    #[test]
    fn z_leaves_the_wave_dash_alone() {
        assert_eq!(z_convert("Ａｂ！～", 0), "Ab!～");
        assert_eq!(z_convert("あ\u{3000}い", 2), "あ  い");
    }
}
