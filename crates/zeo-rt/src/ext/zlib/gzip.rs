//! The state behind `Zlib::GzipFile` and its two subclasses, plus the
//! whole-string `Zlib.gzip`/`Zlib.gunzip`.
//!
//! A `GzipFile` is a codec bolted to a Ruby IO: the writer compresses into it,
//! the reader pulls compressed bytes out of it. Neither holds a Rust stream --
//! the underlying object is any Ruby value answering `read`/`write`, reached
//! through an ordinary send, which is what lets a `StringIO`, a socket or a
//! `File` all work.
//!
//! # When the footer is checked
//!
//! CRuby verifies a member's CRC-32 and length at one precise moment: when the
//! deflate stream has ended AND every byte it produced has been handed out
//! (`GZFILE_IS_FINISHED`, which is `ZSTREAM_IS_FINISHED && ZSTREAM_BUF_FILLED
//! == 0`). That is not a detail -- it is directly observable, and surprising:
//!
//! ```text
//! Zlib::GzipReader.new(corrupt).read      # => the data, NO error
//! Zlib::GzipReader.new(corrupt).readlines # => Zlib::GzipFile::CRCError
//! ```
//!
//! `read` fills the buffer to the end of the stream and answers it in one go,
//! so the buffer is never empty at the moment the check would run; `gets`
//! empties it and asks again, and that call checks. Both behaviours are
//! reproduced here by [`ReadState::check_when_drained`], called from every
//! read entry point after filling -- one rule, rather than a per-method
//! decision that would drift.

use super::codec::{self, Deflating, Wrap};
use super::frame::{self, Corrupt, Header};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{ClassId, RubyValue, Signal, Symbol};
use flate2::{Compression, FlushCompress, FlushDecompress, Status};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// How many compressed bytes to pull from the IO per refill.
const READ_CHUNK: i64 = 16 * 1024;

/// `Zlib.gzip(str, level:)` -- a whole member, header and footer included.
pub(crate) fn gzip_string(data: &[u8], level: Compression) -> Result<RubyValue, Signal> {
    codec::one_shot_deflate(data, level, Wrap::Gzip)
}

/// `Zlib.gunzip(str)` -- a whole member, footer verified. Unlike a
/// `GzipReader`, this always checks: there is no buffer left half-consumed.
pub(crate) fn gunzip_string(data: &[u8]) -> Result<RubyValue, Signal> {
    let (bytes, _, _) = codec::gunzip_bytes(data)?;
    Ok(super::bin_str(bytes))
}

/// Which direction a `GzipFile` runs in. `GzipFile` itself is never
/// constructed -- CRuby's has no `new` either -- so there is no third arm.
pub(super) enum Direction {
    Write(WriteState),
    Read(ReadState),
}

pub(super) struct WriteState {
    pub(super) comp: Deflating,
    /// Set once the header bytes have gone to the IO, after which the header
    /// fields are frozen -- CRuby raises rather than silently dropping a late
    /// `orig_name=`.
    pub(super) header_written: bool,
}

pub(super) struct ReadState {
    dec: flate2::Decompress,
    /// Compressed bytes pulled from the IO but not yet decoded, plus whatever
    /// followed the member -- which is what `#unused` answers.
    input: Vec<u8>,
    /// Decoded bytes not yet handed to Ruby, and how far into them we are.
    /// CRuby calls handing them out "detaching the buffer".
    buf: Vec<u8>,
    at: usize,
    /// The deflate stream hit its end marker.
    stream_end: bool,
    /// The IO answered nil: there is no more compressed input, ever.
    io_eof: bool,
    footer_checked: bool,
    pub(super) lineno: i64,
}

pub(super) struct GzipState {
    pub(super) io: RubyValue,
    pub(super) header: Header,
    pub(super) closed: bool,
    /// `GzipWriter.open`/`GzipReader.open` opened the file themselves, so
    /// `#close` closes it too. A caller-supplied IO is left alone.
    pub(super) owns_io: bool,
    pub(super) sync: bool,
    /// The running CRC-32 and byte count of the UNCOMPRESSED data -- the
    /// footer's two fields, and what `#crc` and `#pos` report.
    pub(super) crc: u32,
    pub(super) size: u32,
    pub(super) dir: Direction,
}

pub struct RGzipFile {
    class: ClassId,
    pub(super) state: Mutex<GzipState>,
    frozen: AtomicBool,
}

impl RubyObject for RGzipFile {
    fn class_id(&self) -> ClassId {
        self.class
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    /// Two `GzipFile`s over one IO would interleave their reads and produce
    /// garbage, so copying one is refused rather than half-supported. CRuby
    /// refuses too -- `GzipFile` defines no `initialize_copy`.
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RGzipFile::refusing_to_copy(self.class))
    }
}

impl RGzipFile {
    pub(super) fn writer(io: RubyValue, level: Compression, owns_io: bool) -> RGzipFile {
        RGzipFile {
            class: zeo_abi::ZLIB_GZIP_WRITER_CLASS,
            state: Mutex::new(GzipState {
                io,
                header: Header {
                    mtime: now_epoch_seconds(),
                    xfl: frame::xfl_for_level(level.level()),
                    ..Header::default()
                },
                closed: false,
                owns_io,
                sync: false,
                crc: 0,
                size: 0,
                dir: Direction::Write(WriteState {
                    comp: Deflating::raw(level),
                    header_written: false,
                }),
            }),
            frozen: AtomicBool::new(false),
        }
    }

    pub(super) fn reader(io: RubyValue, owns_io: bool) -> RGzipFile {
        RGzipFile {
            class: zeo_abi::ZLIB_GZIP_READER_CLASS,
            state: Mutex::new(GzipState {
                io,
                header: Header::default(),
                closed: false,
                owns_io,
                sync: false,
                crc: 0,
                size: 0,
                dir: Direction::Read(ReadState {
                    dec: flate2::Decompress::new(false),
                    input: Vec::new(),
                    buf: Vec::new(),
                    at: 0,
                    stream_end: false,
                    io_eof: false,
                    footer_checked: false,
                    lineno: 0,
                }),
            }),
            frozen: AtomicBool::new(false),
        }
    }

    /// The placeholder `dup_object` hands back: closed, so every method on it
    /// reports the "closed gzip stream" a copy has no way to avoid being.
    fn refusing_to_copy(class: ClassId) -> RGzipFile {
        let f = RGzipFile::reader(RubyValue::Nil, false);
        f.state.lock().closed = true;
        RGzipFile {
            class,
            state: Mutex::new(f.state.into_inner()),
            frozen: AtomicBool::new(false),
        }
    }
}

pub(super) fn gz_of(recv: &RubyValue) -> &RGzipFile {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RGzipFile>()
            .expect("the GzipFile tables only dispatch on GzipFile receivers"),
        _ => unreachable!("the GzipFile tables only dispatch on GzipFile receivers"),
    }
}

/// An open stream's state, or CRuby's error for one already closed.
pub(super) fn open(recv: &RubyValue) -> Result<parking_lot::MutexGuard<'_, GzipState>, Signal> {
    let st = gz_of(recv).state.lock();
    if st.closed {
        return Err(gz_error("closed gzip stream"));
    }
    Ok(st)
}

pub(super) fn gz_error(msg: &str) -> Signal {
    raise_error("Zlib::GzipFile::Error", msg.to_string())
}

/// A gzip failure, as the class CRuby raises for it.
pub(super) fn gz_corrupt(c: Corrupt) -> Signal {
    let class = match c {
        Corrupt::CrcMismatch => "Zlib::GzipFile::CRCError",
        Corrupt::LengthMismatch => "Zlib::GzipFile::LengthError",
        Corrupt::NotGzip | Corrupt::UnsupportedMethod => "Zlib::GzipFile::Error",
    };
    raise_error(class, c.message().to_string())
}

pub(super) fn now_epoch_seconds() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32)
}

// ------------------------------------------------------------------- writing

impl WriteState {
    /// Compress `data` and push everything that came out to the IO, writing
    /// the gzip header first if it has not gone out yet.
    pub(super) fn write(
        &mut self,
        io: &RubyValue,
        header: &Header,
        data: &[u8],
        flush: FlushCompress,
    ) -> Result<(), Signal> {
        let mut out = Vec::new();
        if !self.header_written {
            out.extend_from_slice(&header.encode());
            self.header_written = true;
        }
        self.comp.run(data, flush, &mut out)?;
        if out.is_empty() {
            return Ok(());
        }
        io_write(io, out)
    }
}

pub(super) fn io_write(io: &RubyValue, bytes: Vec<u8>) -> Result<(), Signal> {
    crate::dispatch::send_value(
        io,
        Symbol::intern("write"),
        &[super::bin_str(bytes)],
        None,
    )?;
    Ok(())
}

// ------------------------------------------------------------------- reading

impl ReadState {
    /// Read the member's header off the front of the IO. Called once, at
    /// construction, because CRuby reports a non-gzip stream from `new`.
    pub(super) fn read_header(&mut self, io: &RubyValue) -> Result<Header, Signal> {
        loop {
            match Header::parse(&self.input) {
                Err(c) => return Err(gz_corrupt(c)),
                Ok(Some((header, used))) => {
                    self.input.drain(..used);
                    return Ok(header);
                }
                Ok(None) => {
                    if !self.pull(io)? {
                        return Err(gz_error("unexpected end of file"));
                    }
                }
            }
        }
    }

    /// Pull one chunk of compressed bytes off the IO. Answers whether any
    /// arrived; `false` means the IO is spent and will not answer again.
    fn pull(&mut self, io: &RubyValue) -> Result<bool, Signal> {
        if self.io_eof {
            return Ok(false);
        }
        let got = crate::dispatch::send_value(
            io,
            Symbol::intern("read"),
            &[RubyValue::Int(READ_CHUNK)],
            None,
        )?;
        match got {
            RubyValue::Str(s) => {
                let bytes = s.lock().bytes().to_vec();
                if bytes.is_empty() {
                    self.io_eof = true;
                    return Ok(false);
                }
                self.input.extend_from_slice(&bytes);
                Ok(true)
            }
            // nil: end of file.
            _ => {
                self.io_eof = true;
                Ok(false)
            }
        }
    }

    /// Decode one round of input into the buffer, pulling more from the IO if
    /// the codec wants it. Answers how many bytes it appended.
    fn decode_more(&mut self, io: &RubyValue, crc: &mut u32, size: &mut u32) -> Result<usize, Signal> {
        if self.stream_end {
            return Ok(0);
        }
        if self.input.is_empty() && !self.pull(io)? {
            // The IO ran out with the deflate stream still open. That is a
            // truncated member, not a missing footer -- `NoFooter` is for a
            // stream that DID end and then came up short of its eight bytes.
            return Err(gz_error("unexpected end of file"));
        }
        let before_len = self.buf.len();
        let before_in = self.dec.total_in();
        self.buf.reserve(READ_CHUNK as usize);
        let status = self
            .dec
            .decompress_vec(&self.input, &mut self.buf, FlushDecompress::None)
            .map_err(|_| gz_error("invalid compressed data -- format violated"))?;
        let consumed = (self.dec.total_in() - before_in) as usize;
        self.input.drain(..consumed);
        if status == Status::StreamEnd {
            self.stream_end = true;
        }
        let fresh = &self.buf[before_len..];
        *crc = super::crc32(fresh, *crc);
        *size = size.wrapping_add(fresh.len() as u32);
        Ok(self.buf.len() - before_len)
    }

    /// CRuby's `GZFILE_IS_FINISHED`: the stream ended AND everything it
    /// produced has been handed out. See this module's header for why the
    /// distinction is visible from Ruby.
    fn drained(&self) -> bool {
        self.stream_end && self.at >= self.buf.len()
    }

    /// Verify the footer, exactly once, at the moment the stream drains. Every
    /// read entry point calls this after filling; it is a no-op until then.
    pub(super) fn check_when_drained(&mut self, io: &RubyValue, crc: u32, size: u32) -> Result<bool, Signal> {
        if !self.drained() {
            return Ok(false);
        }
        if self.footer_checked {
            return Ok(true);
        }
        while self.input.len() < 8 && self.pull(io)? {}
        self.footer_checked = true;
        let Ok(footer) = <[u8; 8]>::try_from(&self.input[..8.min(self.input.len())]) else {
            return Err(raise_error(
                "Zlib::GzipFile::NoFooter",
                "footer is not found".to_string(),
            ));
        };
        self.input.drain(..8);
        frame::check_footer(&footer, crc, size).map_err(gz_corrupt)?;
        Ok(true)
    }

    fn pending(&self) -> &[u8] {
        &self.buf[self.at..]
    }

    fn take(&mut self, n: usize) -> Vec<u8> {
        let n = n.min(self.buf.len() - self.at);
        let out = self.buf[self.at..self.at + n].to_vec();
        self.at += n;
        out
    }
}

/// The read entry points, each following the same shape: fill for what this
/// call needs, then let `check_when_drained` decide whether the answer is
/// data or the end of the stream.
impl GzipState {
    fn reader(&mut self) -> Result<(&mut ReadState, &RubyValue, &mut u32, &mut u32), Signal> {
        match &mut self.dir {
            Direction::Read(r) => Ok((r, &self.io, &mut self.crc, &mut self.size)),
            Direction::Write(_) => Err(gz_error("not opened for reading")),
        }
    }

    /// Decompress to the end of the member. Answers `None` once drained --
    /// which the callers turn into `""` or `nil` as their method requires.
    pub(super) fn read_all(&mut self) -> Result<Option<Vec<u8>>, Signal> {
        let (r, io, crc, size) = self.reader()?;
        while !r.stream_end {
            r.decode_more(io, crc, size)?;
        }
        if r.check_when_drained(io, *crc, *size)? {
            return Ok(None);
        }
        let n = r.buf.len() - r.at;
        Ok(Some(r.take(n)))
    }

    /// Decompress at least `want` bytes, or to the end of the member.
    pub(super) fn read_n(&mut self, want: usize) -> Result<Option<Vec<u8>>, Signal> {
        let (r, io, crc, size) = self.reader()?;
        while !r.stream_end && r.buf.len() - r.at < want {
            r.decode_more(io, crc, size)?;
        }
        if r.check_when_drained(io, *crc, *size)? {
            return Ok(None);
        }
        Ok(Some(r.take(want)))
    }

    /// Whatever has already been decoded, decoding one more round only if
    /// nothing is pending -- `readpartial`'s "don't block twice" contract.
    pub(super) fn read_partial(&mut self, want: usize) -> Result<Option<Vec<u8>>, Signal> {
        let (r, io, crc, size) = self.reader()?;
        if r.at >= r.buf.len() && !r.stream_end {
            r.decode_more(io, crc, size)?;
        }
        if r.check_when_drained(io, *crc, *size)? {
            return Ok(None);
        }
        Ok(Some(r.take(want)))
    }

    /// One line, up to and including `sep`. A `None` separator means "the
    /// rest of the member" (`gets(nil)`).
    pub(super) fn read_line(&mut self, sep: Option<&[u8]>) -> Result<Option<Vec<u8>>, Signal> {
        let (r, io, crc, size) = self.reader()?;
        let mut searched = 0;
        loop {
            if let Some(sep) = sep.filter(|s| !s.is_empty()) {
                let hay = r.pending();
                if let Some(hit) = find(&hay[searched.min(hay.len())..], sep) {
                    let end = searched.min(hay.len()) + hit + sep.len();
                    let line = r.take(end);
                    r.lineno += 1;
                    return Ok(Some(line));
                }
                searched = r.pending().len().saturating_sub(sep.len() - 1);
            }
            if r.stream_end {
                break;
            }
            r.decode_more(io, crc, size)?;
        }
        if r.check_when_drained(io, *crc, *size)? {
            return Ok(None);
        }
        let n = r.buf.len() - r.at;
        let line = r.take(n);
        r.lineno += 1;
        Ok(Some(line))
    }

    /// Push bytes back in front of the read position -- `ungetc`/`ungetbyte`.
    pub(super) fn unread(&mut self, bytes: &[u8]) -> Result<(), Signal> {
        let (r, _, _, _) = self.reader()?;
        let mut restored = bytes.to_vec();
        restored.extend_from_slice(r.pending());
        r.buf = restored;
        r.at = 0;
        Ok(())
    }

    pub(super) fn at_eof(&mut self) -> Result<bool, Signal> {
        let (r, io, crc, size) = self.reader()?;
        // A reader that has produced nothing yet has to decode at least once
        // before it can say -- an empty member is at EOF from the start.
        while !r.stream_end && r.at >= r.buf.len() {
            r.decode_more(io, crc, size)?;
        }
        Ok(r.drained())
    }

    /// `GzipReader#unused` -- the bytes that followed the member's footer, or
    /// nil while the member is still being read.
    ///
    /// Reading it CONSUMES the footer (and so can raise `CRCError`), because
    /// CRuby's does: `gzfile_reader_get_unused` runs the same
    /// finished-and-drained check every read entry point runs.
    pub(super) fn unused(&mut self) -> Result<Option<Vec<u8>>, Signal> {
        let (r, io, crc, size) = self.reader()?;
        if !r.check_when_drained(io, *crc, *size)? {
            return Ok(None);
        }
        Ok(Some(r.input.clone()))
    }

    pub(super) fn position(&mut self) -> Result<i64, Signal> {
        match &self.dir {
            // A writer's position is how much has gone IN, a reader's how much
            // has come OUT -- both are the uncompressed byte count.
            Direction::Write(_) => Ok(i64::from(self.size)),
            Direction::Read(r) => Ok(r.at as i64),
        }
    }

    pub(super) fn read_state(&mut self) -> Result<&mut ReadState, Signal> {
        match &mut self.dir {
            Direction::Read(r) => Ok(r),
            Direction::Write(_) => Err(gz_error("not opened for reading")),
        }
    }

    /// Start the member over: rewind the IO, throw the codec away, re-read the
    /// header. Only a seekable IO can do this, which is CRuby's rule too.
    pub(super) fn rewind(&mut self) -> Result<(), Signal> {
        crate::dispatch::send_value(&self.io, Symbol::intern("rewind"), &[], None)?;
        self.crc = 0;
        self.size = 0;
        let mut fresh = ReadState {
            dec: flate2::Decompress::new(false),
            input: Vec::new(),
            buf: Vec::new(),
            at: 0,
            stream_end: false,
            io_eof: false,
            footer_checked: false,
            lineno: 0,
        };
        self.header = fresh.read_header(&self.io)?;
        self.dir = Direction::Read(fresh);
        Ok(())
    }
}

// ------------------------------------------------------------------- closing

impl GzipState {
    /// Finish the member: a writer flushes the deflate tail and appends the
    /// footer, a reader just stops. `close_io` distinguishes `#close` (which
    /// closes the IO underneath) from `#finish` (which does not) -- and even
    /// then only an IO this stream opened itself is closed.
    pub(super) fn shut_down(&mut self, close_io: bool) -> Result<RubyValue, Signal> {
        if !self.closed {
            if let Direction::Write(w) = &mut self.dir {
                let mut out = Vec::new();
                if !w.header_written {
                    out.extend_from_slice(&self.header.encode());
                    w.header_written = true;
                }
                w.comp.run(&[], FlushCompress::Finish, &mut out)?;
                out.extend_from_slice(&frame::footer(self.crc, self.size));
                io_write(&self.io, out)?;
            }
            self.closed = true;
            if close_io && self.owns_io {
                crate::dispatch::send_value(&self.io, Symbol::intern("close"), &[], None)?;
            }
        }
        Ok(self.io.clone())
    }
}

/// The first offset of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_locates_a_separator_or_answers_none() {
        assert_eq!(find(b"hello\nworld\n", b"\n"), Some(5));
        assert_eq!(find(b"hello", b"\n"), None);
        assert_eq!(find(b"aXXb", b"XX"), Some(1));
        assert_eq!(find(b"a", b"XX"), None);
        assert_eq!(find(b"", b"\n"), None);
    }
}
