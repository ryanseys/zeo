//! `cgi/escape` (CRuby's C-accelerated `CGI` escape helpers). `require
//! "cgi/escape"` (or `"cgi"`/`"cgi/util"`, canonicalized by the loader)
//! activates the `CGI` module's URL/HTML escape functions.
//!
//! Pure string transforms, oracle-verified against ruby 4.0.6:
//! `escape`/`unescape` are `application/x-www-form-urlencoded` (space<->`+`);
//! `escapeURIComponent`/`unescapeURIComponent` percent-encode space as `%20`;
//! `escapeHTML`/`unescapeHTML` map `& < > " '`. The unreserved set kept by the
//! URL escapers is alphanumerics plus `_.-~`.

use crate::{RubyValue, string_new};
use zeo_macros::{ruby_class, ruby_module};

fn in_bytes(v: &RubyValue) -> Vec<u8> {
    match v {
        RubyValue::Str(s) => s.lock().bytes().to_vec(),
        other => other.to_display_string().into_bytes(),
    }
}

fn out(text: String) -> RubyValue {
    RubyValue::Str(string_new(text))
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

fn html_escape(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'&' => s.push_str("&amp;"),
            b'<' => s.push_str("&lt;"),
            b'>' => s.push_str("&gt;"),
            b'"' => s.push_str("&quot;"),
            b'\'' => s.push_str("&#39;"),
            b => s.push(b as char),
        }
    }
    s
}

fn html_unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            _ => entity
                .strip_prefix('#')
                .and_then(|num| {
                    num.strip_prefix(['x', 'X'])
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| num.parse::<u32>().ok())
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
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

/// The byte ranges of the `<tag ...>` / `</tag>` spans naming one of `names`,
/// where `open`/`close` are the delimiters as they appear in this text --
/// `<`/`>` before escaping, `&lt;`/`&gt;` after it. CRuby writes this as a
/// regexp (`/<\/?(?:A|B)(?!\w)(?:.|\n)*?>/i`); the name run is read to its end
/// here, which is that `(?!\w)` -- `<ABBR>` is not `<A>`.
fn element_spans(text: &str, names: &[String], open: &str, close: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut i = 0;
    while let Some(rel) = text[i..].find(open) {
        let start = i + rel;
        let mut j = start + open.len();
        if text[j..].starts_with('/') {
            j += 1;
        }
        let name_start = j;
        while j < text.len() && {
            let b = text.as_bytes()[j];
            b.is_ascii_alphanumeric() || b == b'_'
        } {
            j += 1;
        }
        let name = &text[name_start..j];
        if !name.is_empty()
            && names.iter().any(|n| n.eq_ignore_ascii_case(name))
            && let Some(rel2) = text[j..].find(close)
        {
            let end = j + rel2 + close.len();
            spans.push((start, end));
            i = end;
            continue;
        }
        i = start + open.len();
    }
    spans
}

/// Rewrite each span through `f` and leave everything between them alone.
fn rewrite_spans(text: &str, spans: &[(usize, usize)], f: impl Fn(&str) -> String) -> String {
    let mut s = String::with_capacity(text.len());
    let mut at = 0;
    for &(start, end) in spans {
        s.push_str(&text[at..start]);
        s.push_str(&f(&text[start..end]));
        at = end;
    }
    s.push_str(&text[at..]);
    s
}

fn escape_element(args: &[RubyValue]) -> RubyValue {
    let text = as_text(&args[0]);
    let names = element_names(&args[1..]);
    let spans = element_spans(&text, &names, "<", ">");
    out(rewrite_spans(&text, &spans, |m| html_escape(m.as_bytes())))
}

fn unescape_element(args: &[RubyValue]) -> RubyValue {
    let text = as_text(&args[0]);
    let names = element_names(&args[1..]);
    let spans = element_spans(&text, &names, "&lt;", "&gt;");
    out(rewrite_spans(&text, &spans, html_unescape))
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
            Ok(out(percent_encode(&in_bytes(arg), true)))
        }
        def "unescape" cfunc (_recv, string, _encoding?) {
            Ok(out(String::from_utf8_lossy(&percent_decode(&in_bytes(string), true)).into_owned()))
        }
        def "escapeURIComponent" | "escape_uri_component" (_recv, arg) {
            Ok(out(percent_encode(&in_bytes(arg), false)))
        }
        def "unescapeURIComponent" arity -1 | "unescape_uri_component" arity -1 (_recv, arg1, _arg2?) {
            Ok(out(String::from_utf8_lossy(&percent_decode(&in_bytes(arg1), false)).into_owned()))
        }
        def "escapeHTML" | "escape_html" | "h" (_recv, arg) {
            Ok(out(html_escape(&in_bytes(arg))))
        }
        def "unescapeHTML" | "unescape_html" (_recv, arg) {
            Ok(out(html_unescape(&as_text(arg))))
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
            Ok(out(percent_encode(&in_bytes(arg), true)))
        }
        def "unescape" cfunc (_recv, string, _encoding?) {
            Ok(out(String::from_utf8_lossy(&percent_decode(&in_bytes(string), true)).into_owned()))
        }
        def "escapeURIComponent" | "escape_uri_component" (_recv, arg) {
            Ok(out(percent_encode(&in_bytes(arg), false)))
        }
        def "unescapeURIComponent" arity -1 | "unescape_uri_component" arity -1 (_recv, arg1, _arg2?) {
            Ok(out(String::from_utf8_lossy(&percent_decode(&in_bytes(arg1), false)).into_owned()))
        }
        def "escapeHTML" (_recv, arg) {
            Ok(out(html_escape(&in_bytes(arg))))
        }
        def "unescapeHTML" (_recv, arg) {
            Ok(out(html_unescape(&as_text(arg))))
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
    // so ruby reaches every one of these as an instance method of a module.
    // The include side works here -- `CGI.new(...).escapeHTML` walks the
    // ancestry -- but the extend side does not: a module's instance row takes
    // an `RObj`, and a class-level call arrives with a `RubyValue::Class`,
    // which is not one. So the class side keeps its own rows, over the same
    // bodies.
    //
    // ONE divergence survives that: `CGI.method(:escapeHTML).owner` answers
    // `CGI` where ruby answers `CGI::EscapeExt`. Every other observable agrees
    // -- both ancestor chains, both `instance_methods` lists, and what each
    // call returns. Closing it needs a class-level call to be able to run a
    // module's instance row, which is a dispatch change, not a CGI one.
    def self."escape" (_recv, arg) {
        Ok(out(percent_encode(&in_bytes(arg), true)))
    }
    def self."unescape" cfunc (_recv, string, _encoding?) {
        Ok(out(String::from_utf8_lossy(&percent_decode(&in_bytes(string), true)).into_owned()))
    }
    def self."escapeURIComponent" | "escape_uri_component" (_recv, arg) {
        Ok(out(percent_encode(&in_bytes(arg), false)))
    }
    def self."unescapeURIComponent" arity -1 | "unescape_uri_component" arity -1 (_recv, arg1, _arg2?) {
        Ok(out(String::from_utf8_lossy(&percent_decode(&in_bytes(arg1), false)).into_owned()))
    }
    def self."escapeHTML" | "escape_html" | "h" (_recv, arg) {
        Ok(out(html_escape(&in_bytes(arg))))
    }
    def self."unescapeHTML" | "unescape_html" (_recv, arg) {
        Ok(out(html_unescape(&as_text(arg))))
    }
    def self."escapeElement" | "escape_element" (_recv, _string, *_elements) {
        Ok(escape_element(__args))
    }
    def self."unescapeElement" | "unescape_element" (_recv, _string, *_elements) {
        Ok(unescape_element(__args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
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
        assert_eq!(html_escape(b"<a>&\"'"), "&lt;a&gt;&amp;&quot;&#39;");
        assert_eq!(html_unescape("&lt;a&gt;&amp;&quot;&#39;"), "<a>&\"'");
        assert_eq!(html_unescape("&#x41;&#66;"), "AB");
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
