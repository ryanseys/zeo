//! The compiler's typed error surface, and how the CLI renders it.
//!
//! `CompileError` says which STAGE failed; the `Lower` variant additionally
//! carries the registered source file and byte span the lowering (`crate::
//! lower`) stamped on the way out, so `main.rs` can render a miette source
//! excerpt pointing at the
//! offending construct. Analyze/codegen errors are typed but message-only
//! for now -- `Analyze` reserves an `Option<Span>` field so individual sites
//! can be located incrementally without another signature migration.
//!
//! Rendering lives at the CLI boundary ONLY (miette is not a dependency of
//! the front end or the runtime): the library API converts to plain `String`s
//! via `Display`/`From`, so the in-process harnesses keep asserting on the
//! exact message text they always have.

use crate::hir::{SourceFile, Span};
use crate::lower_error::{LowerError, LowerErrorKind};
use miette::{Diagnostic, LabeledSpan, NamedSource, SourceCode};
use std::fmt;
use thiserror::Error;

/// A located lowering failure: the `LowerError` plus everything a renderer
/// needs (file name, source text, byte span). Built at the driver boundary
/// (`parse::parse_and_lower_with`), the one place both the error and the
/// `Hir::files` table are in scope.
///
/// `Diagnostic` is implemented by hand rather than derived because the
/// source/span pair is genuinely optional (pre-parse failures, the
/// exception prelude, `eval` bodies) and the code/help/label text depends
/// on the KIND -- both awkward to express in the derive.
#[derive(Debug)]
pub struct LowerDiagnostic {
    kind: LowerErrorKind,
    message: String,
    /// Boxed so `CompileError` stays a small `Err` payload
    /// (clippy::result_large_err) -- the source text only exists on the
    /// cold path anyway.
    src: Option<Box<NamedSource<String>>>,
    /// `(byte offset, length)` into `src`.
    span: Option<(usize, usize)>,
}

impl LowerDiagnostic {
    fn new(err: LowerError, files: &[SourceFile]) -> LowerDiagnostic {
        let located = err.span.map(|s: Span| {
            let f = &files[s.file.0 as usize];
            (
                Box::new(NamedSource::new(&f.name, f.source.clone())),
                (s.start as usize, (s.end - s.start) as usize),
            )
        });
        let (src, span) = match located {
            Some((src, span)) => (Some(src), Some(span)),
            None => (None, None),
        };
        LowerDiagnostic {
            kind: err.kind,
            message: err.message,
            src,
            span,
        }
    }
}

impl fmt::Display for LowerDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LowerDiagnostic {}

impl Diagnostic for LowerDiagnostic {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        Some(Box::new(match self.kind {
            LowerErrorKind::Syntax => "zeo::lower::syntax",
            LowerErrorKind::Unsupported => "zeo::lower::unsupported",
        }))
    }

    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
        Some(Box::new(match self.kind {
            LowerErrorKind::Syntax => "this source is not valid Ruby -- `ruby -c` rejects it too",
            LowerErrorKind::Unsupported => {
                "valid Ruby that zeo does not compile yet (NotImplementedError territory, \
                 not a SyntaxError)"
            }
        }))
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.src.as_ref().map(|s| &**s as &dyn SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let (start, len) = self.span?;
        let label = match self.kind {
            LowerErrorKind::Syntax => "not valid Ruby",
            LowerErrorKind::Unsupported => "not lowered yet",
        };
        Some(Box::new(std::iter::once(LabeledSpan::new(
            Some(label.to_string()),
            start,
            len,
        ))))
    }
}

/// One of Ruby's own PARSE-time warnings, carried from the compiler to the
/// compiled program. CRuby prints these before the program runs; a zeo binary
/// prints them at startup, which is the same position relative to any program
/// output. `Display` is CRuby's exact line.
pub struct CompileWarning {
    pub file: String,
    pub line: u32,
    pub message: String,
}

impl std::fmt::Display for CompileWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: warning: {}", self.file, self.line, self.message)
    }
}

/// Which compile stage failed. The `From<CompileError> for String` shim keeps
/// every `Result<_, String>` consumer working unchanged (the message text is
/// exactly what those callers always saw).
#[derive(Debug, Error, Diagnostic)]
pub enum CompileError {
    #[error(transparent)]
    #[diagnostic(transparent)]
    Lower(#[from] LowerDiagnostic),

    #[error("{message}")]
    #[diagnostic(code(zeo::analyze))]
    Analyze {
        message: String,
        /// Reserved: analyze sites gain locations incrementally (a `NodeId`
        /// in scope resolves to a span via `Hir::span`); none are stamped
        /// yet, so rendering ignores this until they are.
        span: Option<Span>,
    },

    #[error("{message}")]
    #[diagnostic(code(zeo::codegen))]
    Codegen { message: String },

    #[error("{message}")]
    #[diagnostic(code(zeo::report))]
    Report { message: String },
}

impl CompileError {
    /// The driver-boundary conversion -- see `LowerDiagnostic`.
    pub fn lower(err: LowerError, files: &[SourceFile]) -> CompileError {
        CompileError::Lower(LowerDiagnostic::new(err, files))
    }

    pub fn analyze(message: impl Into<String>) -> CompileError {
        CompileError::Analyze {
            message: message.into(),
            span: None,
        }
    }

    pub fn codegen(message: impl Into<String>) -> CompileError {
        CompileError::Codegen {
            message: message.into(),
        }
    }
}

/// The compatibility shim: `Result<_, String>` consumers (the test
/// harnesses, `compile_to_rust`) get the plain message, kind and span
/// dropped at this one boundary.
impl From<CompileError> for String {
    fn from(err: CompileError) -> String {
        err.to_string()
    }
}
