//! [`LiteralPattern`]: the regexps a fold may answer exactly -- plain
//! character alternatives, anchors, and `.` -- and nothing more.

use super::*;

/// One position of an alternative.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Elem {
    /// A character that means itself -- including `\.`, the escape a version
    /// test spells the dot with (`/^1\.8/`).
    Lit(char),
    /// `.` -- exactly one character, and never a newline. The one regexp
    /// METAcharacter this reduction admits, admitted because it is exact:
    /// nothing here approximates it. A quantifier is still refused, so a `.`
    /// can never stand for more or less than one character.
    AnyChar,
}

/// One `|`-alternative, position by position.
struct Alternative(Vec<Elem>);

impl Alternative {
    /// How many BYTES of `subject` this alternative consumes from its start, or
    /// `None` if it does not match there.
    fn match_at(&self, subject: &str, ignore_case: bool) -> Option<usize> {
        let mut chars = subject.chars();
        let mut consumed = 0;
        for elem in &self.0 {
            let c = chars.next()?;
            let ok = match elem {
                Elem::Lit(l) => {
                    c == *l || (ignore_case && c.eq_ignore_ascii_case(l) && c.is_ascii())
                }
                Elem::AnyChar => c != '\n',
            };
            if !ok {
                return None;
            }
            consumed += c.len_utf8();
        }
        Some(consumed)
    }
}

/// A regexp whose whole meaning is a plain character test -- so folding it here
/// cannot disagree with a real regexp engine. Either `|`-separated alternatives
/// matched anywhere in the subject, or ONE alternative anchored at the start
/// and/or end. Half the corpus writes its platform gate this way
/// (`/mswin|mingw|windows/`), and a version gate its prefix
/// (`/^1.8/`).
///
/// `None` for anything carrying regexp syntax beyond `.` (classes, quantifiers,
/// groups, other escapes), an interpolated pattern, or a flag that changes what
/// the pattern MEANS (`/x`, `/m`). Those stay undecided rather than being
/// answered by an approximation.
pub(crate) struct LiteralPattern {
    anchored_start: bool,
    anchored_end: bool,
    /// `/i`. ASCII-folded here, so [`LiteralPattern::matches`] declines a
    /// subject that isn't ASCII -- ruby folds the full Unicode case table, and
    /// a narrower rule must not answer where the two could part.
    ignore_case: bool,
    alts: Vec<Alternative>,
}

impl LiteralPattern {
    /// Parse a regexp SOURCE with its flags, independent of where the regexp
    /// literal was found -- HIR (`literal_pattern`) and the lower-stage
    /// class-body guard hand their own literal's pieces to this one parser,
    /// so the two folds can never disagree about what a pattern admits.
    /// The encoding flag is ignored: it changes what BYTES the pattern
    /// matches, not what the literal fold below can admit.
    pub(crate) fn parse(src: &str, flags: &crate::hir::RegexpFlags) -> Option<LiteralPattern> {
        let ignore_case = flags.ignore_case;
        if flags.extended || flags.multiline {
            return None;
        }
        let mut src = src;
        let anchored_start = ["\\A", "^"].iter().any(|p| match src.strip_prefix(p) {
            Some(rest) => {
                src = rest;
                true
            }
            None => false,
        });
        let anchored_end = ["\\z", "$"].iter().any(|s| match src.strip_suffix(s) {
            Some(rest) => {
                src = rest;
                true
            }
            None => false,
        });
        // An anchor binds only ONE alternative in ruby -- `/^a|b/` is `(^a)|b`,
        // not `^(a|b)` -- so an anchored pattern that alternates is not this
        // simple.
        if (anchored_start || anchored_end) && src.contains('|') {
            return None;
        }
        let alts: Vec<Alternative> = src
            .split('|')
            .map(literal_alternative)
            .collect::<Option<_>>()?;
        // ASCII case folding is only ruby's answer for an ASCII pattern.
        if ignore_case
            && !alts.iter().all(|a| {
                a.0.iter()
                    .all(|e| !matches!(e, Elem::Lit(c) if !c.is_ascii()))
            })
        {
            return None;
        }
        Some(LiteralPattern {
            anchored_start,
            anchored_end,
            ignore_case,
            alts,
        })
    }

    pub(crate) fn matches(&self, subject: &str) -> Option<bool> {
        if self.ignore_case && !subject.is_ascii() {
            return None;
        }
        // `^`/`$` are LINE anchors, and this treats them as string anchors. The
        // two agree on a subject with no newline in it, which every build-time
        // string here is; anything else declines rather than pick a reading.
        if (self.anchored_start || self.anchored_end) && subject.contains('\n') {
            return None;
        }
        Some(self.alts.iter().any(|alt| self.matches_alt(alt, subject)))
    }

    fn matches_alt(&self, alt: &Alternative, subject: &str) -> bool {
        let starts: Box<dyn Iterator<Item = usize>> = if self.anchored_start {
            Box::new(std::iter::once(0))
        } else {
            Box::new((0..=subject.len()).filter(|&i| subject.is_char_boundary(i)))
        };
        starts
            .into_iter()
            .any(|i| match alt.match_at(&subject[i..], self.ignore_case) {
                Some(n) if self.anchored_end => i + n == subject.len(),
                Some(_) => true,
                None => false,
            })
    }
}

/// One alternative parsed into its positions, or `None` if it uses regexp
/// syntax this reduction does not admit.
fn literal_alternative(src: &str) -> Option<Alternative> {
    // `.` is deliberately absent: it is handled below, exactly. Every
    // quantifier stays here, which is what keeps a `.` bound to one character.
    const SYNTAX: &[char] = &['^', '$', '[', ']', '(', ')', '*', '+', '?', '{', '}', '|'];
    let mut out = Vec::new();
    let mut chars = src.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            '\\' => match chars.next() {
                Some('.') => Elem::Lit('.'),
                _ => return None,
            },
            '.' => Elem::AnyChar,
            c if SYNTAX.contains(&c) => return None,
            c => Elem::Lit(c),
        });
    }
    (!out.is_empty()).then_some(Alternative(out))
}

pub(super) fn literal_pattern(compiler: &Compiler, node: NodeId) -> Option<LiteralPattern> {
    let HirNode::RegexpLit(parts, flags) = &compiler.hir[node] else {
        return None;
    };
    let [StrPart::Lit(src)] = parts.as_slice() else {
        return None;
    };
    LiteralPattern::parse(src, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fold decides which branch of a program is COMPILED, so a pattern it
    /// reads wrongly picks the wrong half. Every expectation checked against
    /// ruby 4.0.6.
    #[test]
    fn an_alternative_is_the_positions_it_spells() {
        let elems = |src| literal_alternative(src).map(|a| a.0);
        // `\.` is an escape that means a real dot; a bare `.` is any character.
        assert_eq!(
            elems(r"1\.8"),
            Some(vec![Elem::Lit('1'), Elem::Lit('.'), Elem::Lit('8')])
        );
        assert_eq!(
            elems("1.8"),
            Some(vec![Elem::Lit('1'), Elem::AnyChar, Elem::Lit('8')])
        );
        assert_eq!(
            elems("mingw"),
            Some("mingw".chars().map(Elem::Lit).collect::<Vec<_>>())
        );
        // Anything with regexp meaning this cannot spell exactly must decline.
        for src in [
            "a+",    // repetition
            ".*",    // a quantified `.` is any NUMBER of characters
            "[0-9]", // a class
            "(a)",   // a group
            r"\d",   // an escape that is not `\.`
            r"a\",   // a dangling backslash
            "",      // nothing to match
        ] {
            assert!(elems(src).is_none(), "{src:?} is not spellable");
        }
    }

    #[test]
    fn an_anchored_pattern_matches_by_position() {
        let p = |anchored_start, anchored_end, alts: &[&str]| LiteralPattern {
            anchored_start,
            anchored_end,
            ignore_case: false,
            alts: alts
                .iter()
                .map(|s| literal_alternative(s).unwrap())
                .collect(),
        };
        // `RUBY_VERSION =~ /^1\.8/` on 4.0.6 -- the ipaddress guard.
        assert_eq!(p(true, false, &[r"1\.8"]).matches("4.0.6"), Some(false));
        assert_eq!(p(true, false, &[r"1\.8"]).matches("1.8.7"), Some(true));
        // Unanchored is a substring test, which is how the platform gates read.
        assert_eq!(
            p(false, false, &["mingw", "mswin"]).matches("x64-mingw32"),
            Some(true)
        );
        assert_eq!(
            p(false, false, &["mingw", "mswin"]).matches("arm64-darwin24"),
            Some(false)
        );
        // A `$` anchor is a suffix, and both anchors together are equality.
        assert_eq!(
            p(false, true, &["darwin24"]).matches("arm64-darwin24"),
            Some(true)
        );
        assert_eq!(p(true, true, &["ruby"]).matches("ruby3"), Some(false));
        assert_eq!(p(true, true, &["ruby"]).matches("ruby"), Some(true));
    }

    /// `.` is ONE character, wherever it sits. gmp's `unless RUBY_VERSION =~
    /// /^1.8/` is the shape, and it reads the same as `/^1\.8/` on every real
    /// version string -- but not on one where the dot is something else, which
    /// is why it is matched rather than assumed.
    #[test]
    fn any_char_consumes_exactly_one_character() {
        let p = |anchored_start, src: &str| LiteralPattern {
            anchored_start,
            anchored_end: false,
            ignore_case: false,
            alts: vec![literal_alternative(src).unwrap()],
        };
        assert_eq!(p(true, "1.8").matches("1.8.7"), Some(true));
        assert_eq!(p(true, "1.8").matches("108"), Some(true));
        assert_eq!(p(true, "1.8").matches("4.0.6"), Some(false));
        // One character, so a subject one short cannot match.
        assert_eq!(p(true, "1.8").matches("18"), Some(false));
        // `.` never matches a newline, even unanchored.
        assert_eq!(p(false, "a.b").matches("a\nb"), Some(false));
        assert_eq!(p(false, "a.b").matches("xaybz"), Some(true));
    }

    /// `/i` is ASCII folding here. ruby's is the full Unicode case table, so a
    /// subject that is not ASCII gets no answer rather than the narrower one.
    #[test]
    fn ignore_case_answers_only_for_ascii() {
        let p = |src: &str| LiteralPattern {
            anchored_start: false,
            anchored_end: false,
            ignore_case: true,
            alts: vec![literal_alternative(src).unwrap()],
        };
        // `RUBY_PLATFORM =~ /java/i`, the shape jruby gates on.
        assert_eq!(p("java").matches("Java-1.7"), Some(true));
        assert_eq!(p("JAVA").matches("x86_64-java"), Some(true));
        assert_eq!(p("java").matches("arm64-darwin24"), Some(false));
        assert_eq!(p("java").matches("\u{212a}elvin"), None);
    }
}
