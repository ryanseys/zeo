//! What went wrong while emitting CLIF from HIR, WHICH KIND of wrong it
//! was, and WHERE.
//!
//! `lower`'s sibling for the back end. The emitter had one error
//! type -- `String` -- for two situations a reader must be able to tell
//! apart:
//!
//! - valid Ruby the CLIF backend does not emit yet, which a source change
//!   can route around (`Unsupported`)
//! - the emitter's own machinery failing -- a cranelift declare/define
//!   error, an ownership imbalance -- which no source change fixes
//!   (`Internal`)
//!
//! Unlike `LowerError` there is NO `From<String>`: every construction site
//! states its kind deliberately, because a bare-message default would let
//! an internal failure masquerade as a scope refusal.
//!
//! The `span` is stamped by the `lower_expr`/`lower_stmt` wrappers on the
//! way OUT: the innermost node being emitted when the error arose wins
//! (`with_span_if_missing`). `None` survives only for errors outside any
//! node frame -- module-level machinery, per-class rejections with no
//! definition node in reach.

use crate::hir::Span;
use std::fmt;

/// The emitter's error channel: every fallible CLIF lowering speaks this.
pub(crate) type CResult<T> = Result<T, CodegenError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodegenErrorKind {
    /// Valid Ruby this backend does not emit yet -- a scope refusal the
    /// user's source sits behind.
    Unsupported,
    /// The emitter's own machinery failed -- an internal compiler error no
    /// source change fixes.
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    pub kind: CodegenErrorKind,
    pub message: String,
    /// Where in the source the refused construct sits -- `None` for errors
    /// with no node in reach (see the module docs).
    pub span: Option<Span>,
}

impl CodegenError {
    pub fn unsupported(message: impl Into<String>, span: Option<Span>) -> CodegenError {
        CodegenError {
            kind: CodegenErrorKind::Unsupported,
            message: message.into(),
            span: span.and_then(Span::known),
        }
    }

    /// An internal failure has no source position by construction: the
    /// source did nothing wrong.
    pub fn internal(message: impl Into<String>) -> CodegenError {
        CodegenError {
            kind: CodegenErrorKind::Internal,
            message: message.into(),
            span: None,
        }
    }

    /// Stamps `span` unless an INNER frame already did -- the
    /// `lower_expr`/`lower_stmt` wrappers call this at every level on the
    /// way out, so the deepest (most precise) location wins.
    pub fn with_span_if_missing(mut self, span: Option<Span>) -> CodegenError {
        if self.span.is_none() {
            self.span = span.and_then(Span::known);
        }
        self
    }
}

impl fmt::Display for CodegenError {
    /// Just the message: locations render at the boundary, not in here.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// The one boundary where kind and span are dropped -- `Result<_, String>`
/// consumers get the plain message, with no location suffix re-added (a
/// caller that wants the location resolves the span itself).
impl From<CodegenError> for String {
    fn from(err: CodegenError) -> String {
        err.message
    }
}

#[cfg(test)]
mod tests {
    use super::{CodegenError, CodegenErrorKind};
    use crate::hir::{FileId, Span};

    #[test]
    fn each_constructor_states_its_kind() {
        let span = Span {
            file: FileId(0),
            start: 2,
            end: 7,
        };
        let e = CodegenError::unsupported("no emit for this", Some(span));
        assert_eq!(e.kind, CodegenErrorKind::Unsupported);
        assert_eq!(e.span, Some(span));
        let e = CodegenError::internal("declare failed");
        assert_eq!(e.kind, CodegenErrorKind::Internal);
        assert_eq!(e.span, None);
    }

    /// `with_span_if_missing` is innermost-wins: once a frame stamped a
    /// span, outer frames leave it alone.
    #[test]
    fn the_innermost_span_wins() {
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
        let e = CodegenError::unsupported("nope", None)
            .with_span_if_missing(Some(inner))
            .with_span_if_missing(Some(outer));
        assert_eq!(e.span, Some(inner));
        // SYNTH never overwrites and never sticks.
        let e = CodegenError::unsupported("nope", None).with_span_if_missing(Some(Span::SYNTH));
        assert_eq!(e.span, None);
    }

    /// The compiler prints the same text either way -- the split adds a
    /// kind and a location, it does not change any existing message.
    #[test]
    fn display_and_string_are_the_message_alone() {
        let e = CodegenError::internal("boom");
        assert_eq!(e.to_string(), "boom");
        assert_eq!(String::from(e), "boom");
    }
}
