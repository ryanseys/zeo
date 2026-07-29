//! The multi-encoding string engine.
//!
//! A Ruby `String` is *bytes plus an interpretation*: the same three bytes
//! `[0xE2, 0x82, 0xAC]` are the single character `€` under UTF-8 but three
//! separate bytes under ASCII-8BIT. So a [`StrBuf`] stores the raw `bytes`
//! and an [`EncodingId`] naming how to read them -- exactly CRuby's own
//! `RString` + `rb_encoding` split.
//!
//! Everything an encoding needs to know lives in one [`EncodingSpec`] row of
//! the static [`ENCODINGS`] table; adding a new encoding (Windows-1252,
//! Shift_JIS, ...) is a new row, never a structural change.
//!
//! This module is a LEAF: it knows nothing of the runtime's value model or
//! exception machinery. Refusals come back as plain error values
//! ([`IncompatibleEncodings`], [`TranscodeError`]) and the runtime maps them
//! to the exact CRuby exception classes at its boundary.

mod case;
mod coderange;
mod defaults;
mod inspect;
pub(crate) mod iso2022jp;
mod mb;
mod single_byte;
mod strbuf;
mod table;
mod transcode;
mod wide;

pub use coderange::CodeRange;
pub use defaults::{
    default_external, default_internal, set_default_external, set_default_internal,
};
pub use inspect::inspect;
pub use mb::{MbCodepointError, MbFamily, mb_codepoint_bytes};
pub use single_byte::SingleByteTable;
pub use strbuf::{IncompatibleEncodings, StrBuf, compat_concat_enc};
pub use table::{
    ASCII_8BIT, BIG5, ENCODINGS, EUC_JP, EncKind, EncodingId, EncodingSpec, GBK, ISO_2022_JP,
    ISO_8859_1, ISO_8859_2, ISO_8859_15, KOI8_R, SHIFT_JIS, US_ASCII, UTF_8, UTF_16, UTF_16BE,
    UTF_16LE, UTF_32, UTF_32BE, UTF_32LE, WINDOWS_31J, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252,
    WINDOWS_1253, WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257, all, find,
};
pub(crate) use transcode::{Unit, decode};
pub use transcode::{
    NewlineMode, TranscodeError, TranscodeFallback, TranscodeOptions, XmlMode, encode_scalar,
    transcode,
};

#[cfg(test)]
mod tests;
