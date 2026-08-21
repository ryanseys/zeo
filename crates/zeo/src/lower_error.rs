//! What went wrong while lowering Ruby source into HIR, WHICH KIND of wrong
//! it was, and WHERE.
//!
//! The front end had one error type -- `String` -- for two situations a Ruby
//! program must be able to tell apart:
//!
//! - the source is not valid Ruby (prism rejected it), which is a `SyntaxError`
//! - the source is valid Ruby that zeo does not lower yet, which is a
//!   `NotImplementedError`
//!
//! For the COMPILER the difference is cosmetic: both print and stop. It stops
//! being cosmetic the moment lowering runs at RUN time -- `eval` of a string
//! has to raise one or the other, and reporting a zeo scope limitation as a
//! `SyntaxError` tells the user their own code is malformed when it isn't.
//! Splitting the type means each rejection site states its kind while the
//! author is right there, instead of a later pass guessing from message text.
//!
//! The `span` is stamped by the `lower_node` wrapper on the way OUT: the
//! innermost node being lowered when the error arose wins
//! (`with_span_if_missing`), so rejection sites keep raising bare messages
//! and still end up located. `None` survives only for errors outside any
//! span frame -- source strings with no registered file (the exception
//! prelude, `eval` bodies) and pre-parse failures.

use crate::hir::Span;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowerErrorKind {
    /// Not valid Ruby -- prism refused to parse it. Ruby: `SyntaxError`.
    Syntax,
    /// Valid Ruby this front end does not lower yet. Ruby:
    /// `NotImplementedError`. The default for a bare message, because that is
    /// what the overwhelming majority of the front end's rejections are.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerError {
    pub kind: LowerErrorKind,
    pub message: String,
    /// Where in the source the rejected construct sits -- see the module
    /// docs for when this is `None`.
    pub span: Option<Span>,
    /// The text a `SyntaxError` raised for this source carries: prism's own
    /// multi-line report (`parse::syntax_report`), which is what CRuby puts
    /// in the exception. `message` stays the one-line form the CLI renders.
    pub report: Option<String>,
}

impl LowerError {
    pub fn syntax(msg: impl Into<String>) -> LowerError {
        LowerError {
            kind: LowerErrorKind::Syntax,
            message: msg.into(),
            span: None,
            report: None,
        }
    }

    /// A parse failure that also carries the rich report -- what a run-time
    /// `eval` of this source raises.
    pub fn syntax_reported(msg: impl Into<String>, report: Option<String>) -> LowerError {
        LowerError {
            report,
            ..LowerError::syntax(msg)
        }
    }

    pub fn unsupported(msg: impl Into<String>) -> LowerError {
        LowerError {
            kind: LowerErrorKind::Unsupported,
            message: msg.into(),
            span: None,
            report: None,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// Stamps `span` unless an INNER frame already did -- the `lower_node`
    /// wrapper calls this at every level on the way out, so the deepest
    /// (most precise) location wins.
    pub fn with_span_if_missing(mut self, span: Span) -> LowerError {
        if self.span.is_none() {
            self.span = span.known();
        }
        self
    }

    /// The Ruby exception class a runtime lowering of this source should
    /// raise. Unused while lowering only ever runs at compile time; it is the
    /// whole reason the variants exist.
    pub fn ruby_class(&self) -> &'static str {
        match self.kind {
            LowerErrorKind::Syntax => "SyntaxError",
            LowerErrorKind::Unsupported => "NotImplementedError",
        }
    }
}

impl fmt::Display for LowerError {
    /// Just the message: the compiler's own output is unchanged by this split.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// A bare message is `Unsupported` -- this is what lets the ~50 `.ok_or("…")`
/// and `.ok_or_else(|| format!("…"))` rejection sites keep working unchanged
/// through `?`, while the handful of genuine parse failures name `Syntax`
/// explicitly.
impl From<String> for LowerError {
    fn from(msg: String) -> LowerError {
        LowerError::unsupported(msg)
    }
}

impl From<&str> for LowerError {
    fn from(msg: &str) -> LowerError {
        LowerError::unsupported(msg.to_string())
    }
}

/// The compiler's public surface is still `Result<_, String>`; this is the one
/// place the kind and span are dropped, and it happens at that boundary rather
/// than inside the front end.
impl From<LowerError> for String {
    fn from(err: LowerError) -> String {
        err.message
    }
}

#[cfg(test)]
mod tests {
    use super::{LowerError, LowerErrorKind};

    #[test]
    fn a_bare_message_defaults_to_unsupported() {
        let e: LowerError = "no good".into();
        assert_eq!(e, LowerError::unsupported("no good"));
        assert_eq!(e.ruby_class(), "NotImplementedError");
    }

    #[test]
    fn a_parse_failure_is_a_syntax_error() {
        let e = LowerError::syntax("unexpected end");
        assert_eq!(e.ruby_class(), "SyntaxError");
    }

    /// The compiler prints the same text either way -- the split adds a kind
    /// and a location, it does not change any existing message.
    #[test]
    fn display_is_the_message_alone() {
        assert_eq!(LowerError::syntax("boom").to_string(), "boom");
        assert_eq!(LowerError::unsupported("boom").to_string(), "boom");
    }

    /// `with_span_if_missing` is innermost-wins: once a frame stamped a
    /// span, outer frames leave it alone.
    #[test]
    fn the_innermost_span_wins() {
        use crate::hir::{FileId, Span};
        let inner = Span {
            file: FileId(0),
            start: 4,
            end: 9,
        };
        let outer = Span {
            file: FileId(0),
            start: 0,
            end: 20,
        };
        let e = LowerError::unsupported("nope")
            .with_span_if_missing(inner)
            .with_span_if_missing(outer);
        assert_eq!(e.span, Some(inner));
        // SYNTH never overwrites and never sticks.
        let e = LowerError::unsupported("nope").with_span_if_missing(Span::SYNTH);
        assert_eq!(e.span, None);
        assert_eq!(e.kind, LowerErrorKind::Unsupported);
    }
}

#[cfg(test)]
mod classification_tests {
    use super::LowerErrorKind;

    /// The distinction has to survive real lowering, not just hold at the type
    /// level: malformed source is the user's `SyntaxError`, while valid Ruby
    /// zeo hasn't implemented is zeo's `NotImplementedError`. Reporting
    /// the second as the first tells the user their code is broken when it
    /// isn't.
    #[test]
    fn lowering_distinguishes_bad_ruby_from_unimplemented_ruby() {
        let mut hir = crate::hir::Hir::default();
        let err = crate::lower::parse_and_lower_into(&mut hir, "def foo(\n")
            .expect_err("malformed source is rejected");
        assert!(
            matches!(err.kind, LowerErrorKind::Syntax),
            "malformed source must be a SyntaxError, got: {err:?}"
        );
        assert_eq!(err.ruby_class(), "SyntaxError");

        // Valid Ruby (prism parses it) that this front end declines to lower:
        // ruby matches a bare condition regexp against `$_`, a concept zeo does
        // not model at all.
        let mut hir = crate::hir::Hir::default();
        let err = crate::lower::parse_and_lower_into(&mut hir, "if /foo/ then 1 end\n")
            .expect_err("unsupported construct is rejected");
        assert!(
            matches!(err.kind, LowerErrorKind::Unsupported),
            "an unimplemented construct must be NotImplementedError, got: {err:?}"
        );
        assert_eq!(err.ruby_class(), "NotImplementedError");
    }
}
