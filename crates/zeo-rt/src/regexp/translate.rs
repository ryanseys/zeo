//! Regexp construction: the `re.c` preprocessing ruby applies BEFORE the
//! engine sees a pattern (`\u` escapes, `\M-`/`\C-`/`\c` byte escapes), the
//! CRuby-shaped compile-error text, and the construction entry points
//! (`regexp_new`, `regexp_new_enc`, the per-site `RegexpSite` cache).
//!
//! Oniguruma reads the pattern as written, in Ruby's own syntax; nothing is
//! translated into another engine's dialect.

use super::*;
use std::borrow::Cow;

/// Builds the Oniguruma engine over the Ruby `source` -- onig speaks Ruby's
/// regex dialect natively through `Syntax::ruby()` (inline flag groups, line
/// anchors, the absence operator, `\Z`/`\z`/`\A`/`\G`, octal, in-pattern
/// backreferences, named-group capture suppression). Ruby's `/m` (dot
/// matches newline) maps to onig's `MULTILINE`; `^`/`$` are line anchors by
/// default under `Syntax::ruby()`.
fn build_onig(
    source: &str,
    ignore_case: bool,
    extended: bool,
    multiline: bool,
) -> Result<onig::Regex, String> {
    let mut opts = onig::RegexOptions::REGEX_OPTION_NONE;
    if ignore_case {
        opts |= onig::RegexOptions::REGEX_OPTION_IGNORECASE;
    }
    if extended {
        opts |= onig::RegexOptions::REGEX_OPTION_EXTEND;
    }
    if multiline {
        opts |= onig::RegexOptions::REGEX_OPTION_MULTILINE;
    }
    onig::Regex::with_options(source, opts, onig::Syntax::ruby()).map_err(|e| e.to_string())
}

/// The pattern with every zero-width atom -- a lookaround group, `^`, `$`,
/// `\b \B \A \z \Z \G` -- wrapped in an atomic group.
///
/// Oniguruma refuses a zero-width atom as a repeat target; ruby's Onigmo is
/// built with `USE_NO_INVALID_QUANTIFIER` and repeats it. `(?>X)` is a target
/// Oniguruma accepts, and it is exact: a lookaround is atomic in Onigmo and an
/// anchor never backtracks. Nothing inside a lookbehind is wrapped, because an
/// atomic group is not a valid lookbehind member.
fn wrap_zero_width(source: &str, extended: bool) -> String {
    let b = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len() + 32);
    // One entry per open group: (its `)` also closes a wrap, it is a lookbehind).
    let mut groups: Vec<(bool, bool)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let in_lookbehind = groups.iter().any(|&(_, lookbehind)| lookbehind);
        let c = b[i];
        match c {
            b'\\' if i + 1 < b.len() => {
                let e = b[i + 1];
                if matches!(e, b'b' | b'B' | b'A' | b'z' | b'Z' | b'G') && !in_lookbehind {
                    out.extend_from_slice(&[b'(', b'?', b'>', b'\\', e, b')']);
                } else {
                    out.extend_from_slice(&b[i..i + 2]);
                }
                i += 2;
            }
            b'^' | b'$' if !in_lookbehind => {
                out.extend_from_slice(&[b'(', b'?', b'>', c, b')']);
                i += 1;
            }
            b'[' => {
                let end = class_end(b, i);
                out.extend_from_slice(&b[i..end]);
                i = end;
            }
            b'#' if extended => {
                let end = b[i..].iter().position(|&x| x == b'\n').map_or(b.len(), |p| i + p);
                out.extend_from_slice(&b[i..end]);
                i = end;
            }
            b'(' if b[i..].starts_with(b"(?#") => {
                let end = b[i..].iter().position(|&x| x == b')').map_or(b.len(), |p| i + p + 1);
                out.extend_from_slice(&b[i..end]);
                i = end;
            }
            b'(' => {
                let rest = &b[i..];
                let lookbehind = rest.starts_with(b"(?<=") || rest.starts_with(b"(?<!");
                let lookahead = rest.starts_with(b"(?=") || rest.starts_with(b"(?!");
                let wrap = (lookbehind || lookahead) && !in_lookbehind;
                if wrap {
                    out.extend_from_slice(b"(?>");
                }
                groups.push((wrap, lookbehind));
                out.push(b'(');
                i += 1;
            }
            b')' => {
                out.push(b')');
                if let Some((true, _)) = groups.pop() {
                    out.push(b')');
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    // Every byte came from `source` or is ASCII, so this is UTF-8 whenever the
    // input was.
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Group names Onigmo reads and Oniguruma refuses.
///
/// Onigmo, as ruby builds it, takes ANY first character in a group name and
/// ends the name at its closing `>`/`'`; a `)` after the first character ends
/// it too, which is an error naming the rest of the pattern. Oniguruma wants a
/// word character first, so a name that opens with `(` or `)` is renamed here
/// -- its definition and every reference -- and the engine maps it back
/// ([`Engine::with_renames`]). Answers `(engine name, name as written)` pairs.
fn rename_groups(source: &str) -> Result<(Cow<'_, str>, Vec<(String, String)>), String> {
    if !["(?<", "(?'", "\\k", "\\g", "(?("].iter().any(|p| source.contains(p)) {
        return Ok((Cow::Borrowed(source), Vec::new()));
    }
    let b = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut renames: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let rest = &b[i..];
        // The opener's length and the name's closing delimiter, when a group
        // name starts after it.
        let opener = if rest[0] == b'\\' {
            match (rest.get(1), rest.get(2)) {
                (Some(b'k' | b'g'), Some(b'<')) => Some((3, b'>')),
                (Some(b'k' | b'g'), Some(b'\'')) => Some((3, b'\'')),
                _ => {
                    let end = (i + 2).min(b.len());
                    out.extend_from_slice(&b[i..end]);
                    i = end;
                    continue;
                }
            }
        } else if rest[0] == b'[' {
            let end = class_end(b, i);
            out.extend_from_slice(&b[i..end]);
            i = end;
            continue;
        } else if rest.starts_with(b"(?<") && !matches!(rest.get(3), Some(b'=' | b'!')) {
            Some((3, b'>'))
        } else if rest.starts_with(b"(?'") {
            Some((3, b'\''))
        } else if rest.starts_with(b"(?(<") {
            Some((4, b'>'))
        } else if rest.starts_with(b"(?('") {
            Some((4, b'\''))
        } else {
            None
        };
        let Some((prefix, close)) = opener else {
            out.push(b[i]);
            i += 1;
            continue;
        };
        let start = i + prefix;
        let mut j = start;
        if j < b.len() && b[j] != close {
            j += utf8_width(b[j]);
        }
        while j < b.len() && b[j] != close && b[j] != b')' {
            j += 1;
        }
        if b.get(j) == Some(&b')') {
            return Err(format!("invalid group name <{}>", &source[start..]));
        }
        let name = &source[start..j.min(b.len())];
        // A reference may carry a nesting level, `\k<n+1>`.
        let bare = name
            .rfind(['+', '-'])
            .filter(|&at| at > 0 && name[at + 1..].bytes().all(|d| d.is_ascii_digit()) && at + 1 < name.len())
            .map_or(name, |at| &name[..at]);
        out.extend_from_slice(&b[i..start]);
        if bare.starts_with(['(', ')']) {
            let engine_name = match renames.iter().find(|(_, w)| w == bare) {
                Some((e, _)) => e.clone(),
                None => {
                    let mut e = format!("zeo_group_{}", renames.len());
                    while source.contains(&e) {
                        e.push('_');
                    }
                    renames.push((e.clone(), bare.to_string()));
                    e
                }
            };
            out.extend_from_slice(engine_name.as_bytes());
            out.extend_from_slice(&name.as_bytes()[bare.len()..]);
        } else {
            out.extend_from_slice(name.as_bytes());
        }
        i = j.min(b.len());
    }
    if renames.is_empty() {
        return Ok((Cow::Borrowed(source), renames));
    }
    // Every byte came from `source` or is ASCII.
    let text = String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    Ok((Cow::Owned(text), renames))
}

/// The length of the UTF-8 sequence a lead byte opens.
fn utf8_width(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// The index just past the bracket class opening at `start` (the end of the
/// input when it never closes). A `]` first, after an optional `^`, is literal.
fn class_end(b: &[u8], start: usize) -> usize {
    let mut j = start + 1;
    if b.get(j) == Some(&b'^') {
        j += 1;
    }
    if b.get(j) == Some(&b']') {
        j += 1;
    }
    let mut depth = 1usize;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    b.len()
}

/// `read_escaped_byte`: one `\M-X` / `\C-X` / `\cX` run, as the BYTE it
/// stands for. `pos` is left just past what was consumed.
///
/// The prefixes nest (`\M-\C-a`), each may be written at most once, and the
/// inner escape may be any of the ordinary byte escapes -- so this is one
/// loop with two sticky flags rather than three separate readers.
fn read_escaped_byte(b: &[u8], pos: &mut usize) -> Result<u8, &'static str> {
    if b.get(*pos) != Some(&b'\\') {
        return Err("too short escaped multibyte character");
    }
    *pos += 1;
    let (mut meta, mut ctrl) = (false, false);
    let code: i32 = loop {
        let Some(&c) = b.get(*pos) else {
            return Err("too short escape sequence");
        };
        *pos += 1;
        match c {
            b'\\' => break i32::from(b'\\'),
            b'n' => break 0x0a,
            b't' => break 0x09,
            b'r' => break 0x0d,
            b'f' => break 0x0c,
            b'v' => break 0x0b,
            b'a' => break 0x07,
            b'e' => break 0x1b,
            b'0'..=b'7' => {
                *pos -= 1;
                let mut v = 0i32;
                let mut n = 0;
                while n < 3 && matches!(b.get(*pos), Some(&d) if d.is_ascii_digit() && d < b'8') {
                    v = v * 8 + i32::from(b[*pos] - b'0');
                    *pos += 1;
                    n += 1;
                }
                break v;
            }
            b'x' => {
                let mut v = 0i32;
                let mut n = 0;
                while n < 2 && matches!(b.get(*pos), Some(d) if d.is_ascii_hexdigit()) {
                    let d = (b[*pos] as char).to_digit(16).expect("hex digit");
                    v = v * 16 + d as i32;
                    *pos += 1;
                    n += 1;
                }
                if n < 1 {
                    return Err("invalid hex escape");
                }
                break v;
            }
            b'M' => {
                if meta {
                    return Err("duplicate meta escape");
                }
                meta = true;
                if b.get(*pos) == Some(&b'-')
                    && let Some(&next) = b.get(*pos + 1)
                    && next & 0x80 == 0
                {
                    *pos += 1;
                    if next == b'\\' {
                        *pos += 1;
                        continue;
                    }
                    *pos += 1;
                    break i32::from(next);
                }
                return Err("too short meta escape");
            }
            b'C' | b'c' => {
                if c == b'C' {
                    if b.get(*pos) != Some(&b'-') {
                        return Err("too short control escape");
                    }
                    *pos += 1;
                }
                if ctrl {
                    return Err("duplicate control escape");
                }
                ctrl = true;
                if let Some(&next) = b.get(*pos)
                    && next & 0x80 == 0
                {
                    if next == b'\\' {
                        *pos += 1;
                        continue;
                    }
                    *pos += 1;
                    break i32::from(next);
                }
                return Err("too short control escape");
            }
            _ => return Err("unexpected escape sequence"),
        }
    };
    if !(0..=0xff).contains(&code) {
        return Err("invalid escape code");
    }
    let mut code = code as u8;
    if ctrl {
        code &= 0x1f;
    }
    if meta {
        code |= 0x80;
    }
    Ok(code)
}

/// `unescape_escaped_nonascii`: the escape run at `pos`, decoded and spliced
/// into `out` as the character it names.
///
/// A byte past 0x7f cannot stand alone in a multi-byte encoding, so CRuby
/// keeps reading escapes until the bytes form a whole character -- which is
/// why `\M-a` (one byte, 0xE1) is "too short" in UTF-8 and fine in binary.
/// A byte the encoding can never start is "invalid" rather than "too short".
fn splice_escaped_char(
    b: &[u8],
    pos: &mut usize,
    binary: bool,
    out: &mut Vec<u8>,
) -> Result<(), &'static str> {
    let mut bytes = vec![read_escaped_byte(b, pos)?];
    if !binary {
        while !bytes.is_empty() && bytes.len() < 4 {
            match std::str::from_utf8(&bytes) {
                Ok(_) => break,
                Err(e) if e.error_len().is_some() => return Err("invalid multibyte escape"),
                // Incomplete: the next escape has to supply the rest.
                Err(_) => bytes.push(read_escaped_byte(b, pos)?),
            }
        }
        if std::str::from_utf8(&bytes).is_err() {
            return Err("invalid multibyte escape");
        }
    }
    // A high byte (or a whole multi-byte character) goes in AS BYTES; an
    // ASCII one is rewritten `\xNN`, so the engine reads a literal rather
    // than a metacharacter.
    match bytes.len() > 1 || bytes[0] & 0x80 != 0 {
        true => out.extend_from_slice(&bytes),
        false => out.extend_from_slice(format!("\\x{:02X}", bytes[0]).as_bytes()),
    }
    Ok(())
}

/// CRuby's `unescape_nonascii`, for the three escapes ruby decodes ITSELF.
/// Onig has its own reading of `\M-`/`\C-`/`\c` and it is not ruby's: ruby
/// decodes them here, before the engine ever sees the pattern, and refuses a
/// byte its encoding cannot hold.
fn preprocess_control_escapes(
    source: &str,
    binary: bool,
) -> Result<std::borrow::Cow<'_, str>, &'static str> {
    if !source.contains('\\') {
        return Ok(std::borrow::Cow::Borrowed(source));
    }
    let b = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut touched = false;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        match b[i + 1] {
            b'M' | b'C' | b'c' => {
                touched = true;
                splice_escaped_char(b, &mut i, binary, &mut out)?;
            }
            // Any other escape passes through WHOLE, so a `\\M` is a literal
            // backslash followed by an M rather than a meta escape.
            other => {
                out.push(b'\\');
                out.push(other);
                i += 2;
            }
        }
    }
    if !touched {
        return Ok(std::borrow::Cow::Borrowed(source));
    }
    match String::from_utf8(out) {
        Ok(s) => Ok(std::borrow::Cow::Owned(s)),
        // Binary bytes the pattern now carries; the engine takes them as
        // bytes, and a lossy rendering is the only way to hand them on.
        Err(e) => Ok(std::borrow::Cow::Owned(
            String::from_utf8_lossy(e.as_bytes()).into_owned(),
        )),
    }
}

/// Rewrite an onig compile error into CRuby's `RegexpError` message shape
/// (`<reason>: /<source>/<flags>`). Onig's own texts are Onigmo's -- ruby's
/// -- for nearly every reason; the arms below cover the few places the two
/// forks spell one differently, and everything else keeps onig's first line.
fn cruby_regex_error(
    source: &str,
    raw: &str,
    ignore_case: bool,
    extended: bool,
    multiline: bool,
) -> String {
    // Ruby closes the message with the pattern AS WRITTEN, flags and all, in
    // its own `mix` order -- `/[z-a]/mi`, never `/[z-a]/im`.
    let mut flags = String::new();
    for (on, ch) in [(multiline, 'm'), (ignore_case, 'i'), (extended, 'x')] {
        if on {
            flags.push(ch);
        }
    }
    let flagged = format!("{source}/{flags}");
    // The onig crate stamps its own name on every message; ruby's text is
    // what follows.
    let raw = raw.strip_prefix("Oniguruma error: ").unwrap_or(raw);
    // `(?` alone ends INSIDE a group's opening sequence, which onig names
    // differently from a group that opened and never closed.
    let unterminated_group_head = source.ends_with("(?");
    let reason = if unterminated_group_head {
        "end pattern in group"
    } else if raw.contains("invalid character property name") {
        // The NAME comes off the source rather than out of the engine's
        // text, so the message survives whatever wording the engine
        // chooses. CRuby's own check is a lookup table; the property list
        // is far too long to carry one here just to name what failed, and
        // the source always has the answer.
        let name = property_name_in(source).unwrap_or_default();
        return format!("invalid character property name {{{name}}}: /{flagged}");
    } else if raw.contains("unknown group name") {
        let name = backref_name_in(source).unwrap_or_default();
        return format!("undefined name <{name}> reference: /{flagged}");
    } else if raw.contains("invalid backref") {
        "invalid backref number/name"
    } else {
        // Onig's own wording, which IS ruby's. Keep the first line: ruby's
        // messages never spill across the page.
        raw.lines().next().unwrap_or(raw).trim_end()
    };
    format!("{reason}: /{flagged}")
}

/// The name inside the first `\\p{...}` of `source`, `^` negation stripped --
/// what CRuby's message quotes.
fn property_name_in(source: &str) -> Option<String> {
    let at = source.find(r"\p{").or_else(|| source.find(r"\P{"))?;
    let rest = &source[at + 3..];
    let end = rest.find('}')?;
    Some(rest[..end].trim_start_matches('^').to_string())
}

/// The name inside the first `\\k<...>` of `source`.
fn backref_name_in(source: &str) -> Option<String> {
    let at = source.find(r"\k<")?;
    let rest = &source[at + 3..];
    let end = rest.find('>')?;
    Some(rest[..end].to_string())
}

/// `/a{2,1}/` -- a repeat range whose upper bound is below its lower. CRuby
/// refuses it. Scans for `{m,n}` outside a character class and outside an
/// escape, exactly where a quantifier can appear.
fn validate_repeat_ranges(source: &str) -> Result<(), String> {
    let bytes = source.as_bytes();
    let (mut i, mut in_class) = (0usize, false);
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'[' if !in_class => in_class = true,
            b']' if in_class => in_class = false,
            b'{' if !in_class => {
                if let Some(end) = source[i..].find('}') {
                    let body = &source[i + 1..i + end];
                    if let Some((lo, hi)) = body.split_once(',')
                        && let (Ok(lo), Ok(hi)) =
                            (lo.trim().parse::<u32>(), hi.trim().parse::<u32>())
                        && lo > hi
                    {
                        return Err(format!(
                            "upper is smaller than lower in repeat range: /{source}/"
                        ));
                    }
                    i += end;
                }
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

/// One non-interpolated regexp LITERAL's compiled form, built on first
/// evaluation and kept.
///
/// Ruby compiles a static literal once per SITE: `2.times { p /a/.object_id }`
/// prints one id twice, while two textually identical literals written in two
/// places are two objects (`/a/.equal?(/a/)` is false). A `static` beside the
/// site is exactly that scope -- which is why this is a per-site cell and not
/// a content-keyed pool like `__LITS`. Frozen strings ARE deduped by content;
/// regexps are not.
///
/// A pattern that fails to compile is not cached -- it raises, and the raise
/// is per evaluation, which is what the literal's documented parse-time
/// approximation already promised.
pub struct RegexpSite(std::sync::OnceLock<RRegexp>);

impl Default for RegexpSite {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexpSite {
    pub const fn new() -> Self {
        RegexpSite(std::sync::OnceLock::new())
    }

    pub fn get(
        &self,
        source: &str,
        ignore_case: bool,
        extended: bool,
        multiline: bool,
        encoding: zeo_abi::RegexpEncoding,
    ) -> Result<RubyValue, String> {
        if let Some(re) = self.0.get() {
            return Ok(RubyValue::Regexp(re.clone()));
        }
        let re = regexp_new_enc(source, ignore_case, extended, multiline, encoding)?;
        // Frozen at birth, real Ruby since 3.0 -- and set BEFORE publishing,
        // so no reader can observe the literal unfrozen.
        re.set_frozen();
        Ok(RubyValue::Regexp(self.0.get_or_init(|| re).clone()))
    }
}

pub fn regexp_new(
    source: &str,
    ignore_case: bool,
    extended: bool,
    multiline: bool,
) -> Result<RRegexp, String> {
    regexp_new_enc(
        source,
        ignore_case,
        extended,
        multiline,
        zeo_abi::RegexpEncoding::Source,
    )
}

/// CRuby's `rb_reg_preprocess` for the `\u` escapes, which the ENGINE never
/// sees: `\u{...}` is a SPACE-SEPARATED LIST of codepoints (`/\u{61 62}/` is
/// `/ab/`) and `\uHHHH` is exactly four hex digits, and both are expanded
/// before compilation.
///
/// An ASCII codepoint becomes `\xHH`, not the character: `re.c::append_utf8`
/// writes it that way so `\u{2e}` stays a literal dot rather than becoming
/// the metacharacter. Above ASCII the character goes in raw.
///
/// `#source` keeps the original text, so this is engine input only.
fn preprocess_unicode(source: &str) -> Result<std::borrow::Cow<'_, str>, String> {
    if !source.contains("\\u") {
        return Ok(std::borrow::Cow::Borrowed(source));
    }
    let b = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let push_code = |out: &mut String, code: u32| -> Result<(), String> {
        let ch = char::from_u32(code).ok_or_else(|| "invalid Unicode range".to_string())?;
        if code < 0x80 {
            out.push_str(&format!("\\x{code:02X}"));
        } else {
            out.push(ch);
        }
        Ok(())
    };
    while i < b.len() {
        if b[i] != b'\\' {
            let ch = source[i..].chars().next().expect("a char boundary");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        if b.get(i + 1) != Some(&b'u') {
            out.push('\\');
            match source[i + 1..].chars().next() {
                Some(ch) => {
                    out.push(ch);
                    i += 1 + ch.len_utf8();
                }
                None => i += 1,
            }
            continue;
        }
        i += 2;
        if b.get(i) == Some(&b'{') {
            i += 1;
            let mut any = false;
            loop {
                while b.get(i).is_some_and(u8::is_ascii_whitespace) {
                    i += 1;
                }
                let start = i;
                while b.get(i).is_some_and(u8::is_ascii_hexdigit) {
                    i += 1;
                }
                if i == start {
                    break;
                }
                if i - start > 6 {
                    return Err("invalid Unicode range".into());
                }
                let code = u32::from_str_radix(&source[start..i], 16)
                    .map_err(|_| "invalid Unicode range".to_string())?;
                push_code(&mut out, code)?;
                any = true;
            }
            if !any || b.get(i) != Some(&b'}') {
                return Err("invalid Unicode list".into());
            }
            i += 1;
        } else {
            if i + 4 > b.len() || !b[i..i + 4].iter().all(u8::is_ascii_hexdigit) {
                return Err("invalid Unicode escape".into());
            }
            let code = u32::from_str_radix(&source[i..i + 4], 16).expect("four hex digits");
            push_code(&mut out, code)?;
            i += 4;
        }
    }
    Ok(std::borrow::Cow::Owned(out))
}

/// `regexp_new` for a LITERAL, which may carry a forced encoding (`/n`, `/e`,
/// `/s`, `/u`). The flag changes nothing about matching -- the pattern is
/// ASCII-only wherever the encoding would otherwise differ, which lowering
/// enforces -- only what the regexp reports about itself.
///
/// Returns a plain `String` error message (not a `Signal`/`RubyValue`) -- the
/// catchable `RegexpError` VALUE is constructed by the `capi::literals`
/// wrappers (`zeo_rt_regexp_lit`/`zeo_rt_regexp_interp`).
pub fn regexp_new_enc(
    source: &str,
    ignore_case: bool,
    extended: bool,
    multiline: bool,
    encoding: zeo_abi::RegexpEncoding,
) -> Result<RRegexp, String> {
    // Oniguruma reads `a{2,1}` as a repeat; Onigmo refuses it.
    validate_repeat_ranges(source)?;
    // The ENGINE sees the expanded pattern; `#source` and every error message
    // keep the text as written.
    let preprocessed = preprocess_unicode(source).map_err(|e| format!("{e}: /{source}/"))?;
    let written = source;
    // `\M-`/`\C-`/`\c` are ruby's, not the engine's -- decoded here, with the
    // pattern's own encoding deciding whether the byte they name is a whole
    // character. A `/n` regexp holds any single byte; a UTF-8 one does not.
    let binary = matches!(encoding, zeo_abi::RegexpEncoding::None);
    let escaped = preprocess_control_escapes(preprocessed.as_ref(), binary)
        .map_err(|e| cruby_regex_error(written, e, ignore_case, extended, multiline))?;
    // Onigmo's character-range modes (`\w` ASCII, `\b` Unicode, `(?a)`,
    // `(?u)`) are Oniguruma's neither; the walk in `charrange` writes them
    // into the pattern text.
    // Before the range walk, which would read a `)` inside a name as a group.
    let (renamed, renames) = rename_groups(escaped.as_ref())
        .map_err(|e| cruby_regex_error(written, &e, ignore_case, extended, multiline))?;
    let ranged = super::charrange::apply(renamed.as_ref(), extended)
        .map_err(|e| cruby_regex_error(written, e, ignore_case, extended, multiline))?;
    let engine = match build_onig(ranged.as_ref(), ignore_case, extended, multiline) {
        Err(e) if e.contains("target of repeat operator is invalid") => {
            let wrapped = wrap_zero_width(ranged.as_ref(), extended);
            build_onig(&wrapped, ignore_case, extended, multiline).map_err(|_| e)
        }
        built => built,
    }
    .map_err(|e| cruby_regex_error(written, &e, ignore_case, extended, multiline))?;
    Ok(Arc::new(RegexpData {
        engine: Engine::new(engine).with_renames(renames),
        source: written.to_string(),
        uninitialized: false,
        ignore_case,
        extended,
        multiline,
        encoding,
        frozen: std::sync::atomic::AtomicBool::new(false),
    }))
}

/// Every named capture group's `(name, 1-based index)`, in source order,
/// INCLUDING duplicates: `(?<a>.)(?<a>.)` yields `[("a", 1), ("a", 2)]`. The
/// engine's own `capture_names` collapses a repeated name onto a single slot,
/// so `Regexp#names`/`#named_captures` parse the source to see every position.
/// Skips `\(`, character classes, and non-capturing `(?...)` groups.
pub(crate) fn named_group_positions(pattern: &str) -> Vec<(String, usize)> {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut count = 0;
    let mut in_class = false;
    let mut class_start = false;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if in_class {
            match c {
                b']' if !class_start => in_class = false,
                b'^' if class_start => {}
                b'\\' if i + 1 < bytes.len() => {
                    class_start = false;
                    i += 2;
                    continue;
                }
                _ => class_start = false,
            }
            i += 1;
            continue;
        }
        match c {
            b'[' => {
                in_class = true;
                class_start = true;
            }
            b'\\' if i + 1 < bytes.len() => {
                i += 2;
                continue;
            }
            b'(' => {
                if bytes.get(i + 1) == Some(&b'?') {
                    // Named forms `(?<name>`, `(?'name'`, `(?P<name>` capture and
                    // carry a name; every other `(?...)` neither counts nor names.
                    let named = if bytes.get(i + 2) == Some(&b'\'') {
                        Some((i + 3, b'\''))
                    } else if bytes.get(i + 2) == Some(&b'<')
                        && !matches!(bytes.get(i + 3), Some(b'=') | Some(b'!'))
                    {
                        Some((i + 3, b'>'))
                    } else if bytes.get(i + 2) == Some(&b'P') && bytes.get(i + 3) == Some(&b'<') {
                        Some((i + 4, b'>'))
                    } else {
                        None
                    };
                    if let Some((name_start, delim)) = named {
                        count += 1;
                        let mut j = name_start;
                        while j < bytes.len() && bytes[j] != delim {
                            j += 1;
                        }
                        if let Ok(name) = std::str::from_utf8(&bytes[name_start..j]) {
                            out.push((name.to_string(), count));
                        }
                    }
                } else {
                    count += 1; // a plain unnamed capturing group
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_width_atom_is_wrapped_atomic_outside_a_lookbehind() {
        assert_eq!(wrap_zero_width(r"(?:(?!a))*b?", false), r"(?:(?>(?!a)))*b?");
        assert_eq!(wrap_zero_width(r"^\b[$^\b]\$", false), r"(?>^)(?>\b)[$^\b]\$");
        assert_eq!(wrap_zero_width(r"(?<=a\b)c", false), r"(?>(?<=a\b))c");
        assert_eq!(wrap_zero_width(r"(?<n>x)(?#^)", false), r"(?<n>x)(?#^)");
        assert_eq!(wrap_zero_width("# ^\n$", true), "# ^\n(?>$)");
    }

    #[test]
    fn a_name_opening_with_a_paren_is_renamed_everywhere() {
        let (text, renames) = rename_groups(r"(?<)>x)\k<)+0>\g')'(?(<)>)y|z)(?<a>b)\k<a>").unwrap();
        assert_eq!(text, r"(?<zeo_group_0>x)\k<zeo_group_0+0>\g'zeo_group_0'(?(<zeo_group_0>)y|z)(?<a>b)\k<a>");
        assert_eq!(renames, vec![("zeo_group_0".to_string(), ")".to_string())]);
        assert_eq!(rename_groups(r"(?<a)>x)yz").unwrap_err(), "invalid group name <a)>x)yz>");
        assert!(matches!(rename_groups(r"[(?<)>](?<n>.)"), Ok((Cow::Borrowed(_), _))));
    }

    #[test]
    fn invalid_pattern_is_a_plain_string_error_not_a_panic() {
        assert!(regexp_new("(", false, false, false).is_err());
    }

    /// The pattern the engine compiles is the pattern as written, plus
    /// ruby's own two preprocessing steps; there is no second dialect.
    #[test]
    fn ruby_only_constructs_compile_and_match() {
        for (pattern, subject) in [
            (r"(\w)\1", "hello"),
            (r"(?<=\$)\d+", "$100"),
            (r"a++b", "aab"),
            (r"(?~abc)", "xyz"),
            (r"(?<n>a)\k<n>", "aa"),
            (r"\Ax\Z", "x\n"),
            (r"[[:alpha:]]+", "héllo"),
            (r"\u{61 62}", "ab"),
            (r"\C-a", "\u{1}"),
        ] {
            let re = regexp_new(pattern, false, false, false)
                .unwrap_or_else(|e| panic!("{pattern}: {e}"));
            assert!(
                regexp_is_match(&re, subject).unwrap(),
                "{pattern} on {subject:?}"
            );
        }
    }
}
