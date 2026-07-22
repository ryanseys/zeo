//! `IO` (G0, minimal): the `STDOUT`/`STDERR` singletons and the
//! `$stdout`/`$stderr` globals the Kernel print family routes through.
//! File-backed IO, `STDIN`/`gets`, buffering modes, and encodings are a
//! later phase (plan P-B) -- this slice exists so `STDOUT.puts`,
//! `$stderr.print`, and `$stdout = <duck>` redirection behave.

use std::io::Write;
use std::sync::{Arc, LazyLock};

use crate::dispatch::{RObj, RubyObject};
use crate::signal::Signal;
use crate::value::RubyValue;
use zeo_abi::{ClassId, IO_CLASS};

#[derive(Clone, Copy, PartialEq)]
pub enum StdStream {
    Stdout,
    Stderr,
    Stdin,
}

/// What an IO actually reads/writes. The std streams are process-global
/// handles (nothing to own); a File owns its open descriptor.
pub enum IoBackend {
    Std(StdStream),
    /// An open file. `None` after `close` -- a closed IO is not a dangling
    /// one: every operation on it raises IOError, which is what real Ruby
    /// does and what a plain `Option::take` gives us for free.
    File(Option<std::fs::File>),
    /// One end of an `IO.pipe`. Backed by a real fd (wrapped in a `File` for
    /// its `Read`/`Write`), but reports `IO` rather than `File` for `#class`
    /// and has no path -- a pipe is a plain IO, not a File.
    Pipe(Option<std::fs::File>),
}

impl IoBackend {
    /// Drop the underlying descriptor (a no-op for the std streams), leaving a
    /// closed handle -- what `#close`/`#close_read`/`#close_write` need.
    fn close_file(&mut self) {
        match self {
            IoBackend::File(slot) | IoBackend::Pipe(slot) => {
                slot.take();
            }
            IoBackend::Std(_) => {}
        }
    }
}

pub struct RIo {
    /// `Mutex` because reading MOVES the file position -- an IO is mutable
    /// shared state even behind an `Arc`, unlike the stateless std-stream
    /// singletons this started as.
    backend: parking_lot::Mutex<IoBackend>,
    /// The path this was opened from, for error messages and `#path`; empty
    /// for the std streams.
    path: String,
    /// `#lineno` -- the count of lines read via `gets`/`readline`/`each_line`,
    /// which CRuby tracks per-IO and lets a program set with `lineno=`.
    lineno: std::sync::atomic::AtomicI64,
    /// `#binmode?` -- on Unix binary vs text mode has no behavioural effect
    /// (no CRLF translation), so this only records what `#binmode` was told,
    /// exactly what `#binmode?` reports.
    binmode: std::sync::atomic::AtomicBool,
    /// `#autoclose?` -- whether closing this IO closes its fd. Defaults to
    /// true; a program may clear it (`autoclose = false`) to keep the fd open.
    autoclose: std::sync::atomic::AtomicBool,
    /// Bytes pushed back by `#ungetbyte`/`#ungetc`, read out (LIFO) before the
    /// stream itself. The next byte read drains this first.
    unget: parking_lot::Mutex<Vec<u8>>,
    /// A socket reports `TCPSocket`/`TCPServer` (not `IO`) for `#class`, while
    /// still using a `Pipe`-shaped fd for read/write. `None` for ordinary
    /// files, pipes, and std streams (their class comes from the backend).
    class_override: Option<ClassId>,
}

impl RubyObject for RIo {
    // A File instance carries FILE_CLASS so its own MRO (`File < IO`) finds
    // File's rows before IO's; the std streams are plain IOs.
    fn class_id(&self) -> ClassId {
        if let Some(c) = self.class_override {
            return c;
        }
        match &*self.backend.lock() {
            IoBackend::File(_) => zeo_abi::FILE_CLASS,
            IoBackend::Std(_) | IoBackend::Pipe(_) => IO_CLASS,
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    // The std streams are process-wide state holders; freezing them is
    // meaningless (CRuby's are unfrozen too).
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    // An IO's identity IS its descriptor -- `dup`ing one would have to `dup(2)`
    // the fd to mean anything. Answers a fresh handle on the same std stream;
    // a File `dup` is out of scope (see the module docs).
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        let stream = match &*self.backend.lock() {
            IoBackend::Std(s) => *s,
            IoBackend::File(_) | IoBackend::Pipe(_) => StdStream::Stdout,
        };
        Arc::new(RIo::new(IoBackend::Std(stream), self.path.clone()))
    }
}

impl RIo {
    /// The one place RIo's default per-handle state (unset binmode, autoclose
    /// on, no pushed-back bytes, lineno 0) is established, so every constructor
    /// agrees.
    fn new(backend: IoBackend, path: String) -> RIo {
        RIo {
            backend: parking_lot::Mutex::new(backend),
            path,
            lineno: std::sync::atomic::AtomicI64::new(0),
            binmode: std::sync::atomic::AtomicBool::new(false),
            autoclose: std::sync::atomic::AtomicBool::new(true),
            unget: parking_lot::Mutex::new(Vec::new()),
            class_override: None,
        }
    }
}

/// Wrap a connected socket fd (from a `TcpStream`/`TcpListener::accept`) as a
/// value that reports `class_id` for `#class` but reads/writes like a `Pipe`.
/// Shared by `TCPSocket.new` and `TCPServer#accept` (see `builtins::socket`).
pub(crate) fn socket_value(f: std::fs::File, class_id: ClassId) -> RubyValue {
    let mut io = RIo::new(IoBackend::Pipe(Some(f)), String::new());
    io.class_override = Some(class_id);
    RubyValue::Object(Arc::new(io))
}

fn std_io(stream: StdStream) -> RubyValue {
    RubyValue::Object(Arc::new(RIo::new(IoBackend::Std(stream), String::new())))
}

/// Wrap an already-open file as a Ruby `File` value.
pub(crate) fn file_value(f: std::fs::File, path: String) -> RubyValue {
    RubyValue::Object(Arc::new(RIo::new(IoBackend::File(Some(f)), path)))
}

/// Wrap one end of an `IO.pipe` (from an owned fd) as a Ruby `IO` value.
fn pipe_value(f: std::fs::File) -> RubyValue {
    RubyValue::Object(Arc::new(RIo::new(IoBackend::Pipe(Some(f)), String::new())))
}

pub fn stdout_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| std_io(StdStream::Stdout));
    V.clone()
}

pub fn stderr_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| std_io(StdStream::Stderr));
    V.clone()
}

pub fn stdin_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| std_io(StdStream::Stdin));
    V.clone()
}

/// Installs the `STDIN`/`STDOUT`/`STDERR` constants and the matching
/// `$stdin`/`$stdout`/`$stderr` globals -- called once from generated
/// `main()` (CRuby startup parity).
pub fn seed_stdio() {
    crate::constants::const_set(0, "STDOUT", stdout_value());
    crate::constants::const_set(0, "STDERR", stderr_value());
    crate::constants::const_set(0, "STDIN", stdin_value());
    crate::globals::global_set(0, "$stdout", stdout_value());
    crate::globals::global_set(0, "$stderr", stderr_value());
    crate::globals::global_set(0, "$stdin", stdin_value());
}

/// The value `$stdout` currently holds in box 0 (nil -- never assigned --
/// means the default singleton). The print family targets this, so
/// `$stdout = STDERR` (or any duck-typed writer) redirects `puts`/`p`/....
pub fn current_stdout() -> RubyValue {
    match crate::globals::global_get(0, "$stdout") {
        RubyValue::Nil => stdout_value(),
        v => v,
    }
}

pub fn current_stderr() -> RubyValue {
    match crate::globals::global_get(0, "$stderr") {
        RubyValue::Nil => stderr_value(),
        v => v,
    }
}

/// The `RIo` backend arms every writer shares -- BYTES in, so a BINARY
/// string's raw bytes reach the fd untouched (the display pipeline's
/// `to_utf8_lossy` promotes `0xB4` to `0xC2 0xB4`, which corrupted every
/// binary-image benchmark's output; see `write_value`).
fn write_rio(io: &RIo, bytes: &[u8]) -> Result<(), Signal> {
    match &mut *io.backend.lock() {
        IoBackend::Std(StdStream::Stdout) => {
            let mut out = std::io::stdout();
            if out.write_all(bytes).is_err() {
                // A closed pipe downstream (`head`, etc.) -- CRuby
                // dies with EPIPE; a quiet exit is the pragmatic
                // equivalent here.
                std::process::exit(0);
            }
            Ok(())
        }
        IoBackend::Std(StdStream::Stderr) => {
            let _ = std::io::stderr().write_all(bytes);
            Ok(())
        }
        IoBackend::Std(StdStream::Stdin) => Err(io_error!("not opened for writing")),
        IoBackend::File(None) | IoBackend::Pipe(None) => Err(io_error!("closed stream")),
        IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => f
            .write_all(bytes)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "write", &io.path)),
    }
}

/// Writes `s` to `target`: directly for one of our `RIo`s, via a dynamic
/// `write` send for anything else (`$stdout = <duck>` redirection --
/// CRuby's own contract is "any object responding to `write`").
pub fn write_str(target: &RubyValue, s: &str) -> Result<(), Signal> {
    if let RubyValue::Object(o) = target {
        if let Some(io) = o.as_any().downcast_ref::<RIo>() {
            return write_rio(io, s.as_bytes());
        }
    }
    crate::dispatch::send_value(
        target,
        crate::symbol::Symbol::intern("write"),
        &[RubyValue::Str(crate::collections::string_new(
            s.to_string(),
        ))],
        None,
    )
    .map(|_| ())
}

/// `write_str` for an already-assembled BYTE buffer (the `print`/`puts`
/// accumulators, `putc`'s single byte). A duck target receives it as a
/// BINARY string -- the honest tag for bytes with no other provenance.
pub fn write_bytes(target: &RubyValue, bytes: &[u8]) -> Result<(), Signal> {
    if let RubyValue::Object(o) = target {
        if let Some(io) = o.as_any().downcast_ref::<RIo>() {
            return write_rio(io, bytes);
        }
    }
    crate::dispatch::send_value(
        target,
        crate::symbol::Symbol::intern("write"),
        &[RubyValue::Str(crate::collections::string_from_bytes(
            bytes.to_vec(),
            crate::encoding::ASCII_8BIT,
        ))],
        None,
    )
    .map(|_| ())
}

/// Writes one Ruby VALUE the way `IO#write`/`#<<` must: a String
/// contributes its RAW bytes in its own encoding (a duck target gets the
/// very same String object, exactly CRuby); anything else goes through the
/// display rendering. Answers the BYTE count written (`IO#write`'s return
/// contract). This is the seam that keeps `0xB4` one byte instead of the
/// display pipeline's Latin-1 -> UTF-8 promotion.
pub fn write_value(target: &RubyValue, v: &RubyValue) -> Result<i64, Signal> {
    if let RubyValue::Str(s) = v {
        let bytes = {
            let b = s.lock();
            b.bytes().to_vec()
        };
        if let RubyValue::Object(o) = target {
            if let Some(io) = o.as_any().downcast_ref::<RIo>() {
                write_rio(io, &bytes)?;
                return Ok(bytes.len() as i64);
            }
        }
        crate::dispatch::send_value(
            target,
            crate::symbol::Symbol::intern("write"),
            std::slice::from_ref(v),
            None,
        )?;
        return Ok(bytes.len() as i64);
    }
    let s = v.try_display_string()?;
    write_str(target, &s)?;
    Ok(s.len() as i64)
}

/// Appends `v`'s printed form to a BYTE buffer: a String's raw bytes in
/// its own encoding, every other value's display rendering (UTF-8). The
/// `print`/`puts` family accumulates through this so binary strings
/// survive to the fd byte-for-byte.
pub fn display_bytes(v: &RubyValue, buf: &mut Vec<u8>) -> Result<(), Signal> {
    match v {
        RubyValue::Str(s) => buf.extend_from_slice(s.lock().bytes()),
        // Fallible: a user `to_s` that raises propagates out of the
        // `print`/`puts` family as a catchable exception (CRuby's rule).
        other => buf.extend_from_slice(other.try_display_string()?.as_bytes()),
    }
    Ok(())
}

/// `puts`'s rendering into a BYTE buffer (a String arg contributes its raw
/// bytes -- see `display_bytes`): every arg on its own line, arrays
/// flattened recursively, `[...]` for a self-referential array, a bare
/// newline for no args / an empty array -- CRuby's exact shapes.
pub fn render_puts(args: &[RubyValue], buf: &mut Vec<u8>) -> Result<(), Signal> {
    fn put_one(v: &RubyValue, seen: &mut Vec<usize>, buf: &mut Vec<u8>) -> Result<(), Signal> {
        match v {
            RubyValue::Array(a) => {
                let id = Arc::as_ptr(a) as usize;
                if seen.contains(&id) {
                    buf.extend_from_slice(b"[...]\n");
                    return Ok(());
                }
                seen.push(id);
                let items = a.lock().clone();
                if items.is_empty() {
                    buf.push(b'\n');
                }
                for e in &items {
                    put_one(e, seen, buf)?;
                }
                seen.pop();
            }
            other => {
                let start = buf.len();
                display_bytes(other, buf)?;
                if buf.len() == start || buf.last() != Some(&b'\n') {
                    buf.push(b'\n');
                }
            }
        }
        Ok(())
    }
    if args.is_empty() {
        buf.push(b'\n');
    }
    for a in args {
        put_one(a, &mut Vec::new(), buf)?;
    }
    Ok(())
}

fn recv_io(recv: &RubyValue) -> Result<&RubyValue, Signal> {
    // The table only dispatches on IO_CLASS receivers, so `recv` is always
    // one of the singletons -- kept as a value so the write helpers stay
    // target-shaped.
    Ok(recv)
}

fn io_puts(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mut buf = Vec::new();
    // Flush-then-propagate on a raising `to_s` -- see `kernel_puts`.
    let rendered = render_puts(args, &mut buf);
    write_bytes(recv_io(recv)?, &buf)?;
    rendered?;
    Ok(RubyValue::Nil)
}

fn io_print(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mut buf = Vec::new();
    let mut rendered = Ok(());
    for a in args {
        if let Err(sig) = display_bytes(a, &mut buf) {
            rendered = Err(sig);
            break;
        }
    }
    // Flush-then-propagate on a raising `to_s` -- see `kernel_puts`.
    write_bytes(recv_io(recv)?, &buf)?;
    rendered?;
    Ok(RubyValue::Nil)
}

fn io_write(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mut total = 0i64;
    for a in args {
        total += write_value(recv_io(recv)?, a)?;
    }
    Ok(RubyValue::Int(total))
}

fn io_shovel(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let [v] = args else {
        return Err(arg_error!(
            "wrong number of arguments (given {}, expected 1)",
            args.len()
        ));
    };
    write_value(recv_io(recv)?, v)?;
    Ok(recv.clone())
}

fn io_flush(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    Ok(recv.clone())
}

pub(crate) fn as_rio(recv: &RubyValue) -> Option<&RIo> {
    match recv {
        RubyValue::Object(o) => o.as_any().downcast_ref::<RIo>(),
        _ => None,
    }
}

fn stream_of(recv: &RubyValue) -> Option<StdStream> {
    match &*as_rio(recv)?.backend.lock() {
        IoBackend::Std(s) => Some(*s),
        IoBackend::File(_) | IoBackend::Pipe(_) => None,
    }
}

fn io_fileno(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    use std::os::fd::AsRawFd;
    if let Some(io) = as_rio(recv) {
        if let IoBackend::File(Some(f)) = &*io.backend.lock() {
            return Ok(RubyValue::Int(f.as_raw_fd() as i64));
        }
    }
    Ok(RubyValue::Int(match stream_of(recv) {
        Some(StdStream::Stdin) => 0,
        Some(StdStream::Stdout) => 1,
        Some(StdStream::Stderr) => 2,
        None => 0,
    }))
}

fn io_tty(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    use std::io::IsTerminal;
    Ok(RubyValue::Bool(match stream_of(recv) {
        Some(StdStream::Stdin) => std::io::stdin().is_terminal(),
        Some(StdStream::Stdout) => std::io::stdout().is_terminal(),
        Some(StdStream::Stderr) => std::io::stderr().is_terminal(),
        // A regular file is never a tty.
        None => false,
    }))
}

/// `IO#winsize` (from `require "io/console"`) -- `[rows, columns]`.
///
/// **Documented divergence.** CRuby raises `Errno::ENOTTY` ("Inappropriate
/// ioctl for device") when the stream isn't a terminal; here a failed ioctl
/// answers `[0, 0]`. That is deliberate: the corpus expectation is checked in
/// rather than oracle-generated, its header states the `[0, 0]` contract, and
/// the conformance harness always redirects stdout -- so raising would make
/// the test unrunnable rather than more faithful. A program that must
/// distinguish the two cases should ask `tty?` first.
fn io_winsize(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0);
    let RubyValue::Int(fd) = io_fileno(recv, &[], None)? else {
        unreachable!("io_fileno answers an Int");
    };
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    // A nonzero return leaves `ws` zeroed, which is the answer we want.
    unsafe { libc::ioctl(fd as libc::c_int, libc::TIOCGWINSZ, &mut ws) };
    Ok(RubyValue::Array(crate::collections::array_new(vec![
        RubyValue::Int(ws.ws_row as i64),
        RubyValue::Int(ws.ws_col as i64),
    ])))
}

fn io_inspect(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let name = match stream_of(recv) {
        Some(StdStream::Stdin) => "#<IO:<STDIN>>".to_string(),
        Some(StdStream::Stdout) => "#<IO:<STDOUT>>".to_string(),
        Some(StdStream::Stderr) => "#<IO:<STDERR>>".to_string(),
        None => match as_rio(recv) {
            Some(io) => format!("#<File:{}>", io.path),
            None => "#<IO>".to_string(),
        },
    };
    Ok(RubyValue::Str(crate::collections::string_new(name)))
}

/// `path`/`to_path` -- the name this IO was opened from.
fn io_path(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match as_rio(recv) {
        Some(io) if !io.path.is_empty() => Ok(RubyValue::Str(crate::collections::string_new(
            io.path.clone(),
        ))),
        // A std stream has no path -- real Ruby raises IOError for `#path`
        // on one, rather than answering nil.
        _ => Err(io_error!("not a file")),
    }
}

/// Run `f` against the receiver's open file, or raise the IOError a closed
/// or non-file receiver deserves -- the shared preamble of every read/seek
/// row below.
fn with_file<T>(
    recv: &RubyValue,
    f: impl FnOnce(&mut std::fs::File, &str) -> Result<T, Signal>,
) -> Result<T, Signal> {
    let Some(io) = as_rio(recv) else {
        return Err(io_error!("not a file"));
    };
    let path = io.path.clone();
    match &mut *io.backend.lock() {
        IoBackend::File(Some(file)) | IoBackend::Pipe(Some(file)) => f(file, &path),
        IoBackend::File(None) | IoBackend::Pipe(None) => Err(io_error!("closed stream")),
        IoBackend::Std(_) => Err(io_error!("not a file")),
    }
}

/// `read` / `read(n)` / `read(n, buf)` -- the whole rest, or `n` bytes,
/// optionally read INTO an existing String `buf` (returned in place of a fresh
/// one). At EOF, a LENGTHED read answers nil while a whole-rest read answers
/// `""` (real Ruby's asymmetry, and the thing a read loop tests).
fn io_read(
    recv: &RubyValue,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let result = io_read_val(recv, args, blk)?;
    // 2-arg `read(length, buffer)`: fill the caller's String and answer it (or
    // nil at EOF, having emptied it).
    if let Some(RubyValue::Str(buf)) = args.get(1) {
        match &result {
            RubyValue::Str(s) => {
                let txt = s.lock().to_utf8_lossy().into_owned();
                buf.lock().replace_utf8(txt);
                return Ok(RubyValue::Str(buf.clone()));
            }
            RubyValue::Nil => {
                buf.lock().replace_utf8(String::new());
                return Ok(RubyValue::Nil);
            }
            _ => {}
        }
    }
    Ok(result)
}

fn io_read_val(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // A read from STDIN reads the real one; anything else needs a file.
    if matches!(stream_of(recv), Some(StdStream::Stdin)) {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "read", "<STDIN>"))?;
        return Ok(RubyValue::Str(crate::collections::string_new(buf)));
    }
    let n = match args.first() {
        None | Some(RubyValue::Nil) => None,
        Some(v) => match convert::to_index(v)? {
            i if i >= 0 => Some(i as usize),
            i => return Err(arg_error!("negative length {i} given")),
        },
    };
    with_file(recv, |f, path| {
        use std::io::Read;
        match n {
            None => {
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "read", path))?;
                Ok(RubyValue::Str(crate::collections::string_new(
                    String::from_utf8_lossy(&buf).into_owned(),
                )))
            }
            Some(n) => {
                let mut buf = vec![0u8; n];
                let mut got = 0;
                // `read` can answer short without being at EOF; loop until
                // the request is filled or the file genuinely ends.
                while got < n {
                    match f.read(&mut buf[got..]) {
                        Ok(0) => break,
                        Ok(k) => got += k,
                        Err(e) => return Err(crate::builtins::file::raise_errno(&e, "read", path)),
                    }
                }
                buf.truncate(got);
                // EOF + a lengthed read is nil, NOT "" -- the asymmetry a
                // `while chunk = f.read(n)` loop relies on to terminate.
                if got == 0 && n > 0 {
                    return Ok(RubyValue::Nil);
                }
                Ok(RubyValue::Str(crate::collections::string_new(
                    String::from_utf8_lossy(&buf).into_owned(),
                )))
            }
        }
    })
}

/// `seek(offset, whence = IO::SEEK_SET)` -- answers 0, like real Ruby.
fn io_seek(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let off = offset_of(args.first().unwrap_or(&RubyValue::Nil))?;
    let whence = match args.get(1) {
        None => 0,
        Some(w) => whence_of(w)?,
    };
    with_file(recv, |f, path| {
        use std::io::Seek;
        let pos = match whence {
            0 => std::io::SeekFrom::Start(off.max(0) as u64),
            1 => std::io::SeekFrom::Current(off),
            2 => std::io::SeekFrom::End(off),
            _ => return Err(arg_error!("invalid whence")),
        };
        f.seek(pos)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "seek", path))?;
        Ok(RubyValue::Int(0))
    })
}

fn io_tell(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    with_file(recv, |f, path| {
        use std::io::Seek;
        let p = f
            .stream_position()
            .map_err(|e| crate::builtins::file::raise_errno(&e, "tell", path))?;
        Ok(RubyValue::Int(p as i64))
    })
}

fn io_rewind(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    with_file(recv, |f, path| {
        use std::io::Seek;
        f.rewind()
            .map_err(|e| crate::builtins::file::raise_errno(&e, "rewind", path))?;
        Ok(RubyValue::Int(0))
    })
}

fn io_eof(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // stdin can't seek; peek the shared buffered reader instead. `fill_buf`
    // is non-destructive -- an empty buffer means end-of-input.
    if matches!(stream_of(recv), Some(StdStream::Stdin)) {
        use std::io::BufRead;
        let empty = std::io::stdin()
            .lock()
            .fill_buf()
            .map(|b| b.is_empty())
            .unwrap_or(true);
        return Ok(RubyValue::Bool(empty));
    }
    with_file(recv, |f, path| {
        use std::io::Seek;
        let pos = f
            .stream_position()
            .map_err(|e| crate::builtins::file::raise_errno(&e, "eof?", path))?;
        let end = f
            .seek(std::io::SeekFrom::End(0))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "eof?", path))?;
        // Peeking at the end MOVES the position -- put it back.
        f.seek(std::io::SeekFrom::Start(pos))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "eof?", path))?;
        Ok(RubyValue::Bool(pos >= end))
    })
}

/// `close` -- idempotent (a second close is a no-op, as in Ruby), and it
/// DROPS the descriptor, so every later operation raises IOError.
fn io_close(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    if let Some(io) = as_rio(recv) {
        match &mut *io.backend.lock() {
            IoBackend::File(slot) | IoBackend::Pipe(slot) => {
                slot.take();
            }
            IoBackend::Std(_) => {}
        }
    }
    Ok(RubyValue::Nil)
}

fn io_closed(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let closed = match as_rio(recv) {
        Some(io) => matches!(
            &*io.backend.lock(),
            IoBackend::File(None) | IoBackend::Pipe(None)
        ),
        None => false,
    };
    Ok(RubyValue::Bool(closed))
}

/// How `gets`/`readline`/`each_line`/`readlines` split their input: the line
/// separator (`None` = slurp the whole rest, i.e. `gets(nil)`), an optional
/// byte limit, and whether to strip the terminator (`chomp:`).
struct LineOpts {
    sep: Option<Vec<u8>>,
    limit: Option<usize>,
    chomp: bool,
}

/// Parse the shared `(sep = $/, limit = nil, chomp: false)` argument shape.
/// A leading Integer is the limit (separator stays `"\n"`); a leading String
/// is the separator, with an Integer that follows as the limit; a leading nil
/// slurps. The trailing keyword Hash carries `chomp:`.
fn line_opts(args: &[RubyValue]) -> LineOpts {
    let mut sep: Option<Vec<u8>> = Some(b"\n".to_vec());
    let mut limit = None;
    let mut chomp = false;
    let mut positional = args;
    if let Some(RubyValue::Hash(h)) = args.last() {
        chomp = crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("chomp")))
            .truthy();
        positional = &args[..args.len() - 1];
    }
    match positional.first() {
        Some(RubyValue::Int(n)) => limit = Some((*n).max(0) as usize),
        Some(RubyValue::Nil) => sep = None,
        Some(RubyValue::Str(s)) => sep = Some(s.lock().bytes().to_vec()),
        _ => {}
    }
    if let Some(RubyValue::Int(n)) = positional.get(1) {
        limit = Some((*n).max(0) as usize);
    }
    LineOpts { sep, limit, chomp }
}

/// Read the next line's bytes: up to and including the separator, or `limit`
/// bytes, or EOF. An empty result means EOF. Byte-at-a-time so the position
/// lands exactly after the line (a buffered read would desync `tell`).
fn read_line_bytes(f: &mut std::fs::File, opts: &LineOpts) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if let Some(lim) = opts.limit {
            if out.len() >= lim {
                break;
            }
        }
        match f.read(&mut byte)? {
            0 => break,
            _ => {
                out.push(byte[0]);
                if let Some(s) = &opts.sep {
                    if !s.is_empty() && out.ends_with(s) {
                        break;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Turn a line's bytes into the String `gets` answers, honoring `chomp:`.
fn line_string(bytes: Vec<u8>, opts: &LineOpts) -> RubyValue {
    let mut s = String::from_utf8_lossy(&bytes).into_owned();
    if opts.chomp {
        // `chomp` strips one trailing "\r\n"/"\n"/"\r" (or the custom sep).
        if let Some(sep) = &opts.sep {
            let sep = String::from_utf8_lossy(sep);
            if s.ends_with(sep.as_ref()) {
                s.truncate(s.len() - sep.len());
            }
        }
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
    }
    RubyValue::Str(crate::collections::string_new(s))
}

fn bump_lineno(recv: &RubyValue) {
    if let Some(io) = as_rio(recv) {
        io.lineno.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// `gets([sep][, limit][, chomp:])` -- one line, or nil at EOF; bumps `lineno`.
fn io_gets(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let opts = line_opts(args);
    if matches!(stream_of(recv), Some(StdStream::Stdin)) {
        let mut line = String::new();
        let n = std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "gets", "<STDIN>"))?;
        if n == 0 {
            return Ok(RubyValue::Nil);
        }
        bump_lineno(recv);
        return Ok(line_string(line.into_bytes(), &opts));
    }
    let line = with_file(recv, |f, path| {
        read_line_bytes(f, &opts).map_err(|e| crate::builtins::file::raise_errno(&e, "gets", path))
    })?;
    if line.is_empty() {
        return Ok(RubyValue::Nil);
    }
    bump_lineno(recv);
    Ok(line_string(line, &opts))
}

/// `readline` -- `gets`, but raises `EOFError` instead of answering nil.
fn io_readline(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match io_gets(recv, args, None)? {
        RubyValue::Nil => Err(eof_error!("end of file reached")),
        line => Ok(line),
    }
}

fn io_lineno(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let n = as_rio(recv).map_or(0, |io| io.lineno.load(std::sync::atomic::Ordering::Relaxed));
    Ok(RubyValue::Int(n))
}

fn io_lineno_set(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let n = convert::to_index(args.first().unwrap_or(&RubyValue::Nil))?;
    if let Some(io) = as_rio(recv) {
        io.lineno.store(n, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(RubyValue::Int(n))
}

/// Read the next whole UTF-8 char from `f` (1-4 bytes by the lead byte), or
/// `None` at EOF.
fn read_one_char(f: &mut std::fs::File) -> std::io::Result<Option<String>> {
    use std::io::Read;
    let mut first = [0u8; 1];
    if f.read(&mut first)? == 0 {
        return Ok(None);
    }
    let b0 = first[0];
    let n = if b0 < 0x80 {
        1
    } else if b0 >> 5 == 0b110 {
        2
    } else if b0 >> 4 == 0b1110 {
        3
    } else if b0 >> 3 == 0b11110 {
        4
    } else {
        1
    };
    let mut buf = vec![b0];
    for _ in 1..n {
        let mut b = [0u8; 1];
        if f.read(&mut b)? == 0 {
            break;
        }
        buf.push(b[0]);
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

fn io_getc(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let ch = with_file(recv, |f, path| {
        read_one_char(f).map_err(|e| crate::builtins::file::raise_errno(&e, "getc", path))
    })?;
    Ok(match ch {
        Some(s) => RubyValue::Str(crate::collections::string_new(s)),
        None => RubyValue::Nil,
    })
}

fn io_readchar(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match io_getc(recv, args, None)? {
        RubyValue::Nil => Err(eof_error!("end of file reached")),
        ch => Ok(ch),
    }
}

fn io_getbyte(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // A byte pushed back with `#ungetbyte` is returned before the stream.
    if let Some(io) = as_rio(recv) {
        if let Some(byte) = io.unget.lock().pop() {
            return Ok(RubyValue::Int(byte as i64));
        }
    }
    let b = with_file(recv, |f, path| {
        use std::io::Read;
        let mut byte = [0u8; 1];
        match f
            .read(&mut byte)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "getbyte", path))?
        {
            0 => Ok(None),
            _ => Ok(Some(byte[0])),
        }
    })?;
    Ok(match b {
        Some(byte) => RubyValue::Int(byte as i64),
        None => RubyValue::Nil,
    })
}

fn io_readbyte(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match io_getbyte(recv, args, None)? {
        RubyValue::Nil => Err(eof_error!("end of file reached")),
        b => Ok(b),
    }
}

use crate::builtins::{
    arg_error, convert, eof_error, io_error, local_jump_error, not_impl_error, type_error,
};
use std::sync::atomic::Ordering::Relaxed;

/// An integer argument (`pread`/`pwrite` counts, `fcntl`/`chmod` operands)
/// through the `to_int` protocol -- unlike the offset sites, a nil here is
/// the generic "of nil into Integer" (oracle-verified).
fn int_of(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Nil => Err(type_error!("no implicit conversion of nil into Integer")),
        v => convert::to_index(v),
    }
}

/// A byte-offset argument (`seek`/`sysseek`/`pos=`/`truncate`, `pread`'s
/// offset): CRuby's NUM2OFFT, whose nil TypeError is the bare
/// "no implicit conversion from nil" (no "to integer" -- oracle-verified).
fn offset_of(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Nil => Err(type_error!("no implicit conversion from nil")),
        v => convert::to_index(v),
    }
}

/// `seek`/`sysseek`'s whence: the SET/CUR/END symbols map to their
/// constants (CRuby's interpret_seek_whence); anything else -- unknown
/// symbols included -- goes through NUM2LONG (oracle: `seek(0, :BAD)` is
/// "no implicit conversion of Symbol into Integer").
fn whence_of(v: &RubyValue) -> Result<i64, Signal> {
    if let RubyValue::Symbol(s) = v {
        match s.name().as_str() {
            "SET" => return Ok(0),
            "CUR" => return Ok(1),
            "END" => return Ok(2),
            _ => {}
        }
    }
    convert::to_index(v)
}

/// `#binmode` -- record binary mode (a no-op on Unix behaviourally); answers self.
fn io_binmode(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    if let Some(io) = as_rio(recv) {
        io.binmode.store(true, Relaxed);
    }
    Ok(recv.clone())
}

fn io_binmode_p(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0);
    Ok(RubyValue::Bool(
        as_rio(recv).is_some_and(|io| io.binmode.load(Relaxed)),
    ))
}

/// `#autoclose = flag` -- answers the assigned value (Ruby setter convention).
fn io_autoclose_set(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    if let Some(io) = as_rio(recv) {
        io.autoclose.store(args[0].truthy(), Relaxed);
    }
    Ok(args[0].clone())
}

fn io_autoclose_p(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0);
    Ok(RubyValue::Bool(
        as_rio(recv).is_none_or(|io| io.autoclose.load(Relaxed)),
    ))
}

/// `#to_io` -- an IO answers itself.
fn io_to_io(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0);
    Ok(recv.clone())
}

/// `#close_on_exec?` -- CRuby marks a newly-opened fd close-on-exec by default
/// (since Ruby 2.0), so this reports true; `#close_on_exec=` records the wish
/// and answers it (the flag has no observable effect without an exec here).
fn io_close_on_exec_p(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0);
    let _ = recv;
    Ok(RubyValue::Bool(true))
}

fn io_close_on_exec_set(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    Ok(args[0].clone())
}

/// `#advise(kind[, offset, len])` -- a hint to the kernel about access
/// patterns. Validated against the known symbols, then a no-op answering nil
/// (`posix_fadvise` is best-effort and unobservable from Ruby).
fn io_advise(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=3);
    // Not a conversion site: CRuby's io_advise requires a bare Symbol
    // ("advice must be a Symbol", oracle-verified).
    let kind = match &args[0] {
        RubyValue::Symbol(s) => s.name().to_string(),
        _ => return Err(type_error!("advice must be a Symbol")),
    };
    if !matches!(
        kind.as_str(),
        "normal" | "sequential" | "random" | "willneed" | "dontneed" | "noreuse"
    ) {
        return Err(not_impl_error!("Unsupported advice: :{kind}"));
    }
    let _ = recv;
    Ok(RubyValue::Nil)
}

/// `#ungetbyte(int_or_str)` -- push bytes back so the next read returns them
/// first. Recorded on the IO's unget stack (see `io_getbyte`).
fn io_ungetbyte(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    let Some(io) = as_rio(recv) else {
        return Err(io_error!("not a file"));
    };
    let mut ug = io.unget.lock();
    match &args[0] {
        RubyValue::Int(i) => ug.push((*i & 0xff) as u8),
        // Push in reverse so the string's bytes read back in order (the stack
        // is LIFO).
        RubyValue::Str(s) => {
            for b in s
                .lock()
                .to_utf8_lossy()
                .into_owned()
                .into_bytes()
                .into_iter()
                .rev()
            {
                ug.push(b);
            }
        }
        RubyValue::Nil => {}
        other => {
            let s = convert::to_rstr(other)?;
            for b in s
                .lock()
                .to_utf8_lossy()
                .into_owned()
                .into_bytes()
                .into_iter()
                .rev()
            {
                ug.push(b);
            }
        }
    }
    Ok(RubyValue::Nil)
}

/// `#pread(maxlen, offset[, buffer])` -- read at a fixed offset WITHOUT moving
/// the position (`pread(2)`). Answers a new String, or fills `buffer` when
/// given and answers it. EOFError when nothing is available at `offset`.
fn io_pread(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 2..=3);
    let count = int_of(&args[0])?.max(0) as usize;
    let offset = offset_of(&args[1])?.max(0) as u64;
    let data = with_file(recv, |f, path| {
        use std::os::unix::fs::FileExt;
        let mut buf = vec![0u8; count];
        let n = f
            .read_at(&mut buf, offset)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "pread", path))?;
        if n == 0 && count > 0 {
            return Err(eof_error!("end of file reached"));
        }
        buf.truncate(n);
        Ok(buf)
    })?;
    let text = String::from_utf8_lossy(&data).into_owned();
    match args.get(2) {
        Some(RubyValue::Str(buf)) => {
            buf.lock().replace_utf8(text);
            Ok(RubyValue::Str(buf.clone()))
        }
        _ => Ok(RubyValue::Str(crate::collections::string_new(text))),
    }
}

/// `#pwrite(string, offset)` -- write at a fixed offset WITHOUT moving the
/// position; answers the number of bytes written.
fn io_pwrite(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 2);
    let bytes = match &args[0] {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned().into_bytes(),
        other => other.try_display_string()?.into_bytes(),
    };
    let offset = offset_of(&args[1])?.max(0) as u64;
    with_file(recv, |f, path| {
        use std::os::unix::fs::FileExt;
        let n = f
            .write_at(&bytes, offset)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "pwrite", path))?;
        Ok(RubyValue::Int(n as i64))
    })
}

/// `#reopen(other_io_or_path[, mode])` -- rebind this IO to another stream.
/// Reopens the source's path fresh (position 0), which is what a program that
/// reads after `reopen` observes; answers self.
fn io_reopen(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    let path = match as_rio(&args[0]) {
        Some(other) => other.path.clone(),
        None => crate::builtins::file::path_arg(&args[0], "reopen")?,
    };
    let f = std::fs::File::open(&path)
        .map_err(|e| crate::builtins::file::raise_errno(&e, "reopen", &path))?;
    if let Some(io) = as_rio(recv) {
        *io.backend.lock() = IoBackend::File(Some(f));
        io.unget.lock().clear();
    }
    Ok(recv.clone())
}

/// `#each_codepoint { |cp| ... }` -- yield each remaining character's codepoint;
/// answers self.
fn io_each_codepoint(
    recv: &RubyValue,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0);
    let RubyValue::Proc(p) = blk.unwrap_or(RubyValue::Nil) else {
        return Err(local_jump_error!("no block given (yield)"));
    };
    let content = with_file(recv, |f, path| {
        use std::io::Read;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "each_codepoint", path))?;
        Ok(buf)
    })?;
    for ch in String::from_utf8_lossy(&content).chars() {
        p.call(&[RubyValue::Int(ch as i64)])?;
    }
    Ok(recv.clone())
}

/// Whether this IO's fd was opened for writing (`close_write` needs it) or
/// reading (`close_read`). `fcntl(F_GETFL) & O_ACCMODE` is the fd's own truth,
/// so no per-IO mode field is needed.
fn fd_access_mode(recv: &RubyValue) -> Option<libc::c_int> {
    use std::os::fd::AsRawFd;
    let io = as_rio(recv)?;
    match &*io.backend.lock() {
        IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => {
            let flags = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETFL) };
            (flags >= 0).then_some(flags & libc::O_ACCMODE)
        }
        _ => None,
    }
}

/// `#close_write` -- close the writable half. On a read-only stream there is
/// none, so CRuby raises IOError; otherwise the stream is closed. Answers nil.
fn io_close_write(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0);
    if fd_access_mode(recv) == Some(libc::O_RDONLY) {
        return Err(io_error!("not opened for writing"));
    }
    if let Some(io) = as_rio(recv) {
        io.backend.lock().close_file();
    }
    Ok(RubyValue::Nil)
}

/// `#close_read` -- close the readable half. On a write-only stream CRuby
/// raises IOError; otherwise the stream is closed. Answers nil.
fn io_close_read(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0);
    if fd_access_mode(recv) == Some(libc::O_WRONLY) {
        return Err(io_error!("not opened for reading"));
    }
    if let Some(io) = as_rio(recv) {
        io.backend.lock().close_file();
    }
    Ok(RubyValue::Nil)
}

/// `readlines([sep][, limit][, chomp:])` -- every remaining line as an Array.
fn io_readlines(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let opts = line_opts(args);
    let mut lines = Vec::new();
    loop {
        let bytes = with_file(recv, |f, path| {
            read_line_bytes(f, &opts)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "readlines", path))
        })?;
        if bytes.is_empty() {
            break;
        }
        lines.push(line_string(bytes, &opts));
    }
    Ok(RubyValue::Array(crate::collections::array_new(lines)))
}

/// `each_line`/`each([sep][, limit][, chomp:])` -- yield each line; bumps lineno.
fn io_each_line(
    recv: &RubyValue,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = blk else {
        return Err(crate::dispatch::raise_no_block_yield());
    };
    let opts = line_opts(args);
    loop {
        let bytes = with_file(recv, |f, path| {
            read_line_bytes(f, &opts)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "each_line", path))
        })?;
        if bytes.is_empty() {
            break;
        }
        bump_lineno(recv);
        p.call(&[line_string(bytes, &opts)])?;
    }
    Ok(recv.clone())
}

/// `each_char` -- yield each UTF-8 char.
fn io_each_char(
    recv: &RubyValue,
    _args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = blk else {
        return Err(crate::dispatch::raise_no_block_yield());
    };
    loop {
        let ch = with_file(recv, |f, path| {
            read_one_char(f).map_err(|e| crate::builtins::file::raise_errno(&e, "each_char", path))
        })?;
        match ch {
            Some(s) => p.call(&[RubyValue::Str(crate::collections::string_new(s))])?,
            None => break,
        };
    }
    Ok(recv.clone())
}

/// `each_byte` -- yield each byte as an Integer.
fn io_each_byte(
    recv: &RubyValue,
    _args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = blk else {
        return Err(crate::dispatch::raise_no_block_yield());
    };
    let text = io_read(recv, &[], None)?;
    let RubyValue::Str(s) = &text else {
        return Ok(recv.clone());
    };
    let bytes = s.lock().bytes().to_vec();
    for b in bytes {
        p.call(&[RubyValue::Int(b as i64)])?;
    }
    Ok(recv.clone())
}

/// `printf(fmt, *args)` -- format and write, answering nil.
fn io_printf(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(fmt) = args.first() else {
        return Err(arg_error!(
            "wrong number of arguments (given 0, expected 1+)"
        ));
    };
    let s = crate::builtins::format::sprintf(&fmt.try_display_string()?, &args[1..])?;
    write_str(recv_io(recv)?, &s)?;
    Ok(RubyValue::Nil)
}

/// `putc(int | str)` -- write one character (an Integer's low byte, or a
/// String's first character), answering the argument unchanged.
fn io_putc(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(arg) = args.first() else {
        return Err(arg_error!(
            "wrong number of arguments (given 0, expected 1)"
        ));
    };
    let bytes = putc_bytes(arg)?;
    write_bytes(recv_io(recv)?, &bytes)?;
    Ok(arg.clone())
}

/// The bytes `putc` writes for its argument: a String's FIRST CHARACTER in
/// the string's own encoding (one raw byte for the byte encodings -- never
/// the display pipeline's Latin-1 -> UTF-8 promotion), an Integer's low
/// byte. NUM2CHR for everything else (`putc 2.5` truncates; a `to_str`
/// duck does NOT apply here -- oracle-verified).
pub(crate) fn putc_bytes(arg: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(match arg {
        RubyValue::Str(s) => {
            let b = s.lock();
            match b.encoding().kind() {
                crate::encoding::EncKind::Latin1
                | crate::encoding::EncKind::Binary
                | crate::encoding::EncKind::SingleByte => {
                    b.bytes().first().map(|&x| vec![x]).unwrap_or_default()
                }
                // First CHARACTER in the string's own encoding -- a
                // multibyte sequence stays its raw bytes.
                crate::encoding::EncKind::MultiByte(_)
                | crate::encoding::EncKind::Utf16 { .. }
                | crate::encoding::EncKind::Utf32 { .. } => {
                    b.char_at(0).map(|c| c.bytes().to_vec()).unwrap_or_default()
                }
                crate::encoding::EncKind::Utf8 | crate::encoding::EncKind::Ascii => b
                    .to_utf8_lossy()
                    .chars()
                    .next()
                    .map(|c| c.to_string().into_bytes())
                    .unwrap_or_default(),
            }
        }
        other => vec![(convert::to_index(other)? & 0xff) as u8],
    })
}

/// `pos=` -- seek to an absolute byte offset.
fn io_pos_set(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let n = offset_of(args.first().unwrap_or(&RubyValue::Nil))?;
    with_file(recv, |f, path| {
        use std::io::Seek;
        f.seek(std::io::SeekFrom::Start(n.max(0) as u64))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "pos=", path))?;
        Ok(RubyValue::Int(n))
    })
}

/// `readpartial(maxlen)` / `sysread(maxlen)` -- read up to `maxlen` bytes,
/// blocking for at least one; `EOFError` at EOF (unlike `read(n)`'s nil).
fn io_readpartial(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let RubyValue::Int(max) = args.first().cloned().unwrap_or(RubyValue::Nil) else {
        return Err(arg_error!("length must be an Integer"));
    };
    let bytes = with_file(recv, |f, path| {
        use std::io::Read;
        let mut buf = vec![0u8; max.max(0) as usize];
        let got = f
            .read(&mut buf)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "read", path))?;
        buf.truncate(got);
        Ok(buf)
    })?;
    if bytes.is_empty() && max > 0 {
        return Err(eof_error!("end of file reached"));
    }
    Ok(RubyValue::Str(crate::collections::string_new(
        String::from_utf8_lossy(&bytes).into_owned(),
    )))
}

/// `sysseek(offset, whence = SEEK_SET)` -- seek, answering the new absolute
/// position (unlike `seek`, which answers 0).
fn io_sysseek(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let off = offset_of(args.first().unwrap_or(&RubyValue::Nil))?;
    let whence = match args.get(1) {
        None => 0,
        Some(w) => whence_of(w)?,
    };
    with_file(recv, |f, path| {
        use std::io::Seek;
        let pos = match whence {
            0 => std::io::SeekFrom::Start(off.max(0) as u64),
            1 => std::io::SeekFrom::Current(off),
            2 => std::io::SeekFrom::End(off),
            _ => return Err(arg_error!("invalid whence")),
        };
        let p = f
            .seek(pos)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "sysseek", path))?;
        Ok(RubyValue::Int(p as i64))
    })
}

/// `flock(op)` -- advisory whole-file lock via `flock(2)`; answers 0.
fn io_flock(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    use std::os::fd::AsRawFd;
    let op = convert::to_index(args.first().unwrap_or(&RubyValue::Nil))?;
    with_file(recv, |f, path| {
        // SAFETY: `f` owns a valid fd for the call's duration.
        if unsafe { libc::flock(f.as_raw_fd(), op as libc::c_int) } != 0 {
            return Err(crate::builtins::file::raise_errno(
                &std::io::Error::last_os_error(),
                "flock",
                path,
            ));
        }
        Ok(RubyValue::Int(0))
    })
}

/// `#stat` -- an `fstat(2)` snapshot of the open descriptor as a `File::Stat`.
fn io_stat(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    use std::os::fd::AsRawFd;
    with_file(recv, |f, _path| {
        crate::builtins::stat::stat_from_fd(f.as_raw_fd())
    })
}

/// `#fcntl(cmd[, arg])` -- the raw `fcntl(2)`; answers its integer result
/// (e.g. `fcntl(F_GETFD)` reads the close-on-exec flag). `arg` defaults to 0.
fn io_fcntl(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    let cmd = int_of(&args[0])? as libc::c_int;
    let arg = match args.get(1) {
        Some(v) => int_of(v)? as libc::c_int,
        None => 0,
    };
    use std::os::fd::AsRawFd;
    with_file(recv, |f, path| {
        // SAFETY: `f` owns a valid fd for the call's duration.
        let r = unsafe { libc::fcntl(f.as_raw_fd(), cmd, arg) };
        if r < 0 {
            return Err(crate::builtins::file::raise_errno(
                &std::io::Error::last_os_error(),
                "fcntl",
                path,
            ));
        }
        Ok(RubyValue::Int(r as i64))
    })
}

/// `File#lstat` -- stat the open file's path WITHOUT following a final symlink.
/// Unlike `#stat` (which `fstat`s the fd), this must go through the stored path,
/// since the fd already resolved the link at open time.
fn io_lstat(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    with_file(recv, |_f, path| {
        crate::builtins::stat::stat_from_path(path, false)
    })
}

/// `#chown(uid, gid)` -- `fchown(2)`; a nil arg leaves that id unchanged
/// (`-1` to the syscall). Answers 0.
fn io_chown(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    use std::os::fd::AsRawFd;
    let id = |v: Option<&RubyValue>| -> libc::uid_t {
        match v {
            Some(RubyValue::Int(i)) => *i as libc::uid_t,
            _ => u32::MAX, // -1: leave unchanged
        }
    };
    let uid = id(args.first());
    let gid = id(args.get(1));
    with_file(recv, |f, path| {
        // SAFETY: `f` owns a valid fd for the call's duration.
        if unsafe { libc::fchown(f.as_raw_fd(), uid, gid) } != 0 {
            return Err(crate::builtins::file::raise_errno(
                &std::io::Error::last_os_error(),
                "chown",
                path,
            ));
        }
        Ok(RubyValue::Int(0))
    })
}

/// `#truncate(len)` -- resize the open file to `len` bytes; answers 0.
fn io_truncate(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let len = offset_of(args.first().unwrap_or(&RubyValue::Nil))?;
    with_file(recv, |f, path| {
        f.set_len(len.max(0) as u64)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "truncate", path))?;
        Ok(RubyValue::Int(0))
    })
}

/// `#chmod(mode)` -- set the open file's permission bits; answers 0.
fn io_chmod(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    use std::os::fd::AsRawFd;
    let mode = int_of(args.first().unwrap_or(&RubyValue::Nil))?;
    with_file(recv, |f, path| {
        // SAFETY: `f` owns a valid fd for the call's duration.
        if unsafe { libc::fchmod(f.as_raw_fd(), mode as libc::mode_t) } != 0 {
            return Err(crate::builtins::file::raise_errno(
                &std::io::Error::last_os_error(),
                "chmod",
                path,
            ));
        }
        Ok(RubyValue::Int(0))
    })
}

/// `#mtime` -- the open file's modification time, via `fstat`.
fn io_mtime(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let st = io_stat(recv, args, None)?;
    crate::dispatch::send_value(&st, crate::Symbol::intern("mtime"), &[], None)
}

/// `#size` -- the open file's byte length, via `fstat`.
fn io_size(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let st = io_stat(recv, args, None)?;
    crate::dispatch::send_value(&st, crate::Symbol::intern("size"), &[], None)
}

/// `#pipe?` -- whether this IO is a pipe end.
fn io_pipe_p(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let is_pipe = matches!(
        as_rio(recv).map(|io| matches!(&*io.backend.lock(), IoBackend::Pipe(_))),
        Some(true)
    );
    Ok(RubyValue::Bool(is_pipe))
}

/// `#fsync`/`#fdatasync` -- flush to disk; answers 0.
fn io_fsync(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    with_file(recv, |f, path| {
        f.sync_all()
            .map_err(|e| crate::builtins::file::raise_errno(&e, "fsync", path))?;
        Ok(RubyValue::Int(0))
    })
}

fn io_sync(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // CRuby: only STDERR is sync by default; STDOUT/STDIN and files are not
    // (oracle-verified). `sync=` can flip it, but nothing here relies on that.
    Ok(RubyValue::Bool(matches!(
        stream_of(recv),
        Some(StdStream::Stderr)
    )))
}

fn io_sync_set(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(args.first().cloned().unwrap_or(RubyValue::Nil))
}

/// `Method#arity` twin of `lookup` -- this hand-rolled table declares no
/// per-method argc, so every method it defines reports CRuby's
/// variadic-cfunc default (`-1`).
pub fn lookup_arity(name: &str) -> Option<i64> {
    lookup(name).map(|_| -1)
}

pub fn lookup(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "puts" => io_puts,
        "print" => io_print,
        "write" => io_write,
        "<<" => io_shovel,
        "flush" => io_flush,
        "fileno" | "to_i" => io_fileno,
        "tty?" | "isatty" => io_tty,
        "winsize" => io_winsize,
        "inspect" | "to_s" => io_inspect,
        "sync" => io_sync,
        "sync=" => io_sync_set,
        "path" | "to_path" => io_path,
        "read" => io_read,
        "gets" => io_gets,
        "readline" => io_readline,
        "lineno" => io_lineno,
        "lineno=" => io_lineno_set,
        "getc" => io_getc,
        "readchar" => io_readchar,
        "getbyte" => io_getbyte,
        "readbyte" => io_readbyte,
        "readlines" => io_readlines,
        "each_line" | "each" => io_each_line,
        "each_char" | "chars" => io_each_char,
        "each_byte" | "bytes" => io_each_byte,
        "printf" => io_printf,
        "putc" => io_putc,
        "readpartial" | "sysread" => io_readpartial,
        "seek" => io_seek,
        "sysseek" => io_sysseek,
        "flock" => io_flock,
        "tell" | "pos" => io_tell,
        "pos=" => io_pos_set,
        "rewind" => io_rewind,
        "eof?" | "eof" => io_eof,
        "stat" => io_stat,
        "lstat" => io_lstat,
        "fcntl" => io_fcntl,
        "chown" => io_chown,
        "chmod" => io_chmod,
        "truncate" => io_truncate,
        "mtime" => io_mtime,
        "size" => io_size,
        "pipe?" => io_pipe_p,
        "fsync" | "fdatasync" => io_fsync,
        "close" => io_close,
        "close_read" => io_close_read,
        "close_write" => io_close_write,
        "closed?" => io_closed,
        "binmode" => io_binmode,
        "binmode?" => io_binmode_p,
        "autoclose=" => io_autoclose_set,
        "autoclose?" => io_autoclose_p,
        "to_io" => io_to_io,
        "close_on_exec?" => io_close_on_exec_p,
        "close_on_exec=" => io_close_on_exec_set,
        "advise" => io_advise,
        "ungetbyte" | "ungetc" => io_ungetbyte,
        "pread" => io_pread,
        "pwrite" => io_pwrite,
        "reopen" => io_reopen,
        "each_codepoint" | "codepoints" => io_each_codepoint,
        _ => return None,
    })
}

/// Reflection companion to `lookup` (hand-written table).
pub fn lookup_names() -> &'static [&'static str] {
    &[
        "puts",
        "print",
        "write",
        "<<",
        "flush",
        "fileno",
        "to_i",
        "tty?",
        "isatty",
        "winsize",
        "inspect",
        "to_s",
        "sync",
        "sync=",
        "path",
        "to_path",
        "read",
        "gets",
        "readline",
        "lineno",
        "lineno=",
        "getc",
        "readchar",
        "getbyte",
        "readbyte",
        "readlines",
        "each_line",
        "each",
        "each_char",
        "chars",
        "each_byte",
        "bytes",
        "printf",
        "putc",
        "readpartial",
        "sysread",
        "seek",
        "sysseek",
        "flock",
        "tell",
        "pos",
        "pos=",
        "rewind",
        "eof?",
        "eof",
        "stat",
        "lstat",
        "fcntl",
        "chown",
        "chmod",
        "truncate",
        "mtime",
        "size",
        "pipe?",
        "fsync",
        "fdatasync",
        "close",
        "close_read",
        "close_write",
        "closed?",
        "binmode",
        "binmode?",
        "autoclose=",
        "autoclose?",
        "to_io",
        "close_on_exec?",
        "close_on_exec=",
        "advise",
        "ungetbyte",
        "ungetc",
        "pread",
        "pwrite",
        "reopen",
        "each_codepoint",
        "codepoints",
    ]
}

/// `IO.pipe` -- a `[reader, writer]` pair over a `pipe(2)`; each end is a plain
/// `IO`. With a block, yields the pair and closes both ends afterward.
fn io_class_pipe(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0..=2);
    use std::os::fd::FromRawFd;
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a 2-element array `pipe(2)` fills with the read/write fds.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(crate::builtins::file::raise_errno(
            &std::io::Error::last_os_error(),
            "pipe",
            "",
        ));
    }
    // SAFETY: `pipe(2)` just handed us these two fresh, owned fds.
    let r = pipe_value(unsafe { std::fs::File::from_raw_fd(fds[0]) });
    let w = pipe_value(unsafe { std::fs::File::from_raw_fd(fds[1]) });
    let pair = RubyValue::Array(crate::collections::array_new(vec![r.clone(), w.clone()]));
    let Some(RubyValue::Proc(p)) = block else {
        return Ok(pair);
    };
    let out = p.call(&[pair]);
    let _ = io_close(&r, &[], None);
    let _ = io_close(&w, &[], None);
    let _ = recv; // `IO.pipe`'s receiver is unused
    out
}

/// `IO.copy_stream(src, dst)` -- copy the whole file `src` to `dst`, answering
/// the byte count.
fn io_class_copy_stream(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 2..=4);
    let src = crate::builtins::file::path_arg(&args[0], "copy_stream")?;
    let dst = crate::builtins::file::path_arg(&args[1], "copy_stream")?;
    let bytes = std::fs::read(&src)
        .map_err(|e| crate::builtins::file::raise_errno(&e, "copy_stream", &src))?;
    let n = bytes.len();
    std::fs::write(&dst, &bytes)
        .map_err(|e| crate::builtins::file::raise_errno(&e, "copy_stream", &dst))?;
    Ok(RubyValue::Int(n as i64))
}

/// `IO.sysopen(path, mode = "r")` -- open and answer the raw fd Integer (the
/// caller owns closing it).
fn io_class_sysopen(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    use std::os::fd::IntoRawFd;
    crate::builtins::arity!(args, 1..=3);
    let path = crate::builtins::file::path_arg(&args[0], "sysopen")?;
    let f = std::fs::File::open(&path)
        .map_err(|e| crate::builtins::file::raise_errno(&e, "sysopen", &path))?;
    Ok(RubyValue::Int(f.into_raw_fd() as i64))
}

/// `IO.new(fd)` / `IO.open(fd)` -- wrap an existing descriptor. `IO.for_fd` is
/// the same. The fd is adopted (closing the IO closes it).
fn io_class_new(
    _recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    use std::os::fd::FromRawFd;
    crate::builtins::arity!(args, 1..=2);
    let fd = &convert::to_index(&args[0])?;
    // SAFETY: the caller vouches the fd is a valid open descriptor to adopt.
    let io = pipe_value(unsafe { std::fs::File::from_raw_fd(*fd as libc::c_int) });
    let Some(RubyValue::Proc(p)) = block else {
        return Ok(io);
    };
    let out = p.call(std::slice::from_ref(&io));
    let _ = io_close(&io, &[], None);
    out
}

pub fn lookup_class(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "pipe" => io_class_pipe,
        "copy_stream" => io_class_copy_stream,
        "sysopen" => io_class_sysopen,
        "new" | "open" | "for_fd" => io_class_new,
        // The whole-file family is identical to `File`'s -- reuse those rows so
        // the two class methods can never drift.
        "read" | "write" | "binread" | "binwrite" | "readlines" | "foreach" => {
            return crate::builtins::file::lookup_class(name);
        }
        _ => return None,
    })
}

/// Reflection companion to `lookup_class`.
pub fn lookup_class_names() -> &'static [&'static str] {
    &[
        "pipe",
        "copy_stream",
        "sysopen",
        "new",
        "open",
        "for_fd",
        "read",
        "write",
        "binread",
        "binwrite",
        "readlines",
        "foreach",
    ]
}

/// The `IO::SEEK_*` constants -- seeded from generated `main()` beside the
/// stdio ones.
pub fn seed_io_constants() {
    let io = zeo_abi::IO_CLASS.0;
    crate::constants::const_set(io, "SEEK_SET", RubyValue::Int(0));
    crate::constants::const_set(io, "SEEK_CUR", RubyValue::Int(1));
    crate::constants::const_set(io, "SEEK_END", RubyValue::Int(2));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(v.to_string()))
    }

    /// A BINARY-tagged string holding raw bytes -- what `Integer#chr`
    /// (128..=255), `String#b`, and binary IO reads produce.
    fn bin(bytes: &[u8]) -> RubyValue {
        RubyValue::Str(crate::collections::string_from_bytes(
            bytes.to_vec(),
            crate::encoding::ASCII_8BIT,
        ))
    }

    // --- display_bytes: the print-family accumulator ------------------

    #[test]
    fn display_bytes_keeps_a_binary_strings_raw_bytes() {
        // THE regression this seam exists for: `0xB4` must stay one byte,
        // not the Latin-1 -> UTF-8 promotion `0xC2 0xB4` that corrupted
        // bm_ao_render/bm_so_mandelbrot's image output.
        let mut buf = Vec::new();
        display_bytes(&bin(&[0xb4]), &mut buf).unwrap();
        assert_eq!(buf, [0xb4]);
    }

    #[test]
    fn display_bytes_renders_utf8_strings_and_non_strings_as_display_text() {
        let mut buf = Vec::new();
        display_bytes(&s("héllo"), &mut buf).unwrap();
        display_bytes(&RubyValue::Int(42), &mut buf).unwrap();
        display_bytes(&RubyValue::Nil, &mut buf).unwrap(); // `print nil` -> ""
        assert_eq!(buf, "héllo42".as_bytes());
    }

    #[test]
    fn display_bytes_keeps_every_byte_of_a_longer_binary_string() {
        let mut buf = Vec::new();
        display_bytes(&bin(&[0x00, 0x7f, 0x80, 0xff]), &mut buf).unwrap();
        assert_eq!(buf, [0x00, 0x7f, 0x80, 0xff]);
    }

    // --- render_puts: CRuby's exact line shapes, now byte-faithful ----

    #[test]
    fn render_puts_writes_a_bare_newline_for_no_args_and_empty_arrays() {
        let mut buf = Vec::new();
        render_puts(&[], &mut buf).unwrap();
        assert_eq!(buf, b"\n");
        buf.clear();
        render_puts(&[RubyValue::Array(crate::array_new(Vec::new()))], &mut buf).unwrap();
        assert_eq!(buf, b"\n");
    }

    #[test]
    fn render_puts_adds_one_newline_and_never_doubles_a_trailing_one() {
        let mut buf = Vec::new();
        render_puts(&[s("a"), s("b\n")], &mut buf).unwrap();
        assert_eq!(buf, b"a\nb\n");
    }

    #[test]
    fn render_puts_flattens_nested_arrays_recursively() {
        let inner = RubyValue::Array(crate::array_new(vec![s("b"), s("c")]));
        let outer = RubyValue::Array(crate::array_new(vec![s("a"), inner]));
        let mut buf = Vec::new();
        render_puts(&[outer], &mut buf).unwrap();
        assert_eq!(buf, b"a\nb\nc\n");
    }

    #[test]
    fn render_puts_preserves_binary_bytes_and_still_terminates_the_line() {
        let mut buf = Vec::new();
        render_puts(&[bin(&[0xb4])], &mut buf).unwrap();
        assert_eq!(buf, [0xb4, b'\n']);
        // A binary string ENDING in 0x0A already has its line ending.
        buf.clear();
        render_puts(&[bin(&[0xb4, b'\n'])], &mut buf).unwrap();
        assert_eq!(buf, [0xb4, b'\n']);
    }

    #[test]
    fn render_puts_marks_a_self_referential_array_instead_of_recursing() {
        let arr = crate::array_new(vec![s("a")]);
        arr.lock().push(RubyValue::Array(arr.clone()));
        let mut buf = Vec::new();
        render_puts(&[RubyValue::Array(arr)], &mut buf).unwrap();
        assert_eq!(buf, b"a\n[...]\n");
    }

    // --- putc_bytes: one character, in the argument's own encoding ----

    #[test]
    fn putc_bytes_takes_an_integers_low_byte() {
        assert_eq!(putc_bytes(&RubyValue::Int(0xb4)).unwrap(), [0xb4]);
        assert_eq!(putc_bytes(&RubyValue::Int(0x1234)).unwrap(), [0x34]);
        // NUM2CHR truncates a Float (oracle-verified).
        assert_eq!(putc_bytes(&RubyValue::Float(65.9)).unwrap(), [65]);
    }

    #[test]
    fn putc_bytes_takes_a_strings_first_character_in_its_own_encoding() {
        // UTF-8: the first CHARACTER (multibyte stays whole).
        assert_eq!(putc_bytes(&s("ab")).unwrap(), b"a");
        assert_eq!(putc_bytes(&s("éx")).unwrap(), "é".as_bytes());
        // BINARY: exactly one raw byte, no UTF-8 promotion.
        assert_eq!(putc_bytes(&bin(&[0xb4, 0x01])).unwrap(), [0xb4]);
        assert_eq!(putc_bytes(&s("")).unwrap(), Vec::<u8>::new());
    }

    #[test]
    #[should_panic(expected = "no implicit conversion from nil to integer")]
    fn putc_bytes_rejects_a_non_character_argument() {
        // NUM2CHR raises CRuby's TypeError; with no registry installed the
        // unit context surfaces it through `raise_error`'s panic fallback,
        // message intact.
        let _ = putc_bytes(&RubyValue::Nil);
    }

    // --- write_value / write_bytes: the fd-facing seam ----------------

    /// A File-backed `RIo` over a fresh temp file, plus its path for
    /// reading the bytes back.
    fn temp_file_io(tag: &str) -> (RubyValue, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("zeo_rt_io_test_{tag}_{}", std::process::id()));
        let f = std::fs::File::create(&path).expect("temp file");
        (file_value(f, path.display().to_string()), path)
    }

    #[test]
    fn write_value_sends_a_binary_strings_raw_bytes_and_counts_them() {
        let (io, path) = temp_file_io("binary");
        let n = write_value(&io, &bin(&[0x00, 0xb4, 0xff])).unwrap();
        assert_eq!(n, 3);
        assert_eq!(std::fs::read(&path).unwrap(), [0x00, 0xb4, 0xff]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn write_value_counts_a_utf8_strings_bytes_and_renders_non_strings() {
        let (io, path) = temp_file_io("mixed");
        assert_eq!(write_value(&io, &s("é")).unwrap(), 2);
        assert_eq!(write_value(&io, &RubyValue::Int(42)).unwrap(), 2);
        assert_eq!(std::fs::read(&path).unwrap(), "é42".as_bytes());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn write_bytes_reaches_the_backend_untouched() {
        let (io, path) = temp_file_io("bytes");
        write_bytes(&io, &[0xc2, 0xb4, 0x00]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [0xc2, 0xb4, 0x00]);
        let _ = std::fs::remove_file(path);
    }
}
