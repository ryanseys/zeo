//! `cgi/escape` (CRuby's C-accelerated `CGI` escape helpers). `require
//! "cgi/escape"` (or `"cgi"`/`"cgi/util"`, canonicalized by the loader)
//! activates the `CGI` module's URL/HTML escape functions.
//!
//! Pure string transforms, oracle-verified against ruby 4.0.5:
//! `escape`/`unescape` are `application/x-www-form-urlencoded` (space<->`+`);
//! `escapeURIComponent`/`unescapeURIComponent` percent-encode space as `%20`;
//! `escapeHTML`/`unescapeHTML` map `& < > " '`. The unreserved set kept by the
//! URL escapers is alphanumerics plus `_.-~`.

use crate::builtins::{arity, builtin_methods};
use crate::{string_new, RubyValue};

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

builtin_methods! {
    pub(crate) fn lookup_class;

    "escape" => fn escape(_recv, args, _block) {
        arity!(args, 1);
        Ok(out(percent_encode(&in_bytes(&args[0]), true)))
    }
    "unescape" => fn unescape(_recv, args, _block) {
        arity!(args, 1..=2); // (str[, encoding]) -- encoding ignored
        Ok(out(String::from_utf8_lossy(&percent_decode(&in_bytes(&args[0]), true)).into_owned()))
    }
    "escapeURIComponent" | "escape_uri_component" => fn escape_uri_component(_recv, args, _block) {
        arity!(args, 1);
        Ok(out(percent_encode(&in_bytes(&args[0]), false)))
    }
    "unescapeURIComponent" | "unescape_uri_component" => fn unescape_uri_component(_recv, args, _block) {
        arity!(args, 1..=2);
        Ok(out(String::from_utf8_lossy(&percent_decode(&in_bytes(&args[0]), false)).into_owned()))
    }
    "escapeHTML" | "escape_html" => fn escape_html(_recv, args, _block) {
        arity!(args, 1);
        Ok(out(html_escape(&in_bytes(&args[0]))))
    }
    "unescapeHTML" | "unescape_html" => fn unescape_html(_recv, args, _block) {
        arity!(args, 1);
        let text = match &args[0] {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => other.to_display_string(),
        };
        Ok(out(html_unescape(&text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    fn t(v: Result<RubyValue, crate::Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn url_escapes_match_ruby() {
        assert_eq!(t(escape(&RubyValue::Nil, &[s("a b&c=d~e.f-g_h")], None)), "a+b%26c%3Dd~e.f-g_h");
        assert_eq!(t(unescape(&RubyValue::Nil, &[s("a+b%26c")], None)), "a b&c");
        assert_eq!(t(escape_uri_component(&RubyValue::Nil, &[s("a b&c")], None)), "a%20b%26c");
    }

    #[test]
    fn html_escapes_match_ruby() {
        assert_eq!(t(escape_html(&RubyValue::Nil, &[s("<a>&\"'")], None)), "&lt;a&gt;&amp;&quot;&#39;");
        assert_eq!(t(unescape_html(&RubyValue::Nil, &[s("&lt;a&gt;&amp;&quot;&#39;")], None)), "<a>&\"'");
        assert_eq!(t(unescape_html(&RubyValue::Nil, &[s("&#x41;&#66;")], None)), "AB");
    }
}
