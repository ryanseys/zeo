//! `stringio` (CRuby's bundled `stringio` gem, a C extension) -- an in-memory
//! bytes buffer with the `IO` read/write surface, no operating-system stream
//! behind it. `require "stringio"` activates it (a built-in feature; see
//! `ext/mod.rs`).
//!
//! Backed by an `RObj` over a `Mutex<State>` (a `StringIO` is mutable and
//! shared by reference). Positions are byte offsets; writes overwrite from the
//! current position and extend the buffer, exactly like a file. The read/write
//! surface -- including `seek`/`getc`/`readline`/`readlines`/`truncate` -- is
//! oracle-verified against ruby 4.0.6.
//!
//! Documented divergence: strings are handed back as UTF-8 (lossy for non-UTF-8
//! bytes), matching this runtime's default `Str` -- CRuby's StringIO preserves
//! arbitrary bytes with an encoding.

use crate::RubyValue;
use crate::builtins::eof_error;
use crate::dispatch::{RObj, RubyObject, raise_error};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_abi::STRINGIO_CLASS;
use zeo_macros::ruby_class;

struct State {
    bytes: Vec<u8>,
    /// The buffer's encoding, which every read but `read(len)` is tagged with.
    ///
    /// A `StringIO` is a byte buffer that REMEMBERS what its bytes mean: the
    /// string it was built from carries an encoding, and CRuby hands that same
    /// encoding back out of `string`/`read`/`gets`/`getc`. Without it, binary
    /// content cannot survive a round-trip -- decoding the bytes as UTF-8 to
    /// answer a read replaces every non-UTF-8 byte with U+FFFD, which is
    /// silent corruption of exactly the data a StringIO is most often used to
    /// carry (a compressed stream, an image, a socket capture).
    enc: crate::encoding::EncodingId,
    pos: usize,
    /// The line counter `gets`/`readline`/`each_line` advance and `lineno=`
    /// sets. It is bookkeeping, not a position: CRuby never derives one from
    /// the other, so seeking does not touch it.
    lineno: i64,
    /// CRuby closes the two directions independently (`close_read` /
    /// `close_write`), and `closed?` is true only when BOTH are. `close`
    /// shuts both.
    closed_read: bool,
    closed_write: bool,
}

pub struct RStringIO {
    state: Mutex<State>,
    frozen: AtomicBool,
}

impl RStringIO {
    fn with_bytes(bytes: Vec<u8>, enc: crate::encoding::EncodingId) -> RStringIO {
        RStringIO {
            state: Mutex::new(State {
                bytes,
                enc,
                pos: 0,
                lineno: 0,
                closed_read: false,
                closed_write: false,
            }),
            frozen: AtomicBool::new(false),
        }
    }
}

impl RubyObject for RStringIO {
    fn class_id(&self) -> crate::ClassId {
        STRINGIO_CLASS
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
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let s = self.state.lock();
        let io = RStringIO::with_bytes(s.bytes.clone(), s.enc);
        io.state.lock().pos = s.pos;
        if copy_frozen {
            io.set_frozen();
        }
        Arc::new(io)
    }
}

/// The `RStringIO` behind a receiver -- the table only dispatches on one.
fn io_of(recv: &RubyValue) -> &RStringIO {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RStringIO>()
            .expect("the StringIO table only dispatches on StringIO receivers"),
        _ => unreachable!("the StringIO table only dispatches on StringIO receivers"),
    }
}

/// The bytes of a `String` argument, or `to_s` of anything else (what
/// `IO#write`/`#print`/`#puts` do to non-strings).
fn arg_bytes(v: &RubyValue) -> Vec<u8> {
    match v {
        RubyValue::Str(s) => s.lock().bytes().to_vec(),
        other => other.to_display_string().into_bytes(),
    }
}

/// Bytes out of the buffer, tagged with the buffer's own encoding -- what
/// `string`/`read`/`gets`/`getc`/`readlines` all answer.
fn bytes_to_str(bytes: &[u8], enc: crate::encoding::EncodingId) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes.to_vec(), enc))
}

/// Bytes out of the buffer as ASCII-8BIT. `read(len)` is the one read that
/// ignores the buffer's encoding -- CRuby documents it as reading in binary
/// mode, because a length in BYTES can land mid-character.
fn bytes_to_binary(bytes: &[u8]) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(
        bytes.to_vec(),
        crate::encoding::ASCII_8BIT,
    ))
}

/// `readpartial`/`sysread`/`read_nonblock`, which differ only at EOF.
///
/// `raises` false is `read_nonblock(n, exception: false)`, which answers nil
/// where the others raise. A zero length never reaches EOF: CRuby answers `""`
/// even on an exhausted buffer.
fn partial_read(
    recv: &RubyValue,
    maxlen: &RubyValue,
    buffer: Option<&RubyValue>,
    raises: bool,
) -> Result<RubyValue, crate::Signal> {
    let n = crate::builtins::convert::to_index(maxlen)?;
    if n < 0 {
        return Err(crate::builtins::arg_error!("negative length {n} given"));
    }
    let outbuf = match buffer {
        None | Some(RubyValue::Nil) => None,
        Some(v) => Some(crate::builtins::convert::to_rstr(v)?),
    };
    let n = n as usize;

    let mut s = io_of(recv).state.lock();
    if s.pos >= s.bytes.len() && n > 0 {
        // CRuby empties the buffer before reporting EOF, so a rescued end
        // leaves no stale bytes from the previous read.
        if let Some(buf) = &outbuf {
            buf.lock().replace_utf8(String::new());
        }
        return if raises {
            Err(crate::builtins::eof_error!("end of file reached"))
        } else {
            Ok(RubyValue::Nil)
        };
    }
    let end = (s.pos + n).min(s.bytes.len());
    let out = s.bytes[s.pos..end].to_vec();
    s.pos = end;
    drop(s);

    match outbuf {
        Some(buf) => {
            buf.lock().replace_bytes(out, crate::encoding::ASCII_8BIT);
            Ok(buffer
                .expect("outbuf is Some only when a buffer was passed")
                .clone())
        }
        None => Ok(bytes_to_binary(&out)),
    }
}

/// Overwrite-from-`pos` write, extending the buffer as a file would.
/// The two direction guards. CRuby closes reading and writing separately and
/// refuses the matching half afterwards, with these exact messages -- rack's
/// Lint and every `IO`-shaped wrapper rely on the refusal, not on a silent
/// empty answer.
///
/// Only the I/O surface is guarded. `string`, `pos` and `size` keep answering
/// on a fully closed StringIO, because they read the OBJECT, not the stream.
fn check_readable(recv: &RubyValue) -> Result<(), crate::Signal> {
    if io_of(recv).state.lock().closed_read {
        return Err(crate::builtins::io_error!("not opened for reading"));
    }
    Ok(())
}

fn check_writable(recv: &RubyValue) -> Result<(), crate::Signal> {
    if io_of(recv).state.lock().closed_write {
        return Err(crate::builtins::io_error!("not opened for writing"));
    }
    Ok(())
}

/// Replace the buffer with `arg`'s bytes and start over: position 0, line 0,
/// and the argument's own encoding, because a StringIO remembers what its
/// bytes mean. Shared by `string=` and `reopen`, which differ only in what
/// they answer.
fn set_buffer(recv: &RubyValue, arg: &RubyValue) {
    let (bytes, enc) = match arg {
        RubyValue::Str(sp) => {
            let g = sp.lock();
            (g.bytes().to_vec(), g.encoding())
        }
        other => (
            other.to_display_string().into_bytes(),
            crate::encoding::UTF_8,
        ),
    };
    let mut s = io_of(recv).state.lock();
    s.bytes = bytes;
    s.enc = enc;
    s.pos = 0;
    s.lineno = 0;
}

fn write_at(state: &mut State, data: &[u8]) {
    let end = state.pos + data.len();
    if state.bytes.len() < end {
        state.bytes.resize(end, 0);
    }
    state.bytes[state.pos..end].copy_from_slice(data);
    state.pos = end;
}

/// The body of `ungetc`/`ungetbyte`: turn the argument into bytes, step back
/// over them, and OVERWRITE from there -- ruby replaces rather than restores,
/// so the pushed-back bytes really appear in the buffer. `byte_form` is
/// `ungetbyte`'s rule, where an Integer contributes its low byte; `ungetc`
/// takes an Integer as a CODEPOINT instead.
fn unget_bytes(
    recv: &RubyValue,
    arg: &RubyValue,
    byte_form: bool,
) -> Result<RubyValue, crate::Signal> {
    let mut s = io_of(recv).state.lock();
    let bytes: Vec<u8> = match arg {
        RubyValue::Nil => return Ok(RubyValue::Nil),
        RubyValue::Str(buf) => buf.lock().bytes().to_vec(),
        RubyValue::Int(n) if byte_form => vec![(*n & 0xFF) as u8],
        RubyValue::Int(n) => match u32::try_from(*n).ok().and_then(char::from_u32) {
            Some(c) => c.to_string().into_bytes(),
            None => return Err(crate::builtins::range_error!("{n} out of char range")),
        },
        other => crate::builtins::convert::to_rstr(other)?
            .lock()
            .bytes()
            .to_vec(),
    };
    if bytes.is_empty() {
        return Ok(RubyValue::Nil);
    }
    let back = bytes.len().min(s.pos);
    s.pos -= back;
    let at = s.pos;
    // Grow first if the write runs past the end, then overwrite in place.
    if at + bytes.len() > s.bytes.len() {
        s.bytes.resize(at + bytes.len(), 0);
    }
    s.bytes[at..at + bytes.len()].copy_from_slice(&bytes);
    Ok(RubyValue::Nil)
}

ruby_class! {
    StringIO = zeo_abi::STRINGIO_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    // The whole buffer as a String, independent of position.
    def "string" (recv) {
        let s = io_of(recv).state.lock();
        Ok(bytes_to_str(&s.bytes, s.enc))
    }
    // The IO encoding pair, as `IO` answers it: the buffer's own encoding
    // outward, and no transcoding on the way in. csv's writer reads both to
    // decide whether it must convert what it is about to emit.
    def "external_encoding" (recv) {
        crate::dispatch::send_value(
            &{ let s = io_of(recv).state.lock(); bytes_to_str(&s.bytes, s.enc) },
            crate::Symbol::intern("encoding"),
            &[],
            None,
        )
    }
    def "internal_encoding" (_recv) {
        Ok(RubyValue::Nil)
    }
    def "read" (recv, arg?) {
        check_readable(recv)?;
        let mut s = io_of(recv).state.lock();
        match arg {
            None | Some(RubyValue::Nil) => {
                let out = s.bytes[s.pos.min(s.bytes.len())..].to_vec();
                s.pos = s.bytes.len();
                Ok(bytes_to_str(&out, s.enc))
            }
            Some(v) => {
                let n = crate::builtins::convert::to_index(v)?;
                if n < 0 {
                    return Err(crate::builtins::arg_error!("negative length {n} given"));
                }
                let n = n as usize;
                if s.pos >= s.bytes.len() && n > 0 {
                    return Ok(RubyValue::Nil); // EOF with a nonzero length is nil
                }
                let end = (s.pos + n).min(s.bytes.len());
                let out = s.bytes[s.pos..end].to_vec();
                s.pos = end;
                Ok(bytes_to_binary(&out))
            }
        }
    }
    // The partial-read family. A StringIO never blocks, so all three take the
    // same bytes `read` would; they differ from it only at EOF, where `read`
    // answers nil and these raise. `read_nonblock`'s `exception: false` asks
    // for nil back instead.
    //
    // `net/protocol` reaches `read_nonblock` through `Net::BufferedIO`, which
    // is how net/http and net/smtp read from any IO.
    def "readpartial" | "sysread" cfunc (recv, maxlen, buffer?, &_blk) {
        check_readable(recv)?;
        partial_read(recv, maxlen, buffer, true)
    }
    def "read_nonblock" cfunc (recv, maxlen, buffer?, **opts, &_blk) {
        check_readable(recv)?;
        let raises = crate::builtins::io::nonblock_raises(opts);
        partial_read(recv, maxlen, buffer, raises)
    }
    def "write" (recv, *args, &_block) {
        check_writable(recv)?;
        let mut written = 0usize;
        let mut s = io_of(recv).state.lock();
        for a in args {
            let b = arg_bytes(a);
            written += b.len();
            write_at(&mut s, &b);
        }
        Ok(RubyValue::Int(written as i64))
    }
    def "<<" (recv, other) {
        check_writable(recv)?;
        let mut s = io_of(recv).state.lock();
        write_at(&mut s, &arg_bytes(other));
        Ok(recv.clone())
    }
    def "print" (recv, *args, &_block) {
        check_writable(recv)?;
        let mut s = io_of(recv).state.lock();
        for a in args {
            write_at(&mut s, &arg_bytes(a));
        }
        Ok(RubyValue::Nil)
    }
    def "puts" (recv, *args, &_block) {
        check_writable(recv)?;
        let mut s = io_of(recv).state.lock();
        if args.is_empty() {
            write_at(&mut s, b"\n");
        }
        for a in args {
            puts_one(&mut s, a);
        }
        Ok(RubyValue::Nil)
    }
    // `gets([sep][, limit][, chomp:])`. The argument shape is IO's, parsed by
    // IO's own `line_opts` -- StringIO is meant to be drop-in, so the two must
    // not drift. The bytes are sliced here rather than reused from IO because a
    // StringIO remembers its buffer's encoding.
    def "gets" as gets (recv, _sep?, _limit?, **_opts, &_blk) {
        check_readable(recv)?;
        let opts = crate::builtins::io::line_opts(__args);
        let mut s = io_of(recv).state.lock();
        if s.pos >= s.bytes.len() {
            return Ok(RubyValue::Nil);
        }
        let rest = &s.bytes[s.pos..];
        let mut end = match &opts.sep {
            // A nil separator slurps the rest.
            None => s.bytes.len(),
            Some(sep) if sep.is_empty() => s.bytes.len(),
            Some(sep) => find_sub(rest, sep)
                .map(|i| s.pos + i + sep.len())
                .unwrap_or(s.bytes.len()),
        };
        if let Some(limit) = opts.limit {
            end = end.min(s.pos + limit);
        }
        let mut line = s.bytes[s.pos..end].to_vec();
        s.pos = end;
        // Every line HANDED BACK advances the counter `lineno` reports --
        // including the ones `each_line`/`readlines` take through here.
        s.lineno += 1;
        crate::builtins::io::chomp_line(&mut line, &opts);
        Ok(bytes_to_str(&line, s.enc))
    }
    def "each_line" | "each" cfunc (recv, _sep?, _limit?, **_opts, &block) {
        // Blockless, this is an Enumerator over the same lines -- Ruby's rule
        // for every `each_*`, and what `each_line.to_a` (csv's reader) needs.
        // The Enumerator has to carry the arguments, or `each_line(chomp: true)
        // .to_a` would replay the walk without them.
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "each_line", __args));
        };
        loop {
            let line = gets(recv, __args, None)?;
            if matches!(line, RubyValue::Nil) {
                break;
            }
            p.call(&[line])?;
        }
        Ok(recv.clone())
    }
    // `each_char` -- yield each character; blockless, an Enumerator.
    def "each_char" (recv, &block) {
        check_readable(recv)?;
        let p = crate::builtins::block_or_enum!(recv, __args, block);
        loop {
            let ch = {
                let mut s = io_of(recv).state.lock();
                if s.pos >= s.bytes.len() {
                    break;
                }
                let len = char_len(s.enc, s.bytes[s.pos]).min(s.bytes.len() - s.pos);
                let ch = s.bytes[s.pos..s.pos + len].to_vec();
                s.pos += len;
                bytes_to_str(&ch, s.enc)
            };
            p.call(&[ch])?;
        }
        Ok(recv.clone())
    }
    // `printf(fmt, *args)`. Without a row here the name resolves to the
    // PRIVATE `Kernel#printf`, so an explicit receiver is refused -- which is
    // exactly what a StringIO is for.
    def "printf" cfunc (recv, fmt, *args, &_blk) {
        check_writable(recv)?;
        let s = crate::builtins::format::sprintf(&fmt.try_display_string()?, args)?;
        write_at(&mut io_of(recv).state.lock(), s.as_bytes());
        Ok(RubyValue::Nil)
    }
    def "eof?" | "eof" (recv) {
        check_readable(recv)?;
        let s = io_of(recv).state.lock();
        Ok(RubyValue::Bool(s.pos >= s.bytes.len()))
    }
    def "rewind" (recv) {
        io_of(recv).state.lock().pos = 0;
        Ok(RubyValue::Int(0))
    }
    def "pos" | "tell" (recv) {
        Ok(RubyValue::Int(io_of(recv).state.lock().pos as i64))
    }
    def "pos=" (recv, arg) {
        let n = &crate::builtins::convert::to_index(arg)?;
        io_of(recv).state.lock().pos = (*n).max(0) as usize;
        Ok((*arg).clone())
    }
    def "size" | "length" (recv) {
        Ok(RubyValue::Int(io_of(recv).state.lock().bytes.len() as i64))
    }
    def "close" (recv) {
        let mut s = io_of(recv).state.lock();
        s.closed_read = true;
        s.closed_write = true;
        Ok(RubyValue::Nil)
    }
    def "closed?" (recv) {
        let s = io_of(recv).state.lock();
        Ok(RubyValue::Bool(s.closed_read && s.closed_write))
    }
    def "close_read" (recv) {
        io_of(recv).state.lock().closed_read = true;
        Ok(RubyValue::Nil)
    }
    def "close_write" (recv) {
        io_of(recv).state.lock().closed_write = true;
        Ok(RubyValue::Nil)
    }
    def "closed_read?" (recv) {
        Ok(RubyValue::Bool(io_of(recv).state.lock().closed_read))
    }
    def "closed_write?" (recv) {
        Ok(RubyValue::Bool(io_of(recv).state.lock().closed_write))
    }

    // The stream methods a StringIO answers WITHOUT a stream behind it.
    // CRuby defines every one of these so a StringIO is drop-in wherever an
    // IO is expected -- rack's Lint refuses an error stream that does not
    // answer `#flush`, and a buffer that cannot say `false` to `tty?` is not
    // a stand-in for anything. The answers are CRuby's own, not plausible
    // ones: `flush`/`binmode` hand back the receiver, `fsync` is 0 (the
    // syscall's success value), `fileno`/`pid` are nil (there is no
    // descriptor and no child), and `sync` is TRUE -- an in-memory buffer is
    // always already flushed.
    def "flush" (recv) {
        Ok(recv.clone())
    }
    def "binmode" (recv) {
        Ok(recv.clone())
    }
    def "fsync" (_recv) {
        Ok(RubyValue::Int(0))
    }
    def "fileno" (_recv) {
        Ok(RubyValue::Nil)
    }
    def "pid" (_recv) {
        Ok(RubyValue::Nil)
    }
    def "isatty" | "tty?" (_recv) {
        Ok(RubyValue::Bool(false))
    }
    def "sync" (_recv) {
        Ok(RubyValue::Bool(true))
    }
    // Accepted and ignored -- there is no buffer to stop buffering. CRuby
    // answers the ARGUMENT, as every `foo=` does.
    def "sync=" (_recv, arg) {
        Ok((*arg).clone())
    }
    // CRuby raises here rather than pretending: `fcntl` is a descriptor
    // operation and a StringIO has no descriptor. The message is the
    // machine-level one it uses.
    def "fcntl" cfunc (_recv, *_args) {
        Err(crate::builtins::not_impl_error!(
            "fcntl() function is unimplemented on this machine"
        ))
    }

    // The line counter. It is bookkeeping the reader advances, never derived
    // from the position -- `seek`/`rewind` leave it alone, which is why it
    // has to be stored rather than computed.
    def "lineno" (recv) {
        Ok(RubyValue::Int(io_of(recv).state.lock().lineno))
    }
    def "lineno=" (recv, arg) {
        let n = crate::builtins::convert::to_index(arg)?;
        io_of(recv).state.lock().lineno = n;
        Ok((*arg).clone())
    }

    // `each_byte` / `each_codepoint` -- the two walks `each_char` did not
    // cover. Blockless, each is an Enumerator, like every `each_*`.
    def "each_byte" (recv, &block) {
        check_readable(recv)?;
        let p = crate::builtins::block_or_enum!(recv, __args, block);
        loop {
            let b = {
                let mut s = io_of(recv).state.lock();
                if s.pos >= s.bytes.len() {
                    break;
                }
                let b = s.bytes[s.pos];
                s.pos += 1;
                b
            };
            p.call(&[RubyValue::Int(i64::from(b))])?;
        }
        Ok(recv.clone())
    }
    def "each_codepoint" (recv, &block) {
        check_readable(recv)?;
        let p = crate::builtins::block_or_enum!(recv, __args, block);
        loop {
            let cp = {
                let mut s = io_of(recv).state.lock();
                if s.pos >= s.bytes.len() {
                    break;
                }
                let len = char_len(s.enc, s.bytes[s.pos]).min(s.bytes.len() - s.pos);
                let chunk = &s.bytes[s.pos..s.pos + len];
                // Undecodable bytes answer their own value, which is what
                // CRuby does for a single-byte encoding and the honest answer
                // for a broken sequence.
                let cp = std::str::from_utf8(chunk)
                    .ok()
                    .and_then(|t| t.chars().next())
                    .map_or(i64::from(chunk[0]), |c| i64::from(c as u32));
                s.pos += len;
                cp
            };
            p.call(&[RubyValue::Int(cp)])?;
        }
        Ok(recv.clone())
    }

    // `putc(obj)` -- write ONE character: an Integer's low byte, or a
    // String's first character. It answers its argument, not the count.
    def "putc" (recv, arg) {
        check_writable(recv)?;
        let byte = match arg {
            RubyValue::Str(_) => {
                // An empty String writes NOTHING and still answers itself --
                // CRuby's own answer, not an error.
                let b = arg_bytes(arg);
                if b.is_empty() {
                    return Ok((*arg).clone());
                }
                let len = { let s = io_of(recv).state.lock(); char_len(s.enc, b[0]).min(b.len()) };
                b[..len].to_vec()
            }
            other => vec![(crate::builtins::convert::to_index(other)? & 0xff) as u8],
        };
        write_at(&mut io_of(recv).state.lock(), &byte);
        Ok((*arg).clone())
    }

    // `pread(len, offset)` -- read WITHOUT moving the position, which is the
    // whole point of the name.
    // `cfunc`: CRuby declares this `argc = -1`, so it reports arity -1 even
    // though it accepts 2..3.
    def "pread" cfunc (recv, len, offset, buffer?) {
        check_readable(recv)?;
        let len = crate::builtins::convert::to_index(len)?.max(0) as usize;
        let off = crate::builtins::convert::to_index(offset)?.max(0) as usize;
        let s = io_of(recv).state.lock();
        // A zero-length read never reaches for a byte, so it answers "" even
        // at the very end; any other read starting AT or past the end is EOF.
        if len > 0 && off >= s.bytes.len() {
            return Err(crate::builtins::eof_error!("end of file reached"));
        }
        let end = (off + len).min(s.bytes.len());
        let (bytes, enc) = (s.bytes[off..end].to_vec(), s.enc);
        drop(s);
        // A buffer argument RECEIVES the bytes and is what comes back, the
        // same contract `readpartial` keeps.
        match buffer {
            None | Some(RubyValue::Nil) => Ok(bytes_to_str(&bytes, enc)),
            Some(v) => {
                crate::builtins::convert::to_rstr(v)?
                    .lock()
                    .replace_bytes(bytes, enc);
                Ok((*v).clone())
            }
        }
    }

    // `string = str` / `reopen(str)` -- replace the buffer and rewind. CRuby
    // answers the new string from the assignment and the receiver from
    // `reopen`, which is the only difference between them here.
    def "string=" (recv, arg) {
        set_buffer(recv, arg);
        Ok((*arg).clone())
    }
    def "reopen" cfunc (recv, arg?, *_rest) {
        if let Some(arg) = arg {
            set_buffer(recv, arg);
        }
        Ok(recv.clone())
    }

    // `set_encoding(enc)` retags the buffer WITHOUT converting the bytes --
    // it declares what they already mean. It answers the receiver.
    def "set_encoding" cfunc (recv, enc, *_rest) {
        io_of(recv).state.lock().enc = crate::builtins::encoding::arg_encoding(enc)?;
        Ok(recv.clone())
    }
    // A leading byte-order mark NAMES the encoding; CRuby consumes it, retags
    // the buffer, and answers the Encoding. No mark means no answer (nil) and
    // nothing consumed.
    def "set_encoding_by_bom" (recv) {
        let mut s = io_of(recv).state.lock();
        let (id, len) = match s.bytes.as_slice() {
            [0xEF, 0xBB, 0xBF, ..] => (crate::encoding::UTF_8, 3usize),
            [0xFF, 0xFE, 0x00, 0x00, ..] => (crate::encoding::UTF_32LE, 4),
            [0x00, 0x00, 0xFE, 0xFF, ..] => (crate::encoding::UTF_32BE, 4),
            [0xFF, 0xFE, ..] => (crate::encoding::UTF_16LE, 2),
            [0xFE, 0xFF, ..] => (crate::encoding::UTF_16BE, 2),
            _ => return Ok(RubyValue::Nil),
        };
        s.enc = id;
        s.pos = len;
        drop(s);
        Ok(crate::builtins::encoding::encoding_value(id))
    }

    // `seek(offset, whence = SEEK_SET)` -- reposition; whence 0/1/2 =
    // absolute/relative/from-end. Returns 0, like CRuby's IO#seek.
    def "seek" cfunc (recv, arg1, arg2?) {
        let off = &crate::builtins::convert::to_index(arg1)?;
        let whence = match arg2 {
            None => 0,
            Some(w) => crate::builtins::convert::to_index(w)?,
        };
        let mut s = io_of(recv).state.lock();
        let base = match whence {
            0 => 0i64,
            1 => s.pos as i64,
            2 => s.bytes.len() as i64,
            _ => return Err(raise_error("Errno::EINVAL", "Invalid argument".to_string())),
        };
        let target = base + off;
        if target < 0 {
            return Err(raise_error("Errno::EINVAL", "Invalid argument - invalid seek".to_string()));
        }
        s.pos = target as usize;
        Ok(RubyValue::Int(0))
    }
    // `getc` -- one character (the next whole UTF-8 char), or nil at EOF.
    def "getc" (recv) {
        check_readable(recv)?;
        let mut s = io_of(recv).state.lock();
        if s.pos >= s.bytes.len() {
            return Ok(RubyValue::Nil);
        }
        let len = char_len(s.enc, s.bytes[s.pos]).min(s.bytes.len() - s.pos);
        let ch = s.bytes[s.pos..s.pos + len].to_vec();
        s.pos += len;
        Ok(bytes_to_str(&ch, s.enc))
    }
    // `ungetc(str_or_int)` / `ungetbyte(str_or_int)` -- push bytes back so the
    // next read sees them. Ruby does NOT restore what was there: it OVERWRITES
    // at the new position, so `read(2); getc; ungetc("Z"); read` on "hello"
    // answers "Zlo". Both answer nil.
    //
    // At position 0 the bytes are PREPENDED instead (there is nothing to back
    // over), which is CRuby's own edge.
    def "ungetc" (recv, arg) {
        check_readable(recv)?;
        unget_bytes(recv, arg, false)
    }
    def "ungetbyte" (recv, arg) {
        check_readable(recv)?;
        unget_bytes(recv, arg, true)
    }
    // `getbyte` -- one BYTE as an Integer, `nil` at end. Byte-wise, not
    // character-wise like `getc`: prism's deserializer reads its buffer this
    // way, and a multi-byte encoding must not make it skip.
    def "getbyte" (recv) {
        check_readable(recv)?;
        let mut s = io_of(recv).state.lock();
        if s.pos >= s.bytes.len() {
            return Ok(RubyValue::Nil);
        }
        let b = s.bytes[s.pos];
        s.pos += 1;
        Ok(RubyValue::Int(i64::from(b)))
    }
    // `readbyte` -- `getbyte`, but raising `EOFError` at end rather than
    // answering nil (the same pairing `getc`/`readchar` have).
    def "readbyte" (recv) {
        check_readable(recv)?;
        let mut s = io_of(recv).state.lock();
        if s.pos >= s.bytes.len() {
            return Err(eof_error!("end of file reached"));
        }
        let b = s.bytes[s.pos];
        s.pos += 1;
        Ok(RubyValue::Int(i64::from(b)))
    }
    // `readline([sep][, limit][, chomp:])` -- `gets`, but raising at end.
    def "readline" (recv, _sep?, _limit?, **_opts, &_blk) {
        match gets(recv, __args, None)? {
            RubyValue::Nil => Err(eof_error!("end of file reached")),
            line => Ok(line),
        }
    }
    // `readlines([sep][, limit][, chomp:])` -- every remaining line as an Array.
    def "readlines" (recv, _sep?, _limit?, **_opts, &_blk) {
        let mut lines = Vec::new();
        loop {
            match gets(recv, __args, None)? {
                RubyValue::Nil => break,
                line => lines.push(line),
            }
        }
        Ok(RubyValue::Array(crate::array_new(lines)))
    }
    // `truncate(len)` -- resize the buffer, zero-padding when it grows.
    // Returns 0 (CRuby's IO#truncate result).
    def "truncate" (recv, arg) {
        check_writable(recv)?;
        let len = &crate::builtins::convert::to_index(arg)?;
        if *len < 0 {
            return Err(raise_error("Errno::EINVAL", "Invalid argument".to_string()));
        }
        io_of(recv).state.lock().bytes.resize(*len as usize, 0);
        Ok(RubyValue::Int(0))
    }

    // `mode` is ignored for now. A block is not `new`'s to run -- CRuby
    // warns and ignores it, naming the caller's line (`rb_warn`'s shape).
    def self."new" allocs (_recv, string?, _mode?, &block) {
        if block.is_some() {
            let mut buf = Vec::new();
            if let Some(&(file, line, _)) = crate::frames::caller_frames(0).first() {
                buf.extend_from_slice(format!("{file}:{line}: ").as_bytes());
            }
            buf.extend_from_slice(
                b"warning: StringIO::new() does not take block; use StringIO::open() instead\n",
            );
            crate::builtins::io::write_bytes(&crate::builtins::io::current_stderr(), &buf)?;
        }
        new_stringio(string)
    }
    // `File.open`'s contract: with a block, yield the new io, answer the
    // BLOCK's value, and close the io on every exit path -- run, stash,
    // close, then propagate.
    def self."open" allocs (_recv, string?, _mode?, &block) {
        let io = new_stringio(string)?;
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(io);
        };
        let out = p.call(std::slice::from_ref(&io));
        let _ = crate::dispatch::send_value(&io, crate::Symbol::intern("close"), &[], None);
        out
    }
}

/// The shared `StringIO.new`/`.open` constructor: an empty `StringIO.new` is
/// UTF-8, as the `""` it stands in for is.
fn new_stringio(string: Option<&RubyValue>) -> Result<RubyValue, crate::Signal> {
    let (bytes, enc) = match string {
        None | Some(RubyValue::Nil) => (Vec::new(), crate::encoding::UTF_8),
        Some(v) => {
            let s = crate::builtins::convert::to_rstr(v)?;
            let s = s.lock();
            (s.bytes().to_vec(), s.encoding())
        }
    };
    Ok(RubyValue::Object(Arc::new(RStringIO::with_bytes(
        bytes, enc,
    ))))
}

/// How many bytes the character starting with `lead` occupies in `enc`. Only
/// UTF-8 is multi-byte here; in a binary buffer every byte is its own
/// character, which is why `getc` on one answers a single byte.
fn char_len(enc: crate::encoding::EncodingId, lead: u8) -> usize {
    if enc == crate::encoding::UTF_8 {
        utf8_char_len(lead)
    } else {
        1
    }
}

/// The byte length of a UTF-8 sequence given its leading byte (1 for ASCII or
/// an invalid lead byte, so `getc` always makes progress).
fn utf8_char_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

/// `puts` for a single argument: arrays flatten (each element on its own
/// line); scalars get a trailing newline unless they already end in one.
fn puts_one(state: &mut State, v: &RubyValue) {
    if let RubyValue::Array(a) = v {
        for el in a.lock().iter() {
            puts_one(state, el);
        }
        return;
    }
    let mut b = arg_bytes(v);
    if !b.ends_with(b"\n") {
        b.push(b'\n');
    }
    write_at(state, &b);
}

/// First index of `needle` in `haystack` (empty needle never matches -- `gets`
/// treats an empty separator specially in CRuby; unsupported here, documented).
fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string_new;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    fn text(v: &RubyValue) -> String {
        match v {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }
    /// `StringIO`'s `ruby_class!`-generated methods have mangled Rust idents, so
    /// the tests reach them through the registered instance/class tables.
    fn tbl() -> &'static crate::builtins::BuiltinClassTable {
        crate::builtins::registered_table(zeo_abi::STRINGIO_CLASS)
            .expect("StringIO is a registered builtin table")
    }
    fn im(name: &str) -> crate::builtins::BuiltinMethodFn {
        let t = tbl()
            .instance
            .as_ref()
            .expect("StringIO has instance methods");
        (t.lookup)(name).unwrap_or_else(|| panic!("StringIO#{name} is defined"))
    }
    fn cm(name: &str) -> crate::builtins::BuiltinMethodFn {
        let t = tbl().class.as_ref().expect("StringIO has class methods");
        (t.lookup)(name).unwrap_or_else(|| panic!("StringIO.{name} is defined"))
    }

    #[test]
    fn puts_then_string_matches_ruby() {
        let io = cm("new")(&RubyValue::Nil, &[], None).unwrap();
        im("puts")(&io, &[s("a")], None).unwrap();
        im("<<")(&io, &[s("b")], None).unwrap();
        assert_eq!(text(&im("string")(&io, &[], None).unwrap()), "a\nb");
    }

    #[test]
    fn read_and_rewind_track_position() {
        let io = cm("new")(&RubyValue::Nil, &[s("hello")], None).unwrap();
        assert_eq!(
            text(&im("read")(&io, &[RubyValue::Int(3)], None).unwrap()),
            "hel"
        );
        assert_eq!(text(&im("read")(&io, &[], None).unwrap()), "lo");
        assert!(matches!(
            im("eof?")(&io, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
        im("rewind")(&io, &[], None).unwrap();
        assert_eq!(text(&im("read")(&io, &[], None).unwrap()), "hello");
    }

    #[test]
    fn gets_returns_lines_then_nil() {
        let io = cm("new")(&RubyValue::Nil, &[s("a\nb")], None).unwrap();
        assert_eq!(text(&im("gets")(&io, &[], None).unwrap()), "a\n");
        assert_eq!(text(&im("gets")(&io, &[], None).unwrap()), "b");
        assert!(matches!(
            im("gets")(&io, &[], None).unwrap(),
            RubyValue::Nil
        ));
    }

    /// A StringIO over BINARY content must hand the same bytes back. Decoding
    /// them as UTF-8 to answer a read replaced every non-UTF-8 byte with
    /// U+FFFD -- silent corruption, and the reason a gzip member could not
    /// survive a `StringIO` round-trip. Byte 0x8b is a gzip header byte and
    /// is not valid UTF-8 on its own, which is what makes it the case to pin.
    #[test]
    fn binary_content_survives_a_round_trip() {
        let raw: Vec<u8> = vec![0x1f, 0x8b, 0x08, 0x00, 0xc8];
        let binary = RubyValue::Str(crate::string_from_bytes(
            raw.clone(),
            crate::encoding::ASCII_8BIT,
        ));
        let io = cm("new")(&RubyValue::Nil, &[binary], None).unwrap();

        for method in ["string", "read"] {
            let got = im(method)(&io, &[], None).unwrap();
            let RubyValue::Str(s) = &got else {
                panic!("{method} should answer a String, got {got:?}");
            };
            assert_eq!(s.lock().bytes(), &raw[..], "{method} corrupted the bytes");
            assert_eq!(s.lock().encoding(), crate::encoding::ASCII_8BIT);
            im("rewind")(&io, &[], None).unwrap();
        }

        // `read(len)` is the one read CRuby answers in binary regardless, and
        // a byte count may land mid-character -- so it must not re-encode.
        let head = im("read")(&io, &[RubyValue::Int(2)], None).unwrap();
        let RubyValue::Str(head) = head else {
            panic!("read(2) should answer a String")
        };
        assert_eq!(head.lock().bytes(), &raw[..2]);

        // In a binary buffer every byte is its own character.
        im("rewind")(&io, &[], None).unwrap();
        let ch = im("getc")(&io, &[], None).unwrap();
        let RubyValue::Str(ch) = ch else {
            panic!("getc should answer a String")
        };
        assert_eq!(ch.lock().bytes(), &[0x1f]);
    }

    /// The other half of the same rule: a UTF-8 buffer keeps ITS encoding, and
    /// `getc` there walks whole characters rather than bytes.
    #[test]
    fn utf8_content_keeps_its_encoding_and_char_boundaries() {
        let io = cm("new")(
            &RubyValue::Nil,
            &[RubyValue::Str(string_new("\u{e9}a".to_string()))],
            None,
        )
        .unwrap();
        let ch = im("getc")(&io, &[], None).unwrap();
        let RubyValue::Str(ch) = ch else {
            panic!("getc should answer a String")
        };
        assert_eq!(ch.lock().bytes(), "\u{e9}".as_bytes());
        assert_eq!(ch.lock().encoding(), crate::encoding::UTF_8);
    }

    #[test]
    fn getc_reads_one_char_then_nil() {
        let io = cm("new")(&RubyValue::Nil, &[s("hé")], None).unwrap();
        assert_eq!(text(&im("getc")(&io, &[], None).unwrap()), "h");
        assert_eq!(text(&im("getc")(&io, &[], None).unwrap()), "é");
        assert!(matches!(
            im("getc")(&io, &[], None).unwrap(),
            RubyValue::Nil
        ));
    }

    #[test]
    fn seek_repositions_by_whence() {
        let io = cm("new")(&RubyValue::Nil, &[s("abcdef")], None).unwrap();
        im("seek")(&io, &[RubyValue::Int(2)], None).unwrap();
        assert_eq!(
            text(&im("read")(&io, &[RubyValue::Int(2)], None).unwrap()),
            "cd"
        );
        im("seek")(&io, &[RubyValue::Int(-1), RubyValue::Int(2)], None).unwrap();
        assert_eq!(text(&im("read")(&io, &[], None).unwrap()), "f");
    }

    #[test]
    fn readlines_collects_every_line() {
        let io = cm("new")(&RubyValue::Nil, &[s("a\nb\nc")], None).unwrap();
        let RubyValue::Array(a) = im("readlines")(&io, &[], None).unwrap() else {
            panic!("expected an Array")
        };
        let lines: Vec<String> = a.lock().iter().map(text).collect();
        assert_eq!(lines, vec!["a\n", "b\n", "c"]);
    }

    #[test]
    fn truncate_resizes_the_buffer() {
        let io = cm("new")(&RubyValue::Nil, &[s("hello world")], None).unwrap();
        im("truncate")(&io, &[RubyValue::Int(5)], None).unwrap();
        assert_eq!(text(&im("string")(&io, &[], None).unwrap()), "hello");
    }
}
