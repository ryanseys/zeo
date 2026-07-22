//! The runtime's view of the encoding engine -- a re-export of the
//! `zeo-enc` crate, so the builtins keep reading `crate::encoding::X`
//! unchanged, plus the ONE piece that cannot live there: mapping the
//! engine's plain error values onto real Ruby exceptions (`Signal`).
//! `zeo-enc` is a leaf crate that knows nothing of the runtime.

pub use zeo_enc::*;

use crate::Signal;

/// `TranscodeError` -> the exact CRuby exception class per refusal kind.
/// A plain function, NOT a `From<TranscodeError> for Signal` impl: a second
/// `From<_>` into `Signal` makes the generated programs' `Ok({...})?`
/// blocks ambiguous (E0283 -- inference could no longer pick `E = Signal`).
pub fn transcode_signal(err: TranscodeError) -> Signal {
    match err {
        TranscodeError::InvalidByteSequence(m) => {
            crate::dispatch::raise_error("Encoding::InvalidByteSequenceError", m)
        }
        TranscodeError::UndefinedConversion(m) => {
            crate::dispatch::raise_error("Encoding::UndefinedConversionError", m)
        }
    }
}
