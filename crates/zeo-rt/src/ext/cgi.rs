//! `cgi/escape` (CRuby's C-accelerated `CGI` escape helpers). `require
//! "cgi/escape"` (or `"cgi"`/`"cgi/util"`, canonicalized by the loader)
//! activates the `CGI` module's URL/HTML escape functions.
//!
//! Pure string transforms, oracle-verified against ruby 4.0.6:
//! `escape`/`unescape` are `application/x-www-form-urlencoded` (space<->`+`);
//! `escapeURIComponent`/`unescapeURIComponent` percent-encode space as `%20`;
//! `escapeHTML`/`unescapeHTML` map `& < > " '`. The unreserved set kept by the
//! URL escapers is alphanumerics plus `_.-~`.

use crate::RubyValue;
use zeo_macros::{ruby_class, ruby_module};

fn in_bytes(v: &RubyValue) -> Vec<u8> {
    match v {
        RubyValue::Str(s) => s.lock().bytes().to_vec(),
        other => other.to_display_string().into_bytes(),
    }
}

/// The encoding an argument carries -- UTF-8 for anything that had to be
/// stringified to get here.
fn enc_of(v: &RubyValue) -> crate::encoding::EncodingId {
    match v {
        RubyValue::Str(s) => s.lock().encoding(),
        _ => crate::encoding::UTF_8,
    }
}

/// One row's answer, in the ARGUMENT's encoding.
///
/// Every escape here is a byte transformation -- it rewrites ASCII
/// punctuation and passes everything else through -- so CRuby hands back a
/// string in the encoding it was given, invalid bytes and all:
/// `CGI.escapeHTML("\xC0<")` on a binary string is `"\xC0&lt;"`, still
/// binary. Decoding first is what turned that into `"À&lt;"` in UTF-8, and
/// what turned a UTF-8 snowman into three Latin-1 characters: `b as char`
/// reads a BYTE as a codepoint, and `String` then re-encodes it.
fn out_like(arg: &RubyValue, bytes: Vec<u8>) -> RubyValue {
    RubyValue::Str(crate::string_wrap(crate::StrBuf::from_bytes(
        bytes,
        enc_of(arg),
    )))
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~')
}

/// Percent-encode, keeping unreserved bytes. `space_plus` chooses form-encoding
/// (`escape`, space -> `+`) vs. component-encoding (`escapeURIComponent`, `%20`).
fn percent_encode(bytes: &[u8], space_plus: bool) -> String {
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes {
        if b == b' ' && space_plus {
            s.push('+');
        } else if is_unreserved(b) {
            s.push(b as char);
        } else {
            s.push('%');
            s.push_str(&format!("{b:02X}"));
        }
    }
    s
}

/// Percent-decode. `plus_space` turns `+` into a space (form-decoding).
fn percent_decode(bytes: &[u8], plus_space: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' if plus_space => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
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

fn html_escape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' => out.extend_from_slice(b"&#39;"),
            b => out.push(b),
        }
    }
    out
}

/// `unescapeHTML`, in bytes and in `enc`.
///
/// A NAMED entity is ASCII and always decodes. A NUMERIC one decodes only
/// when `enc` can represent the codepoint, and is left standing when it
/// cannot -- oracle-verified, and CRuby's own rule: `&#9731;` is a snowman
/// in a UTF-8 string, stays `&#9731;` in a binary one (no byte can hold
/// it) and stays in Latin-1 too, while `&#233;` in Latin-1 decodes to the
/// single byte `\xE9`.
fn html_unescape(bytes: &[u8], enc: crate::encoding::EncodingId) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let Some(rel) = bytes[i..].iter().position(|&b| b == b';') else {
            out.push(b'&');
            i += 1;
            continue;
        };
        let entity = &bytes[i + 1..i + rel];
        let decoded = match entity {
            b"amp" => Some('&'),
            b"lt" => Some('<'),
            b"gt" => Some('>'),
            b"quot" => Some('"'),
            b"apos" | b"#39" => Some('\''),
            _ => std::str::from_utf8(entity)
                .ok()
                .and_then(|e| e.strip_prefix('#'))
                .and_then(|num| {
                    num.strip_prefix(['x', 'X'])
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| num.parse::<u32>().ok())
                })
                .and_then(char::from_u32),
        };
        match decoded.and_then(|c| crate::encoding::encode_scalar(enc, c)) {
            Some(b) => {
                out.extend_from_slice(&b);
                i += rel + 1;
            }
            None => {
                out.push(b'&');
                i += 1;
            }
        }
    }
    out
}

fn as_text(v: &RubyValue) -> String {
    match v {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        other => other.to_display_string(),
    }
}

/// `escapeElement(string, "A", "B")` and `escapeElement(string, ["A", "B"])`
/// name the same two elements -- CRuby splats the list either way.
fn element_names(args: &[RubyValue]) -> Vec<String> {
    let mut names = Vec::new();
    for a in args {
        match a {
            RubyValue::Array(arr) => {
                names.extend(arr.lock().to_vec().iter().map(as_text));
            }
            other => names.push(as_text(other)),
        }
    }
    names
}

/// Which alphabet an element span is read in: the tag as written, or the
/// tag as `escapeHTML` left it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TagText {
    Plain,
    Escaped,
}

/// The byte ranges of the `<tag ...>` / `</tag>` spans naming one of `names`.
///
/// CRuby writes each as a regexp, and the two differ only in the ALPHABET
/// (`cgi/escape.rb`, ruby 4.0.6):
///
/// ```text
///   escapeElement    /<\/?(?:A|B)\b[^<>]*+>?/im
///   unescapeElement  /&lt;\/?(?:A|B)\b(?>[^&]+|&(?![gl]t;)\w+;)*(?:&gt;)?/im
/// ```
///
/// Both read the tag name, then run to the next DELIMITER -- an open or a
/// close -- and take the close only when that is what stopped them. Two
/// consequences the older "find the next close" reading got wrong: an
/// UNTERMINATED tag still matches (`&lt;A` unescapes), and a tag whose
/// attributes carry an escaped delimiter ends AT it, so
/// `&lt;A HREF=&quot;a&lt;b&quot;&gt;` unescapes only as far as the `a`.
///
/// The escaped form also stops at any `&` that does not begin a `&word;`
/// entity, which is why a NUMERIC reference ends a span: `&#62;` has no
/// `\w` after the `&`.
///
/// The name run is read to its end, which is the regexp's `\b` -- `<ABBR>`
/// is not `<A>`.
fn element_spans(text: &[u8], names: &[String], mode: TagText) -> Vec<(usize, usize)> {
    let (open, close): (&[u8], &[u8]) = match mode {
        TagText::Plain => (b"<", b">"),
        TagText::Escaped => (b"&lt;", b"&gt;"),
    };
    let find = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).position(|w| w == needle);
    let mut spans = Vec::new();
    let mut i = 0;
    while let Some(rel) = find(&text[i..], open) {
        let start = i + rel;
        let mut j = start + open.len();
        if text.get(j) == Some(&b'/') {
            j += 1;
        }
        let name_start = j;
        while j < text.len() && (text[j].is_ascii_alphanumeric() || text[j] == b'_') {
            j += 1;
        }
        let name = &text[name_start..j];
        if !name.is_empty()
            && let Ok(name) = std::str::from_utf8(name)
            && names.iter().any(|n| n.eq_ignore_ascii_case(name))
        {
            let body_end = tag_body_end(text, j, mode);
            let end = match text[body_end..].starts_with(close) {
                true => body_end + close.len(),
                false => body_end,
            };
            spans.push((start, end));
            i = end;
            continue;
        }
        i = start + open.len();
    }
    spans
}

/// Where a tag's attribute run ends: at the next delimiter, and for escaped
/// text at any `&` that does not begin a `&word;` entity other than `&lt;`
/// or `&gt;`. See [`element_spans`] for the two regexps this is.
fn tag_body_end(text: &[u8], from: usize, mode: TagText) -> usize {
    let mut k = from;
    match mode {
        TagText::Plain => {
            while k < text.len() && text[k] != b'<' && text[k] != b'>' {
                k += 1;
            }
            k
        }
        TagText::Escaped => loop {
            while k < text.len() && text[k] != b'&' {
                k += 1;
            }
            match entity_run(&text[k..]) {
                Some(n) => k += n,
                None => return k,
            }
        },
    }
}

/// The length of a `&word;` entity at the head of `text`, or `None` when
/// there is none or it is the `&lt;`/`&gt;` a tag span ends at. The regexp
/// is `&(?![gl]t;)\w+;`, so a numeric reference never qualifies.
fn entity_run(text: &[u8]) -> Option<usize> {
    if text.first() != Some(&b'&') || text.starts_with(b"&lt;") || text.starts_with(b"&gt;") {
        return None;
    }
    let mut k = 1;
    while k < text.len() && (text[k].is_ascii_alphanumeric() || text[k] == b'_') {
        k += 1;
    }
    match k > 1 && text.get(k) == Some(&b';') {
        true => Some(k + 1),
        false => None,
    }
}

/// Rewrite each span through `f` and leave everything between them alone.
fn rewrite_spans(text: &[u8], spans: &[(usize, usize)], f: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    for &(start, end) in spans {
        out.extend_from_slice(&text[at..start]);
        out.extend_from_slice(&f(&text[start..end]));
        at = end;
    }
    out.extend_from_slice(&text[at..]);
    out
}

fn escape_element(args: &[RubyValue]) -> RubyValue {
    let text = in_bytes(&args[0]);
    let names = element_names(&args[1..]);
    let spans = element_spans(&text, &names, TagText::Plain);
    out_like(&args[0], rewrite_spans(&text, &spans, |m| html_escape(m)))
}

fn unescape_element(args: &[RubyValue]) -> RubyValue {
    let text = in_bytes(&args[0]);
    let names = element_names(&args[1..]);
    let enc = enc_of(&args[0]);
    let spans = element_spans(&text, &names, TagText::Escaped);
    out_like(
        &args[0],
        rewrite_spans(&text, &spans, |m| html_unescape(m, enc)),
    )
}

/// `CGI::EscapeExt` -- the module CRuby's `cgi/escape` PREPENDS to
/// `CGI::Escape`, so it wins every name the two share. Its own additions are
/// `h` and the `escape_html`/`unescape_html` snake spellings.
///
/// Its own inline `mod` for the reason `file_constants` gives: one
/// `ruby_module!` per module scope, since each emits a `lookup` of its own.
mod escape_ext {
    use super::*;

    ruby_module! {
        EscapeExt = zeo_abi::CGI_ESCAPE_EXT_MODULE;

        // Arities match ruby 4.0.6.
        def "escape" (_recv, arg) {
            Ok(out_like(arg, percent_encode(&in_bytes(arg), true).into_bytes()))
        }
        def "unescape" cfunc (_recv, string, _encoding?) {
            Ok(out_like(string, percent_decode(&in_bytes(string), true)))
        }
        def "escapeURIComponent" | "escape_uri_component" (_recv, arg) {
            Ok(out_like(arg, percent_encode(&in_bytes(arg), false).into_bytes()))
        }
        def "unescapeURIComponent" arity -1 | "unescape_uri_component" arity -1 (_recv, arg1, _arg2?) {
            Ok(out_like(arg1, percent_decode(&in_bytes(arg1), false)))
        }
        def "escapeHTML" | "escape_html" | "h" (_recv, arg) {
            Ok(out_like(arg, html_escape(&in_bytes(arg))))
        }
        def "unescapeHTML" | "unescape_html" (_recv, arg) {
            Ok(out_like(arg, html_unescape(&in_bytes(arg), enc_of(arg))))
        }
    }
}

/// `CGI::Escape` -- what `CGI` both includes and extends. It defines the same
/// eight names `EscapeExt` does (which prepends ahead of them, so those answer
/// from there) plus the `*Element` family, which is its alone.
mod escape {
    use super::*;

    ruby_module! {
        Escape = zeo_abi::CGI_ESCAPE_MODULE;

        def "escape" (_recv, arg) {
            Ok(out_like(arg, percent_encode(&in_bytes(arg), true).into_bytes()))
        }
        def "unescape" cfunc (_recv, string, _encoding?) {
            Ok(out_like(string, percent_decode(&in_bytes(string), true)))
        }
        def "escapeURIComponent" | "escape_uri_component" (_recv, arg) {
            Ok(out_like(arg, percent_encode(&in_bytes(arg), false).into_bytes()))
        }
        def "unescapeURIComponent" arity -1 | "unescape_uri_component" arity -1 (_recv, arg1, _arg2?) {
            Ok(out_like(arg1, percent_decode(&in_bytes(arg1), false)))
        }
        def "escapeHTML" (_recv, arg) {
            Ok(out_like(arg, html_escape(&in_bytes(arg))))
        }
        def "unescapeHTML" (_recv, arg) {
            Ok(out_like(arg, html_unescape(&in_bytes(arg), enc_of(arg))))
        }
        // `escapeElement(string, *elements)` escapes the TAGS of the named
        // elements and nothing else -- not the text between them, and not a
        // tag it was not asked about.
        def "escapeElement" | "escape_element" (_recv, _string, *_elements) {
            Ok(escape_element(__args))
        }
        def "unescapeElement" | "unescape_element" (_recv, _string, *_elements) {
            Ok(unescape_element(__args))
        }
    }
}

ruby_class! {
    CGI = zeo_abi::CGI_MODULE < zeo_abi::OBJECT_CLASS;
    include zeo_abi::CGI_ESCAPE_MODULE;

    // `CGI` INCLUDES and EXTENDS `CGI::Escape` (see `zeo_abi::BUILTIN_EXTENDS`),
    // so every escape is an instance method of a module and `CGI` itself
    // defines none of them. `CGI.escapeHTML` runs `EscapeExt`'s row through
    // the extend edge, which is why `.owner` names the module.
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(text.to_string()))
    }
    /// A byte answer as text, for an assertion written in ASCII.
    fn b(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).expect("the fixtures are ASCII")
    }
    fn t(v: RubyValue) -> String {
        match v {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn url_escapes_match_ruby() {
        assert_eq!(
            percent_encode(b"a b&c=d~e.f-g_h", true),
            "a+b%26c%3Dd~e.f-g_h"
        );
        assert_eq!(
            String::from_utf8_lossy(&percent_decode(b"a+b%26c", true)),
            "a b&c"
        );
        assert_eq!(percent_encode(b"a b&c", false), "a%20b%26c");
    }

    #[test]
    fn html_escapes_match_ruby() {
        let utf8 = crate::encoding::UTF_8;
        assert_eq!(b(html_escape(b"<a>&\"'")), "&lt;a&gt;&amp;&quot;&#39;");
        assert_eq!(
            b(html_unescape(b"&lt;a&gt;&amp;&quot;&#39;", utf8)),
            "<a>&\"'"
        );
        assert_eq!(b(html_unescape(b"&#x41;&#66;", utf8)), "AB");
        // A byte no encoding decodes passes through untouched, and the
        // numeric entity for a character the encoding cannot hold is left
        // standing -- CRuby's rule, and what a lossy decode destroyed.
        assert_eq!(html_escape(b"\xC0<"), b"\xC0&lt;".to_vec());
        assert_eq!(
            html_unescape(b"&#9731;", crate::encoding::ASCII_8BIT),
            b"&#9731;".to_vec()
        );
        assert_eq!(
            html_unescape(b"&#9731;", utf8),
            "\u{2603}".as_bytes().to_vec()
        );
    }

    #[test]
    fn element_escapes_name_only_what_they_were_asked_about() {
        // The tags of the named element, and nothing between or beside them.
        assert_eq!(
            t(escape_element(&[s("<A HREF='x'>t</A><B>b</B>"), s("A")])),
            "&lt;A HREF=&#39;x&#39;&gt;t&lt;/A&gt;<B>b</B>"
        );
        // Several names, spelled either way.
        assert_eq!(
            t(escape_element(&[s("<A><B>"), s("A"), s("B")])),
            "&lt;A&gt;&lt;B&gt;"
        );
        assert_eq!(
            t(escape_element(&[
                s("<A><B>"),
                RubyValue::Array(crate::array_new(vec![s("A")]))
            ])),
            "&lt;A&gt;<B>"
        );
        // No names at all leaves the string alone.
        assert_eq!(t(escape_element(&[s("<A>")])), "<A>");
        // A longer tag is not the one named -- CRuby's `(?!\w)`.
        assert_eq!(t(escape_element(&[s("<ABBR>"), s("A")])), "<ABBR>");
        // The inverse, over the escaped delimiters.
        assert_eq!(
            t(unescape_element(&[s("&lt;A&gt;&lt;B&gt;"), s("A")])),
            "<A>&lt;B&gt;"
        );
    }
}
