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
}

pub struct RIo {
    stream: StdStream,
}

impl RubyObject for RIo {
    fn class_id(&self) -> ClassId {
        IO_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    // The two singletons are process-wide and effectively immutable state
    // holders; freezing them is meaningless (CRuby's are unfrozen too).
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RIo { stream: self.stream })
    }
}

pub fn stdout_value() -> RubyValue {
    static V: LazyLock<RubyValue> =
        LazyLock::new(|| RubyValue::Object(Arc::new(RIo { stream: StdStream::Stdout })));
    V.clone()
}

pub fn stderr_value() -> RubyValue {
    static V: LazyLock<RubyValue> =
        LazyLock::new(|| RubyValue::Object(Arc::new(RIo { stream: StdStream::Stderr })));
    V.clone()
}

/// Installs the `STDOUT`/`STDERR` constants and the `$stdout`/`$stderr`
/// globals -- called once from generated `main()` (CRuby startup parity).
pub fn seed_stdio() {
    crate::constants::const_set(0, "STDOUT", stdout_value());
    crate::constants::const_set(0, "STDERR", stderr_value());
    crate::globals::global_set(0, "$stdout", stdout_value());
    crate::globals::global_set(0, "$stderr", stderr_value());
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
            match io.stream {
                StdStream::Stdout => {
                    let mut out = std::io::stdout();
                    if out.write_all(s.as_bytes()).is_err() {
                        // A closed pipe downstream (`head`, etc.) -- CRuby
                        // dies with EPIPE; a quiet exit is the pragmatic
                        // spike-scope equivalent.
                        std::process::exit(0);
                    }
                    return Ok(());
                }
                StdStream::Stderr => {
                    let _ = std::io::stderr().write_all(s.as_bytes());
                    return Ok(());
                }
            }
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

fn stream_of(recv: &RubyValue) -> Option<StdStream> {
    if let RubyValue::Object(o) = recv {
        return o.as_any().downcast_ref::<RIo>().map(|io| io.stream);
    }
    None
}

fn io_fileno(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Int(match stream_of(recv) {
        Some(StdStream::Stdout) => 1,
        Some(StdStream::Stderr) => 2,
        None => 0,
    }))
}

fn io_tty(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    use std::io::IsTerminal;
    Ok(RubyValue::Bool(match stream_of(recv) {
        Some(StdStream::Stdout) => std::io::stdout().is_terminal(),
        Some(StdStream::Stderr) => std::io::stderr().is_terminal(),
        None => false,
    }))
}

fn io_inspect(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let name = match stream_of(recv) {
        Some(StdStream::Stdout) => "#<IO:<STDOUT>>",
        Some(StdStream::Stderr) => "#<IO:<STDERR>>",
        None => "#<IO>",
    };
    Ok(RubyValue::Str(crate::collections::string_new(name.to_string())))
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
        "fileno" => io_fileno,
        "tty?" | "isatty" => io_tty,
        "inspect" | "to_s" => io_inspect,
        "sync" => io_sync,
        "sync=" => io_sync_set,
        _ => return None,
    })
}
