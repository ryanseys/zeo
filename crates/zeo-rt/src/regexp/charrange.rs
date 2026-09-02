//! Ruby's character-range modes, which Oniguruma does not have.
//!
//! Onigmo (ruby's fork) reads a pattern under `ONIG_OPTION_ASCII_RANGE`: `\w`,
//! `\d` and `\s` are ASCII-only while `\b`, `\B` and the POSIX brackets are
//! Unicode, and the inline options `(?a)`, `(?u)` and `(?d)` move the line.
//! Oniguruma's `WORD_IS_ASCII` family cannot express that split (it moves
//! `\b` and `\p{Word}` with `\w`), and its Ruby syntax refuses the three
//! letters. So the split is made in the pattern text before the engine sees
//! it, one scope at a time.
//!
//! Two further Onigmo-only rules ride on the same walk:
//! - a bare `\p{...}` case-folds under `/i` in Onigmo; Oniguruma folds only a
//!   bracket class, so a bare property is wrapped in one;
//! - a ctype-derived ASCII member never folds outside ASCII in Onigmo
//!   (`/\w/i` does not match `ſ`), so the rewritten class is wrapped in
//!   `(?-i:...)` outside a bracket. Inside one Oniguruma folds the whole
//!   class, which reaches `ſ` and `K` through `s` and `k`; that is the one
//!   residue, ledgered in docs/COMPATIBILITY.md.
//!
//! `Regexp#source` and every error message keep the text as written; only
//! what the engine compiles changes.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Ruby's default: `\w \d \s` ASCII, `\b` and POSIX brackets Unicode.
    Default,
    /// `(?a)`: everything ASCII.
    Ascii,
    /// `(?u)`: everything Unicode, which is Oniguruma's own default.
    Unicode,
}

#[derive(Clone, Copy)]
struct Frame {
    mode: Mode,
    extended: bool,
}

const WORD: &str = "a-zA-Z0-9_";
const DIGIT: &str = "0-9";
const SPACE: &str = " \\t\\n\\v\\f\\r";

/// The ASCII body of a POSIX bracket name, for `(?a)`.
fn posix_ascii_body(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "a-zA-Z",
        "alnum" => "a-zA-Z0-9",
        "blank" => " \\t",
        "cntrl" => "\\x00-\\x1f\\x7f",
        "digit" => DIGIT,
        "graph" => "\\x21-\\x7e",
        "lower" => "a-z",
        "print" => "\\x20-\\x7e",
        "punct" => "!-/:-@\\[-`{-~",
        "space" => SPACE,
        "upper" => "A-Z",
        "xdigit" => "0-9a-fA-F",
        "word" => WORD,
        "ascii" => "\\x00-\\x7f",
        _ => return None,
    })
}

/// Rewrite `source` so Oniguruma reads it the way Onigmo would. Answers the
/// input untouched when nothing in it is mode-sensitive.
pub(super) fn apply(
    source: &str,
    extended: bool,
) -> Result<std::borrow::Cow<'_, str>, &'static str> {
    if !source.contains('\\') && !source.contains("(?") && !source.contains("[:") {
        return Ok(std::borrow::Cow::Borrowed(source));
    }
    let b = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len() + 16);
    let mut stack = vec![Frame {
        mode: Mode::Default,
        extended,
    }];
    let mut touched = false;
    let mut i = 0;
    // Nesting depth of bracket classes; 0 = outside.
    let mut class_depth = 0usize;
    while i < b.len() {
        let top = *stack.last().expect("the outermost frame is never popped");
        let c = b[i];
        if class_depth > 0 {
            match c {
                b'\\' if i + 1 < b.len() => {
                    let e = b[i + 1];
                    if let Some(body) = escape_class_body(e, top.mode) {
                        touched = true;
                        // A ctype member cannot end or start a range. Onigmo
                        // refuses both; the nested class written here would
                        // not, so the refusal is made first.
                        check_range_position(b, i, i + 2)?;
                        push_nested(&mut out, body, e.is_ascii_uppercase());
                    } else {
                        out.push(b'\\');
                        out.push(e);
                    }
                    i += 2;
                }
                b'[' if b.get(i + 1) == Some(&b':') => {
                    // `[:name:]` or `[:^name:]`: a unit, ASCII under `(?a)`.
                    let end = find_posix_end(b, i);
                    match end {
                        Some(end) if top.mode == Mode::Ascii => {
                            let negated = b[i + 2] == b'^';
                            let name_start = if negated { i + 3 } else { i + 2 };
                            let name = std::str::from_utf8(&b[name_start..end]).unwrap_or("");
                            if let Some(body) = posix_ascii_body(name) {
                                touched = true;
                                check_range_position(b, i, end + 2)?;
                                push_nested(&mut out, body, negated);
                            } else {
                                out.extend_from_slice(&b[i..end + 2]);
                            }
                            i = end + 2;
                        }
                        Some(end) => {
                            out.extend_from_slice(&b[i..end + 2]);
                            i = end + 2;
                        }
                        None => {
                            // Not a POSIX bracket: a nested class opening.
                            class_depth += 1;
                            out.push(c);
                            i += 1;
                        }
                    }
                }
                b'[' => {
                    class_depth += 1;
                    out.push(c);
                    i += 1;
                }
                b']' => {
                    class_depth -= 1;
                    out.push(c);
                    i += 1;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            }
            continue;
        }
        match c {
            b'\\' if i + 1 < b.len() => {
                let e = b[i + 1];
                if let Some(body) = escape_class_body(e, top.mode) {
                    touched = true;
                    out.extend_from_slice(b"(?-i:[");
                    if e.is_ascii_uppercase() {
                        out.push(b'^');
                    }
                    out.extend_from_slice(body.as_bytes());
                    out.extend_from_slice(b"])");
                    i += 2;
                } else if (e == b'b' || e == b'B') && top.mode == Mode::Ascii {
                    touched = true;
                    let w = format!("[{WORD}]");
                    let text = if e == b'b' {
                        format!("(?-i:(?<={w})(?!{w})|(?<!{w})(?={w}))")
                    } else {
                        format!("(?-i:(?<={w})(?={w})|(?<!{w})(?!{w}))")
                    };
                    out.extend_from_slice(text.as_bytes());
                    i += 2;
                } else if (e == b'p' || e == b'P') && b.get(i + 2) == Some(&b'{') {
                    // A bare property folds under `/i` in Onigmo; a bracket
                    // class is what folds in Oniguruma.
                    let close = b[i..].iter().position(|&x| x == b'}').map(|p| i + p);
                    match close {
                        Some(close) => {
                            touched = true;
                            // Onigmo folds a NEGATED property as a not-flagged
                            // class (`é` leaves `\P{Lower}` under `/i`), which
                            // is `[^\p{...}]`, not `[\P{...}]`.
                            let caret = b[i + 3] == b'^';
                            let negated = (e == b'P') != caret;
                            let name_start = if caret { i + 4 } else { i + 3 };
                            out.extend_from_slice(if negated { b"[^\\p{" } else { b"[\\p{" });
                            out.extend_from_slice(&b[name_start..=close]);
                            out.push(b']');
                            i = close + 1;
                        }
                        None => {
                            out.extend_from_slice(&b[i..i + 2]);
                            i += 2;
                        }
                    }
                } else {
                    out.push(b'\\');
                    out.push(e);
                    i += 2;
                }
            }
            b'[' => {
                class_depth = 1;
                out.push(c);
                i += 1;
                // A `]` (after an optional `^`) right at the start is literal.
                if b.get(i) == Some(&b'^') {
                    out.push(b'^');
                    i += 1;
                }
                if b.get(i) == Some(&b']') {
                    out.push(b']');
                    i += 1;
                }
            }
            b'#' if top.extended => {
                // An extended-mode comment runs to the end of the line.
                let end = b[i..]
                    .iter()
                    .position(|&x| x == b'\n')
                    .map_or(b.len(), |p| i + p);
                out.extend_from_slice(&b[i..end]);
                i = end;
            }
            b'(' if b.get(i + 1) == Some(&b'?') => {
                if b.get(i + 2) == Some(&b'#') {
                    // A comment group: copied whole.
                    let end = b[i..]
                        .iter()
                        .position(|&x| x == b')')
                        .map_or(b.len(), |p| i + p + 1);
                    out.extend_from_slice(&b[i..end]);
                    i = end;
                    continue;
                }
                // Option letters, if any, up to `:` or `)`.
                let mut j = i + 2;
                while j < b.len() && matches!(b[j], b'i' | b'm' | b'x' | b'd' | b'a' | b'u' | b'-')
                {
                    j += 1;
                }
                let letters = &b[i + 2..j];
                let terminator = b.get(j).copied();
                let is_option_group =
                    !letters.is_empty() && matches!(terminator, Some(b':') | Some(b')'));
                if !is_option_group {
                    // `(?:`, `(?=`, `(?<name>`, `(?>`, `(?~` ...: a group
                    // inheriting its enclosing scope.
                    stack.push(top);
                    out.extend_from_slice(b"(?");
                    i += 2;
                    continue;
                }
                let mut frame = top;
                let mut kept: Vec<u8> = Vec::with_capacity(letters.len());
                let mut negating = false;
                for &l in letters {
                    match l {
                        b'-' => {
                            negating = true;
                            kept.push(l);
                        }
                        b'd' | b'a' | b'u' if !negating => {
                            touched = true;
                            frame.mode = match l {
                                b'd' => Mode::Default,
                                b'a' => Mode::Ascii,
                                _ => Mode::Unicode,
                            };
                        }
                        b'x' => {
                            frame.extended = !negating;
                            kept.push(l);
                        }
                        _ => kept.push(l),
                    }
                }
                let bare = terminator == Some(b')');
                if bare {
                    // `(?a)`: the rest of the enclosing group.
                    *stack.last_mut().expect("non-empty") = frame;
                    let only_a_dash = kept.iter().all(|&l| l == b'-');
                    if !only_a_dash {
                        out.extend_from_slice(b"(?");
                        out.extend_from_slice(&kept);
                        out.push(b')');
                    }
                } else {
                    stack.push(frame);
                    out.extend_from_slice(b"(?");
                    out.extend_from_slice(&kept);
                    out.push(b':');
                }
                i = j + 1;
            }
            b'(' => {
                stack.push(top);
                out.push(c);
                i += 1;
            }
            b')' => {
                if stack.len() > 1 {
                    stack.pop();
                }
                out.push(c);
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    if !touched {
        return Ok(std::borrow::Cow::Borrowed(source));
    }
    // Every byte written came from `source` or from an ASCII table, so the
    // result is UTF-8 whenever the input was.
    Ok(std::borrow::Cow::Owned(
        String::from_utf8(out)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
    ))
}

/// Ruby's two range rules for a class member that is a set rather than a
/// character: the member at `start..end` may neither close a pending `X-`
/// nor open a range of its own.
fn check_range_position(b: &[u8], start: usize, end: usize) -> Result<(), &'static str> {
    let opens_class = |j: usize| b[j] == b'[' || (b[j] == b'^' && j > 0 && b[j - 1] == b'[');
    // A `-` is the range operator only when it is not itself escaped.
    let escaped = |j: usize| b[..j].iter().rev().take_while(|&&x| x == b'\\').count() % 2 == 1;
    if start >= 2
        && b[start - 1] == b'-'
        && !escaped(start - 1)
        && !opens_class(start - 2)
        && b[start - 2] != b'&'
    {
        return Err("char-class value at end of range");
    }
    if b.get(end) == Some(&b'-') && !matches!(b.get(end + 1), None | Some(b']') | Some(b'&')) {
        return Err("unmatched range specifier in char-class");
    }
    Ok(())
}

/// `[body]` or `[^body]` as a member of an enclosing class.
fn push_nested(out: &mut Vec<u8>, body: &str, negated: bool) {
    out.extend_from_slice(if negated { b"[^" } else { b"[" });
    out.extend_from_slice(body.as_bytes());
    out.push(b']');
}

/// The bracket body an ASCII-range escape stands for under `mode`, or `None`
/// when the escape is not one of `\w \W \d \D \s \S` or the mode leaves it to
/// the engine.
fn escape_class_body(e: u8, mode: Mode) -> Option<&'static str> {
    if mode == Mode::Unicode {
        return None;
    }
    Some(match e {
        b'w' | b'W' => WORD,
        b'd' | b'D' => DIGIT,
        b's' | b'S' => SPACE,
        _ => return None,
    })
}

/// For a `[:` at `start` inside a class, the index of the closing `:]`'s
/// colon when the text really is a POSIX bracket.
fn find_posix_end(b: &[u8], start: usize) -> Option<usize> {
    let mut j = start + 2;
    if b.get(j) == Some(&b'^') {
        j += 1;
    }
    let name_start = j;
    while j < b.len() && b[j].is_ascii_alphabetic() {
        j += 1;
    }
    (j > name_start && b.get(j) == Some(&b':') && b.get(j + 1) == Some(&b']')).then_some(j)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rw(s: &str) -> String {
        apply(s, false).unwrap().into_owned()
    }

    #[test]
    fn the_default_mode_makes_the_three_escapes_ascii_and_nothing_else() {
        assert_eq!(
            rw(r"\w+\d\s"),
            "(?-i:[a-zA-Z0-9_])+(?-i:[0-9])(?-i:[ \\t\\n\\v\\f\\r])"
        );
        assert_eq!(rw(r"\W"), "(?-i:[^a-zA-Z0-9_])");
        assert_eq!(rw(r"[\w-]"), "[[a-zA-Z0-9_]-]");
        assert_eq!(rw(r"[^\W\d]"), "[^[^a-zA-Z0-9_][0-9]]");
        // `\b`, POSIX brackets, `\h` and `\p{Word}` are Unicode here.
        assert_eq!(rw(r"\b[[:word:]]\h\p{Word}"), "\\b[[:word:]]\\h[\\p{Word}]");
        assert_eq!(rw(r"\\w"), r"\\w");
    }

    #[test]
    fn the_ascii_mode_moves_the_boundary_and_the_brackets_too() {
        assert!(rw(r"(?a)\b").starts_with("(?-i:(?<=[a-zA-Z0-9_])"));
        assert_eq!(rw(r"(?a)[[:alpha:][:^digit:]]"), "[[a-zA-Z][^0-9]]");
        assert_eq!(
            rw(r"(?a:[[:alpha:]])[[:alpha:]]"),
            "(?:[[a-zA-Z]])[[:alpha:]]"
        );
        assert_eq!(rw(r"(?u)\w(?a)\w"), "\\w(?-i:[a-zA-Z0-9_])");
        assert_eq!(rw(r"(?iu)\w"), "(?i)\\w");
        assert_eq!(rw(r"(?u-i:\w)\w"), "(?-i:\\w)(?-i:[a-zA-Z0-9_])");
        // A bare option holds to the end of its enclosing group.
        assert_eq!(rw(r"((?u)\w)\w"), "(\\w)(?-i:[a-zA-Z0-9_])");
        // Ruby refuses a negated mode letter, so it is left for the engine to
        // refuse the same way.
        assert_eq!(rw(r"(?-u)\w"), "(?-u)(?-i:[a-zA-Z0-9_])");
    }

    #[test]
    fn comments_classes_and_escapes_do_not_confuse_the_walk() {
        assert_eq!(rw("(?#[)\\w"), "(?#[)(?-i:[a-zA-Z0-9_])");
        assert_eq!(
            apply("# [ \\w\n\\w", true).unwrap().as_ref(),
            "# [ \\w\n(?-i:[a-zA-Z0-9_])"
        );
        assert_eq!(rw(r"[]\w]"), "[][a-zA-Z0-9_]]");
        assert_eq!(rw(r"[[:^alpha:]\w]"), "[[:^alpha:][a-zA-Z0-9_]]");
        assert_eq!(rw(r"[a[b\w]]"), "[a[b[a-zA-Z0-9_]]]");
        assert_eq!(
            rw(r"\p{^Lower}\P{L}\P{^N}"),
            "[^\\p{Lower}][^\\p{L}][\\p{N}]"
        );
        assert_eq!(rw(r"[\p{L}]"), "[\\p{L}]");
        assert!(matches!(
            apply("abc", false),
            Ok(std::borrow::Cow::Borrowed(_))
        ));
        assert!(matches!(
            apply(r"(?:a)\n", false),
            Ok(std::borrow::Cow::Borrowed(_))
        ));
    }

    #[test]
    fn a_set_member_can_neither_open_nor_close_a_range() {
        assert_eq!(
            apply(r"[\d-z]", false),
            Err("unmatched range specifier in char-class")
        );
        assert_eq!(
            apply(r"[a-\w]", false),
            Err("char-class value at end of range")
        );
        assert_eq!(
            apply(r"(?a)[a-[:alpha:]]", false),
            Err("char-class value at end of range")
        );
        assert!(apply(r"[\w-]", false).is_ok());
        assert!(apply(r"[-\w]", false).is_ok());
        assert!(apply(r"[^\w-]", false).is_ok());
        assert!(apply(r"[\w-&&a]", false).is_ok());
        assert!(apply(r"[\-\w]", false).is_ok());
        assert!(apply(r"[\\-\w]", false).is_err());
    }
}
