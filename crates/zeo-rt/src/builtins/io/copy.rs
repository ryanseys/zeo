//! `IO.copy_stream`: what counts as a source, what counts as a sink, and
//! the chunk size between them.

use super::*;

/// Whether `v` is an IO-like object (`IO`/`File`/`StringIO`) rather than a
/// filename. CRuby's `copy_stream` uses an IO argument at its CURRENT position;
/// only a String/`to_path` argument names a file to open.
pub(super) fn is_io_object(v: &RubyValue) -> bool {
    matches!(
        v,
        RubyValue::Object(o)
            if matches!(
                o.class_id(),
                IO_CLASS | zeo_abi::FILE_CLASS | zeo_abi::STRINGIO_CLASS
            )
    )
}

/// Whether `IO.copy_stream` should READ this argument as a stream rather than
/// open it as a file.
///
/// Ruby's rule is a duck test, not a class list: a String or anything with a
/// `to_path` names a FILE, and everything else must answer `read`. Testing
/// for `IO`/`File`/`StringIO` instead refused a `Zlib::GzipReader` with "no
/// implicit conversion into String" -- which is how rubygems unpacks every
/// `.gem`, so no gem could be extracted.
pub(super) fn reads_like_io(v: &RubyValue) -> bool {
    if names_a_file(v) {
        return false;
    }
    crate::dispatch::responds_to_value(v, crate::Symbol::intern("read"), true)
}

/// Whether `v` is a REAL IO -- one with a descriptor behind it.
///
/// Narrower than [`is_io_object`], which counts `StringIO`. `copy_stream`'s
/// `src_offset` needs a descriptor to `pread` at, and CRuby refuses the
/// argument for a `StringIO` by name: "cannot specify src_offset for non-IO".
pub(super) fn is_real_io(v: &RubyValue) -> bool {
    matches!(v, RubyValue::Object(o) if matches!(o.class_id(), IO_CLASS | zeo_abi::FILE_CLASS))
}

/// How much `copy_stream` asks for next: a whole chunk, or what is left of a
/// `copy_length` when that is smaller. Zero means the copy is done.
///
/// Chunked rather than one big read: reading a whole source into memory made
/// the peak the size of the file. 16 KiB is CRuby's own chunk.
///
/// zeo asks a source through `read(n)` where CRuby uses `readpartial(n, buf)`
/// falling back to `read(n, buf)`. The bytes are the same for any source that
/// implements `read` the way `IO` does; an unbounded copy from a LIVE pipe
/// reaches the destination later, because `read(n)` waits for the chunk.
pub(super) fn chunk_len(remaining: Option<u64>) -> usize {
    const CHUNK: u64 = 16 * 1024;
    match remaining {
        None => CHUNK as usize,
        Some(left) => left.min(CHUNK) as usize,
    }
}

/// `copy_stream`'s destination: an object that answers `write`, or a file to
/// create. Opened BEFORE the source is read, so copying a path onto itself
/// truncates first and answers 0 -- CRuby's behaviour.
pub(super) enum Sink<'a> {
    Io(&'a RubyValue),
    File(std::fs::File, String),
}

impl<'a> Sink<'a> {
    pub(super) fn open(dst: &'a RubyValue) -> Result<Sink<'a>, Signal> {
        if writes_like_io(dst) {
            return Ok(Sink::Io(dst));
        }
        let path = crate::builtins::file::path_arg(dst, "copy_stream")?;
        let file = crate::gvl::without_gvl(|| std::fs::File::create(&path))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "rb_sysopen", &path))?;
        Ok(Sink::File(file, path))
    }

    pub(super) fn write(&mut self, bytes: Vec<u8>) -> Result<(), Signal> {
        match self {
            Sink::Io(dst) => {
                let s =
                    RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT));
                crate::dispatch::send_value(dst, crate::Symbol::intern("write"), &[s], None)?;
                Ok(())
            }
            Sink::File(file, path) => {
                use std::io::Write as _;
                crate::gvl::without_gvl(|| file.write_all(&bytes))
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "copy_stream", path))
            }
        }
    }

    /// A Ruby sink flushes itself; `write` is the whole contract.
    pub(super) fn finish(&mut self) -> Result<(), Signal> {
        match self {
            Sink::Io(_) => Ok(()),
            Sink::File(file, path) => {
                use std::io::Write as _;
                crate::gvl::without_gvl(|| file.flush())
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "copy_stream", path))
            }
        }
    }
}

/// The WRITE half of the same test.
pub(super) fn writes_like_io(v: &RubyValue) -> bool {
    if names_a_file(v) {
        return false;
    }
    crate::dispatch::responds_to_value(v, crate::Symbol::intern("write"), true)
}

/// Whether this argument NAMES a file: a String, or an object with `to_path`
/// (which is what `Pathname` has). Checked first, because ruby checks it
/// first -- a `File` has `to_path` too, and is still a stream.
pub(super) fn names_a_file(v: &RubyValue) -> bool {
    if is_io_object(v) {
        return false;
    }
    matches!(v, RubyValue::Str(_))
        || crate::dispatch::responds_to_value(v, crate::Symbol::intern("to_path"), true)
}
