//! Ruby-regex to Rust-engine translation and Regexp construction: the
//! escape/POSIX-class rewriter, the fast/fancy/onig engine choice, the
//! CRuby-shaped syntax validation, and the construction entry points
//! (`regexp_new`, `regexp_new_enc`, the per-site `RegexpSite` cache).

use super::*;

/// Rewrites the Ruby-flavoured escapes the Rust `regex` crate doesn't
/// recognise into equivalents it does, leaving everything else byte-for-byte
/// untouched. Today that is just `\e` (Ruby's ESC, U+001B) -> `\x1b`; the
/// crate already accepts `\a \f \n \r \t \v` and escaped metacharacters. A
/// backslash always consumes the character after it, so `\\e` (an escaped
/// backslash followed by a literal `e`) is left alone, as is an `e` inside a
/// character class that isn't preceded by a backslash.
fn translate_ruby_escapes(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    // A character class changes escape meaning: `\1`..`\7` become OCTAL (not a
    // backreference), and `\k`/`\g` are literal letters (Onigmo) -- both of
    // which the linear `regex` crate rejects verbatim, so they are rewritten.
    // `]` closes the class unless it is the first member (`[]` / `[^]`), and
    // `\]` inside the class is an escaped literal.
    let mut in_class = false;
    let mut class_start = false;
    while let Some(c) = chars.next() {
        if !in_class {
            match c {
                '[' => {
                    in_class = true;
                    class_start = true;
                    out.push('[');
                }
                '\\' => translate_escape_outside(&mut out, &mut chars),
                _ => out.push(c),
            }
            continue;
        }
        match c {
            ']' if !class_start => {
                in_class = false;
                out.push(']');
            }
            // A leading `^` keeps the "next `]` is literal" rule alive (`[^]`).
            '^' if class_start => out.push('^'),
            '[' if chars.peek() == Some(&':') => {
                class_start = false;
                match take_posix_class(&mut chars) {
                    Some(expansion) => out.push_str(&expansion),
                    None => out.push('['),
                }
            }
            '\\' => {
                class_start = false;
                translate_escape_in_class(&mut out, &mut chars);
            }
            _ => {
                class_start = false;
                out.push(c);
            }
        }
    }
    out
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// The Unicode set a POSIX bracket class denotes, as a bare class body. Ruby's
/// `[[:alpha:]]` matches any Unicode letter; the `regex` crate's own POSIX
/// classes are ASCII-only, so they are spelled out as properties instead.
/// `None` leaves the source untouched, which sends the pattern to Onig.
fn posix_class_expansion(name: &str, negated: bool) -> Option<String> {
    let (p, n) = if negated {
        ("\\P", "\\p")
    } else {
        ("\\p", "\\P")
    };
    Some(match name {
        "alpha" => format!("{p}{{Alphabetic}}"),
        "upper" => format!("{p}{{Uppercase}}"),
        "lower" => format!("{p}{{Lowercase}}"),
        "digit" => format!("{p}{{Nd}}"),
        "space" => format!("{p}{{White_Space}}"),
        "cntrl" => format!("{p}{{Cc}}"),
        "alnum" if !negated => "\\p{Alphabetic}\\p{Nd}".to_string(),
        "punct" if !negated => "\\p{P}\\p{S}".to_string(),
        "word" if !negated => "\\p{Alphabetic}\\p{M}\\p{Nd}\\p{Pc}".to_string(),
        "blank" if !negated => "\\p{Zs}\\t".to_string(),
        "graph" if !negated => "^\\p{Z}\\p{C}".to_string(),
        "print" if !negated => format!("{n}{{C}}"),
        // ASCII in Ruby too, so these need no widening.
        "xdigit" if !negated => "0-9A-Fa-f".to_string(),
        "ascii" if !negated => "\\x00-\\x7F".to_string(),
        _ => return None,
    })
}

/// Consumes a `[:name:]` / `[:^name:]` sequence (the opening `[` is already
/// eaten) and answers its expansion. Leaves `chars` untouched on anything that
/// isn't a well-formed POSIX class.
fn take_posix_class(chars: &mut Chars) -> Option<String> {
    let mut probe = chars.clone();
    probe.next()?; // the ':'
    let negated = probe.peek() == Some(&'^');
    if negated {
        probe.next();
    }
    let mut name = String::new();
    loop {
        match probe.next()? {
            ':' if probe.peek() == Some(&']') => {
                probe.next();
                break;
            }
            c if c.is_ascii_alphabetic() => name.push(c),
            _ => return None,
        }
    }
    let expansion = posix_class_expansion(&name, negated)?;
    *chars = probe;
    Some(expansion)
}

/// Escape translation OUTSIDE a character class: `\e` -> ESC, `\0...` octal
/// (a leading zero is never a backreference), everything else (`\1`..`\9`
/// backrefs, `\k`, `\d`, ...) passes through for the engine to interpret.
/// The ASCII expansion of a perl-style class escape, as a bare set body (no
/// brackets) plus whether it is the negated form. Ruby's `\w`/`\d`/`\s`/`\h`
/// are ASCII-only; both Rust engines read them as Unicode by default, so they
/// have to be spelled out rather than passed through.
fn ascii_class_body(c: char) -> Option<(&'static str, bool)> {
    match c {
        'w' => Some(("0-9A-Za-z_", false)),
        'W' => Some(("0-9A-Za-z_", true)),
        'd' => Some(("0-9", false)),
        'D' => Some(("0-9", true)),
        // Ruby's `\s` has included \v since 2.0. The space is spelled `\x20`
        // rather than written literally because the `regex` crate's EXTENDED
        // mode strips whitespace inside a character class too -- so a literal
        // one here would leave `/\s/x` unable to match a space.
        's' => Some(("\\x20\\t\\r\\n\\x0b\\x0c", false)),
        'S' => Some(("\\x20\\t\\r\\n\\x0b\\x0c", true)),
        'h' => Some(("0-9A-Fa-f", false)),
        'H' => Some(("0-9A-Fa-f", true)),
        _ => None,
    }
}

fn translate_escape_outside(out: &mut String, chars: &mut Chars) {
    match chars.next() {
        Some('e') => out.push_str("\\x1b"),
        Some('0') => push_octal(out, 0, chars),
        Some(c) if ascii_class_body(c).is_some() => {
            let (body, negated) = ascii_class_body(c).expect("checked above");
            out.push('[');
            if negated {
                out.push('^');
            }
            out.push_str(body);
            out.push(']');
        }
        // `\b` is an ASCII word boundary in Ruby; `(?-u:...)` scopes the
        // assertion's own word definition without letting the pattern match
        // invalid UTF-8 (a zero-width assertion can't).
        Some(c @ ('b' | 'B')) => {
            out.push_str("(?-u:\\");
            out.push(c);
            out.push(')');
        }
        // Ruby `\Z` = end of string, or just before a single trailing newline.
        // Neither Rust engine knows `\Z`; the equivalent lookahead does the
        // same job and (via its `(?=`) routes the pattern to the fancy engine.
        Some('Z') => out.push_str("(?=\\n?\\z)"),
        Some(next) => {
            out.push('\\');
            out.push(next);
        }
        None => out.push('\\'),
    }
}

/// Escape translation INSIDE a character class: `\0`..`\7` are octal, `\k`/`\g`
/// are literal letters (the `regex` crate rejects `\k` in a class), and valid
/// class escapes (`\d` `\w` `\]` `\-` ...) pass through.
fn translate_escape_in_class(out: &mut String, chars: &mut Chars) {
    match chars.next() {
        Some('e') => out.push_str("\\x1b"),
        Some(d @ '0'..='7') => push_octal(out, d.to_digit(8).unwrap(), chars),
        Some(c @ ('k' | 'g')) => out.push(c),
        // Inside a class the expansion nests: `[\w-]` -> `[[0-9A-Za-z_]-]`.
        Some(c) if ascii_class_body(c).is_some() => {
            let (body, negated) = ascii_class_body(c).expect("checked above");
            out.push('[');
            if negated {
                out.push('^');
            }
            out.push_str(body);
            out.push(']');
        }
        Some(next) => {
            out.push('\\');
            out.push(next);
        }
        None => out.push('\\'),
    }
}

/// Reads up to two more octal digits after `first` (3 total, Ruby's max) and
/// emits the code point as `\xHH` (or `\x{...}` past 0xff), which both engines
/// understand -- and which keeps `needs_fancy` from mistaking a `\0`-style
/// octal for a backreference.
fn push_octal(out: &mut String, first: u32, chars: &mut Chars) {
    let mut val = first;
    for _ in 0..2 {
        match chars.peek() {
            Some(d @ '0'..='7') => {
                val = val * 8 + d.to_digit(8).unwrap();
                chars.next();
            }
            _ => break,
        }
    }
    if val > 0xff {
        out.push_str(&format!("\\x{{{val:x}}}"));
    } else {
        out.push_str(&format!("\\x{val:02x}"));
    }
}

/// Whether `pattern` uses a construct the linear-time `regex` crate cannot
/// compile, so the backtracking `fancy-regex` engine must back it: an
/// in-pattern backreference (`\1`..`\9`, `\k<name>`), any look-around
/// (`(?=` `(?!` `(?<=` `(?<!`), an atomic group `(?>`, an inline comment
/// `(?#`, or a possessive quantifier (`*+` `++` `?+` `}+`). Scanned on the
/// ALREADY-escape-translated pattern; a `\\` consumes its next char so an
/// escaped backslash before a digit isn't mistaken for a backreference.
/// Whether `pattern` names a captured group again -- `\1`..`\9`, `\k<name>`,
/// `\k'name'`. Under `/i` ruby compares the two FOLDED (`/(a)\1/i` matches
/// `"aA"`), which of this crate's engines only Oniguruma does.
fn has_backref(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if matches!(bytes.get(i + 1), Some(n) if n.is_ascii_digit() || *n == b'k') {
                return true;
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    false
}

/// Whether `pattern` LOOPS a capturing group whose body can match empty --
/// `(a?)*`, `(a*)*`, `(|a)*`.
///
/// Ruby runs one final iteration that consumes nothing, and the group holds
/// the EMPTY string at the position the loop stopped; the Rust engines keep
/// the last non-empty match, so the group and the overall match disagree
/// about where the loop ended. Onig follows ruby's rule (it is ruby's rule),
/// and over-routing to it costs speed and never correctness -- so the body
/// test is deliberately conservative: a body that always CONSUMES
/// (`(a)*`, `(\d+)*`) is left where it was, and that is the common shape.
fn empty_iteration_capture(pattern: &str) -> bool {
    let b = pattern.as_bytes();
    // (body start, capturing)
    let mut open: Vec<(usize, bool)> = Vec::new();
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false;
    while i < b.len() {
        let c = b[i];
        if c == b'\\' {
            i += 2;
            continue;
        }
        if in_class {
            match c {
                b']' if !class_start => in_class = false,
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
            b'(' => {
                let capturing = match b.get(i + 1) {
                    Some(b'?') => {
                        b.get(i + 2) == Some(&b'\'')
                            || (b.get(i + 2) == Some(&b'<')
                                && !matches!(b.get(i + 3), Some(b'=' | b'!')))
                    }
                    _ => true,
                };
                open.push((i + 1, capturing));
            }
            b')' => {
                if let Some((start, capturing)) = open.pop()
                    && capturing
                    && matches!(b.get(i + 1), Some(b'*' | b'+' | b'{'))
                {
                    let body = &pattern[start..i];
                    // A body that can produce nothing: an optional or starred
                    // atom, an empty alternative, or an empty body.
                    if body.is_empty()
                        || body.contains('?')
                        || body.contains('*')
                        || body.starts_with('|')
                        || body.ends_with('|')
                        || body.contains("||")
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn needs_fancy(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if let Some(&n) = bytes.get(i + 1) {
                    // `\1`..`\9` (backref) or `\k` (named backref).
                    if n.is_ascii_digit() || n == b'k' {
                        return true;
                    }
                }
                i += 2; // skip the escaped char
                continue;
            }
            b'(' if bytes.get(i + 1) == Some(&b'?') => match bytes.get(i + 2) {
                Some(b'=') | Some(b'!') | Some(b'>') | Some(b'#') => return true,
                Some(b'<') if matches!(bytes.get(i + 3), Some(b'=') | Some(b'!')) => return true,
                _ => {}
            },
            // Possessive quantifiers: a quantifier immediately followed by `+`.
            b'+' if i > 0 && matches!(bytes[i - 1], b'*' | b'+' | b'?' | b'}') => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Builds the `fancy-regex` engine, applying Ruby's flag semantics via inline
/// flag groups (fancy-regex's builder exposes only case-insensitivity): `(?m)`
/// is unconditional (Ruby's `^`/`$` are always line-anchored), `s` maps Ruby's
/// `/m` (dot matches newline), `x` maps `/x`.
fn build_fancy(
    translated: &str,
    ignore_case: bool,
    extended: bool,
    multiline: bool,
) -> Result<fancy_regex::Regex, String> {
    let mut flags = String::from("m");
    if multiline {
        flags.push('s');
    }
    if ignore_case {
        flags.push('i');
    }
    if extended {
        flags.push('x');
    }
    fancy_regex::Regex::new(&format!("(?{flags}){translated}")).map_err(|e| e.to_string())
}

/// Builds the real Oniguruma engine over the RAW Ruby `source` -- onig speaks
/// Ruby's regex dialect natively through `Syntax::ruby()` (inline flag groups,
/// line anchors, the absence operator, `\Z`/`\z`/`\A`/`\G`, octal, in-pattern
/// backreferences), so NO escape translation is applied. Ruby's `/m` (dot
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

/// Whether `source` uses a Ruby-specific construct that the Rust engines
/// mis-handle, so real Oniguruma must back it: a line anchor `^`/`$` (whose
/// trailing-newline semantics `regex`'s `multi_line` gets wrong), an inline
/// flag group `(?flags)` / `(?flags:...)` / `(?-flags...)` (Ruby's `/m` is
/// DOTALL, not multi-line, and the Rust crates read `m` the opposite way), or
/// the absence operator `(?~...)`. Scanned on the RAW source, honoring escapes
/// and character classes so an escaped `\^`/`\$` or a `[$^]` set never counts.
fn needs_onig(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' {
            // `\b`/`\B` read the WORD definition of the pattern's encoding, so
            // in UTF-8 they see a boundary either side of a Japanese word. The
            // Rust engines have no such rule and the ASCII-scoped stand-in the
            // translation used got it backwards.
            if matches!(bytes.get(i + 1), Some(b'b' | b'B')) && !in_class {
                return true;
            }
            i += 2; // an escaped char is never an anchor / group opener
            continue;
        }
        if in_class {
            match c {
                b']' if !class_start => in_class = false,
                // A POSIX bracket class is encoding-aware in ways the
                // property-set expansion is not: `/[[:lower:]]/i` does NOT
                // case-fold in ruby, and `\p{Lowercase}` under `/i` does.
                b'[' if bytes.get(i + 1) == Some(&b':') => return true,
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
            b'^' | b'$' => return true,
            b'(' if bytes.get(i + 1) == Some(&b'?') => {
                // An inline flag group / absence operator begins with a flag
                // letter, `-`, or `~` right after `(?` -- distinct from the
                // non-capturing/assertion forms (`(?:`, `(?=`, `(?!`, `(?<`,
                // `(?>`, `(?#`, `(?'`, `(?P`).
                if matches!(
                    bytes.get(i + 2),
                    Some(b'i' | b'm' | b'x' | b'a' | b'd' | b'u' | b'-' | b'~')
                ) {
                    return true;
                }
                // A NAMED group -- `(?<name>` (not the `(?<=`/`(?<!`
                // lookbehinds) or `(?'name'` -- flips Ruby's capture rule:
                // plain `(...)` groups stop capturing entirely. Onig's ruby
                // syntax implements that natively; the Rust engines number
                // plain groups regardless, so these patterns go to onig.
                if bytes.get(i + 2) == Some(&b'\'')
                    || (bytes.get(i + 2) == Some(&b'<')
                        && !matches!(bytes.get(i + 3), Some(b'=' | b'!')))
                {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// The POSIX bracket class names Onigmo/CRuby accept inside `[[:name:]]`. An
/// unknown one is a `RegexpError` at compile time, not a silent literal set.
const POSIX_CLASSES: &[&str] = &[
    "alpha", "alnum", "blank", "cntrl", "digit", "graph", "lower", "print", "punct", "space",
    "upper", "xdigit", "word", "ascii",
];

/// Reject a `[[:bogus:]]` with an unknown POSIX class name, matching CRuby's
/// `invalid POSIX bracket type: /<source>/`. Only the `[:name:]` form INSIDE a
/// character class is a POSIX class; a bare `[:name:]` is an ordinary set.
fn validate_posix_classes(source: &str) -> Result<(), String> {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut in_class = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if !in_class {
            if c == b'[' {
                in_class = true;
            }
            i += 1;
            continue;
        }
        if c == b'[' && bytes.get(i + 1) == Some(&b':') {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == b':' && bytes[j + 1] == b']') {
                j += 1;
            }
            if j + 1 < bytes.len() {
                let name = source[start..j]
                    .strip_prefix('^')
                    .unwrap_or(&source[start..j]);
                if !POSIX_CLASSES.contains(&name) {
                    return Err(format!("invalid POSIX bracket type: /{source}/"));
                }
                i = j + 2;
                continue;
            }
        }
        if c == b']' {
            in_class = false;
        }
        i += 1;
    }
    Ok(())
}

/// Rewrite a regex-engine compile error into CRuby's own `RegexpError` message
/// shape (`<reason>: /<source>/`) for the common cases; otherwise keep the
/// engine's text. CRuby names the offending construct and echoes the pattern.
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
    let reason = if raw.contains("unclosed character class") {
        "premature end of char-class"
    } else if unterminated_group_head {
        "end pattern in group"
    } else if raw.contains("unclosed group") || raw.contains("unclosed") && raw.contains("(") {
        "end pattern with unmatched parenthesis"
    } else if raw.contains("unopened group") || raw.contains("unmatched") && raw.contains(")") {
        "unmatched close parenthesis"
    } else if raw.contains("invalid character class range") {
        "empty range in char class"
    } else if raw.contains("repetition operator missing expression") {
        "target of repeat operator is not specified"
    } else if raw.contains("repetition quantifier expects a valid decimal") {
        "invalid repeat range"
    } else if raw.contains("Unknown property name")
        || raw.contains("unknown property")
        || raw.contains("property not found")
        // Onig's own wording, reached when the pattern went to Onig first.
        || raw.contains("invalid character property name")
    {
        // The NAME comes off the source rather than out of the engine's
        // text, so the message survives whatever wording the engine
        // chooses. CRuby's own check is a lookup table; the property list
        // is far too long to carry one here just to name what failed, and
        // the source always has the answer.
        let name = property_name_in(source).unwrap_or_default();
        return format!("invalid character property name {{{name}}}: /{flagged}");
    } else if raw.contains("Invalid group name in back reference")
        || raw.contains("unknown group name")
    {
        let name = backref_name_in(source).unwrap_or_default();
        return format!("undefined name <{name}> reference: /{flagged}");
    } else if raw.contains("Invalid back reference") || raw.contains("invalid backref") {
        "invalid backref number/name"
    } else {
        // Anything else is onig's own wording, which IS ruby's. Keep the
        // first line: ruby's messages never spill across the page.
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
/// refuses it; the `regex` crate accepts it as an empty repetition, so the
/// check has to happen here. Scans for `{m,n}` outside a character class and
/// outside an escape, exactly where a quantifier can appear.
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
/// The cost it removes is the larger half: every evaluation used to run
/// `validate_posix_classes` and a full engine build, inside a hot loop
/// included.
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

/// `regexp_new` for a LITERAL, which may carry a forced encoding (`/n`, `/e`,
/// `/s`, `/u`). The flag changes nothing about matching -- the pattern is
/// ASCII-only wherever the encoding would otherwise differ, which lowering
/// enforces -- only what the regexp reports about itself.
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

/// Builds a real `regex::Regex`, translating Ruby's flag semantics --
/// crucially, Ruby's `^`/`$` ALWAYS match at line boundaries (there is no
/// separate "multi-line mode" opt-in the way most other regex flavors work),
/// so `multi_line(true)` is unconditional here, independent of any of the
/// three `RegexpFlags`. Ruby's `/m` flag instead makes `.` match a newline
/// too -- `regex`'s `dot_matches_new_line`, NOT its own `multi_line` (a
/// same-named-but-different-meaning trap between the two flag vocabularies).
/// `/x` maps directly to `ignore_whitespace`. Returns a plain `String` error
/// message (not a `Signal`/`RubyValue`) -- the catchable `RegexpError`
/// VALUE is constructed by the `capi::literals` wrappers
/// (`zeo_rt_regexp_lit`/`zeo_rt_regexp_interp`).
pub fn regexp_new_enc(
    source: &str,
    ignore_case: bool,
    extended: bool,
    multiline: bool,
    encoding: zeo_abi::RegexpEncoding,
) -> Result<RRegexp, String> {
    validate_posix_classes(source)?;
    validate_repeat_ranges(source)?;
    // The ENGINE sees the expanded pattern; `#source` and every error message
    // keep the text as written.
    let preprocessed = preprocess_unicode(source).map_err(|e| format!("{e}: /{source}/"))?;
    let written = source;
    let source = preprocessed.as_ref();
    // ONIGURUMA DECIDES. It is ruby's own engine, so what it refuses ruby
    // refuses and what it accepts ruby accepts -- no Rust engine may overrule
    // it in either direction. Every pattern compiles here first, and the two
    // faster engines below only ever choose a quicker way to run a pattern
    // onig has already approved.
    let onig = match build_onig(source, ignore_case, extended, multiline) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            return Err(cruby_regex_error(
                written,
                &e,
                ignore_case,
                extended,
                multiline,
            ));
        }
    };
    // A pattern with Ruby-specific semantics runs on onig too.
    if needs_onig(source) || (ignore_case && has_backref(source)) || empty_iteration_capture(source)
    {
        return Ok(Arc::new(RegexpData {
            engine: Engine::Onig(onig),
            source: written.to_string(),
            ignore_case,
            extended,
            multiline,
            encoding,
            frozen: std::sync::atomic::AtomicBool::new(false),
        }));
    }
    let translated = translate_ruby_escapes(source);
    let engine = if needs_fancy(&translated) {
        match build_fancy(&translated, ignore_case, extended, multiline) {
            Ok(r) => Engine::Fancy(r),
            // A forward numbered backreference (`/[\]]\1(a)/`) is valid in
            // Onigmo but rejected by fancy-regex, and onig has already said
            // the pattern is good -- so this never raises, it only picks the
            // engine that can run it.
            Err(_) if forward_backref_only(&translated) => Engine::Unmatchable,
            Err(_) => Engine::Onig(onig),
        }
    } else {
        match regex::RegexBuilder::new(&translated)
            .case_insensitive(ignore_case)
            .ignore_whitespace(extended)
            .dot_matches_new_line(multiline)
            .multi_line(true)
            .build()
        {
            Ok(r) => Engine::Fast(r),
            Err(_) => match build_fancy(&translated, ignore_case, extended, multiline) {
                Ok(r) => Engine::Fancy(r),
                Err(_) => Engine::Onig(onig),
            },
        }
    };
    Ok(Arc::new(RegexpData {
        engine,
        source: written.to_string(),
        ignore_case,
        extended,
        multiline,
        encoding,
        frozen: std::sync::atomic::AtomicBool::new(false),
    }))
}

/// Whether `pattern` (already escape-translated) fails to compile ONLY because
/// of a forward numbered backreference: there is at least one `\N` backref, and
/// every one names a capture group that exists somewhere in the pattern (`N <=`
/// the capture-group count). A backref past the group count is a genuine
/// invalid-backref error and returns false, so `regexp_new` still raises. Named
/// backrefs (`\k<...>`) are treated conservatively as NOT forward-only (they
/// compile in fancy-regex when the name is defined, so a failure is a real
/// error). Character-class contents are octal after translation and are skipped.
fn forward_backref_only(pattern: &str) -> bool {
    let groups = count_capture_groups(pattern);
    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false;
    let mut saw_backref = false;
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
                i += 1;
            }
            b'\\' if i + 1 < bytes.len() => {
                let n = bytes[i + 1];
                if n == b'k' {
                    return false; // a named backref failing is a real error
                }
                if n.is_ascii_digit() && n != b'0' {
                    let mut j = i + 1;
                    let mut num = 0usize;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        num = num * 10 + (bytes[j] - b'0') as usize;
                        j += 1;
                    }
                    if num > groups {
                        return false; // references a group that doesn't exist
                    }
                    saw_backref = true;
                    i = j;
                    continue;
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    saw_backref
}

/// Counts capture groups in `pattern`: an unescaped `(` that is not a
/// non-capturing/assertion group (`(?:`/`(?=`/`(?!`/`(?<=`/`(?<!`/`(?>`/`(?#`/
/// `(?flags)`). A named group (`(?<name>`/`(?'name'`/`(?P<name>`) DOES count.
/// Skips character classes and escaped parens.
fn count_capture_groups(pattern: &str) -> usize {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut count = 0;
    let mut in_class = false;
    let mut class_start = false;
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
                let capturing = if bytes.get(i + 1) == Some(&b'?') {
                    // `(?<name>` / `(?'name'` / `(?P<name>` capture; the rest
                    // (`(?:`, `(?=`, `(?<=`, flags, ...) do not.
                    matches!(bytes.get(i + 2), Some(b'\''))
                        || (bytes.get(i + 2) == Some(&b'<')
                            && !matches!(bytes.get(i + 3), Some(b'=') | Some(b'!')))
                        || (bytes.get(i + 2) == Some(&b'P') && bytes.get(i + 3) == Some(&b'<'))
                } else {
                    true
                };
                if capturing {
                    count += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    count
}

/// Every named capture group's `(name, 1-based index)`, in source order,
/// INCLUDING duplicates: `(?<a>.)(?<a>.)` yields `[("a", 1), ("a", 2)]`. The
/// engine's own `capture_names` collapses a repeated name onto a single slot,
/// so `Regexp#names`/`#named_captures` parse the source to see every position.
/// Shares `count_capture_groups`'s scanning rules (skip `\(`, char classes, and
/// non-capturing `(?...)` groups).
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
    fn needs_fancy_detects_only_unsupported_constructs() {
        // Fancy-only constructs.
        for p in [
            r"(\w)\1",        // backreference
            r"foo(?=bar)",    // lookahead
            r"foo(?!bar)",    // negative lookahead
            r"(?<=\$)\d+",    // lookbehind
            r"(?<!x)y",       // negative lookbehind
            r"(?>ab)",        // atomic group
            r"a(?#note)b",    // inline comment
            r"a++",           // possessive
            r"(?<n>\w)\k<n>", // named backref
        ] {
            assert!(needs_fancy(p), "{p} should need fancy");
        }
        // Plain patterns stay on the fast engine.
        for p in [
            r"\d+",
            r"(?<year>\d{4})", // named GROUP is fine on regex
            r"[a-z]\\1",       // an escaped backslash then literal 1, not a backref
            r"a|b",
            r"(?i)abc", // inline flag, supported by regex
        ] {
            assert!(!needs_fancy(p), "{p} should NOT need fancy");
        }
    }

    #[test]
    fn invalid_pattern_is_a_plain_string_error_not_a_panic() {
        assert!(regexp_new("(", false, false, false).is_err());
    }
}
