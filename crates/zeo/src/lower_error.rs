//! What went wrong while lowering Ruby source into HIR, and WHICH KIND of
//! wrong it was.
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
//! Splitting the type now means each rejection site states its kind while the
//! author is right there, instead of a later pass guessing from message text.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    /// Not valid Ruby -- prism refused to parse it. Ruby: `SyntaxError`.
    Syntax(String),
    /// Valid Ruby this front end does not lower yet. Ruby:
    /// `NotImplementedError`. The default for a bare message, because that is
    /// what the overwhelming majority of the front end's rejections are.
    Unsupported(String),
}

impl LowerError {
    pub fn syntax(msg: impl Into<String>) -> LowerError {
        LowerError::Syntax(msg.into())
    }

    pub fn unsupported(msg: impl Into<String>) -> LowerError {
        LowerError::Unsupported(msg.into())
    }

    pub fn message(&self) -> &str {
        match self {
            LowerError::Syntax(m) | LowerError::Unsupported(m) => m,
        }
    }

    /// The Ruby exception class a runtime lowering of this source should
    /// raise. Unused while lowering only ever runs at compile time; it is the
    /// whole reason the variants exist.
    pub fn ruby_class(&self) -> &'static str {
        match self {
            LowerError::Syntax(_) => "SyntaxError",
            LowerError::Unsupported(_) => "NotImplementedError",
        }
    }
}

impl fmt::Display for LowerError {
    /// Just the message: the compiler's own output is unchanged by this split.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// A bare message is `Unsupported` -- this is what lets the ~50 `.ok_or("…")`
/// and `.ok_or_else(|| format!("…"))` rejection sites keep working unchanged
/// through `?`, while the handful of genuine parse failures name `Syntax`
/// explicitly.
impl From<String> for LowerError {
    fn from(msg: String) -> LowerError {
        LowerError::Unsupported(msg)
    }
}

impl From<&str> for LowerError {
    fn from(msg: &str) -> LowerError {
        LowerError::Unsupported(msg.to_string())
    }
}

/// The compiler's public surface is still `Result<_, String>`; this is the one
/// place the kind is dropped, and it happens at that boundary rather than
/// inside the front end.
impl From<LowerError> for String {
    fn from(err: LowerError) -> String {
        err.message().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::LowerError;

    #[test]
    fn a_bare_message_defaults_to_unsupported() {
        let e: LowerError = "no good".into();
        assert_eq!(e, LowerError::Unsupported("no good".to_string()));
        assert_eq!(e.ruby_class(), "NotImplementedError");
    }

    #[test]
    fn a_parse_failure_is_a_syntax_error() {
        let e = LowerError::syntax("unexpected end");
        assert_eq!(e.ruby_class(), "SyntaxError");
    }

    /// The compiler prints the same text either way -- the split adds a kind,
    /// it does not change any existing message.
    #[test]
    fn display_is_the_message_alone() {
        assert_eq!(LowerError::syntax("boom").to_string(), "boom");
        assert_eq!(LowerError::unsupported("boom").to_string(), "boom");
    }
}

#[cfg(test)]
mod classification_tests {
    use super::LowerError;

    /// The distinction has to survive real lowering, not just hold at the type
    /// level: malformed source is the user's `SyntaxError`, while valid Ruby
    /// zeo hasn't implemented is zeo's `NotImplementedError`. Reporting
    /// the second as the first tells the user their code is broken when it
    /// isn't.
    #[test]
    fn lowering_distinguishes_bad_ruby_from_unimplemented_ruby() {
        let err = crate::parse::parse_and_lower("def foo(\n")
            .err()
            .expect("malformed source is rejected");
        assert!(
            matches!(err, LowerError::Syntax(_)),
            "malformed source must be a SyntaxError, got: {err:?}"
        );
        assert_eq!(err.ruby_class(), "SyntaxError");

        // Valid Ruby (prism parses it) that this front end declines to lower.
        let err = crate::parse::parse_and_lower("p(/foo/e)\n")
            .err()
            .expect("unsupported construct is rejected");
        assert!(
            matches!(err, LowerError::Unsupported(_)),
            "an unimplemented construct must be NotImplementedError, got: {err:?}"
        );
        assert_eq!(err.ruby_class(), "NotImplementedError");
    }
}
