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
//!   class, so a bracket class holding such a member is split under `/i`
//!   ([`split_folding_class`]).
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
    ignore_case: bool,
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
    ignore_case: bool,
) -> Result<std::borrow::Cow<'_, str>, &'static str> {
    if !source.contains('\\') && !source.contains("(?") && !source.contains("[:") {
        return Ok(std::borrow::Cow::Borrowed(source));
    }
    let b = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len() + 16);
    let mut stack = vec![Frame {
        mode: Mode::Default,
        extended,
        ignore_case,
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
            b'[' if top.ignore_case && split_folding_class(b, i, top.mode)?.is_some() => {
                let (text, end) = split_folding_class(b, i, top.mode)?.expect("checked by the guard");
                touched = true;
                out.extend_from_slice(text.as_bytes());
                i = end;
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
                        b'i' => {
                            frame.ignore_case = !negating;
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

/// One `&&` operand of a bracket class under `/i`, split by how Onigmo folds
/// each member.
#[derive(Default)]
struct Operand {
    /// ASCII-range members, each as a nested class: they fold within ASCII.
    ascii: Vec<String>,
    /// Every other member, as written: it folds the ordinary way.
    rest: Vec<u8>,
}

/// Onigmo's `/i` rule for a bracket class holding an ASCII-range member --
/// `\w \d \s` and their negations outside `(?u)`, a POSIX bracket under
/// `(?a)`. Such a member folds only within ASCII, while the rest of the
/// class folds the ordinary way, past ASCII included (`regparse.c`'s
/// `asc_cc`). Oniguruma folds a whole class one way, so the class becomes a
/// group that folds each part as Onigmo does. Answers the group and the index
/// past the class, or `None` to leave the class to the ordinary walk: nothing
/// in it is ASCII-range, or its shape is one this does not split (a negated
/// nested class holding one, an operand mixing both kinds).
fn split_folding_class(b: &[u8], start: usize, mode: Mode) -> Result<Option<(String, usize)>, &'static str> {
    let end = class_end(b, start);
    if b.get(end - 1) != Some(&b']') || end - 1 <= start {
        return Ok(None);
    }
    let mut i = start + 1;
    let negated = b.get(i) == Some(&b'^');
    if negated {
        i += 1;
    }
    let mut operands = vec![Operand::default()];
    if b.get(i) == Some(&b']') {
        operands[0].rest.push(b']');
        i += 1;
    }
    if !collect_members(b, i, end - 1, mode, &mut operands)? {
        return Ok(None);
    }
    if operands.iter().all(|o| o.ascii.is_empty()) {
        return Ok(None);
    }
    let caret = if negated { "^" } else { "" };
    let text = if let [only] = operands.as_slice() {
        let w = only.ascii.concat();
        let e = class_text(&only.rest);
        match (negated, e.is_empty()) {
            (false, true) => format!("(?-i:[{w}])"),
            (false, false) => format!("(?:[{e}]|(?-i:[{w}]))"),
            (true, true) => format!("(?-i:[^{w}])"),
            (true, false) => format!("(?:(?!(?-i:[{w}]))[^{e}])"),
        }
    } else {
        if operands.iter().any(|o| {
            (!o.ascii.is_empty() && !o.rest.is_empty()) || (o.ascii.is_empty() && o.rest.is_empty())
        }) {
            return Ok(None);
        }
        let ascii: Vec<String> = operands
            .iter()
            .filter(|o| !o.ascii.is_empty())
            .map(|o| format!("[{}]", o.ascii.concat()))
            .collect();
        let rest: Vec<String> = operands
            .iter()
            .filter(|o| !o.rest.is_empty())
            .map(|o| class_text(&o.rest))
            .collect();
        if rest.is_empty() {
            format!("(?-i:[{caret}{}])", ascii.join("&&"))
        } else {
            // The intersection folds within each side of ASCII only: an
            // ASCII-range operand leaves nothing for a cross-ASCII fold.
            let looks: String = ascii.iter().map(|w| format!("(?=(?-i:{w}))")).collect();
            let e = rest.join("&&");
            let same_side = format!(
                "(?:(?-i:(?=[\\x00-\\x7f]))(?i:[{e}&&[\\x00-\\x7f]])|(?-i:(?=[^\\x00-\\x7f]))(?i:[{e}&&[^\\x00-\\x7f]]))"
            );
            if negated {
                format!("(?:(?!{looks}{same_side})(?m:.))")
            } else {
                format!("(?:{looks}{same_side})")
            }
        }
    };
    Ok(Some((text, end)))
}

/// A class body as written, with a leading `^` escaped so it stays literal
/// wherever the body is placed.
fn class_text(rest: &[u8]) -> String {
    let text = String::from_utf8_lossy(rest).into_owned();
    if text.starts_with('^') { format!("\\{text}") } else { text }
}

/// Sorts the members of `b[from..to]` into `operands`, a new operand at each
/// `&&`. A nested class is flattened into its operand when it is plain and
/// copied whole when negated and free of ASCII-range members. Answers false
/// for a shape [`split_folding_class`] does not split.
fn collect_members(
    b: &[u8],
    from: usize,
    to: usize,
    mode: Mode,
    operands: &mut Vec<Operand>,
) -> Result<bool, &'static str> {
    let mut i = from;
    while i < to {
        let op = operands.last_mut().expect("never empty");
        match b[i] {
            b'&' if b.get(i + 1) == Some(&b'&') => {
                operands.push(Operand::default());
                i += 2;
            }
            b'\\' if i + 1 < to => {
                let e = b[i + 1];
                if let Some(body) = escape_class_body(e, mode) {
                    check_range_position(b, i, i + 2)?;
                    op.ascii.push(nested(body, e.is_ascii_uppercase()));
                    i += 2;
                } else {
                    let mut j = i + 2;
                    if matches!(e, b'p' | b'P' | b'x' | b'u') && b.get(j) == Some(&b'{') {
                        j = b[j..to].iter().position(|&x| x == b'}').map_or(to, |p| j + p + 1);
                    }
                    op.rest.extend_from_slice(&b[i..j]);
                    i = j;
                }
            }
            b'[' if b.get(i + 1) == Some(&b':') && find_posix_end(b, i).is_some() => {
                let pend = find_posix_end(b, i).expect("checked by the guard");
                let inverted = b[i + 2] == b'^';
                let name = std::str::from_utf8(&b[if inverted { i + 3 } else { i + 2 }..pend]).unwrap_or("");
                match posix_ascii_body(name).filter(|_| mode == Mode::Ascii) {
                    Some(body) => {
                        check_range_position(b, i, pend + 2)?;
                        // Folding within ASCII pairs the two letter cases.
                        let body = match name {
                            "upper" | "lower" if !inverted => "a-zA-Z",
                            _ => body,
                        };
                        op.ascii.push(nested(body, inverted));
                    }
                    None => op.rest.extend_from_slice(&b[i..pend + 2]),
                }
                i = pend + 2;
            }
            b'[' => {
                let nend = class_end(b, i);
                if b.get(nend - 1) != Some(&b']') || nend > to + 1 {
                    return Ok(false);
                }
                let mut inner = vec![Operand::default()];
                let mut k = i + 1;
                let inner_negated = b.get(k) == Some(&b'^');
                if inner_negated {
                    k += 1;
                }
                if b.get(k) == Some(&b']') {
                    inner[0].rest.push(b']');
                    k += 1;
                }
                if !collect_members(b, k, nend - 1, mode, &mut inner)? || inner.len() > 1 {
                    return Ok(false);
                }
                let inner = inner.pop().expect("one operand");
                let op = operands.last_mut().expect("never empty");
                if inner_negated {
                    if !inner.ascii.is_empty() {
                        return Ok(false);
                    }
                    op.rest.extend_from_slice(&b[i..nend]);
                } else {
                    op.ascii.extend(inner.ascii);
                    if !inner.rest.is_empty() {
                        op.rest.push(b'[');
                        op.rest.extend_from_slice(&inner.rest);
                        op.rest.push(b']');
                    }
                }
                i = nend;
            }
            c => {
                op.rest.push(c);
                i += 1;
            }
        }
    }
    Ok(true)
}

/// `[body]` or `[^body]`, as text.
fn nested(body: &str, negated: bool) -> String {
    format!("[{}{body}]", if negated { "^" } else { "" })
}

/// The index just past the bracket class opening at `start` (the end of the
/// input when it never closes). A `]` first, after an optional `^`, is literal.
pub(super) fn class_end(b: &[u8], start: usize) -> usize {
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
        apply(s, false, false).unwrap().into_owned()
    }

    fn rwi(s: &str) -> String {
        apply(s, false, true).unwrap().into_owned()
    }

    fn apply_plain(s: &str, extended: bool) -> Result<std::borrow::Cow<'_, str>, &'static str> {
        apply(s, extended, false)
    }

    #[test]
    fn under_i_an_ascii_range_class_member_folds_within_ascii() {
        assert_eq!(rwi(r"[\w]"), "(?-i:[[a-zA-Z0-9_]])");
        assert_eq!(rwi(r"[a-z\w]"), "(?:[a-z]|(?-i:[[a-zA-Z0-9_]]))");
        assert_eq!(rwi(r"[^\W]"), "(?-i:[^[^a-zA-Z0-9_]])");
        assert_eq!(rwi(r"[^é\w]"), "(?:(?!(?-i:[[a-zA-Z0-9_]]))[^é])");
        assert_eq!(rwi(r"(?a)[[:upper:]]"), "(?-i:[[a-zA-Z]])");
        assert_eq!(rwi(r"[[\w]k]"), "(?:[k]|(?-i:[[a-zA-Z0-9_]]))");
        assert_eq!(
            rwi(r"[\W&&[^ſ]]"),
            "(?:(?=(?-i:[[^a-zA-Z0-9_]]))(?:(?-i:(?=[\\x00-\\x7f]))(?i:[[^ſ]&&[\\x00-\\x7f]])\
             |(?-i:(?=[^\\x00-\\x7f]))(?i:[[^ſ]&&[^\\x00-\\x7f]])))"
        );
        // Nothing ASCII-range, a Unicode mode, or no `/i`: left as it was.
        assert_eq!(rwi(r"[a-z]"), "[a-z]");
        assert_eq!(rwi(r"(?u)[\w]"), "[\\w]");
        assert_eq!(rw(r"[\w]"), "[[a-zA-Z0-9_]]");
        assert_eq!(rwi(r"(?-i:[\w])"), "(?-i:[[a-zA-Z0-9_]])");
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
            apply_plain("# [ \\w\n\\w", true).unwrap().as_ref(),
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
            apply_plain("abc", false),
            Ok(std::borrow::Cow::Borrowed(_))
        ));
        assert!(matches!(
            apply_plain(r"(?:a)\n", false),
            Ok(std::borrow::Cow::Borrowed(_))
        ));
    }

    #[test]
    fn a_set_member_can_neither_open_nor_close_a_range() {
        assert_eq!(
            apply_plain(r"[\d-z]", false),
            Err("unmatched range specifier in char-class")
        );
        assert_eq!(
            apply_plain(r"[a-\w]", false),
            Err("char-class value at end of range")
        );
        assert_eq!(
            apply_plain(r"(?a)[a-[:alpha:]]", false),
            Err("char-class value at end of range")
        );
        assert!(apply_plain(r"[\w-]", false).is_ok());
        assert!(apply_plain(r"[-\w]", false).is_ok());
        assert!(apply_plain(r"[^\w-]", false).is_ok());
        assert!(apply_plain(r"[\w-&&a]", false).is_ok());
        assert!(apply_plain(r"[\-\w]", false).is_ok());
        assert!(apply_plain(r"[\\-\w]", false).is_err());
    }
}
