//! `Zlib::GzipFile` -- the header accessors and lifecycle both directions
//! share, inherited by `GzipWriter` and `GzipReader` through the MRO.
//!
//! CRuby's `GzipFile` has no `new`: it exists to hold exactly these rows.

use super::frame;
use super::gzip::{gz_of, open};
use crate::RubyValue;
use zeo_macros::ruby_class;

ruby_class! {
    GzipFile = zeo_abi::ZLIB_GZIP_FILE_CLASS < zeo_abi::OBJECT_CLASS;

    // `#close` shuts the member down and closes the IO underneath if this
    // stream opened it; `#finish` leaves the IO alone. Both answer the IO --
    // which is how `GzipWriter.open(path) { … }` hands back the File.
    def "close" (recv) {
        gz_of(recv).state.lock().shut_down(true)
    }
    def "finish" (recv) {
        gz_of(recv).state.lock().shut_down(false)
    }
    def "closed?" (recv) {
        Ok(RubyValue::Bool(gz_of(recv).state.lock().closed))
    }
    def "to_io" (recv) {
        Ok(open(recv)?.io.clone())
    }

    // The header fields. A gzip header records only the two extreme
    // compression levels, so `#level` answers -1 for everything between --
    // see `frame::level_from_xfl`.
    def "orig_name" (recv) {
        Ok(text_or_nil(open(recv)?.header.orig_name.as_deref()))
    }
    def "comment" (recv) {
        Ok(text_or_nil(open(recv)?.header.comment.as_deref()))
    }
    def "level" (recv) {
        Ok(RubyValue::Int(frame::level_from_xfl(open(recv)?.header.xfl)))
    }
    def "os_code" (recv) {
        Ok(RubyValue::Int(i64::from(open(recv)?.header.os)))
    }
    def "mtime" (recv) {
        let secs = i64::from(open(recv)?.header.mtime);
        Ok(crate::builtins::time::time_from_parts(secs, 0))
    }
    // The running CRC-32 of the uncompressed bytes -- the value that goes into
    // the footer, and that a reader has verified once it reaches the end.
    def "crc" (recv) {
        Ok(RubyValue::Int(i64::from(open(recv)?.crc)))
    }

    // zeo writes through to the IO on every call, so `sync` is already the
    // behaviour `sync = true` asks for; the flag is recorded and reported so a
    // caller reading it back sees what it set.
    def "sync" (recv) {
        Ok(RubyValue::Bool(open(recv)?.sync))
    }
    def "sync=" (recv, arg) {
        open(recv)?.sync = (*arg).truthy();
        Ok((*arg).clone())
    }
}

/// A header text field as a Ruby String, or nil when the header didn't carry
/// it. gzip does not say what encoding the bytes are in, and neither does
/// CRuby -- both hand back the bytes as they arrived.
fn text_or_nil(bytes: Option<&[u8]>) -> RubyValue {
    match bytes {
        Some(b) => super::text_str(b.to_vec()),
        None => RubyValue::Nil,
    }
}
