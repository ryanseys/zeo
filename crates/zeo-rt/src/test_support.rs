//! What the unit tests across this crate share. Compiled under `cfg(test)`
//! only, so nothing here reaches a shipped runtime.

use crate::RubyValue;

/// A ruby String holding `text`.
pub(crate) fn str_value(text: &str) -> RubyValue {
    RubyValue::Str(crate::string_new(text.to_string()))
}
