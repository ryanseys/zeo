//! The compiler's typed error surface, and how the CLI renders it.
//!
//! `CompileError` says which STAGE failed; the `Lower` and `Codegen`
//! variants additionally carry the registered source file and byte span
//! their pass (`crate::lower`, `crate::clif`) stamped on the way out, so
//! `main.rs` can render a miette source excerpt pointing at the offending
//! construct. Analyze errors are typed but message-only for now --
//! `Analyze` reserves an `Option<Span>` field so individual sites can be
//! located incrementally without another signature migration.
//!
//! Rendering lives at the CLI boundary ONLY (miette is not a dependency of
//! the front end or the runtime): the library API converts to plain `String`s
//! via `Display`/`From`, so the in-process harnesses keep asserting on the
//! exact message text they always have.

use crate::analyze_error::AnalyzeError;
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
    /// prism's own multi-line report -- see `LowerError::report`.
    report: Option<String>,
}

impl LowerDiagnostic {
    /// See `CompileError::syntax_message`. The `parse error:` lead is this
    /// diagnostic's own -- it names the PASS for a reader of the CLI --
    /// while ruby's `SyntaxError` carries prism's message alone.
    fn syntax_message(&self) -> Option<&str> {
        if self.kind != LowerErrorKind::Syntax {
            return None;
        }
        if let Some(report) = &self.report {
            return Some(report.as_str());
        }
        let msg = self.message.as_str();
        Some(msg.strip_prefix("parse error: ").unwrap_or(msg))
    }

    fn new(err: LowerError, files: &[SourceFile]) -> LowerDiagnostic {
        let located = err.span.map(|s: Span| {
            let f = &files[s.file.0 as usize];
            (
                // `miette` wants an owned source; this is the error path, so
                // the copy costs nothing a successful compile pays.
                Box::new(NamedSource::new(&f.name, f.source.to_string())),
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
            report: err.report,
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
        // One code per PASS, named for the pass: `zeo::parse`, `zeo::lower`,
        // `zeo::analyze`, `zeo::codegen`. The code is how a caller learns
        // WHICH pass refused without parsing the message, so the four are
        // kept the same shape.
        Some(Box::new(match self.kind {
            LowerErrorKind::Syntax => "zeo::parse",
            LowerErrorKind::Unsupported => "zeo::lower",
        }))
    }

    /// rustc's rule is that `help` shows a change the reader can MAKE and a
    /// `note` carries everything else; miette has no `note`, so these two --
    /// which say which KIND of problem this is rather than how to fix it --
    /// live here for want of anywhere better. The fix, where there is one, is
    /// already in the message: `ffi_lib` names the forms it accepts.
    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
        Some(Box::new(match self.kind {
            LowerErrorKind::Syntax => "a Ruby syntax error, not a zeo gap",
            LowerErrorKind::Unsupported => "zeo can't compile this yet",
        }))
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.src.as_ref().map(|s| &**s as &dyn SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let (start, len) = self.span?;
        // A label names what is true AT THIS SPOT and leaves the WHAT to the
        // message above it (RFC 1644's `second mutable borrow occurs here`) --
        // so neither of these restates its own message.
        let label = match self.kind {
            LowerErrorKind::Syntax => "syntax error here",
            LowerErrorKind::Unsupported => "zeo stops here",
        };
        Some(Box::new(std::iter::once(LabeledSpan::new(
            Some(label.to_string()),
            start,
            len,
        ))))
    }
}

/// A located analyze failure -- `LowerDiagnostic`'s sibling, built at the pass
/// boundary (`analyze::analyze`), the one place both the error and
/// `Hir::files` are in scope.
///
/// Separate from `LowerDiagnostic` rather than shared with it because the two
/// say different things to the reader: lowering refuses a CONSTRUCT and points
/// at the token, analyze refuses a DEFINITION and points at the statement.
/// The code and help text differ accordingly.
#[derive(Debug)]
pub struct AnalyzeDiagnostic {
    message: String,
    /// Boxed for the same reason `LowerDiagnostic`'s is -- see there.
    src: Option<Box<NamedSource<String>>>,
    /// `(byte offset, length)` into `src`.
    span: Option<(usize, usize)>,
}

impl AnalyzeDiagnostic {
    fn new(err: AnalyzeError, files: &[SourceFile]) -> AnalyzeDiagnostic {
        let located = err.span.and_then(|s: Span| {
            // A span outliving its file table would be a bug, but a panic in
            // the error path would replace a real diagnostic with a worse one.
            let f = files.get(s.file.0 as usize)?;
            Some((
                // `miette` wants an owned source; this is the error path, so
                // the copy costs nothing a successful compile pays.
                Box::new(NamedSource::new(&f.name, f.source.to_string())),
                (s.start as usize, (s.end - s.start) as usize),
            ))
        });
        let (src, span) = match located {
            Some((src, span)) => (Some(src), Some(span)),
            None => (None, None),
        };
        AnalyzeDiagnostic {
            message: err.message,
            src,
            span,
        }
    }
}

impl fmt::Display for AnalyzeDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AnalyzeDiagnostic {}

impl Diagnostic for AnalyzeDiagnostic {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        Some(Box::new("zeo::analyze"))
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.src.as_ref().map(|s| &**s as &dyn SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let (start, len) = self.span?;
        Some(Box::new(std::iter::once(LabeledSpan::new(
            Some("rejected here".to_string()),
            start,
            len,
        ))))
    }
}

/// A codegen rejection with the Ruby it could not emit.
///
/// Its own type rather than a reuse of `AnalyzeDiagnostic` for the reason that
/// one is separate from `LowerDiagnostic`: the three stages refuse different
/// things and the reader is owed the difference. Codegen refuses a node in a
/// POSITION -- a definition where a value belongs -- so the label names the
/// position rather than the construct.
#[derive(Debug)]
pub struct CodegenDiagnostic {
    message: String,
    /// Boxed for the same reason `LowerDiagnostic`'s is -- see there.
    src: Option<Box<NamedSource<String>>>,
    /// `(byte offset, length)` into `src`.
    span: Option<(usize, usize)>,
}

impl CodegenDiagnostic {
    fn new(message: String, span: Option<Span>, files: &[SourceFile]) -> CodegenDiagnostic {
        let located = span.and_then(|s: Span| {
            let f = files.get(s.file.0 as usize)?;
            Some((
                // `miette` wants an owned source; this is the error path, so
                // the copy costs nothing a successful compile pays.
                Box::new(NamedSource::new(&f.name, f.source.to_string())),
                (s.start as usize, (s.end - s.start) as usize),
            ))
        });
        let (src, span) = match located {
            Some((src, span)) => (Some(src), Some(span)),
            None => (None, None),
        };
        CodegenDiagnostic { message, src, span }
    }
}

impl fmt::Display for CodegenDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodegenDiagnostic {}

impl Diagnostic for CodegenDiagnostic {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        Some(Box::new("zeo::codegen"))
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.src.as_ref().map(|s| &**s as &dyn SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let (start, len) = self.span?;
        Some(Box::new(std::iter::once(LabeledSpan::new(
            Some("cannot be emitted here".to_string()),
            start,
            len,
        ))))
    }
}

/// One of Ruby's own PARSE-time warnings, carried from the compiler to the
/// compiled program. CRuby prints these before the program runs; a zeo binary
/// prints them at startup, which is the same position relative to any program
/// output. `Display` is CRuby's exact line.
#[derive(Clone)]
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

    #[error(transparent)]
    #[diagnostic(transparent)]
    Analyze(AnalyzeDiagnostic),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Codegen(CodegenDiagnostic),

    #[error("{message}")]
    #[diagnostic(code(zeo::report))]
    Report { message: String },
}

impl CompileError {
    /// The message of a SOURCE-level rejection -- one prism refused to
    /// parse -- and `None` for every rejection about a shape zeo declines.
    /// A run-time `eval` needs the two apart: the first is the program's
    /// own `SyntaxError` to raise, the second is a compiler limit.
    #[must_use]
    pub fn syntax_message(&self) -> Option<&str> {
        match self {
            CompileError::Lower(d) => d.syntax_message(),
            _ => None,
        }
    }

    /// The driver-boundary conversion -- see `LowerDiagnostic`.
    pub fn lower(err: LowerError, files: &[SourceFile]) -> CompileError {
        CompileError::Lower(LowerDiagnostic::new(err, files))
    }

    /// An unlocated analyze rejection -- for callers outside the pass, which
    /// have no `Hir::files` to resolve a span against.
    pub fn analyze(message: impl Into<String>) -> CompileError {
        CompileError::Analyze(AnalyzeDiagnostic {
            message: message.into(),
            src: None,
            span: None,
        })
    }

    /// The driver-boundary conversion for the analyze pass -- see
    /// `AnalyzeDiagnostic`.
    pub fn analyze_located(err: AnalyzeError, files: &[SourceFile]) -> CompileError {
        CompileError::Analyze(AnalyzeDiagnostic::new(err, files))
    }

    /// An unlocated codegen rejection.
    pub fn codegen(message: impl Into<String>) -> CompileError {
        CompileError::Codegen(CodegenDiagnostic {
            message: message.into(),
            src: None,
            span: None,
        })
    }

    /// A codegen rejection that names the Ruby it could not emit. `span` is the
    /// node's own, resolved against the same file table the other two stages
    /// use -- so a codegen gap renders the same excerpt header, and the gem
    /// probe reads a location out of it exactly as it does for the others.
    pub fn codegen_located(
        message: impl Into<String>,
        span: Option<Span>,
        files: &[SourceFile],
    ) -> CompileError {
        CompileError::Codegen(CodegenDiagnostic::new(message.into(), span, files))
    }

    /// The driver-boundary conversion for the emitter -- see
    /// `CodegenDiagnostic`. The kind is dropped here: unlike lowering, a
    /// codegen error never has to become a Ruby exception through this
    /// path (the run-time `eval` boundary converts for itself).
    pub fn from_codegen(
        err: crate::codegen_error::CodegenError,
        files: &[SourceFile],
    ) -> CompileError {
        CompileError::codegen_located(err.message, err.span, files)
    }
}

/// The compatibility shim: `Result<_, String>` consumers (the test
/// harnesses, `check_program`) get the plain message, kind and span
/// dropped at this one boundary.
impl From<CompileError> for String {
    fn from(err: CompileError) -> String {
        err.to_string()
    }
}
