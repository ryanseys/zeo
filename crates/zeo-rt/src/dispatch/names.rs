//! The name predicates ruby's reflection surface validates against.
//!
//! Sixteen "zeo accepts what ruby refuses" gaps were written one at a time,
//! over `rmodule.rs`, `rclass.rs`, `array.rs`, `rproc.rs` and `ivars.rs`.
//! There was no shared mechanism, so there was no shared omission, so there
//! were sixteen separate misses. This is the mechanism.
//!
//! Every rule below is probe-verified against ruby 4.0.6, and the surprising
//! ones are worth stating: an identifier here takes NO trailing `?`/`!`/`=`
//! (`attr_accessor :x?` is refused), non-ASCII letters ARE identifier
//! characters (`日本` is a valid name), and a constant is a SINGLE segment
//! (`const_set("A::B")` is `wrong constant name A::B`, not a nested write).

/// A local-variable or method-name identifier: an identifier start, then
/// identifier characters. No trailing `?`, `!` or `=` -- those are method
/// SPELLINGS, and the reflection rows that take a bare name refuse them.
pub(crate) fn is_local_or_const_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_alphabetic() && !c.is_ascii_digit())
        && chars.all(is_ident_char)
}

/// One constant SEGMENT: an ASCII-uppercase first character, then identifier
/// characters. Single segment on purpose -- see the module docs.
pub(crate) fn is_const_segment(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase()) && chars.all(is_ident_char)
}

/// An instance-variable name, `@` included: strip exactly one `@`, then the
/// local rule. `@@x` fails on the second `@` and `@1x` on the digit, which
/// is what makes the class-variable spelling a NameError rather than a
/// silent nil.
pub(crate) fn is_ivar_name(s: &str) -> bool {
    s.strip_prefix('@').is_some_and(is_local_or_const_name)
}

fn is_ident_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_names_take_no_trailing_sigil() {
        for ok in ["x", "_x", "x1", "foo_bar", "日本", "X"] {
            assert!(is_local_or_const_name(ok), "{ok}");
        }
        for bad in ["", "1bad", "x?", "x!", "x=", "@x", "a b", "a-b", "A::B"] {
            assert!(!is_local_or_const_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_constant_is_one_uppercase_segment() {
        for ok in ["A", "Ab", "A1", "A_b"] {
            assert!(is_const_segment(ok), "{ok}");
        }
        for bad in ["", "a", "1A", "A::B", "A b", "日本"] {
            assert!(!is_const_segment(bad), "{bad}");
        }
    }

    #[test]
    fn an_ivar_strips_exactly_one_at() {
        for ok in ["@x", "@_x", "@X", "@日本"] {
            assert!(is_ivar_name(ok), "{ok}");
        }
        for bad in ["", "@", "@@x", "@1x", "x", "@x?"] {
            assert!(!is_ivar_name(bad), "{bad}");
        }
    }
}
