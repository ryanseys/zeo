//! The warnings Onigmo prints while it parses a pattern, which Oniguruma
//! never prints: a `\p` with no braces, and a repeat of a repeat
//! (`regparse.c`'s `set_quantifier`). Ruby prints them from `Regexp.new` and
//! an interpolated literal, at the caller's line.

/// Onigmo's `PopularQStr`: a quantifier's text by its popular number.
const POPULAR: [&str; 6] = ["?", "*", "+", "??", "*?", "+?"];

#[derive(Clone, Copy)]
enum Reduce {
    AsIs,
    Del,
    Star,
    LazyStar,
    LazyQuestion,
    PlusLazyQuestion,
}

/// Onigmo's `ReduceTypeTable[inner][outer]`, as ruby 4.0 answers it.
const REDUCE: [[Reduce; 6]; 6] = {
    use Reduce::*;
    [
        [Del, Star, Star, LazyQuestion, LazyStar, AsIs],
        [Del, Del, Del, PlusLazyQuestion, PlusLazyQuestion, Del],
        [Star, Star, Del, AsIs, PlusLazyQuestion, Del],
        [Del, LazyStar, LazyStar, Del, LazyStar, LazyStar],
        [Del; 6],
        [AsIs, AsIs, AsIs, LazyStar, LazyStar, Del],
    ]
};

/// What a parsed item is to a quantifier that follows it.
#[derive(Clone, Copy)]
enum Item {
    Plain,
    /// A quantifier node, with its popular number (`None` for an interval
    /// such as `{2,3}`).
    Repeat(Option<usize>),
}

/// Every warning ruby prints for `source` (the pattern after the `\u`
/// expansion), in parse order, each with the pattern appended the way
/// Onigmo's `onig_syntax_warn` appends it.
pub(super) fn warnings(source: &str, extended: bool) -> Vec<String> {
    let mut lint = Lint { b: source.as_bytes(), i: 0, comments: Vec::new(), notes: Vec::new() };
    while lint.i < lint.b.len() {
        lint.seq(extended);
        // A stray `)` the engine accepted.
        lint.i += 1;
    }
    if lint.notes.is_empty() {
        return lint.notes;
    }
    let mut shown = Vec::with_capacity(source.len());
    let mut at = 0;
    for &(from, to) in &lint.comments {
        shown.extend_from_slice(&lint.b[at..from]);
        at = to;
    }
    shown.extend_from_slice(&lint.b[at..]);
    let text = display(&String::from_utf8_lossy(&shown));
    // `onig_syntax_warn` formats into 256 bytes and drops the pattern
    // unless four bytes per pattern byte would fit.
    lint.notes
        .into_iter()
        .map(|n| if n.len() + shown.len() * 4 + 4 < 256 { format!("{n}: /{text}/") } else { n })
        .collect()
}

/// The pattern as Onigmo prints it: a control character as `\xhh`, a bare
/// `/` escaped, an escape pair kept whole.
fn display(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push('\\');
                out.extend(chars.next());
            }
            '/' => out.push_str("\\/"),
            c if (c as u32) < 0x20 || c == '\x7f' => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

struct Lint<'a> {
    b: &'a [u8],
    i: usize,
    /// `(?#...)` and extended-mode `#` comments, which ruby drops before
    /// Onigmo sees the pattern.
    comments: Vec<(usize, usize)>,
    notes: Vec<String>,
}

impl Lint<'_> {
    /// One group body up to its `)` (not consumed) or the end. Answers the
    /// quantifier node the body is when it is exactly one, which is what a
    /// `(?:...)` hands the quantifier after it.
    fn seq(&mut self, mut extended: bool) -> Item {
        let (mut items, mut branches, mut wrapped) = (0usize, 1usize, false);
        let mut last: Option<Item> = None;
        loop {
            if extended {
                self.skip_space();
            }
            let Some(&c) = self.b.get(self.i) else { break };
            match c {
                b')' => break,
                b'|' => {
                    branches += 1;
                    last = None;
                    self.i += 1;
                }
                b'?' | b'*' | b'+' if last.is_some() => {
                    last = last.map(|t| self.quantify(t));
                }
                b'{' if last.is_some() && interval(self.b, self.i).is_some() => {
                    last = last.map(|t| self.quantify(t));
                }
                _ => {
                    if let Some(item) = self.atom(&mut extended, &mut wrapped) {
                        items += 1;
                        last = Some(item);
                    }
                }
            }
        }
        match last {
            Some(item) if branches == 1 && items == 1 && !wrapped => item,
            _ => Item::Plain,
        }
    }

    fn skip_space(&mut self) {
        while let Some(&c) = self.b.get(self.i) {
            match c {
                b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r' => self.i += 1,
                b'#' => {
                    let from = self.i;
                    while self.b.get(self.i).is_some_and(|&c| c != b'\n') {
                        self.i += 1;
                    }
                    self.i = (self.i + 1).min(self.b.len());
                    self.comments.push((from, self.i));
                }
                _ => break,
            }
        }
    }

    /// A quantifier at `self.i` applied to `target`.
    fn quantify(&mut self, target: Item) -> Item {
        let (lower, upper, interval_kind) = match self.b[self.i] {
            b'?' => (0, Some(1), None),
            b'*' => (0, None, None),
            b'+' => (1, None, None),
            _ => {
                let (lower, upper, fixed, end) = interval(self.b, self.i).expect("checked by the caller");
                self.i = end - 1;
                (lower, upper, Some(fixed))
            }
        };
        self.i += 1;
        let next = self.b.get(self.i).copied();
        let (mut greedy, mut possessive) = (true, false);
        match interval_kind {
            None if next == Some(b'?') => greedy = false,
            None if next == Some(b'+') => possessive = true,
            // A fixed `{n}` takes no lazy mark: a `?` after it is a repeat.
            Some(false) if next == Some(b'?') => greedy = false,
            _ => {}
        }
        if !greedy || possessive {
            self.i += 1;
        }
        // `{1}` leaves its target as it is.
        if lower == 1 && upper == Some(1) {
            return target;
        }
        let outer = match (lower, upper) {
            (0, Some(1)) => Some(0),
            (0, None) => Some(1),
            (1, None) => Some(2),
            _ => None,
        }
        .map(|n| if greedy { n } else { n + 3 });
        let result = match (target, outer) {
            (Item::Repeat(Some(inner)), Some(outer)) => {
                let reduce = REDUCE[inner][outer];
                let (kind, into) = match reduce {
                    Reduce::AsIs => (outer, None),
                    Reduce::Del => (inner, None),
                    Reduce::Star => (1, Some("*")),
                    Reduce::LazyStar => (4, Some("*?")),
                    Reduce::LazyQuestion => (3, Some("??")),
                    Reduce::PlusLazyQuestion => (3, Some("+ and ??")),
                };
                match (reduce, into) {
                    (Reduce::AsIs, _) => {}
                    (_, None) => self.notes.push(format!(
                        "regular expression has redundant nested repeat operator '{}'",
                        POPULAR[inner]
                    )),
                    (_, Some(into)) => self.notes.push(format!(
                        "nested repeat operator '{}' and '{}' was replaced with '{into}' in regular expression",
                        POPULAR[inner], POPULAR[outer]
                    )),
                }
                Item::Repeat(Some(kind))
            }
            _ => Item::Repeat(outer),
        };
        // A possessive repeat is wrapped in an atomic group.
        if possessive { Item::Plain } else { result }
    }

    /// One atom, or `None` for what is no item: a `(?#...)` comment or an
    /// inline option switch.
    fn atom(&mut self, extended: &mut bool, wrapped: &mut bool) -> Option<Item> {
        match self.b[self.i] {
            b'\\' => self.escape(),
            b'[' => self.class(),
            b'(' => return self.group(extended, wrapped),
            _ => self.char(),
        }
        Some(Item::Plain)
    }

    fn char(&mut self) {
        self.i += 1;
        while self.b.get(self.i).is_some_and(|&c| c & 0xC0 == 0x80) {
            self.i += 1;
        }
    }

    fn skip_past(&mut self, close: u8) {
        while self.b.get(self.i).is_some_and(|&c| c != close) {
            self.i += 1;
        }
        self.i = (self.i + 1).min(self.b.len());
    }

    fn escape(&mut self) {
        self.i += 1;
        let Some(&e) = self.b.get(self.i) else { return };
        let next = self.b.get(self.i + 1).copied();
        match e {
            b'p' | b'P' => {
                self.i += 1;
                if next == Some(b'{') {
                    self.skip_past(b'}');
                } else {
                    self.notes.push(format!("invalid Unicode Property \\{}", e as char));
                }
            }
            b'x' if next == Some(b'{') => {
                self.i += 1;
                self.skip_past(b'}');
            }
            b'k' | b'g' if matches!(next, Some(b'<' | b'\'')) => {
                self.i += 2;
                self.skip_past(if next == Some(b'<') { b'>' } else { b'\'' });
            }
            b'M' | b'C' if next == Some(b'-') => {
                self.i += 2;
                self.escaped_char();
            }
            b'c' => {
                self.i += 1;
                self.escaped_char();
            }
            _ => self.char(),
        }
    }

    /// The character a `\M-`, `\C-` or `\c` names, itself maybe an escape.
    fn escaped_char(&mut self) {
        match self.b.get(self.i) {
            Some(b'\\') => self.escape(),
            Some(_) => self.char(),
            None => {}
        }
    }

    /// A bracket class, where only a bare `\p` warns.
    fn class(&mut self) {
        self.i += 1;
        if self.b.get(self.i) == Some(&b'^') {
            self.i += 1;
        }
        if self.b.get(self.i) == Some(&b']') {
            self.i += 1;
        }
        let mut depth = 1usize;
        while let Some(&c) = self.b.get(self.i) {
            match c {
                b'\\' => {
                    if let Some(&e @ (b'p' | b'P')) = self.b.get(self.i + 1)
                        && self.b.get(self.i + 2) != Some(&b'{')
                    {
                        self.notes.push(format!("invalid Unicode Property \\{}", e as char));
                    }
                    self.i += 1;
                }
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        self.i += 1;
                        return;
                    }
                }
                _ => {}
            }
            self.i += 1;
        }
    }

    fn group(&mut self, extended: &mut bool, wrapped: &mut bool) -> Option<Item> {
        let at = |k: usize| self.b.get(self.i + k).copied();
        let (second, third, fourth) = (at(1), at(2), at(3));
        if second != Some(b'?') {
            self.i += 1;
            return Some(self.body(*extended));
        }
        match third {
            Some(b'#') => {
                let from = self.i;
                self.i += 3;
                while let Some(&c) = self.b.get(self.i) {
                    self.i += if c == b'\\' { 2 } else { 1 };
                    if c == b')' {
                        break;
                    }
                }
                self.i = self.i.min(self.b.len());
                self.comments.push((from, self.i));
                None
            }
            Some(b':') => {
                self.i += 3;
                let inner = self.seq(*extended);
                self.close();
                Some(inner)
            }
            Some(b'<') if !matches!(fourth, Some(b'=' | b'!')) => {
                self.i += 3;
                self.skip_past(b'>');
                Some(self.body(*extended))
            }
            Some(b'\'') => {
                self.i += 3;
                self.skip_past(b'\'');
                Some(self.body(*extended))
            }
            Some(b'(') => {
                self.i += 3;
                self.skip_past(b')');
                Some(self.body(*extended))
            }
            Some(b'<') => {
                self.i += 4;
                Some(self.body(*extended))
            }
            _ => {
                let mut k = self.i + 2;
                let (mut on, mut x) = (true, None);
                while let Some(&c) = self.b.get(k) {
                    match c {
                        b'-' => on = false,
                        b'x' => x = Some(on),
                        b'i' | b'm' | b'a' | b'd' | b'u' => {}
                        _ => break,
                    }
                    k += 1;
                }
                let inner = x.unwrap_or(*extended);
                match self.b.get(k) {
                    Some(b')') if k > self.i + 2 => {
                        self.i = k + 1;
                        *extended = inner;
                        *wrapped = true;
                        None
                    }
                    Some(b':') if k > self.i + 2 => {
                        self.i = k + 1;
                        Some(self.body(inner))
                    }
                    // `(?=`, `(?!`, `(?>`, `(?~` and the like.
                    _ => {
                        self.i += 3;
                        Some(self.body(*extended))
                    }
                }
            }
        }
    }

    /// The body of a group that is no quantifier target itself.
    fn body(&mut self, extended: bool) -> Item {
        self.seq(extended);
        self.close();
        Item::Plain
    }

    fn close(&mut self) {
        if self.b.get(self.i) == Some(&b')') {
            self.i += 1;
        }
    }
}

/// A valid interval at `b[start]` (`{n}`, `{n,}`, `{,m}`, `{n,m}`): its
/// bounds, whether it is the fixed `{n}` form, and the index past it.
/// Anything else is a literal `{`.
fn interval(b: &[u8], start: usize) -> Option<(u32, Option<u32>, bool, usize)> {
    let digits = |from: usize| {
        let n = b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
        let value = std::str::from_utf8(&b[from..from + n]).ok().and_then(|s| s.parse().ok());
        (n, value)
    };
    let mut i = start + 1;
    let (n, low) = digits(i);
    i += n;
    if b.get(i) == Some(&b'}') {
        return low.map(|low| (low, Some(low), true, i + 1));
    }
    if b.get(i) != Some(&b',') {
        return None;
    }
    i += 1;
    let (m, up) = digits(i);
    i += m;
    if b.get(i) != Some(&b'}') || (n == 0 && m == 0) {
        return None;
    }
    Some((low.unwrap_or(0), up, false, i + 1))
}

#[cfg(test)]
mod tests {
    use super::warnings;

    fn one(source: &str) -> Vec<String> {
        warnings(source, false)
    }

    #[test]
    fn a_repeat_of_a_repeat_warns_as_onigmo_reduces_it() {
        let redundant = |q: &str, p: &str| {
            format!("regular expression has redundant nested repeat operator '{q}': /{p}/")
        };
        assert_eq!(one("a***"), vec![redundant("*", "a***"), redundant("*", "a***")]);
        assert_eq!(one("(?:a*?)+"), vec![redundant("*?", "(?:a*?)+")]);
        assert_eq!(
            one("(?:a*)??"),
            vec!["nested repeat operator '*' and '??' was replaced with '+ and ??' in regular expression: /(?:a*)??/"]
        );
        assert_eq!(one("a+?**"), vec![redundant("*", "a+?**")]);
        assert_eq!(one("a{0,}**").len(), 2);
        assert_eq!(one("a{2}?*").len(), 1);
        assert_eq!(one("a***+").len(), 2);
        // A capture, an option group, a possessive repeat, an interval or a
        // list stops the chain.
        for quiet in ["(a*)*", "(?i:a*)*", "a*+*", "(?:a*){2,3}", "(?:ab*)*", "(?:|a*)*", "(?: a*)*"] {
            assert!(one(quiet).is_empty(), "{quiet}");
        }
        assert_eq!(one("(?:a*(?#c))*"), vec![redundant("*", "(?:a*)*")]);
    }

    #[test]
    fn a_bare_property_escape_warns_where_it_stands() {
        assert_eq!(
            one("\\p**"),
            vec![
                "invalid Unicode Property \\p: /\\p**/".to_string(),
                "regular expression has redundant nested repeat operator '*': /\\p**/".into(),
            ]
        );
        assert_eq!(one("[\\P]"), vec!["invalid Unicode Property \\P: /[\\P]/".to_string()]);
        assert!(one("\\p{L}\\\\p[\\pL&&\\p{Word}]").len() == 1);
    }

    #[test]
    fn the_pattern_is_shown_as_onigmo_prints_it() {
        assert_eq!(
            warnings("a* # c\n *", true),
            vec!["regular expression has redundant nested repeat operator '*': /a*  */".to_string()]
        );
        assert!(one("a/\t**")[0].ends_with(": /a\\/\\x09**/"));
        assert!(one(&format!("{}**", "a".repeat(46)))[0].ends_with("**/"));
        assert!(one(&format!("{}**", "a".repeat(47)))[0].ends_with("'*'"));
    }
}
