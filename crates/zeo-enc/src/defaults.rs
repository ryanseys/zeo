//! The process-wide default external/internal encodings
//! (`Encoding.default_external` / `default_internal`).

use crate::table::{EncodingId, UTF_8};
use std::sync::atomic::{AtomicU8, Ordering};

// `u8::MAX` sentinel = "unset" for the optional default_internal.
static DEFAULT_EXTERNAL: AtomicU8 = AtomicU8::new(UTF_8.0);
static DEFAULT_INTERNAL: AtomicU8 = AtomicU8::new(u8::MAX);

pub fn default_external() -> EncodingId {
    EncodingId(DEFAULT_EXTERNAL.load(Ordering::Relaxed))
}
pub fn set_default_external(enc: EncodingId) {
    DEFAULT_EXTERNAL.store(enc.0, Ordering::Relaxed);
}
pub fn default_internal() -> Option<EncodingId> {
    match DEFAULT_INTERNAL.load(Ordering::Relaxed) {
        u8::MAX => None,
        v => Some(EncodingId(v)),
    }
}
pub fn set_default_internal(enc: Option<EncodingId>) {
    DEFAULT_INTERNAL.store(enc.map_or(u8::MAX, |e| e.0), Ordering::Relaxed);
}
