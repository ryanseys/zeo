//! The runtime's view of the encoding engine -- a re-export of the `crate::enc`
//! module, so the builtins read `crate::encoding::X`, plus the ONE piece that
//! cannot live there:
//! mapping the engine's plain error values onto real Ruby exceptions
//! (`Signal`). `crate::enc` knows nothing of the runtime's dispatch layer.

pub use crate::enc::*;

use crate::Signal;

/// `TranscodeError` -> the exact CRuby exception class per refusal kind.
/// A plain function, NOT a `From<TranscodeError> for Signal` impl: a second
/// `From<_>` into `Signal` makes the generated programs' `Ok({...})?`
/// blocks ambiguous (E0283 -- inference could no longer pick `E = Signal`).
pub fn transcode_signal(err: TranscodeError) -> Signal {
    let (class, message, detail) = match err {
        TranscodeError::InvalidByteSequence(m, d) => ("Encoding::InvalidByteSequenceError", m, d),
        TranscodeError::UndefinedConversion(m, d) => ("Encoding::UndefinedConversionError", m, d),
        // Carries no encoding pair to attach: the refusal is about the
        // MISSING converter, not about a particular offending character.
        TranscodeError::NoConverter(m) => {
            return crate::dispatch::raise_error("Encoding::ConverterNotFoundError", m);
        }
    };
    let signal = crate::dispatch::raise_error(class, message);
    // The encoding pair and the offending input travel on the exception, not
    // only in its message -- which is what `#source_encoding`, `#error_bytes`
    // and `#error_char` read back.
    if let Signal::Raise(exc) = &signal {
        crate::builtins::exception::attach_transcode_detail(exc, &detail);
    }
    signal
}
