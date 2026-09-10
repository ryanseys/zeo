//! What went wrong during the analyze pass, and WHERE.
//!
//! The pass raised bare `String`s, so a rejection said what it refused but not
//! which line of Ruby it refused -- and analyze is where most of the front
//! end's refusals live (an unsupported superclass, an operator `def` on a
//! reopened builtin, an alias with no source, a class body under an undecidable
//! guard). A ledger row reading `subclassing the built-in type \`Module\`
//! isn't supported yet` names a construct that appears in dozens of files.
//!
//! This is `lower::LowerError`'s smaller sibling and works the same way:
//! rejection sites keep raising bare messages (`Err("...".into())` still
//! compiles, via `From`), and the STATEMENT WALK stamps the span on the way out
//! -- `process_top_stmt` knows the `NodeId` it was handed, so every site inside
//! it is located without touching the site. Statement granularity is the right
//! unit here: analyze refuses definitions, and a definition IS a statement.
//!
//! There is no `kind` field, unlike `LowerError`. That split exists because
//! `eval` must raise `SyntaxError` or `NotImplementedError` depending on which
//! it was; analyze rejections are all the second kind.

use crate::hir::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzeError {
    pub message: String,
    /// Where in the source the rejected construct sits. `None` for an error
    /// raised outside any statement frame -- the whole-program invariant
    /// checks, and anything reached before the walk starts.
    pub span: Option<Span>,
}

impl AnalyzeError {
    /// Stamps `span` unless an inner frame already claimed one. The innermost
    /// frame wins, so a nested walk keeps the precise statement rather than
    /// the outer one that contains it.
    pub fn with_span_if_missing(mut self, span: Option<Span>) -> AnalyzeError {
        if self.span.is_none() {
            self.span = span;
        }
        self
    }
}

impl fmt::Display for AnalyzeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<String> for AnalyzeError {
    fn from(message: String) -> AnalyzeError {
        AnalyzeError {
            message,
            span: None,
        }
    }
}

impl From<&str> for AnalyzeError {
    fn from(message: &str) -> AnalyzeError {
        AnalyzeError::from(message.to_string())
    }
}

/// A lowering that runs INSIDE the analyze pass -- the runtime spelling of a
/// `undef`, say -- reports through this pass's error. The span comes with it,
/// which is more precise than the statement the walk would stamp.
impl From<crate::diagnostics::lower::LowerError> for AnalyzeError {
    fn from(e: crate::diagnostics::lower::LowerError) -> AnalyzeError {
        AnalyzeError {
            message: e.message,
            span: e.span,
        }
    }
}

/// The pass still reports a plain message to anything reading its text -- the
/// in-process harnesses assert on exact strings, and the ledger records them.
impl From<AnalyzeError> for String {
    fn from(e: AnalyzeError) -> String {
        e.message
    }
}
