//! `IO` (G0, minimal): the `STDOUT`/`STDERR` singletons and the
//! `$stdout`/`$stderr` globals the Kernel print family routes through.
//! File-backed IO, `STDIN`/`gets`, buffering modes, and encodings are a
//! later phase (plan P-B) -- this slice exists so `STDOUT.puts`,
//! `$stderr.print`, and `$stdout = <duck>` redirection behave.

use std::io::Write;
use std::sync::{Arc, LazyLock};

use crate::dispatch::{raise_error, RObj, RubyObject};
use crate::signal::Signal;
use crate::value::RubyValue;
use spinel_abi::{ClassId, IO_CLASS};

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
}

pub struct RIo {
    /// `Mutex` because reading MOVES the file position -- an IO is mutable
    /// shared state even behind an `Arc`, unlike the stateless std-stream
    /// singletons this started as.
    backend: parking_lot::Mutex<IoBackend>,
    /// The path this was opened from, for error messages and `#path`; empty
    /// for the std streams.
    path: String,
}

impl RubyObject for RIo {
    // A File instance carries FILE_CLASS so its own MRO (`File < IO`) finds
    // File's rows before IO's; the std streams are plain IOs.
    fn class_id(&self) -> ClassId {
        match &*self.backend.lock() {
            IoBackend::File(_) => spinel_abi::FILE_CLASS,
            IoBackend::Std(_) => IO_CLASS,
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
            IoBackend::File(_) => StdStream::Stdout,
        };
        Arc::new(RIo {
            backend: parking_lot::Mutex::new(IoBackend::Std(stream)),
            path: self.path.clone(),
        })
    }
}

fn std_io(stream: StdStream) -> RubyValue {
    RubyValue::Object(Arc::new(RIo {
        backend: parking_lot::Mutex::new(IoBackend::Std(stream)),
        path: String::new(),
    }))
}

/// Wrap an already-open file as a Ruby `File` value.
pub(crate) fn file_value(f: std::fs::File, path: String) -> RubyValue {
    RubyValue::Object(Arc::new(RIo {
        backend: parking_lot::Mutex::new(IoBackend::File(Some(f))),
        path,
    }))
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

/// Writes `s` to `target`: directly for one of our `RIo`s, via a dynamic
/// `write` send for anything else (`$stdout = <duck>` redirection --
/// CRuby's own contract is "any object responding to `write`").
pub fn write_str(target: &RubyValue, s: &str) -> Result<(), Signal> {
    if let RubyValue::Object(o) = target {
        if let Some(io) = o.as_any().downcast_ref::<RIo>() {
            return match &mut *io.backend.lock() {
                IoBackend::Std(StdStream::Stdout) => {
                    let mut out = std::io::stdout();
                    if out.write_all(s.as_bytes()).is_err() {
                        // A closed pipe downstream (`head`, etc.) -- CRuby
                        // dies with EPIPE; a quiet exit is the pragmatic
                        // spike-scope equivalent.
                        std::process::exit(0);
                    }
                    Ok(())
                }
                IoBackend::Std(StdStream::Stderr) => {
                    let _ = std::io::stderr().write_all(s.as_bytes());
                    Ok(())
                }
                IoBackend::Std(StdStream::Stdin) => Err(raise_error(
                    "IOError",
                    "not opened for writing".to_string(),
                )),
                IoBackend::File(None) => {
                    Err(raise_error("IOError", "closed stream".to_string()))
                }
                IoBackend::File(Some(f)) => f
                    .write_all(s.as_bytes())
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "write", &io.path)),
            };
        }
    }
    crate::dispatch::send_value(
        target,
        crate::symbol::Symbol::intern("write"),
        &[RubyValue::Str(crate::collections::string_new(s.to_string()))],
        None,
    )
    .map(|_| ())
}

/// `puts`'s rendering into a buffer: every arg on its own line, arrays
/// flattened recursively, `[...]` for a self-referential array, a bare
/// newline for no args / an empty array -- CRuby's exact shapes.
pub fn render_puts(args: &[RubyValue], buf: &mut String) {
    fn put_one(v: &RubyValue, seen: &mut Vec<usize>, buf: &mut String) {
        match v {
            RubyValue::Array(a) => {
                let id = Arc::as_ptr(a) as usize;
                if seen.contains(&id) {
                    buf.push_str("[...]\n");
                    return;
                }
                seen.push(id);
                let items = a.lock().clone();
                if items.is_empty() {
                    buf.push('\n');
                }
                for e in &items {
                    put_one(e, seen, buf);
                }
                seen.pop();
            }
            other => {
                let s = other.to_display_string();
                buf.push_str(&s);
                if !s.ends_with('\n') {
                    buf.push('\n');
                }
            }
        }
    }
    if args.is_empty() {
        buf.push('\n');
    }
    for a in args {
        put_one(a, &mut Vec::new(), buf);
    }
}

fn recv_io(recv: &RubyValue) -> Result<&RubyValue, Signal> {
    // The table only dispatches on IO_CLASS receivers, so `recv` is always
    // one of the singletons -- kept as a value so the write helpers stay
    // target-shaped.
    Ok(recv)
}

fn io_puts(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let mut buf = String::new();
    render_puts(args, &mut buf);
    write_str(recv_io(recv)?, &buf)?;
    Ok(RubyValue::Nil)
}

fn io_print(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let mut buf = String::new();
    for a in args {
        buf.push_str(&a.to_display_string());
    }
    write_str(recv_io(recv)?, &buf)?;
    Ok(RubyValue::Nil)
}

fn io_write(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let mut total = 0i64;
    for a in args {
        let s = a.to_display_string();
        total += s.len() as i64;
        write_str(recv_io(recv)?, &s)?;
    }
    Ok(RubyValue::Int(total))
}

fn io_shovel(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let [v] = args else {
        return Err(raise_error(
            "ArgumentError",
            format!("wrong number of arguments (given {}, expected 1)", args.len()),
        ));
    };
    write_str(recv_io(recv)?, &v.to_display_string())?;
    Ok(recv.clone())
}

fn io_flush(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
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
        IoBackend::File(_) => None,
    }
}

fn io_fileno(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
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

fn io_tty(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    use std::io::IsTerminal;
    Ok(RubyValue::Bool(match stream_of(recv) {
        Some(StdStream::Stdin) => std::io::stdin().is_terminal(),
        Some(StdStream::Stdout) => std::io::stdout().is_terminal(),
        Some(StdStream::Stderr) => std::io::stderr().is_terminal(),
        // A regular file is never a tty.
        None => false,
    }))
}

fn io_inspect(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
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
fn io_path(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    match as_rio(recv) {
        Some(io) if !io.path.is_empty() => Ok(RubyValue::Str(crate::collections::string_new(
            io.path.clone(),
        ))),
        // A std stream has no path -- real Ruby raises IOError for `#path`
        // on one, rather than answering nil.
        _ => Err(raise_error("IOError", "not a file".to_string())),
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
        return Err(raise_error("IOError", "not a file".to_string()));
    };
    let path = io.path.clone();
    match &mut *io.backend.lock() {
        IoBackend::File(Some(file)) => f(file, &path),
        IoBackend::File(None) => Err(raise_error("IOError", "closed stream".to_string())),
        IoBackend::Std(_) => Err(raise_error("IOError", "not a file".to_string())),
    }
}

/// `read` / `read(n)` -- the whole rest, or exactly `n` bytes. At EOF, a
/// LENGTHED read answers nil while a whole-rest read answers `""` (real
/// Ruby's asymmetry, and the thing a read loop tests).
fn io_read(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    // A read from STDIN reads the real one; anything else needs a file.
    if matches!(stream_of(recv), Some(StdStream::Stdin)) {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "read", "<STDIN>"))?;
        return Ok(RubyValue::Str(crate::collections::string_new(buf)));
    }
    let n = match args.first() {
        None | Some(RubyValue::Nil) => None,
        Some(RubyValue::Int(i)) if *i >= 0 => Some(*i as usize),
        Some(RubyValue::Int(_)) => {
            return Err(raise_error(
                "ArgumentError",
                "negative length".to_string(),
            ))
        }
        Some(other) => {
            return Err(raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(other)
                ),
            ))
        }
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
                        Err(e) => {
                            return Err(crate::builtins::file::raise_errno(&e, "read", path))
                        }
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
fn io_seek(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let off = match args.first() {
        Some(RubyValue::Int(i)) => *i,
        _ => {
            return Err(raise_error(
                "TypeError",
                "no implicit conversion into Integer".to_string(),
            ))
        }
    };
    let whence = match args.get(1) {
        None => 0,
        Some(RubyValue::Int(w)) => *w,
        Some(_) => {
            return Err(raise_error(
                "TypeError",
                "no implicit conversion into Integer".to_string(),
            ))
        }
    };
    with_file(recv, |f, path| {
        use std::io::Seek;
        let pos = match whence {
            0 => std::io::SeekFrom::Start(off.max(0) as u64),
            1 => std::io::SeekFrom::Current(off),
            2 => std::io::SeekFrom::End(off),
            _ => return Err(raise_error("ArgumentError", "invalid whence".to_string())),
        };
        f.seek(pos)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "seek", path))?;
        Ok(RubyValue::Int(0))
    })
}

fn io_tell(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    with_file(recv, |f, path| {
        use std::io::Seek;
        let p = f
            .stream_position()
            .map_err(|e| crate::builtins::file::raise_errno(&e, "tell", path))?;
        Ok(RubyValue::Int(p as i64))
    })
}

fn io_rewind(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    with_file(recv, |f, path| {
        use std::io::Seek;
        f.rewind()
            .map_err(|e| crate::builtins::file::raise_errno(&e, "rewind", path))?;
        Ok(RubyValue::Int(0))
    })
}

fn io_eof(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
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
fn io_close(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    if let Some(io) = as_rio(recv) {
        if let IoBackend::File(slot) = &mut *io.backend.lock() {
            slot.take();
        }
    }
    Ok(RubyValue::Nil)
}

fn io_closed(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let closed = match as_rio(recv) {
        Some(io) => matches!(&*io.backend.lock(), IoBackend::File(None)),
        None => false,
    };
    Ok(RubyValue::Bool(closed))
}

/// `each_line`/`each` over a file's lines, and `readlines`/`gets` beside it.
fn io_readlines(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let text = io_read(recv, &[], None)?.to_display_string();
    Ok(RubyValue::Array(crate::collections::array_new(
        crate::builtins::string::split_lines(&text),
    )))
}

fn io_each_line(recv: &RubyValue, _args: &[RubyValue], blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = blk else {
        return Err(raise_error(
            "LocalJumpError",
            "no block given (yield)".to_string(),
        ));
    };
    let text = io_read(recv, &[], None)?.to_display_string();
    for l in crate::builtins::string::split_lines(&text) {
        p.call(&[l])?;
    }
    Ok(recv.clone())
}

/// `gets` -- one line, or nil at EOF. Reads a byte at a time so the file
/// position lands exactly after the newline (a buffered read would consume
/// more than the line and desync `tell`).
fn io_gets(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    if matches!(stream_of(recv), Some(StdStream::Stdin)) {
        let mut line = String::new();
        let n = std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "gets", "<STDIN>"))?;
        if n == 0 {
            return Ok(RubyValue::Nil);
        }
        return Ok(RubyValue::Str(crate::collections::string_new(line)));
    }
    with_file(recv, |f, path| {
        use std::io::Read;
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match f.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    line.push(byte[0]);
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(e) => return Err(crate::builtins::file::raise_errno(&e, "gets", path)),
            }
        }
        if line.is_empty() {
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Str(crate::collections::string_new(
            String::from_utf8_lossy(&line).into_owned(),
        )))
    })
}

fn io_sync(_recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    // Our writes are unbuffered `write_all`s; reporting `sync == true` is
    // the honest answer.
    Ok(RubyValue::Bool(true))
}

fn io_sync_set(_recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(args.first().cloned().unwrap_or(RubyValue::Nil))
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
        "inspect" | "to_s" => io_inspect,
        "sync" => io_sync,
        "sync=" => io_sync_set,
        "path" | "to_path" => io_path,
        "read" => io_read,
        "gets" => io_gets,
        "readlines" => io_readlines,
        "each_line" | "each" => io_each_line,
        "seek" => io_seek,
        "tell" | "pos" => io_tell,
        "rewind" => io_rewind,
        "eof?" | "eof" => io_eof,
        "close" => io_close,
        "closed?" => io_closed,
        _ => return None,
    })
}

/// The `IO::SEEK_*` constants -- seeded from generated `main()` beside the
/// stdio ones.
pub fn seed_io_constants() {
    let io = spinel_abi::IO_CLASS.0;
    crate::constants::const_set(io, "SEEK_SET", RubyValue::Int(0));
    crate::constants::const_set(io, "SEEK_CUR", RubyValue::Int(1));
    crate::constants::const_set(io, "SEEK_END", RubyValue::Int(2));
}
