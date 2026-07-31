//! The compression engine behind `Zlib::ZStream` and its two subclasses.
//!
//! All three classes share one Rust value, [`RZStream`], tagged with the class
//! it answers as. That is what lets `ZStream`'s rows -- the counters, the
//! lifecycle, the buffer flushes (`zstream.rs`) -- be written once and
//! inherited by `Deflate` (`deflate.rs`) and `Inflate` (`inflate.rs`), exactly
//! as CRuby writes them once on `cZStream`. The rows live in their own files
//! because one `ruby_class!` per module is the DSL's rule.
//!
//! # The buffer is the surface
//!
//! zlib's incremental API is what CRuby exposes, so a whole-buffer wrapper
//! (`flate2::write::ZlibEncoder`) cannot implement it: `deflate(s)` must hand
//! back the bytes flushed *so far* (often none), `<<` must produce nothing and
//! leave them queued, and `finish` must drain whatever is left. So a stream
//! keeps an output queue: the run functions append to it, and the methods that
//! CRuby documents as "detaching the buffer" drain it.
//!
//! # Window bits choose the framing
//!
//! zlib overloads `window_bits` to select the container as well as the window
//! size, and that is the ONE part of the argument zeo acts on -- the pure-Rust
//! backend has a fixed 32KB window (see docs/COMPATIBILITY.md).

use super::frame::{self, Corrupt, Header};
use super::{bin_str, crc32};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{ClassId, RubyValue, Signal};
use flate2::{Compression, FlushCompress, FlushDecompress, Status};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// How much room to offer the codec per call. Large enough that a typical
/// `deflate` finishes in one pass, small enough not to over-allocate for the
/// one-byte writes `GzipWriter#putc` makes.
const CHUNK: usize = 16 * 1024;

/// The container a stream reads or writes, chosen by `window_bits`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Wrap {
    /// A zlib stream: a 2-byte header and an Adler-32 trailer (8..=15).
    Zlib,
    /// Bare deflate data, no framing at all (a NEGATIVE window_bits).
    Raw,
    /// A gzip member (window_bits + 16).
    Gzip,
    /// Detect zlib or gzip from the first bytes (window_bits + 32). Inflate
    /// only -- there is nothing to detect when compressing.
    Auto,
}

/// zlib's own reading of `window_bits`. The magnitude must name a legal window
/// (8..=15); the offset chooses the container.
pub(super) fn wrap_of(bits: i64, allow_auto: bool) -> Result<Wrap, Signal> {
    let wrap = match bits {
        -15..=-8 => Wrap::Raw,
        8..=15 => Wrap::Zlib,
        24..=31 => Wrap::Gzip,
        40..=47 if allow_auto => Wrap::Auto,
        _ => return Err(stream_error()),
    };
    Ok(wrap)
}

pub(super) fn stream_error() -> Signal {
    raise_error("Zlib::StreamError", "stream error".to_string())
}

pub(super) fn data_error(msg: &str) -> Signal {
    raise_error("Zlib::DataError", msg.to_string())
}

pub(super) fn buf_error() -> Signal {
    raise_error("Zlib::BufError", "buffer error".to_string())
}

/// A compressing stream. For [`Wrap::Gzip`] the codec runs RAW and this adds
/// the container itself, because flate2's pure-Rust backend has no gzip mode.
pub(super) struct Deflating {
    pub(super) comp: flate2::Compress,
    pub(super) wrap: Wrap,
    pub(super) level: Compression,
    pub(super) strategy: i64,
    /// Set once the gzip header has gone out, so it is written exactly once
    /// however many times `deflate` is called.
    pub(super) header_written: bool,
    /// The gzip footer's two fields, accumulated over the input.
    pub(super) crc: u32,
    pub(super) size: u32,
}

/// Which Ruby-level API a decompressor is serving. The same corrupt footer is
/// a different exception class depending on it: `Zlib.gunzip` reports gzip's
/// `GzipFile::CRCError`, while `Zlib::Inflate` over a gzip-wrapped stream
/// reports zlib's own `DataError`. Recording the API is how one codec answers
/// for both.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Api {
    Stream,
    GzipFile,
}

/// A decompressing stream.
pub(super) struct Inflating {
    pub(super) dec: flate2::Decompress,
    pub(super) wrap: Wrap,
    /// Input held back because the container's header is not complete yet.
    /// Only ever non-empty before the first deflate byte is handed to flate2.
    pub(super) header_pending: Vec<u8>,
    pub(super) header_done: bool,
    /// The gzip header, once parsed -- what `GzipReader`'s accessors read.
    pub(super) header: Option<Header>,
    /// Trailing bytes after the member ended: a gzip footer to check, and then
    /// whatever followed it (`GzipReader#unused`).
    pub(super) trailer: Vec<u8>,
    /// The footer's two fields, accumulated over the OUTPUT.
    pub(super) crc: u32,
    pub(super) size: u32,
    /// Set once a gzip footer has been checked, so it is checked exactly once.
    pub(super) footer_checked: bool,
    pub(super) api: Api,
}

pub(super) enum Codec {
    Deflate(Deflating),
    Inflate(Inflating),
}

pub(super) struct State {
    pub(super) codec: Codec,
    /// Output produced but not yet handed to Ruby. CRuby calls draining this
    /// "detaching the buffer"; `<<` deliberately does not.
    pub(super) out: Vec<u8>,
    /// The running Adler-32 (zlib framing) or CRC-32 (gzip framing) that
    /// `ZStream#adler` reports.
    pub(super) adler: u32,
    pub(super) total_in: u64,
    pub(super) total_out: u64,
    /// The deflate stream has ended: `finished?`/`stream_end?`.
    pub(super) finished: bool,
    /// `close`/`end` has been called: `closed?`/`ended?`.
    pub(super) closed: bool,
    /// `avail_out=`'s value. zlib sizes its output buffer with this; zeo grows
    /// its own on demand, so the knob is recorded and reported but inert.
    pub(super) avail_out: i64,
    /// What `data_type` answers -- see [`classify`].
    pub(super) data_type: i64,
}

pub(super) struct RZStream {
    class: ClassId,
    pub(super) state: Mutex<State>,
    frozen: AtomicBool,
}

impl RubyObject for RZStream {
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
    /// A codec's position in the stream cannot be cloned -- flate2's
    /// `Compress`/`Decompress` are not `Clone`, and zlib's `deflateCopy` has
    /// no counterpart here. `dup` therefore answers a FRESH stream with the
    /// same settings, which is what CRuby's `Zlib::ZStream` refuses outright
    /// (it has no `initialize_copy`, so `dup` raises there).
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let st = self.state.lock();
        let fresh = match &st.codec {
            Codec::Deflate(d) => RZStream::deflating(d.wrap, d.level, d.strategy),
            Codec::Inflate(i) => RZStream::inflating(i.wrap),
        };
        if copy_frozen {
            fresh.set_frozen();
        }
        Arc::new(fresh)
    }
}

impl RZStream {
    fn new(class: ClassId, codec: Codec) -> RZStream {
        let adler = match &codec {
            // zlib seeds an Adler-32 at 1 and a CRC-32 at 0.
            Codec::Deflate(d) if d.wrap == Wrap::Gzip => 0,
            Codec::Inflate(i) if i.wrap == Wrap::Gzip => 0,
            _ => 1,
        };
        RZStream {
            class,
            state: Mutex::new(State {
                codec,
                out: Vec::new(),
                adler,
                total_in: 0,
                total_out: 0,
                finished: false,
                closed: false,
                avail_out: 0,
                data_type: 2, // Zlib::UNKNOWN, until data says otherwise
            }),
            frozen: AtomicBool::new(false),
        }
    }

    pub(super) fn deflating(wrap: Wrap, level: Compression, strategy: i64) -> RZStream {
        RZStream::new(
            zeo_abi::ZLIB_DEFLATE_CLASS,
            Codec::Deflate(Deflating {
                comp: flate2::Compress::new(level, wrap == Wrap::Zlib),
                wrap,
                level,
                strategy,
                header_written: false,
                crc: 0,
                size: 0,
            }),
        )
    }

    pub(super) fn inflating(wrap: Wrap) -> RZStream {
        RZStream::new(
            zeo_abi::ZLIB_INFLATE_CLASS,
            Codec::Inflate(Inflating {
                dec: flate2::Decompress::new(wrap == Wrap::Zlib),
                wrap,
                header_pending: Vec::new(),
                header_done: false,
                header: None,
                trailer: Vec::new(),
                crc: 0,
                size: 0,
                footer_checked: false,
                api: Api::Stream,
            }),
        )
    }
}

pub(super) fn zs_of(recv: &RubyValue) -> &RZStream {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RZStream>()
            .expect("the ZStream tables only dispatch on ZStream receivers"),
        _ => unreachable!("the ZStream tables only dispatch on ZStream receivers"),
    }
}

/// A live stream's state, or CRuby's error for one that has been closed.
pub(super) fn ready(recv: &RubyValue) -> Result<parking_lot::MutexGuard<'_, State>, Signal> {
    let st = zs_of(recv).state.lock();
    if st.closed {
        return Err(raise_error(
            "Zlib::Error",
            "stream is not ready".to_string(),
        ));
    }
    Ok(st)
}

/// zlib's `data_type` heuristic, reduced to the distinction it actually
/// publishes: a block that is all printable text is `Zlib::TEXT`, anything
/// else `Zlib::BINARY`. zlib decides this from the literal/length histogram it
/// builds while compressing, which the pure-Rust backend does not expose; the
/// classification agrees with zlib's on ordinary text and ordinary binary and
/// can differ on a mixture (see docs/COMPATIBILITY.md).
pub(super) fn classify(data: &[u8]) -> i64 {
    let textual = data
        .iter()
        .all(|&b| matches!(b, 0x20..=0x7e | b'\n' | b'\r' | b'\t'));
    if textual { 1 } else { 0 }
}

pub(super) fn bytes_of(v: Option<&RubyValue>) -> Result<Vec<u8>, Signal> {
    super::bytes_arg(v)
}

// ---------------------------------------------------------------- compressing

impl Deflating {
    /// A bare deflate compressor, no container. `GzipWriter` uses one and
    /// writes its own gzip header and footer, because it has to expose the
    /// header's FIELDS -- `mtime`, `orig_name`, `comment` -- which a codec
    /// that owned the framing would have already spent.
    pub(super) fn raw(level: Compression) -> Deflating {
        Deflating {
            comp: flate2::Compress::new(level, false),
            wrap: Wrap::Raw,
            level,
            strategy: 0,
            header_written: false,
            crc: 0,
            size: 0,
        }
    }

    /// Feed `input` to the codec, appending everything it produces to `out`.
    /// Answers whether the stream reached its end.
    pub(super) fn run(
        &mut self,
        input: &[u8],
        flush: FlushCompress,
        out: &mut Vec<u8>,
    ) -> Result<bool, Signal> {
        if self.wrap == Wrap::Gzip && !self.header_written {
            out.extend_from_slice(
                &Header {
                    xfl: frame::xfl_for_level(self.level.level()),
                    mtime: super::gzip::now_epoch_seconds(),
                    ..Header::default()
                }
                .encode(),
            );
            self.header_written = true;
        }
        if self.wrap == Wrap::Gzip {
            self.crc = crc32(input, self.crc);
            self.size = self.size.wrapping_add(input.len() as u32);
        }

        let mut consumed = 0;
        let ended = loop {
            out.reserve(CHUNK);
            let room = out.capacity() - out.len();
            let before_in = self.comp.total_in();
            let before_out = self.comp.total_out();
            let status = self
                .comp
                .compress_vec(&input[consumed..], out, flush)
                .map_err(|_| stream_error())?;
            consumed += (self.comp.total_in() - before_in) as usize;
            let produced = (self.comp.total_out() - before_out) as usize;
            match status {
                Status::StreamEnd => break true,
                // The output vec always had room, so a buffer error here means
                // the codec has nothing further to do.
                Status::BufError => break false,
                // zlib's own termination rule (zlib.h: "call again if it
                // returned with avail_out == 0"): the codec is done when it
                // took all the input AND left output room. Testing "produced
                // nothing" instead loops forever under SYNC_FLUSH, which emits
                // a fresh sync marker on every call.
                Status::Ok if consumed == input.len() && produced < room => break false,
                Status::Ok => {}
            }
        };
        if ended && self.wrap == Wrap::Gzip {
            out.extend_from_slice(&frame::footer(self.crc, self.size));
        }
        Ok(ended)
    }
}

// -------------------------------------------------------------- decompressing

impl Inflating {
    /// Consume as much of `input` as the container's header needs, answering
    /// what is left for the codec. Answers `None` while the header is still
    /// incomplete -- the case a stream fed one byte at a time hits ten times
    /// in a row, and the reason this buffers rather than demanding a header
    /// arrive whole.
    fn take_header(&mut self, input: &[u8]) -> Result<Option<Vec<u8>>, Signal> {
        if self.header_done {
            return Ok(Some(input.to_vec()));
        }
        if self.wrap == Wrap::Raw {
            self.header_done = true;
            return Ok(Some(input.to_vec()));
        }
        self.header_pending.extend_from_slice(input);

        // A zlib header is flate2's to CONSUME -- the codec is configured to
        // read it -- but zeo checks it first, because the pure-Rust backend's
        // error for a bad one carries no message and CRuby's exact
        // "incorrect header check" would be unreachable.
        if self.wrap == Wrap::Zlib {
            if self.header_pending.len() < 2 {
                return Ok(None);
            }
            check_zlib_header(&self.header_pending)?;
            self.header_done = true;
            return Ok(Some(std::mem::take(&mut self.header_pending)));
        }

        // Auto-detect: gzip announces itself, so anything else must be zlib.
        if self.wrap == Wrap::Auto
            && self
                .header_pending
                .first()
                .is_some_and(|&b| b != frame::MAGIC[0])
        {
            if self.header_pending.len() < 2 {
                return Ok(None);
            }
            check_zlib_header(&self.header_pending)?;
            self.wrap = Wrap::Zlib;
            self.dec = flate2::Decompress::new(true);
            self.header_done = true;
            return Ok(Some(std::mem::take(&mut self.header_pending)));
        }

        match Header::parse(&self.header_pending).map_err(|c| self.corrupt(c))? {
            None => Ok(None),
            Some((header, used)) => {
                self.wrap = Wrap::Gzip;
                self.header = Some(header);
                self.header_done = true;
                Ok(Some(self.header_pending.split_off(used)))
            }
        }
    }

    pub(super) fn run(
        &mut self,
        input: &[u8],
        flush: FlushDecompress,
        out: &mut Vec<u8>,
    ) -> Result<bool, Signal> {
        let Some(input) = self.take_header(input)? else {
            return Ok(false);
        };

        let start = out.len();
        let mut consumed = 0;
        let ended = loop {
            out.reserve(CHUNK);
            let room = out.capacity() - out.len();
            let before_in = self.dec.total_in();
            let before_out = self.dec.total_out();
            let status = self
                .dec
                .decompress_vec(&input[consumed..], out, flush)
                .map_err(|e| decompress_error(&e))?;
            consumed += (self.dec.total_in() - before_in) as usize;
            let produced = (self.dec.total_out() - before_out) as usize;
            match status {
                Status::StreamEnd => break true,
                Status::BufError => break false,
                // Same rule as the compressing side: done when it took all the
                // input and left output room.
                Status::Ok if consumed == input.len() && produced < room => break false,
                Status::Ok => {}
            }
        };

        if self.wrap == Wrap::Gzip {
            let fresh = &out[start..];
            self.crc = crc32(fresh, self.crc);
            self.size = self.size.wrapping_add(fresh.len() as u32);
        }
        // Whatever the codec did not consume is past the end of the member:
        // a gzip footer, then anything that followed it.
        self.trailer.extend_from_slice(&input[consumed..]);
        self.header_pending.clear();
        if ended {
            self.check_footer()?;
        }
        Ok(ended)
    }

    /// Verify a finished gzip member against its footer. A member whose footer
    /// has not fully arrived is left alone -- `GzipReader` reports that as
    /// `NoFooter` when it runs out of input, and `Zlib::Inflate` (which zlib
    /// treats as still mid-stream) does not report it at all.
    pub(super) fn check_footer(&mut self) -> Result<(), Signal> {
        if self.wrap != Wrap::Gzip || self.footer_checked {
            return Ok(());
        }
        let Ok(bytes) = <[u8; 8]>::try_from(&self.trailer[..8.min(self.trailer.len())]) else {
            return Ok(());
        };
        self.footer_checked = true;
        self.trailer.drain(..8);
        frame::check_footer(&bytes, self.crc, self.size).map_err(|c| self.corrupt(c))
    }

    /// The exception a container fault raises, as the API this codec serves
    /// reports it.
    fn corrupt(&self, c: Corrupt) -> Signal {
        match self.api {
            Api::GzipFile => super::gzip::gz_corrupt(c),
            Api::Stream => match c {
                Corrupt::CrcMismatch => data_error("incorrect data check"),
                Corrupt::LengthMismatch => data_error("incorrect length check"),
                other => data_error(other.message()),
            },
        }
    }
}

/// zlib's own header check (RFC 1950 §2.2): the low nibble of CMF names the
/// compression method, and the two bytes together are a multiple of 31.
pub(super) fn check_zlib_header(bytes: &[u8]) -> Result<(), Signal> {
    if bytes.len() < 2 {
        return Ok(());
    }
    let (cmf, flg) = (bytes[0], bytes[1]);
    let word = (u16::from(cmf) << 8) | u16::from(flg);
    if cmf & 0x0f != 8 || word % 31 != 0 {
        return Err(data_error("incorrect header check"));
    }
    Ok(())
}

/// flate2's decompression failures. Its pure-Rust backend carries no message
/// (`DecompressError::message` is `None` there), so the wording is zlib's
/// generic one rather than the specific "invalid distance too far back"
/// family CRuby can report -- see docs/COMPATIBILITY.md.
fn decompress_error(e: &flate2::DecompressError) -> Signal {
    match e.message() {
        Some(msg) => data_error(msg),
        None => data_error("invalid or incomplete deflate data"),
    }
}

// ----------------------------------------------------------- the shared rows

/// `ZStream#finish`, shared by both directions: run to the end, then drain.
pub(super) fn finish(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let st = &mut *ready(recv)?;
    if st.finished {
        return Ok(bin_str(std::mem::take(&mut st.out)));
    }
    let mut out = std::mem::take(&mut st.out);
    let before = out.len();
    let ended = match &mut st.codec {
        Codec::Deflate(d) => d.run(&[], FlushCompress::Finish, &mut out)?,
        Codec::Inflate(i) => i.run(&[], FlushDecompress::Finish, &mut out)?,
    };
    st.total_out += (out.len() - before) as u64;
    st.finished = ended;
    // zlib reports a stream that CANNOT be finished -- input that stopped
    // mid-member -- as a buffer error, and so does CRuby. A compressor can
    // always finish, so this only ever fires on the inflate side.
    if !ended && matches!(st.codec, Codec::Inflate(_)) {
        st.out = out;
        return Err(buf_error());
    }
    Ok(bin_str(out))
}

/// The shared body of `deflate`/`inflate`/`<<`: run the codec over `input`,
/// then either drain the queue (`detach`) or leave it for the next call --
/// which is the whole difference between `deflate(s)` and `<< s`.
pub(super) fn run_and_maybe_detach(
    recv: &RubyValue,
    input: &[u8],
    flush: Flush,
    detach: bool,
) -> Result<RubyValue, Signal> {
    let st = &mut *ready(recv)?;
    if st.finished {
        // The two directions part company once the stream has ended. More
        // input to a finished COMPRESSOR is a mistake -- there is no stream
        // left to put it in -- and zlib reports it. A finished DECOMPRESSOR
        // just keeps it: those bytes are whatever followed the member, which
        // is exactly what a byte-at-a-time reader hands over on its last few
        // calls, and CRuby answers "" rather than raising.
        let Codec::Inflate(i) = &mut st.codec else {
            return Err(stream_error());
        };
        i.trailer.extend_from_slice(input);
        return Ok(if detach {
            bin_str(Vec::new())
        } else {
            recv.clone()
        });
    }
    let mut out = std::mem::take(&mut st.out);
    let before = out.len();
    let ended = match (&mut st.codec, flush) {
        (Codec::Deflate(d), Flush::Compress(f)) => {
            let gzip = d.wrap == Wrap::Gzip;
            let ended = d.run(input, f, &mut out)?;
            st.adler = if gzip {
                crc32(input, st.adler)
            } else {
                super::adler32(input, st.adler)
            };
            if !input.is_empty() {
                st.data_type = classify(input);
            }
            ended
        }
        (Codec::Inflate(i), Flush::Decompress(f)) => {
            let gzip = i.wrap == Wrap::Gzip;
            let ended = i.run(input, f, &mut out)?;
            let fresh = &out[before..];
            st.adler = if gzip {
                crc32(fresh, st.adler)
            } else {
                super::adler32(fresh, st.adler)
            };
            if !fresh.is_empty() {
                st.data_type = classify(fresh);
            }
            ended
        }
        _ => unreachable!("a stream's codec and its flush kind are chosen together"),
    };
    st.total_in += input.len() as u64;
    st.total_out += (out.len() - before) as u64;
    st.finished = ended;
    if detach {
        Ok(bin_str(out))
    } else {
        st.out = out;
        Ok(recv.clone())
    }
}

/// A flush mode for whichever direction the stream runs in. The two halves of
/// flate2's API take different enums, so the caller's Ruby-level flush
/// constant is resolved to one of these before it reaches the shared driver.
#[derive(Clone, Copy)]
pub(super) enum Flush {
    Compress(FlushCompress),
    Decompress(FlushDecompress),
}

/// Ruby's flush constant as flate2's. zlib's `Z_PARTIAL_FLUSH` (1) is
/// deprecated and CRuby does not name it, so it is not accepted here either.
pub(super) fn flush_of(v: Option<&RubyValue>) -> Result<FlushCompress, Signal> {
    match v {
        None | Some(RubyValue::Nil) | Some(RubyValue::Int(0)) => Ok(FlushCompress::None),
        Some(RubyValue::Int(2)) => Ok(FlushCompress::Sync),
        Some(RubyValue::Int(3)) => Ok(FlushCompress::Full),
        Some(RubyValue::Int(4)) => Ok(FlushCompress::Finish),
        _ => Err(stream_error()),
    }
}

/// `Deflate#deflate`/`#flush` hand their output to a block when given one and
/// then answer nil -- CRuby's `rb_yield`-or-return shape.
pub(super) fn yield_or_return(
    out: RubyValue,
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match block {
        Some(RubyValue::Proc(p)) => {
            p.call(&[out])?;
            Ok(RubyValue::Nil)
        }
        _ => Ok(out),
    }
}

// ----------------------------------------------------------- one-shot helpers

/// `Zlib.deflate`/`Zlib::Deflate.deflate`: a fresh stream, one call, finished.
pub(crate) fn one_shot_deflate(
    data: &[u8],
    level: Compression,
    wrap: Wrap,
) -> Result<RubyValue, Signal> {
    let mut codec = Deflating {
        comp: flate2::Compress::new(level, wrap == Wrap::Zlib),
        wrap,
        level,
        strategy: 0,
        header_written: false,
        crc: 0,
        size: 0,
    };
    let mut out = Vec::new();
    codec.run(data, FlushCompress::Finish, &mut out)?;
    Ok(bin_str(out))
}

/// `Zlib.inflate`/`Zlib::Inflate.inflate`: a fresh stream, one call, and the
/// buffer error CRuby reports for input that ends mid-stream.
pub(crate) fn one_shot_inflate(data: &[u8], wrap: Wrap) -> Result<RubyValue, Signal> {
    let mut codec = Inflating {
        dec: flate2::Decompress::new(wrap == Wrap::Zlib),
        wrap,
        header_pending: Vec::new(),
        header_done: false,
        header: None,
        trailer: Vec::new(),
        crc: 0,
        size: 0,
        footer_checked: false,
        api: Api::Stream,
    };
    let mut out = Vec::new();
    if !codec.run(data, FlushDecompress::Finish, &mut out)? {
        return Err(buf_error());
    }
    Ok(bin_str(out))
}

/// Decompress a whole gzip member, answering the bytes and the header --
/// `Zlib.gunzip`'s engine, and `GzipReader`'s for a member already in memory.
pub(crate) fn gunzip_bytes(data: &[u8]) -> Result<(Vec<u8>, Header, Vec<u8>), Signal> {
    let mut codec = Inflating {
        dec: flate2::Decompress::new(false),
        wrap: Wrap::Gzip,
        header_pending: Vec::new(),
        header_done: false,
        header: None,
        trailer: Vec::new(),
        crc: 0,
        size: 0,
        footer_checked: false,
        api: Api::GzipFile,
    };
    let mut out = Vec::new();
    let ended = codec.run(data, FlushDecompress::Finish, &mut out)?;
    if !ended {
        return Err(raise_error(
            "Zlib::GzipFile::Error",
            "unexpected end of string".to_string(),
        ));
    }
    if !codec.footer_checked {
        return Err(raise_error(
            "Zlib::GzipFile::NoFooter",
            "footer is not found".to_string(),
        ));
    }
    let header = codec.header.take().unwrap_or_default();
    Ok((out, header, codec.trailer))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare unit test has no class registry, so `raise_error` panics rather
    /// than building an exception. Catching the panic is how the error PATHS
    /// stay testable here; the messages themselves are pinned by the golden.
    fn refuses(f: impl FnOnce() -> Result<RubyValue, Signal> + std::panic::UnwindSafe) -> bool {
        std::panic::catch_unwind(f).is_err()
    }

    fn deflater(wrap: Wrap) -> Deflating {
        Deflating {
            comp: flate2::Compress::new(Compression::default(), wrap == Wrap::Zlib),
            wrap,
            level: Compression::default(),
            strategy: 0,
            header_written: false,
            crc: 0,
            size: 0,
        }
    }

    fn deflate_all(data: &[u8], wrap: Wrap) -> Vec<u8> {
        let mut codec = Deflating {
            comp: flate2::Compress::new(Compression::default(), wrap == Wrap::Zlib),
            wrap,
            level: Compression::default(),
            strategy: 0,
            header_written: false,
            crc: 0,
            size: 0,
        };
        let mut out = Vec::new();
        assert!(codec.run(data, FlushCompress::Finish, &mut out).unwrap());
        out
    }

    fn inflate_all(data: &[u8], wrap: Wrap) -> Vec<u8> {
        let mut codec = Inflating {
            dec: flate2::Decompress::new(wrap == Wrap::Zlib),
            wrap,
            header_pending: Vec::new(),
            header_done: false,
            header: None,
            trailer: Vec::new(),
            crc: 0,
            size: 0,
            footer_checked: false,
            api: Api::Stream,
        };
        let mut out = Vec::new();
        codec.run(data, FlushDecompress::Finish, &mut out).unwrap();
        out
    }

    /// Every container round-trips, and each writes the framing its name
    /// claims -- the zlib and gzip magic bytes, and none at all for raw.
    #[test]
    fn each_container_round_trips_with_its_own_framing() {
        let text = b"the quick brown fox jumps over the lazy dog";
        for wrap in [Wrap::Zlib, Wrap::Raw, Wrap::Gzip] {
            let comp = deflate_all(text, wrap);
            assert_eq!(inflate_all(&comp, wrap), text, "{wrap:?} should round-trip");
        }
        assert_eq!(deflate_all(text, Wrap::Zlib)[0], 0x78);
        assert_eq!(deflate_all(text, Wrap::Gzip)[..2], frame::MAGIC);
        assert_ne!(deflate_all(text, Wrap::Raw)[..2], frame::MAGIC);
    }

    /// The +32 form Net::HTTP uses: one stream that reads whichever of the two
    /// containers actually arrives.
    #[test]
    fn auto_detect_reads_zlib_and_gzip_alike() {
        let text = b"auto-detected";
        assert_eq!(
            inflate_all(&deflate_all(text, Wrap::Zlib), Wrap::Auto),
            text
        );
        assert_eq!(
            inflate_all(&deflate_all(text, Wrap::Gzip), Wrap::Auto),
            text
        );
    }

    /// A header split across writes has to be reassembled -- the case a
    /// socket-backed stream hits constantly and a whole-buffer test never
    /// does. Feeding one byte at a time is the worst case of it.
    #[test]
    fn a_container_header_may_arrive_one_byte_at_a_time() {
        let text = b"split across writes";
        for wrap in [Wrap::Gzip, Wrap::Auto] {
            let comp = deflate_all(text, Wrap::Gzip);
            let mut codec = Inflating {
                dec: flate2::Decompress::new(false),
                wrap,
                header_pending: Vec::new(),
                header_done: false,
                header: None,
                trailer: Vec::new(),
                crc: 0,
                size: 0,
                footer_checked: false,
                api: Api::Stream,
            };
            let mut out = Vec::new();
            for byte in &comp {
                codec
                    .run(std::slice::from_ref(byte), FlushDecompress::None, &mut out)
                    .unwrap();
            }
            assert_eq!(out, text, "{wrap:?} should reassemble a split header");
            assert!(
                codec.footer_checked,
                "{wrap:?} should have checked a footer"
            );
        }
    }

    /// The gzip footer is the whole point of the container, so a member whose
    /// checksum or length was tampered with must not decode silently.
    #[test]
    fn a_tampered_gzip_footer_is_refused() {
        let good = deflate_all(b"hello", Wrap::Gzip);
        for at in [good.len() - 8, good.len() - 4] {
            let mut bad = good.clone();
            bad[at] ^= 0xff;
            assert!(
                refuses(move || {
                    let mut out = Vec::new();
                    let mut codec = Inflating {
                        dec: flate2::Decompress::new(false),
                        wrap: Wrap::Gzip,
                        header_pending: Vec::new(),
                        header_done: false,
                        header: None,
                        trailer: Vec::new(),
                        crc: 0,
                        size: 0,
                        footer_checked: false,
                        api: Api::Stream,
                    };
                    codec
                        .run(&bad, FlushDecompress::Finish, &mut out)
                        .map(|_| bin_str(out))
                }),
                "a footer corrupted at {at} should be refused"
            );
        }
    }

    /// zlib's own header rule, and the reason it is checked here rather than
    /// left to flate2: the pure-Rust backend's error carries no message, so
    /// CRuby's "incorrect header check" would be unreachable.
    #[test]
    fn a_non_zlib_header_is_rejected_by_the_checksum_rule() {
        assert!(refuses(
            || check_zlib_header(b"not compressed at all").map(|_| RubyValue::Nil)
        ));
        // A valid method nibble but a checksum that doesn't divide by 31.
        assert!(refuses(
            || check_zlib_header(b"\x78\x9d").map(|_| RubyValue::Nil)
        ));
        // Too short to judge yet -- the codec will ask again.
        assert!(check_zlib_header(b"\x78").is_ok());
        assert!(check_zlib_header(b"\x78\x9c").is_ok());
    }

    #[test]
    fn window_bits_choose_the_container() {
        assert_eq!(wrap_of(15, false).unwrap(), Wrap::Zlib);
        assert_eq!(wrap_of(-15, false).unwrap(), Wrap::Raw);
        assert_eq!(wrap_of(31, false).unwrap(), Wrap::Gzip);
        assert_eq!(wrap_of(47, true).unwrap(), Wrap::Auto);
        // Auto-detect is inflate-only, and 99 is nobody's window.
        assert!(refuses(|| wrap_of(47, false).map(|_| RubyValue::Nil)));
        assert!(refuses(|| wrap_of(99, true).map(|_| RubyValue::Nil)));
        assert!(refuses(|| wrap_of(0, true).map(|_| RubyValue::Nil)));
    }

    /// The incremental surface, which is the whole reason this drives flate2's
    /// low-level API: three `deflate` calls and a `finish` have to concatenate
    /// into ONE valid stream, with the container's header emitted exactly once.
    #[test]
    fn a_stream_deflated_in_pieces_reassembles() {
        let mut d = deflater(Wrap::Zlib);
        let mut parts = Vec::new();
        for chunk in [&b"hello "[..], &b"wor"[..], &b"ld"[..]] {
            let mut out = Vec::new();
            d.run(chunk, FlushCompress::None, &mut out).unwrap();
            parts.push(out);
        }
        let mut tail = Vec::new();
        assert!(d.run(&[], FlushCompress::Finish, &mut tail).unwrap());
        parts.push(tail);
        assert_eq!(inflate_all(&parts.concat(), Wrap::Zlib), b"hello world");
    }

    /// SYNC_FLUSH is what makes a compressed stream usable over a socket: the
    /// bytes so far must decode on their own, before the stream ends.
    ///
    /// It is also the case that pins the termination rule. Each SYNC_FLUSH
    /// call emits a fresh sync marker, so a loop that ran "until the codec
    /// produces nothing" would never exit -- this hung before `run` switched
    /// to zlib's own rule, "until it leaves output room".
    #[test]
    fn sync_flush_produces_a_decodable_prefix_and_terminates() {
        let mut d = deflater(Wrap::Zlib);
        let mut first = Vec::new();
        d.run(b"hello", FlushCompress::Sync, &mut first).unwrap();
        assert!(!first.is_empty(), "SYNC_FLUSH should push bytes out");

        // The prefix decodes on its own...
        let mut partial = Inflating {
            dec: flate2::Decompress::new(true),
            wrap: Wrap::Zlib,
            header_pending: Vec::new(),
            header_done: false,
            header: None,
            trailer: Vec::new(),
            crc: 0,
            size: 0,
            footer_checked: false,
            api: Api::Stream,
        };
        let mut early = Vec::new();
        partial
            .run(&first, FlushDecompress::None, &mut early)
            .unwrap();
        assert_eq!(early, b"hello");

        // ...and the whole stream still decodes once finished.
        let mut tail = Vec::new();
        d.run(&[], FlushCompress::Finish, &mut tail).unwrap();
        assert_eq!(inflate_all(&[first, tail].concat(), Wrap::Zlib), b"hello");
    }

    /// A repeated SYNC_FLUSH with no new input: the pathological shape of the
    /// loop above, run enough times that a non-terminating rule would hang.
    #[test]
    fn repeated_sync_flushes_with_no_input_terminate() {
        let mut d = deflater(Wrap::Zlib);
        let mut out = Vec::new();
        for _ in 0..50 {
            d.run(&[], FlushCompress::Sync, &mut out).unwrap();
        }
        d.run(b"payload", FlushCompress::Finish, &mut out).unwrap();
        assert_eq!(inflate_all(&out, Wrap::Zlib), b"payload");
    }

    /// zlib publishes only the text/binary distinction, and that is what a
    /// caller reads `data_type` for.
    #[test]
    fn data_type_separates_text_from_binary() {
        assert_eq!(classify(b"hello\nworld\t"), 1);
        assert_eq!(classify(b"\x00\x01\x02"), 0);
        assert_eq!(classify(&[]), 1);
    }
}
