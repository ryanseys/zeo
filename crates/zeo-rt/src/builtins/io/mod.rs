//! `IO` (CRuby io.c): the `STDIN`/`STDOUT`/`STDERR` singletons and the
//! `$stdin`/`$stdout`/`$stderr` globals the Kernel print family routes
//! through, plus the descriptor-backed instance surface -- reads, writes,
//! read-ahead buffering, sync modes, encodings, pipes, `popen`, `select`.
//!
//! The `ruby_class!` block below is the whole IO surface and stays in one
//! piece; the files beside it hold what its rows call.

use std::io::Write;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, LazyLock};

use crate::builtins::{arg_error, convert, eof_error, io_error, not_impl_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::signal::Signal;
use crate::value::RubyValue;
use zeo_abi::{ClassId, IO_CLASS};
use zeo_macros::ruby_class;

mod copy;
mod handle;
mod open;
mod read;
mod stdio;
mod wait;
mod write;

use copy::*;
use handle::*;
use open::*;
use read::*;
use wait::*;

pub use handle::{IoBackend, RIo, StdStream, as_rio, fmode_bits};
pub use read::has_buffered_bytes;
pub use stdio::{
    current_stderr, current_stdout, seed_stdio, stderr_value, stdin_value, stdout_value,
};
pub use write::{display_bytes, render_puts, write_bytes, write_str, write_value};

pub(crate) use handle::{
    check_readable, check_writable, file_value, file_value_mode, io_is_closed, pipe_value, raw_fd,
    set_handle_encodings, stream_of, uninit_io,
};
pub(crate) use open::{
    dup_fd_file, nonblock_raises, popen_value, set_fd_cloexec, set_fd_nonblock, would_block,
};
pub(crate) use read::{int_of, offset_of, stat_value, with_file};

// The only callers are extensions, so a build with the ext features off
// reaches none of these.
#[allow(unused_imports, reason = "each name is reached from an ext crate only")]
pub(crate) use handle::{pipe_value_named, socket_from_raw_fd, socket_raw_fd, stream_label};
#[allow(unused_imports, reason = "each name is reached from an ext crate only")]
pub(crate) use read::{chomp_line, line_opts};
pub(crate) use write::putc_bytes;

/// The whole-file class methods (`IO.read`, `IO.foreach`, ...) are identical to
/// `File`'s -- run File's own row rather than restating it.
fn file_class_row(
    name: &str,
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let row = crate::builtins::file::lookup_class(name)
        .unwrap_or_else(|| unreachable!("File.{name} is a row"));
    row(recv, args, block)
}

ruby_class! {
    IO = zeo_abi::IO_CLASS < zeo_abi::OBJECT_CLASS;

    allocate io_allocate;
    include zeo_abi::ENUMERABLE_CLASS;

    seed seed_io_constants;
    seed seed_stdio;
    // The open/lock flags, shared with `File` -- see `file.rs`. Listed after
    // Enumerable, so the resulting `IO.ancestors` is CRuby's
    // `[IO, File::Constants, Enumerable, ...]`.
    include zeo_abi::FILE_CONSTANTS_MODULE;

    def "puts" (recv, *args, &_blk) {
        let mut buf = Vec::new();
        // Flush-then-propagate on a raising `to_s` -- see `kernel_puts`.
        let rendered = render_puts(args, &mut buf);
        write_bytes(recv_io(recv)?, &buf)?;
        rendered?;
        Ok(RubyValue::Nil)
    }

    // The arguments are joined by `$,` and closed by `$\`, both nil (so
    // both empty) unless the program sets them -- `Kernel#print` is the
    // same body over `$stdout`, and neither reads them at stream creation.
    def "print" (recv, *args, &_blk) {
        let mut buf = Vec::new();
        let mut rendered = Ok(());
        let field_sep = crate::builtins::kernel::output_separator("$,");
        for (i, a) in args.iter().enumerate() {
            if i > 0 && let Some(s) = &field_sep {
                buf.extend_from_slice(s.bytes());
            }
            if let Err(sig) = display_bytes(a, &mut buf) {
                rendered = Err(sig);
                break;
            }
        }
        if rendered.is_ok()
            && let Some(s) = crate::builtins::kernel::output_separator("$\\")
        {
            buf.extend_from_slice(s.bytes());
        }
        // Flush-then-propagate on a raising `to_s` -- see `kernel_puts`.
        write_bytes(recv_io(recv)?, &buf)?;
        rendered?;
        Ok(RubyValue::Nil)
    }

    def "write" (recv, *args, &_blk) {
        let mut total = 0i64;
        for a in args {
            total += write_value(recv_io(recv)?, a)?;
        }
        Ok(RubyValue::Int(total))
    }

    def "<<" (recv, value, &_blk) {
        write_value(recv_io(recv)?, value)?;
        Ok(recv.clone())
    }

    // `#syswrite(str)` -- one unbuffered write, answering the byte count.
    // Every write here already reaches the descriptor at once, so this is
    // `#write` narrowed to a single argument, which is what CRuby's takes.
    def "syswrite" (recv, value, &_blk) {
        Ok(RubyValue::Int(write_value(recv_io(recv)?, value)?))
    }

    // `#ioctl(cmd, arg = 0)` -- the raw `ioctl(2)`. An Integer `arg` passes by
    // value; a String passes its buffer, which the call may WRITE THROUGH (the
    // whole point of the String form: the answer comes back in the buffer).
    def "ioctl" cfunc (recv, cmd, arg?) {
        let request = crate::builtins::convert::to_index(cmd)? as libc::c_ulong;
        let RubyValue::Int(fd) = fileno_value(recv)? else {
            return Err(io_error!("closed stream"));
        };
        let fd = fd as libc::c_int;
        let rc = match arg {
            Some(RubyValue::Str(s)) => {
                // The buffer must be at least as big as the REQUEST NUMBER
                // says, not as big as the String happens to be: the kernel
                // writes `IOCPARM_LEN(request)` bytes whatever the caller
                // passed, so `$stdout.ioctl(TIOCGWINSZ, "")` wrote eight
                // bytes through a zero-capacity allocation. CRuby's
                // `setup_narg` grows the String first, and so does this.
                let want = ioctl_param_len(request);
                let mut buf = s.lock().bytes().to_vec();
                let given = buf.len();
                buf.resize(given.max(want), 0);
                // SAFETY: `buf` now holds at least the byte count the request
                // number encodes, so the call cannot write past it.
                let rc = unsafe { libc::ioctl(fd, request, buf.as_mut_ptr()) };
                if rc >= 0 {
                    // The String keeps the length the caller gave it, as
                    // CRuby's does when the request asked for no more.
                    buf.truncate(given.max(want));
                    let mut g = s.lock();
                    let enc = g.encoding();
                    g.replace_bytes(buf, enc);
                }
                rc
            }
            Some(v) if !v.is_nil() => {
                let n = crate::builtins::convert::to_index(v)? as libc::c_int;
                // SAFETY: the by-value form passes an integer, not a pointer.
                unsafe { libc::ioctl(fd, request, n) }
            }
            // SAFETY: as above, with CRuby's default argument.
            _ => unsafe { libc::ioctl(fd, request, 0 as libc::c_int) },
        };
        if rc < 0 {
            let path = as_rio(recv).and_then(|io| io.path.lock().clone()).unwrap_or_default();
            return Err(crate::builtins::file::raise_errno(
                &std::io::Error::last_os_error(), "ioctl", &path));
        }
        Ok(RubyValue::Int(rc as i64))
    }

    // `#timeout`/`#timeout=` -- recorded and read back. CRuby raises
    // `IO::TimeoutError` when a blocking read outlives the value; zeo's reads
    // block, so nothing enforces it (documented in COMPATIBILITY.md). nil,
    // the default, means no timeout in CRuby either.
    def "timeout" (recv, &_blk) {
        Ok(match as_rio(recv) {
            Some(io) => io.timeout.lock().clone(),
            None => RubyValue::Nil,
        })
    }
    def "timeout=" (recv, seconds, &_blk) {
        if let Some(io) = as_rio(recv) {
            *io.timeout.lock() = seconds.clone();
        }
        Ok(seconds.clone())
    }

    // `#set_encoding_by_bom` -- if the stream STARTS with a byte-order mark,
    // consume it, make that the external encoding and answer it; otherwise
    // touch nothing and answer nil.
    def "set_encoding_by_bom" (recv, &_blk) {
        let Some(io) = as_rio(recv) else {
            return Ok(RubyValue::Nil);
        };
        // ASCII-8BIT does NOT conflict: `rb_io_set_encoding_by_bom` REQUIRES
        // binmode, and binmode is exactly what sets the external encoding to
        // ASCII-8BIT. Only some other explicit encoding is the conflict.
        if io
            .encodings
            .lock()
            .0
            .is_some_and(|e| e != crate::encoding::ASCII_8BIT)
        {
            return Err(arg_error!("encoding is set to UTF-8 already"));
        }
        let Some((id, len)) = read_bom(recv)? else {
            return Ok(RubyValue::Nil);
        };
        // Only the BOM's own bytes are consumed; `with_file` left the
        // descriptor at the position Ruby believes in, so seeking forward by
        // the mark's length is what "skip it" means.
        with_file(recv, |f, path| {
            use std::io::Seek;
            f.seek(std::io::SeekFrom::Start(len as u64))
                .map_err(|e| crate::builtins::file::raise_errno(&e, "seek", path))?;
            Ok(())
        })?;
        io.encodings.lock().0 = Some(id);
        Ok(crate::builtins::encoding::encoding_value(id))
    }

    def "flush" (recv, &_blk) {
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        Ok(recv.clone())
    }

    def "fileno" | "to_i" (recv, &_blk) {
        fileno_value(recv)
    }

    def "tty?" | "isatty" (recv, &_blk) {
        // `rb_io_isatty` asks the descriptor, so a pty a gem opened answers
        // true -- reading the std streams alone called every other terminal
        // a file, and `io/console` reads this word before it goes raw.
        let fd = raw_fd(recv)?;
        // SAFETY: a plain query on a descriptor this handle owns.
        Ok(RubyValue::Bool(unsafe { libc::isatty(fd) } == 1))
    }

    // `IO#winsize` (from `require "io/console"`) -- `[rows, columns]`, or
    // `Errno::ENOTTY` when the stream isn't a terminal, as CRuby answers. The
    // rest of the console surface is in `io_console.rs`; this row predates it.
    #[cfg(feature = "ext-io-console")]
    def "winsize" gated "io/console" (recv, &_blk) {
        let fd = raw_fd(recv)?;
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } != 0 {
            return Err(crate::ext::io_console::not_a_terminal(recv, "IO#winsize"));
        }
        Ok(RubyValue::Array(crate::collections::array_new(vec![
            RubyValue::Int(ws.ws_row as i64),
            RubyValue::Int(ws.ws_col as i64),
        ])))
    }

    // `IO#nonblock?` -- the descriptor's own `O_NONBLOCK`, from
    // `require "io/nonblock"`.
    def "nonblock?" gated "io/nonblock" (recv, &_blk) {
        let Some(fd) = io_raw_fd(recv) else {
            return Ok(RubyValue::Bool(false));
        };
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return Err(io_error!("closed stream"));
        }
        Ok(RubyValue::Bool(flags & libc::O_NONBLOCK != 0))
    }

    // `IO#nonblock(flag = true) { ... }` -- sets the flag for the block only and
    // restores it after, answering the block's value. CRuby REQUIRES the block
    // here (`#nonblock?` is the reader), so a blockless call is a LocalJumpError.
    def "nonblock" gated "io/nonblock" (recv, mode?, &blk) {
        let Some(RubyValue::Proc(p)) = blk else {
            return Err(crate::builtins::local_jump_error!("no block given"));
        };
        let on = mode.is_none_or(|v| v.truthy());
        let Some(fd) = io_raw_fd(recv) else {
            return Err(io_error!("closed stream"));
        };
        let was = unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_NONBLOCK != 0;
        set_fd_nonblock(fd, on)?;
        let out = p.call(&[]);
        set_fd_nonblock(fd, was)?;
        out
    }

    // `IO#nonblock = flag` -- the plain setter, answering the flag it set.
    def "nonblock=" gated "io/nonblock" (recv, nonblock, &_blk) {
        let on = nonblock.truthy();
        let Some(fd) = io_raw_fd(recv) else {
            return Err(io_error!("closed stream"));
        };
        set_fd_nonblock(fd, on)?;
        Ok(RubyValue::Bool(on))
    }

    def "wait_readable" cfunc (recv, timeout?, &_blk) {
        io_wait_for(recv, timeout, libc::POLLIN)
    }

    def "wait_writable" cfunc (recv, timeout?, &_blk) {
        io_wait_for(recv, timeout, libc::POLLOUT)
    }

    // `wait_priority(timeout = nil)` -- out-of-band data only, which is why a
    // pipe carrying ordinary bytes answers nil. Goes through `select(2)`, not
    // `poll(2)`; see [`select_ready`].
    def "wait_priority" cfunc (recv, timeout?, &_blk) {
        let (_, _, e) = select_ready(&[], &[], &[raw_fd(recv)?], wait_timeout_ms(timeout)?)?;
        Ok(if e.first() == Some(&true) {
            recv.clone()
        } else {
            RubyValue::Nil
        })
    }

    // `wait(*args)` -- the three `wait_*` methods behind one name. Arguments
    // are sorted by TYPE, not by position: every Symbol is a mode (several may
    // be combined), anything else is the timeout. `io.wait(:read, 0.1)` and
    // `io.wait(0.1, :read)` are the same call.
    //
    // Reading the FIRST argument as the timeout made `io.wait(:read)` -- the
    // spelling the docs lead with -- pass a Symbol into the timeout slot.
    def "wait" (recv, *args, &_blk) {
        let mut events: libc::c_short = 0;
        let mut timeout: Option<&RubyValue> = None;
        for a in args {
            match a {
                RubyValue::Symbol(s) => {
                    events |= match s.name().as_str() {
                        "read" | "readable" => libc::POLLIN,
                        "write" | "writable" => libc::POLLOUT,
                        "priority" => libc::POLLPRI,
                        other => return Err(arg_error!("unsupported mode: {other}")),
                    };
                }
                other => timeout = Some(other),
            }
        }
        if events == 0 {
            events = libc::POLLIN;
        }
        io_wait_for(recv, timeout, events)
    }

    // `IO#to_s` is NOT `#inspect`: CRuby leaves `to_s` as `Object`'s address
    // form (`#<IO:0x...>`, `#<File:0x...>`) even for `STDIN`, and only
    // `inspect` describes the stream. Interpolating an IO shows the address.
    def "inspect" (recv, &_blk) {
        let name = match stream_of(recv) {
            Some(StdStream::Stdin) => "#<IO:<STDIN>>".to_string(),
            Some(StdStream::Stdout) => "#<IO:<STDOUT>>".to_string(),
            Some(StdStream::Stderr) => "#<IO:<STDERR>>".to_string(),
            None => match as_rio(recv) {
                // A handle names its path when it has one, its descriptor when it
                // does not (a pipe end, a socket), and neither once it is closed.
                // A handle that was never opened has no path and no
                // descriptor to name, so ruby falls back to the address.
                Some(io) if matches!(&*io.backend.lock(), IoBackend::Uninit) => {
                    let class = crate::builtins::class_name_of(recv);
                    match recv {
                        RubyValue::Object(o) => format!(
                            "#<{class}:0x{:016x}>",
                            Arc::as_ptr(o) as *const () as usize
                        ),
                        _ => format!("#<{class}>"),
                    }
                }
                Some(io) => {
                    let class = crate::builtins::class_name_of(recv);
                    let closed = matches!(
                        &*io.backend.lock(),
                        IoBackend::File(None) | IoBackend::Pipe(None)
                    );
                    // The path is COPIED out before the descriptor is asked
                    // for. Reading it inside the `match` held the path lock
                    // across `raw_fd`, which takes the backend lock -- while
                    // `#path` takes them the other way round, so one thread
                    // in `pipe.inspect` and one in `pipe.path` deadlocked.
                    let path = io.path.lock().clone();
                    match (path.as_deref(), closed) {
                        (Some(p), false) => format!("#<{class}:{p}>"),
                        (Some(p), true) => format!("#<{class}:{p} (closed)>"),
                        (None, false) => format!("#<{class}:fd {}>", raw_fd(recv)?),
                        (None, true) => format!("#<{class}:(closed)>"),
                    }
                }
                None => "#<IO>".to_string(),
            },
        };
        Ok(RubyValue::Str(crate::collections::string_new(name)))
    }

    def "sync" (recv, &_blk) {
        let on = as_rio(recv).is_some_and(|io| io.sync.load(std::sync::atomic::Ordering::Relaxed));
        Ok(RubyValue::Bool(on))
    }

    def "sync=" (recv, sync, &_blk) {
        let v = sync.clone();
        if let Some(io) = as_rio(recv) {
            io.sync
                .store(v.truthy(), std::sync::atomic::Ordering::Relaxed);
        }
        Ok(v)
    }

    // `path`/`to_path` -- the name this IO was opened from.
    def "path" | "to_path" (recv, &_blk) {
        let Some(io) = as_rio(recv) else {
            return Err(io_error!("not a file"));
        };
        // A std stream reports its bracketed name -- unless a `#reopen` gave
        // it a real path, which CRuby then answers instead (`$stderr.reopen(
        // IO::NULL).path` is "/dev/null"). Anything else answers its path, or
        // nil when it has none (a pipe, or `File.new(fd)` with no `path:`).
        if let (IoBackend::Std(stream), None) = (&*io.backend.lock(), io.path.lock().as_ref()) {
            let name = match stream {
                StdStream::Stdin => "<STDIN>",
                StdStream::Stdout => "<STDOUT>",
                StdStream::Stderr => "<STDERR>",
            };
            return Ok(RubyValue::Str(crate::collections::string_new(
                name.to_string(),
            )));
        }
        Ok(match &*io.path.lock() {
            Some(p) => RubyValue::Str(crate::collections::string_new(p.clone())),
            None => RubyValue::Nil,
        })
    }

    // `read` / `read(n)` / `read(n, buf)` -- the whole rest, or `n` bytes,
    // optionally read INTO an existing String `buf` (returned in place of a fresh
    // one). At EOF, a LENGTHED read answers nil while a whole-rest read answers
    // `""` (real Ruby's asymmetry, and the thing a read loop tests).
    def "read" cfunc (recv, _length?, outbuf?, &blk) {
        let result = io_read_val(recv, __args, blk)?;
        // 2-arg `read(length, buffer)`: fill the caller's String and answer it (or
        // nil at EOF, having emptied it).
        if let Some(RubyValue::Str(buf)) = outbuf {
            match &result {
                RubyValue::Str(s) => {
                    // The BYTES, not a lossy re-encoding of them. Going
                    // through UTF-8 turns every byte no character claims into
                    // a three-byte replacement, so a 100-byte read filled the
                    // caller's buffer with 125 -- see `readpartial`.
                    let bytes = s.lock().bytes().to_vec();
                    let mut g = buf.lock();
                    // The buffer KEEPS its own encoding. Measured: ruby fills
                    // a `+""` and it stays UTF-8, fills a `"".b` and it stays
                    // binary, fills a EUC-JP one and it stays EUC-JP. Only
                    // the bytes are replaced.
                    let enc = g.encoding();
                    g.replace_bytes(bytes, enc);
                    drop(g);
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

    // `gets([sep][, limit][, chomp:])` -- one line, or nil at EOF; bumps `lineno`.
    def "gets" cfunc (recv, _sep?, _limit?, **_opts, &_blk) {
        let args = __args;
        gets_value(recv, args)
    }

    // `readline` -- `gets`, but raises `EOFError` instead of answering nil.
    def "readline" params "sep = nil, limit = nil, chomp: nil" cfunc (recv, _sep?, _limit?, **_opts, &_blk) {
        let args = __args;
        match gets_value(recv, args)? {
            RubyValue::Nil => Err(eof_error!("end of file reached")),
            line => Ok(line),
        }
    }

    def "lineno" (recv, &_blk) {
        let n = as_rio(recv).map_or(0, |io| io.lineno.load(std::sync::atomic::Ordering::Relaxed));
        Ok(RubyValue::Int(n))
    }

    def "lineno=" (recv, lineno, &_blk) {
        let n = convert::to_index(lineno)?;
        if let Some(io) = as_rio(recv) {
            io.lineno.store(n, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(RubyValue::Int(n))
    }

    def "getc" (recv, &_blk) {
        getc_value(recv)
    }

    def "readchar" (recv, &_blk) {
        match getc_value(recv)? {
            RubyValue::Nil => Err(eof_error!("end of file reached")),
            ch => Ok(ch),
        }
    }

    def "getbyte" (recv, &_blk) {
        getbyte_value(recv)
    }

    def "readbyte" (recv, &_blk) {
        match getbyte_value(recv)? {
            RubyValue::Nil => Err(eof_error!("end of file reached")),
            b => Ok(b),
        }
    }

    // `readlines([sep][, limit][, chomp:])` -- every remaining line as an Array.
    // Drains through `gets`, so `$stdin` reads its own way (see `gets_value`)
    // instead of demanding a real file.
    def "readlines" cfunc (recv, _sep?, _limit?, **_opts, &_blk) {
        let args = __args;
        let mut lines = Vec::new();
        while let line @ (RubyValue::Str(_) | RubyValue::Object(_)) = gets_value(recv, args)? {
            lines.push(line);
        }
        Ok(RubyValue::Array(crate::collections::array_new(lines)))
    }

    // `each_line`/`each([sep][, limit][, chomp:])` -- yield each line; bumps
    // lineno. Drains through `gets` for the same reason `readlines` does.
    def "each_line" | "each" cfunc (recv, _sep?, _limit?, **_opts, &blk) {
        let args = __args;
        let p = crate::builtins::block_or_enum!(recv, args, blk);
        while let line @ (RubyValue::Str(_) | RubyValue::Object(_)) = gets_value(recv, args)? {
            p.call(&[line])?;
        }
        Ok(recv.clone())
    }

    // `each_char` -- yield each UTF-8 char.
    def "each_char" (recv, &blk) {
        let p = crate::builtins::block_or_enum!(recv, __args, blk);
        loop {
            let ch = with_buffered_file(recv, |io, f, path| {
                read_one_char(io, f)
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "each_char", path))
            })?;
            match ch {
                Some(s) => p.call(&[RubyValue::Str(crate::collections::string_new(s))])?,
                None => break,
            };
        }
        Ok(recv.clone())
    }

    // `each_byte` -- yield each byte as an Integer.
    def "each_byte" (recv, &blk) {
        let p = crate::builtins::block_or_enum!(recv, __args, blk);
        let text = io_read_val(recv, &[], None)?;
        let RubyValue::Str(s) = &text else {
            return Ok(recv.clone());
        };
        let bytes = s.lock().bytes().to_vec();
        for b in bytes {
            p.call(&[RubyValue::Int(b as i64)])?;
        }
        Ok(recv.clone())
    }

    // `printf(fmt, *args)` -- format and write, answering nil.
    def "printf" cfunc (recv, fmt, *args, &_blk) {
        let s = crate::builtins::format::sprintf(&fmt.try_display_string()?, args)?;
        write_str(recv_io(recv)?, &s)?;
        Ok(RubyValue::Nil)
    }

    // `putc(int | str)` -- write one character (an Integer's low byte, or a
    // String's first character), answering the argument unchanged.
    def "putc" (recv, char, &_blk) {
        let bytes = putc_bytes(char)?;
        write_bytes(recv_io(recv)?, &bytes)?;
        Ok(char.clone())
    }

    // `readpartial(maxlen)` / `sysread(maxlen)` -- read up to `maxlen` bytes,
    // blocking for at least one; `EOFError` at EOF (unlike `read(n)`'s nil).
    def "readpartial" | "sysread" cfunc (recv, maxlen, outbuf?, &_blk) {
        check_readable(recv)?;
        let RubyValue::Int(max) = maxlen else {
            return Err(arg_error!("length must be an Integer"));
        };
        let max = *max;
        // A negative length is refused, not clamped to zero: clamping made
        // `f.readpartial(-1)` answer an empty String where ruby raises.
        if max < 0 {
            return Err(arg_error!("negative length {max} given"));
        }
        // The original argument rides along: CRuby answers the very object the
        // caller passed, not the String its `to_str` gave.
        let outbuf = match outbuf {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some((v, crate::builtins::convert::to_rstr(v)?)),
        };
        let bytes = with_file(recv, |f, path| {
            let mut buf = read_buffer(max.max(0) as usize)?;
            let mut got = peeked_read(as_rio(recv), f, &mut buf)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "read", path))?;
            // A SEEKABLE stream tops the answer up. CRuby's readpartial serves
            // whatever the buffer holds, and on a file that is a whole 8 KiB
            // of read-ahead -- so it short-reads only at EOF. zeo's buffer is
            // handed back to the descriptor at every `with_file`, so without
            // this a `getc; ungetc; readpartial(3)` answered ONE byte.
            //
            // A pipe or socket keeps the short read, which is the whole point
            // of readpartial there: it must answer as soon as anything
            // arrives rather than wait for the rest.
            if got < buf.len() && got > 0 && {
                use std::io::Seek;
                f.stream_position().is_ok()
            } {
                while got < buf.len() {
                    match peeked_read(as_rio(recv), f, &mut buf[got..]) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => got += k,
                    }
                }
            }
            buf.truncate(got);
            Ok(buf)
        })?;
        if bytes.is_empty() && max > 0 {
            // CRuby empties the buffer before raising, so a rescued EOF leaves no
            // stale bytes from the previous read.
            if let Some((_, buf)) = &outbuf {
                buf.lock().replace_utf8(String::new());
            }
            return Err(eof_error!("end of file reached"));
        }
        // BINARY, and the bytes exactly as read. `readpartial` and `sysread`
        // are byte reads: ruby tags what they answer ASCII-8BIT whatever the
        // stream's encoding is.
        //
        // This went through `String::from_utf8_lossy`, which turns every byte
        // no character claims into a three-byte replacement -- so a 559-byte
        // read answered 1,002 bytes of something else. rubygems digests a
        // `.gem`'s members through exactly this call, so every downloaded gem
        // failed its checksum and nothing could be installed.
        let read = crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT);
        // The second argument is an output BUFFER: CRuby fills it in place and
        // returns that same object, so the caller may read the bytes back out of
        // it or compare with `equal?`.
        match outbuf {
            Some((orig, buf)) => {
                let bytes = read.lock().bytes().to_vec();
                // The buffer keeps its own encoding -- see `read(n, buf)`.
                let mut g = buf.lock();
                let enc = g.encoding();
                g.replace_bytes(bytes, enc);
                drop(g);
                Ok(orig.clone())
            }
            None => Ok(RubyValue::Str(read)),
        }
    }

    // `read_nonblock(maxlen, outbuf = nil, exception: true)` -- one `read(2)` that
    // never waits. The descriptor is marked `O_NONBLOCK` first and LEFT that way,
    // as CRuby leaves it; every blocking row here already parks in `poll(2)` on
    // `EAGAIN`, so an ordinary `#gets` on the same handle still blocks.
    def "read_nonblock" params "len, buf = nil, exception: nil" (recv, maxlen, buffer?, **opts, &_blk) {
        check_readable(recv)?;
        let raises = nonblock_raises(opts);
        let max = convert::to_index(maxlen)?.max(0) as usize;
        let outbuf = match buffer {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(convert::to_rstr(v)?),
        };
        set_fd_nonblock(raw_fd(recv)?, true)?;
        let read = with_file(recv, |f, _path| {
            let mut buf = read_buffer(max)?;
            loop {
                match std::io::Read::read(f, &mut buf) {
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Ok(n) => {
                        buf.truncate(n);
                        return Ok(Ok(buf));
                    }
                    Err(e) => return Ok(Err(e)),
                }
            }
        })?;
        let bytes = match read {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                return would_block(false, raises, "read");
            }
            Err(e) => return Err(crate::builtins::file::raise_errno(&e, "read", "")),
        };
        if bytes.is_empty() && max > 0 {
            // CRuby empties the buffer before reporting EOF, so a rescued end
            // leaves no stale bytes from the previous read.
            if let Some(buf) = &outbuf {
                buf.lock().replace_utf8(String::new());
            }
            return if raises {
                Err(eof_error!("end of file reached"))
            } else {
                Ok(RubyValue::Nil)
            };
        }
        // TAG the bytes rather than decoding them: a byte count can land
        // mid-character, so binary is the only honest answer (as `#read(n)`).
        match outbuf {
            Some(buf) => {
                buf.lock().replace_bytes(bytes, crate::encoding::ASCII_8BIT);
                Ok(buffer.expect("outbuf is Some only when a buffer was passed").clone())
            }
            None => Ok(RubyValue::Str(crate::string_from_bytes(
                bytes,
                crate::encoding::ASCII_8BIT,
            ))),
        }
    }

    // `write_nonblock(string, exception: true)` -- one `write(2)` that never
    // waits, answering the count it managed. A partial write is the caller's to
    // resume, which is the whole point of the method.
    def "write_nonblock" params "buf, exception: nil" (recv, buffer, **opts, &_blk) {
        check_writable(recv)?;
        let raises = nonblock_raises(opts);
        // Every write path renders its argument with `to_s`
        // (`rb_obj_as_string`), not the `to_str` protocol -- this one asked
        // for `to_str` and refused an object that `IO#write` accepts.
        let bytes = match buffer {
            RubyValue::Str(s) => s.lock().bytes().to_vec(),
            other => other.try_display_string()?.into_bytes(),
        };
        set_fd_nonblock(raw_fd(recv)?, true)?;
        let wrote = with_file(recv, |f, _path| {
            loop {
                match std::io::Write::write(f, &bytes) {
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    other => return Ok(other),
                }
            }
        })?;
        match wrote {
            Ok(n) => Ok(RubyValue::Int(n as i64)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => would_block(true, raises, "write"),
            Err(e) => Err(crate::builtins::file::raise_errno(&e, "write", "")),
        }
    }

    // `seek(offset, whence = IO::SEEK_SET)` -- answers 0, like real Ruby.
    def "seek" cfunc (recv, offset, whence?, &_blk) {
        let off = offset_of(offset)?;
        let whence = match whence {
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
        })?;
        drop_unget(recv);
        Ok(RubyValue::Int(0))
    }

    // `sysseek(offset, whence = SEEK_SET)` -- seek, answering the new absolute
    // position (unlike `seek`, which answers 0).
    def "sysseek" cfunc (recv, offset, whence?, &_blk) {
        let off = offset_of(offset)?;
        let whence = match whence {
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

    def "tell" | "pos" (recv, &_blk) {
        // `fptr->pos` minus what has been pushed back but not handed out.
        // `with_file` gives the ordinary read-ahead back to the descriptor on
        // its way in, so only the `#ungetc` bytes are left to subtract -- and
        // they are read AFTER that, not before, or they count twice.
        with_file(recv, |f, path| {
            use std::io::Seek;
            let p = f
                .stream_position()
                .map_err(|e| crate::builtins::file::raise_errno(&e, "tell", path))?;
            let pushed = as_rio(recv).map_or(0, |io| io.rbuf.lock().pending()) as i64;
            Ok(RubyValue::Int((p as i64 - pushed).max(0)))
        })
    }

    // `pos=` -- seek to an absolute byte offset.
    def "pos=" (recv, pos, &_blk) {
        let n = offset_of(pos)?;
        with_file(recv, |f, path| {
            use std::io::Seek;
            f.seek(std::io::SeekFrom::Start(n.max(0) as u64))
                .map_err(|e| crate::builtins::file::raise_errno(&e, "pos=", path))?;
            Ok(RubyValue::Int(n))
        })
    }

    def "rewind" (recv, &_blk) {
        with_file(recv, |f, path| {
            use std::io::Seek;
            f.rewind()
                .map_err(|e| crate::builtins::file::raise_errno(&e, "rewind", path))?;
            Ok(RubyValue::Int(0))
        })?;
        // A seek DISCARDS anything pushed back: `unread` gave the read-ahead
        // to the descriptor, but a pushed-back byte has no descriptor to go
        // to, and ruby drops it here.
        drop_unget(recv);
        Ok(RubyValue::Int(0))
    }

    // Two implementations, because there are two kinds of descriptor.
    //
    // A SEEKABLE one compares the position against the end, which is exact
    // and costs nothing. On a PIPE or SOCKET that seek is `ESPIPE`, so
    // those PEEK one byte instead and park it where the readers already
    // look.
    def "eof?" | "eof" (recv, &_blk) {
        check_readable(recv)?;
        // stdin can't seek; peek the shared buffered reader instead. `fill_buf`
        // is non-destructive -- an empty buffer means end-of-input.
        if matches!(stream_of(recv), Some(StdStream::Stdin)) {
            use std::io::BufRead;
            // `fill_buf` on an empty buffer BLOCKS for the next chunk of input.
            let empty = crate::gvl::without_gvl(|| {
                std::io::stdin()
                    .lock()
                    .fill_buf()
                    .map(|b| b.is_empty())
                    .unwrap_or(true)
            });
            return Ok(RubyValue::Bool(empty));
        }
        with_buffered_file(recv, |io, f, path| {
            // Anything already read ahead means there IS more to come, on
            // either kind of descriptor.
            if io.rbuf.lock().pending() > 0 {
                return Ok(RubyValue::Bool(false));
            }
            use std::io::Seek;
            if let Ok(pos) = f.stream_position() {
                let end = f
                    .seek(std::io::SeekFrom::End(0))
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "eof?", path))?;
                // Peeking at the end MOVES the position -- put it back.
                f.seek(std::io::SeekFrom::Start(pos))
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "eof?", path))?;
                return Ok(RubyValue::Bool(pos >= end));
            }
            // Not seekable: read one byte and keep it. `peeked_read` hands it
            // to whichever reader comes next, so nothing is lost.
            let mut one = [0u8; 1];
            let got = blocking_read(f, &mut one)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "eof?", path))?;
            if got == 1 {
                let mut buf = io.rbuf.lock();
                let consumed = buf.pos;
                buf.data.drain(..consumed);
                buf.pos = 0;
                buf.data.push(one[0]);
            }
            Ok(RubyValue::Bool(got == 0))
        })
    }

    // `#stat` -- an `fstat(2)` snapshot of the open descriptor as a `File::Stat`.
    def "stat" (recv, &_blk) {
        stat_value(recv)
    }

    // `#fcntl(cmd[, arg])` -- the raw `fcntl(2)`; answers its integer result
    // (e.g. `fcntl(F_GETFD)` reads the close-on-exec flag). `arg` defaults to 0.
    def "fcntl" cfunc (recv, cmd, arg?, &_blk) {
        let cmd = int_of(cmd)? as libc::c_int;
        let arg = match arg {
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

    // `pid` -- the child an `IO.popen` handle is connected to, `nil` for every
    // other IO (CRuby's split exactly).
    def "pid" (recv, &_blk) {
        match as_rio(recv).map(|io| io.child_pid.load(std::sync::atomic::Ordering::Relaxed)) {
            Some(pid) if pid != 0 => Ok(RubyValue::Int(pid)),
            _ => Ok(RubyValue::Nil),
        }
    }

    // `#fsync`/`#fdatasync` -- flush to disk; answers 0.
    def "fsync" | "fdatasync" (recv, &_blk) {
        with_file(recv, |f, path| {
            f.sync_all()
                .map_err(|e| crate::builtins::file::raise_errno(&e, "fsync", path))?;
            Ok(RubyValue::Int(0))
        })
    }

    // `close` -- idempotent (a second close is a no-op, as in Ruby), and it
    // DROPS the descriptor, so every later operation raises IOError. Closing an
    // `IO.popen` handle also waits for the child and sets `$?`, CRuby's contract
    // (the fd must drop FIRST -- a `w`-mode child only exits on stdin's EOF).
    def "close" (recv, &_blk) {
        close_io(recv)
    }

    // `#close_read` -- close the readable half. On a write-only stream CRuby
    // raises IOError; otherwise the stream is closed. Answers nil.
    def "close_read" (recv, &_blk) {
        if fd_access_mode(recv) == Some(libc::O_WRONLY) {
            return Err(io_error!("not opened for reading"));
        }
        if let Some(io) = as_rio(recv) {
            let mut b = io.backend.lock();
            if let IoBackend::Pipe(Some(f)) = &*b {
                use std::os::fd::AsRawFd;
                // The read-direction mirror of `close_write` above.
                // SAFETY: the fd is open, borrowed from the live backend.
                if unsafe { libc::shutdown(f.as_raw_fd(), libc::SHUT_RD) } == 0 {
                    return Ok(RubyValue::Nil);
                }
            }
            // The same close(2) `#close` performs, so it reports the same way.
            let closed = b.close_file();
            drop(b);
            closed.map_err(|e| crate::builtins::file::raise_bare_errno(&e))?;
        }
        Ok(RubyValue::Nil)
    }

    // `#close_write` -- close the writable half. On a read-only stream there is
    // none, so CRuby raises IOError; otherwise the stream is closed. Answers nil.
    def "close_write" (recv, &_blk) {
        if fd_access_mode(recv) == Some(libc::O_RDONLY) {
            return Err(io_error!("not opened for writing"));
        }
        if let Some(io) = as_rio(recv) {
            let mut b = io.backend.lock();
            if let IoBackend::Pipe(Some(f)) = &*b {
                use std::os::fd::AsRawFd;
                // A socket half-closes: `shutdown(2)` the write direction and keep
                // reading -- what `Socket#close_write` means, and what lets a
                // duplex `IO.popen` handle deliver EOF to the child while the
                // parent still reads its answer. ENOTSOCK (an ordinary pipe fd)
                // falls through to the full close.
                // SAFETY: the fd is open, borrowed from the live backend.
                if unsafe { libc::shutdown(f.as_raw_fd(), libc::SHUT_WR) } == 0 {
                    return Ok(RubyValue::Nil);
                }
            }
            // The same close(2) `#close` performs, so it reports the same way.
            let closed = b.close_file();
            drop(b);
            closed.map_err(|e| crate::builtins::file::raise_bare_errno(&e))?;
        }
        Ok(RubyValue::Nil)
    }

    def "closed?" (recv, &_blk) {
        // A handle that was never opened is neither open nor closed, and ruby
        // refuses the question rather than guessing an answer.
        if let Some(io) = as_rio(recv)
            && matches!(&*io.backend.lock(), IoBackend::Uninit)
        {
            return Err(io_error!("uninitialized stream"));
        }
        let closed = match as_rio(recv) {
            Some(io) => matches!(
                &*io.backend.lock(),
                IoBackend::File(None) | IoBackend::Pipe(None)
            ),
            None => false,
        };
        Ok(RubyValue::Bool(closed))
    }

    // `#binmode` -- binary mode. The newline half is a no-op on Unix, but the
    // ENCODING half is not: CRuby's `rb_io_binmode` sets the external
    // encoding to ASCII-8BIT, so everything the stream hands back is bytes.
    def "binmode" (recv, &_blk) {
        if let Some(io) = as_rio(recv) {
            io.binmode.store(true, Relaxed);
            let mut encs = io.encodings.lock();
            encs.0 = Some(crate::encoding::ASCII_8BIT);
            encs.1 = None;
        }
        Ok(recv.clone())
    }

    def "binmode?" (recv, &_blk) {
        Ok(RubyValue::Bool(
            as_rio(recv).is_some_and(|io| io.binmode.load(Relaxed)),
        ))
    }

    // `#external_encoding` -- the encoding this stream's bytes are read as.
    // Unset, a READABLE stream answers `Encoding.default_external`; a write-only
    // one answers nil, since nothing is being decoded.
    def "external_encoding" (recv, &_blk) {
        let Some(io) = as_rio(recv) else {
            return Ok(RubyValue::Nil);
        };
        if let Some(id) = io.encodings.lock().0 {
            return Ok(crate::builtins::encoding::encoding_value(id));
        }
        // CRuby's `rb_io_external_encoding` asks WRITABLE, not write-only:
        // a stream opened `"w+"` answers nil too, because nothing has said
        // what its bytes are to be read as.
        let writable = matches!(stream_of(recv), Some(StdStream::Stdout | StdStream::Stderr))
            || matches!(fd_access_mode(recv), Some(libc::O_WRONLY | libc::O_RDWR));
        Ok(if writable {
            RubyValue::Nil
        } else {
            crate::builtins::encoding::encoding_value(crate::encoding::default_external())
        })
    }

    // `#internal_encoding` -- what reads are transcoded TO. nil unless asked for,
    // which is the default for every stream.
    def "internal_encoding" (recv, &_blk) {
        Ok(match as_rio(recv).and_then(|io| io.encodings.lock().1) {
            Some(id) => crate::builtins::encoding::encoding_value(id),
            None => RubyValue::Nil,
        })
    }

    // `#set_encoding(ext[, int])` -- record the pair and answer the receiver. A
    // single `"EXT:INT"` string names both. zeo's IO reads bytes and tags them,
    // so this is what the tag comes from; no transcoding happens on the way in.
    def "set_encoding" cfunc (recv, external, internal?, &_blk) {
        let Some(io) = as_rio(recv) else {
            return Ok(recv.clone());
        };
        let parse = |v: &RubyValue| -> Result<Option<crate::encoding::EncodingId>, Signal> {
            match v {
                RubyValue::Nil => Ok(None),
                other => Ok(Some(crate::builtins::encoding::arg_encoding(other)?)),
            }
        };
        // The combined `"UTF-8:BINARY"` spelling, which only a String can carry.
        if internal.is_none()
            && let RubyValue::Str(sp) = external {
                let spec = sp.lock().to_utf8_lossy().into_owned();
                if let Some((ext, int)) = spec.split_once(':') {
                    let ext = crate::builtins::encoding::arg_encoding(&RubyValue::Str(
                        crate::string_new(ext.to_string()),
                    ))?;
                    let int = crate::builtins::encoding::arg_encoding(&RubyValue::Str(
                        crate::string_new(int.to_string()),
                    ))?;
                    *io.encodings.lock() = (Some(ext), Some(int));
                    return Ok(recv.clone());
                }
            }
        let ext = parse(external)?;
        let int = match internal {
            Some(v) => parse(v)?,
            None => None,
        };
        *io.encodings.lock() = (ext, int);
        Ok(recv.clone())
    }

    // `#autoclose = flag` -- answers the assigned value (Ruby setter convention).
    def "autoclose=" (recv, autoclose, &_blk) {
        set_autoclose(recv, autoclose);
        Ok(autoclose.clone())
    }

    def "autoclose?" (recv, &_blk) {
        Ok(RubyValue::Bool(
            as_rio(recv).is_none_or(|io| io.autoclose.load(Relaxed)),
        ))
    }

    // `#to_io` -- an IO answers itself.
    def "to_io" (recv, &_blk) {
        Ok(recv.clone())
    }

    // `#close_on_exec?` -- CRuby marks a newly-opened fd close-on-exec by default
    // (since Ruby 2.0), so this reports true; `#close_on_exec=` records the wish
    // and answers it (the flag has no observable effect without an exec here).
    def "close_on_exec?" (recv, &_blk) {
        let _ = recv;
        Ok(RubyValue::Bool(true))
    }

    def "close_on_exec=" (_recv, close_on_exec, &_blk) {
        Ok(close_on_exec.clone())
    }

    // `#advise(kind[, offset, len])` -- a hint to the kernel about access
    // patterns. Validated against the known symbols, then a no-op answering nil
    // (`posix_fadvise` is best-effort and unobservable from Ruby).
    def "advise" cfunc (recv, advice, _offset?, _len?, &_blk) {
        // Not a conversion site: CRuby's io_advise requires a bare Symbol
        // ("advice must be a Symbol", oracle-verified).
        let kind = match advice {
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

    // `#ungetbyte(int_or_str)` / `#ungetc(str)` -- push bytes back so the next
    // read returns them first.
    //
    // They go into the READ BUFFER, at the cursor, which is the one place
    // every reader already looks (`peeked_read`). A stack of their own was
    // drained only by `#getbyte`, so `f.ungetc("Z"); f.getc` answered the
    // stream's next character and the pushed byte was simply lost -- the
    // failure `peeked_read`'s own comment warns about.
    def "ungetbyte" | "ungetc" (recv, byte, &_blk) {
        check_readable(recv)?;
        let Some(io) = as_rio(recv) else {
            return Err(io_error!("not a file"));
        };
        let bytes: Vec<u8> = match byte {
            RubyValue::Nil => return Ok(RubyValue::Nil),
            RubyValue::Int(i) => vec![(*i & 0xff) as u8],
            RubyValue::Str(s) => s.lock().bytes().to_vec(),
            other => convert::to_rstr(other)?.lock().bytes().to_vec(),
        };
        let mut rb = io.rbuf.lock();
        let at = rb.pos;
        rb.unget += bytes.len();
        rb.data.splice(at..at, bytes);
        Ok(RubyValue::Nil)
    }

    // `#pread(maxlen, offset[, buffer])` -- read at a fixed offset WITHOUT moving
    // the position (`pread(2)`). Answers a new String, or fills `buffer` when
    // given and answers it. EOFError when nothing is available at `offset`.
    def "pread" cfunc (recv, maxlen, offset, buffer?, &_blk) {
        let count = int_of(maxlen)?.max(0) as usize;
        let offset = offset_of(offset)?.max(0) as u64;
        let data = with_file(recv, |f, path| {
            use std::os::unix::fs::FileExt;
            let mut buf = read_buffer(count)?;
            let n = f
                .read_at(&mut buf, offset)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "pread", path))?;
            if n == 0 && count > 0 {
                return Err(eof_error!("end of file reached"));
            }
            buf.truncate(n);
            Ok(buf)
        })?;
        // Binary, and the bytes as read -- a byte count can land mid-character,
        // so decoding here would both change the length and lie about it.
        match buffer {
            Some(RubyValue::Str(buf)) => {
                // The buffer keeps its own encoding -- see `read(n, buf)`.
                let mut g = buf.lock();
                let enc = g.encoding();
                g.replace_bytes(data, enc);
                drop(g);
                Ok(RubyValue::Str(buf.clone()))
            }
            _ => Ok(RubyValue::Str(crate::string_from_bytes(
                data,
                crate::encoding::ASCII_8BIT,
            ))),
        }
    }

    // `#pwrite(string, offset)` -- write at a fixed offset WITHOUT moving the
    // position; answers the number of bytes written.
    def "pwrite" (recv, buffer, offset, &_blk) {
        let bytes = match buffer {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned().into_bytes(),
            other => other.try_display_string()?.into_bytes(),
        };
        let offset = offset_of(offset)?.max(0) as u64;
        with_file(recv, |f, path| {
            use std::os::unix::fs::FileExt;
            let n = f
                .write_at(&bytes, offset)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "pwrite", path))?;
            Ok(RubyValue::Int(n as i64))
        })
    }

    // `#reopen(other_io_or_path[, mode])` -- `rb_io_reopen`: the receiver KEEPS
    // its descriptor NUMBER and starts referring to the target, which is the
    // whole point (`$stderr.reopen(path)` redirects fd 2, so a child process
    // and a C-level write follow it too). So: open the target, `dup2` it onto
    // the receiver's fd, and drop the temporary.
    //
    // Opening the target FIRST is what the mode argument is for -- `"w"`
    // implies `O_CREAT|O_TRUNC`, so a missing path is created rather than
    // ENOENT. Answers self.
    def "reopen" cfunc (recv, target, mode?, &_blk) {
        use std::os::fd::AsRawFd;
        let Some(io) = as_rio(recv) else {
            return Ok(recv.clone());
        };
        // The IO-to-IO form duplicates the OTHER IO's live descriptor; the
        // path form opens one, honouring the mode.
        match as_rio(target) {
            Some(other) => {
                let path = other.path.lock().clone();
                let fd = other
                    .raw_fd()
                    .ok_or_else(|| crate::builtins::io_error!("closed stream"))?;
                io.dup2_from(fd, path)?;
            }
            None => {
                let path = crate::builtins::file::path_arg(target, "reopen")?;
                // With no mode of its own the call INHERITS the receiver's --
                // `File.open(x, "w").reopen(y)` writes `y`, where a default
                // of "r" would answer EBADF on the first write.
                let inherited = io.open_mode.lock().clone().map(|m| {
                    RubyValue::Str(crate::collections::string_new(m))
                });
                let mode = mode.or(inherited.as_ref());
                let f = crate::builtins::file::open_options_for(mode, None, None)?
                    .open(&path)
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "reopen", &path))?;
                io.dup2_from(f.as_raw_fd(), Some(path))?;
            }
        }
        Ok(recv.clone())
    }

    // `#each_codepoint { |cp| ... }` -- yield each remaining character's codepoint;
    // answers self.
    def "each_codepoint" (recv, &blk) {
        check_readable(recv)?;
        let p = crate::builtins::block_or_enum!(recv, __args, blk);
        let content = with_file(recv, |f, path| {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                match peeked_read(as_rio(recv), f, &mut chunk)
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "each_codepoint", path))?
                {
                    0 => break,
                    k => buf.extend_from_slice(&chunk[..k]),
                }
            }
            Ok(buf)
        })?;
        for ch in String::from_utf8_lossy(&content).chars() {
            p.call(&[RubyValue::Int(ch as i64)])?;
        }
        Ok(recv.clone())
    }

    // `IO.pipe` -- a `[reader, writer]` pair over a `pipe(2)`; each end is a plain
    // `IO`. With a block, yields the pair and closes both ends afterward.
    def self."pipe" (recv, _external?, _internal?, &block) {
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
        // Both ends start non-blocking, as CRuby's do -- `IO.pipe` there hands back
        // fds it has already marked, which `#nonblock?` reports -- and
        // close-on-exec, also as CRuby's (see `set_fd_cloexec`).
        for fd in fds {
            set_fd_nonblock(fd, true)?;
            set_fd_cloexec(fd);
        }
        // SAFETY: `pipe(2)` just handed us these two fresh, owned fds.
        let r = pipe_value(unsafe { std::fs::File::from_raw_fd(fds[0]) });
        let w = pipe_value(unsafe { std::fs::File::from_raw_fd(fds[1]) });
        // CRuby marks the WRITE end unbuffered, and only that end.
        if let Some(io) = as_rio(&w) {
            io.sync.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let pair = RubyValue::Array(crate::collections::array_new(vec![r.clone(), w.clone()]));
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(pair);
        };
        let out = p.call(&[pair]);
        let _ = close_io(&r);
        let _ = close_io(&w);
        let _ = recv; // `IO.pipe`'s receiver is unused
        out
    }

    // `IO.popen([env,] cmd, mode = "r" [, opts])` -- spawn `cmd` with the far end
    // of a pipe as its stdout (`"r"`), its stdin (`"w"`), or both (`"r+"`/`"w+"`),
    // answering the near end as an IO that knows its child: `#pid` answers the
    // child's, and `#close` reaps it into `$?`. `cmd` is a shell String or a
    // direct argv Array; env and options hashes ride through the spawn builder.
    // A duplex mode uses a `socketpair(2)` so ONE descriptor serves both
    // directions (the historical popen trick), keeping the handle on the
    // ordinary pipe backend. With a block, yields the IO and closes it after.
    def self."popen" cfunc (_recv, _command, _mode?, _opt?, _extra?, &block) {
        use std::os::fd::FromRawFd;
        use std::process::Stdio;

        let mut rest = __args;
        let mut spawn_args: Vec<RubyValue> = Vec::new();
        if let Some(env @ RubyValue::Hash(_)) = rest.first() {
            spawn_args.push(env.clone());
            rest = &rest[1..];
        }
        let cmd_arg = rest.first().ok_or_else(|| arg_error!("no command given"))?;
        rest = &rest[1..];
        match cmd_arg {
            // `IO.popen("-")` forks the interpreter itself -- there is no second
            // interpreter image to run in an AOT-compiled program.
            RubyValue::Str(s) if s.lock().to_utf8_lossy() == "-" => {
                return Err(not_impl_error!(
                    "IO.popen(\"-\") (fork) is not supported by zeo"
                ));
            }
            RubyValue::Array(a) => spawn_args.extend(a.lock().iter().cloned()),
            other => spawn_args.push(other.clone()),
        }
        let mode = match rest.first() {
            Some(RubyValue::Str(m)) => {
                rest = &rest[1..];
                m.lock().to_utf8_lossy().into_owned()
            }
            _ => "r".to_string(),
        };
        if let Some(opts @ RubyValue::Hash(_)) = rest.first() {
            spawn_args.push(opts.clone());
        }
        // "r+", "rb:UTF-8", ... -- only the direction matters on Unix.
        let mode = mode.split(':').next().unwrap_or("r");
        let duplex = mode.contains('+');
        let write = mode.starts_with('w') || mode.starts_with('a');

        let mut cmd = crate::builtins::process::build_spawn_command(&spawn_args)?;
        let io = if duplex {
            let mut fds = [0 as libc::c_int; 2];
            // SAFETY: `fds` is a 2-element array `socketpair(2)` fills.
            if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) } != 0 {
                return Err(crate::builtins::file::raise_errno(
                    &std::io::Error::last_os_error(),
                    "popen",
                    "",
                ));
            }
            // Close-on-exec like every fd this runtime creates; the child's copies
            // are re-opened by the exec-time `dup2` (see `set_fd_cloexec`).
            for fd in fds {
                set_fd_cloexec(fd);
            }
            // SAFETY: `socketpair(2)` just handed us these two fresh, owned fds.
            let parent = unsafe { std::fs::File::from_raw_fd(fds[0]) };
            // SAFETY: as above -- the child end, handed to the Command.
            let child_end = unsafe { std::fs::File::from_raw_fd(fds[1]) };
            cmd.stdin(Stdio::from(
                child_end
                    .try_clone()
                    .map_err(|e| crate::builtins::process::spawn_error(&e))?,
            ));
            cmd.stdout(Stdio::from(child_end));
            let child = cmd
                .spawn()
                .map_err(|e| crate::builtins::process::spawn_error(&e))?;
            popen_value(parent, i64::from(child.id()))
        } else {
            if write {
                cmd.stdin(Stdio::piped());
            } else {
                cmd.stdout(Stdio::piped());
            }
            let mut child = cmd
                .spawn()
                .map_err(|e| crate::builtins::process::spawn_error(&e))?;
            let f: std::fs::File = if write {
                std::os::fd::OwnedFd::from(child.stdin.take().expect("stdin was piped")).into()
            } else {
                std::os::fd::OwnedFd::from(child.stdout.take().expect("stdout was piped")).into()
            };
            popen_value(f, i64::from(child.id()))
        };
        // `Command` keeps the Stdio fds it was handed until it drops -- for the
        // duplex socketpair that is the child's own end, and holding it here would
        // deny the parent its EOF forever. The block form reads below, so drop NOW.
        drop(cmd);

        let Some(RubyValue::Proc(p)) = block else {
            return Ok(io);
        };
        let out = p.call(std::slice::from_ref(&io));
        // The close both drops the fd and reaps the child into `$?`, so the block
        // form leaves `$?` set even when the block never read to EOF.
        let _ = close_io(&io);
        out
    }

    // `IO.copy_stream(src, dst, copy_length = nil, src_offset = nil)` -- copy
    // `src` to `dst`, answering the byte count.
    //
    // Each end is either an IO-like object (read from / written to at its
    // current position, via `read`/`write`) or a filename (String/`to_path`).
    //
    // `copy_length` is what rubygems unpacks every `.gem` with
    // (`copy_stream(tar.io, out, entry.size)`); ignoring it would append
    // the rest of the archive to every extracted file.
    def self."copy_stream" cfunc (_recv, src, dst, copy_length?, src_offset?, &_blk) {
        // A negative length or offset is IGNORED, not refused: `-1` copies
        // the whole file and a negative offset reads from the start.
        let limit: Option<u64> = match copy_length {
            None | Some(RubyValue::Nil) => None,
            Some(v) => match offset_of(v)? {
                n if n < 0 => None,
                n => Some(n as u64),
            },
        };
        let offset: Option<u64> = match src_offset {
            None | Some(RubyValue::Nil) => None,
            Some(v) => match offset_of(v)? {
                n if n < 0 => None,
                n => Some(n as u64),
            },
        };
        // An offset needs a descriptor to `pread` at. CRuby refuses it for a
        // StringIO or any other duck by name, so the message is theirs.
        if offset.is_some() && reads_like_io(src) && !is_real_io(src) {
            return Err(arg_error!("cannot specify src_offset for non-IO"));
        }

        // The destination opens FIRST: `copy_stream(path, path)` truncates
        // before it reads, so a file copied onto itself answers 0.
        let mut sink = Sink::open(dst)?;
        let mut copied: u64 = 0;
        let mut remaining = limit;

        if reads_like_io(src) {
            // An offset reads through `pread`, which leaves the position
            // where it was.
            if let Some(mut at) = offset.filter(|_| is_real_io(src)) {
                loop {
                    let want = chunk_len(remaining);
                    if want == 0 {
                        break;
                    }
                    let got = with_file(src, |f, path| {
                        use std::os::unix::fs::FileExt;
                        let mut buf = vec![0u8; want];
                        let n = f.read_at(&mut buf, at).map_err(|e| {
                            crate::builtins::file::raise_errno(&e, "copy_stream", path)
                        })?;
                        buf.truncate(n);
                        Ok(buf)
                    })?;
                    if got.is_empty() {
                        break;
                    }
                    at += got.len() as u64;
                    copied += got.len() as u64;
                    remaining = remaining.map(|r| r - got.len() as u64);
                    sink.write(got)?;
                }
            } else {
                loop {
                    let want = chunk_len(remaining);
                    if want == 0 {
                        break;
                    }
                    let read = crate::dispatch::send_value(
                        src,
                        crate::Symbol::intern("read"),
                        &[RubyValue::Int(want as i64)],
                        None,
                    )?;
                    let got = match read {
                        // `read(n)` answers nil at EOF, which is the loop's end.
                        RubyValue::Nil => break,
                        RubyValue::Str(s) => s.lock().bytes().to_vec(),
                        other => crate::builtins::convert::to_rstr(&other)?
                            .lock()
                            .bytes()
                            .to_vec(),
                    };
                    if got.is_empty() {
                        break;
                    }
                    copied += got.len() as u64;
                    remaining = remaining.map(|r| r.saturating_sub(got.len() as u64));
                    sink.write(got)?;
                }
            }
        } else {
            let path = crate::builtins::file::path_arg(src, "copy_stream")?;
            // `rb_sysopen`, not `copy_stream`: CRuby names the OPEN in the
            // Errno message, and a caller matching on it reads that name.
            let mut file = crate::gvl::without_gvl(|| std::fs::File::open(&path))
                .map_err(|e| crate::builtins::file::raise_errno(&e, "rb_sysopen", &path))?;
            if let Some(at) = offset {
                use std::io::Seek as _;
                crate::gvl::without_gvl(|| file.seek(std::io::SeekFrom::Start(at)))
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "copy_stream", &path))?;
            }
            loop {
                use std::io::Read as _;
                let want = chunk_len(remaining);
                if want == 0 {
                    break;
                }
                let got = crate::gvl::without_gvl(|| {
                    let mut buf = vec![0u8; want];
                    let n = (&mut file).take(want as u64).read(&mut buf)?;
                    buf.truncate(n);
                    Ok::<Vec<u8>, std::io::Error>(buf)
                })
                .map_err(|e| crate::builtins::file::raise_errno(&e, "copy_stream", &path))?;
                if got.is_empty() {
                    break;
                }
                copied += got.len() as u64;
                remaining = remaining.map(|r| r - got.len() as u64);
                sink.write(got)?;
            }
        }
        sink.finish()?;
        Ok(RubyValue::Int(copied as i64))
    }

    // `IO.sysopen(path, mode = "r", perm = 0o666)` -- open and answer the raw fd
    // Integer (the caller owns closing it). Same flag rules as `File.open`,
    // through the same helper, so an `IO.new(fd, "w")` over the result can
    // write.
    def self."sysopen" cfunc (_recv, path, mode?, perm?, &_blk) {
        use std::os::fd::IntoRawFd;
        let path = crate::builtins::file::path_arg(path, "sysopen")?;
        let opts = crate::builtins::file::open_options_for(mode, perm, None)?;
        // Gvl-released for the reason `File.open` releases it: open(2) blocks
        // on a FIFO with no peer.
        let f = crate::gvl::without_gvl(|| opts.open(&path))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "rb_sysopen", &path))?;
        Ok(RubyValue::Int(f.into_raw_fd() as i64))
    }

    // `IO.select(read, write, except, timeout = nil)` -- which of the given
    // handles are ready, as `[readable, writable, exceptional]`, or nil if the
    // timeout expires first. One `poll(2)` over every listed descriptor; a handle
    // may appear in more than one list and is polled once per appearance, so the
    // answer keeps each list's own order.
    def self."select" (_recv, read?, write?, error?, timeout?, &_blk) {
        let list = |v: Option<&RubyValue>| -> Result<Vec<RubyValue>, Signal> {
            match v {
                None | Some(RubyValue::Nil) => Ok(Vec::new()),
                Some(v) => Ok(crate::builtins::convert::to_rary(v)?.lock().to_vec()),
            }
        };
        let sets = [
            (list(read)?, libc::POLLIN),
            (list(write)?, libc::POLLOUT),
            (list(error)?, libc::POLLPRI),
        ];
        let timeout_ms = wait_timeout_ms(timeout)?;

        if sets.iter().all(|(ios, _)| ios.is_empty()) {
            // Nothing to watch: CRuby still honours the timeout, then answers nil.
            if timeout_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
            }
            return Ok(RubyValue::Nil);
        }
        let mut fds: [Vec<libc::c_int>; 3] = Default::default();
        for (i, (ios, _)) in sets.iter().enumerate() {
            for io in ios {
                fds[i].push(select_fd(io)?);
            }
        }
        let (r, w, e) = select_ready(&fds[0], &fds[1], &fds[2], timeout_ms)?;
        let mut any = false;
        let mut out = Vec::new();
        for ((ios, _), flags) in sets.iter().zip([r, w, e]) {
            let ready: Vec<RubyValue> = ios
                .iter()
                .zip(flags)
                .filter(|(_, ok)| *ok)
                .map(|(io, _)| io.clone())
                .collect();
            any |= !ready.is_empty();
            out.push(RubyValue::Array(crate::collections::array_new(ready)));
        }
        if !any {
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Array(crate::collections::array_new(out)))
    }

    // `IO.try_convert(obj)`: `obj` if it is already an IO, its `to_io` if it
    // defines one, else nil.
    def self."try_convert" (_recv, object, &_blk) {
        if as_rio(object).is_some() {
            return Ok(object.clone());
        }
        let sym = crate::Symbol::intern("to_io");
        if !crate::dispatch::responds_to_value(object, sym, true) {
            return Ok(RubyValue::Nil);
        }
        let answer = crate::dispatch::send_value(object, sym, &[], None)?;
        if answer.is_nil() || as_rio(&answer).is_some() {
            return Ok(answer);
        }
        Err(crate::builtins::type_error!(
            "can't convert {0} to IO ({0}#to_io gives {1})",
            crate::builtins::convert_name_of(object),
            crate::builtins::class_name_of(&answer)
        ))
    }

    // `IO.new` -- and, through the singleton class File inherits, `File.new`.
    //
    // CRuby defines this trio on IO ALONE (`io.c`'s `Init_IO`; the
    // `rb_define_singleton_method(rb_cFile, "open", ...)` beside it sits inside
    // an `#if 0` that exists only to make RDoc document `File::open`). The
    // RECEIVER then decides what the first argument means, because
    // `rb_io_s_new` calls `klass.new`, which reaches `rb_file_initialize` for a
    // File and `rb_io_initialize` for an IO. That one indirection is why
    // `File.method(:new).owner` answers `#<Class:IO>`, why the same method
    // reports `1..3` to File and `1..2` to IO, and why `Class.new(File).new`
    // answers an instance of the subclass.
    //
    // A block here is a mistake -- `new` never yields -- so CRuby warns and
    // hands back the handle anyway.
    def self."new" cfunc allocs (recv, *args, **opts!, &block) {
        if block.is_some() {
            // The RECEIVER's own name, not its class's -- CRuby's
            // `rb_obj_as_string(klass)`.
            let name = recv.to_display_string();
            crate::builtins::warning::rb_warn(&format!(
                "{name}::new() does not take block; use {name}::open() instead"
            ));
        }
        new_handle(recv, args, opts)
    }

    // `IO.open` / `File.open` -- `new`, plus the block form: the handle is
    // yielded and CLOSED afterwards no matter how the block leaves (return,
    // raise, break), answering the block's value. Without a block the caller
    // gets the open handle to close.
    def self."open" cfunc allocs (recv, *args, **opts!, &block) {
        let io = new_handle(recv, args, opts)?;
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(io);
        };
        // The close must happen on EVERY exit path, which is what makes this
        // the idiom it is -- run the block, stash its outcome, close, and only
        // then propagate.
        let out = p.call(std::slice::from_ref(&io));
        let _ = crate::dispatch::send_value(&io, crate::Symbol::intern("close"), &[], None);
        out
    }

    // `IO.for_fd` is `IO.new`'s DESCRIPTOR form on any receiver: CRuby's
    // `rb_io_s_for_fd` allocates the receiver and then calls `rb_io_initialize`
    // by name rather than dispatching, so `File.for_fd(fd)` reads a descriptor
    // where `File.new(fd)` would still be asking File what its argument means.
    def self."for_fd" cfunc allocs (recv, *args, **opts!, &_block) {
        crate::builtins::check_arity(args.len(), 1, Some(2))?;
        let io = io_from_fd(&args[0], args.get(1), opts)?;
        tag_receiver_class(&io, recv);
        Ok(io)
    }

    // Re-init rebinds this handle to `fd` (+ an optional mode string and
    // opts), the same descriptor adoption `IO.new` performs, and the
    // per-handle state (lineno, pushed-back bytes, read-ahead, encodings)
    // starts over. The previous descriptor is RELEASED, not closed -- re-init
    // closes nothing -- and the File-vs-pipe shape is kept so `#class` stays
    // what it was.
    private def "initialize" cfunc (recv, fd, mode?, **opts!, &_block) {
        use std::os::fd::FromRawFd;
        let Some(io) = as_rio(recv) else {
            return Err(crate::builtins::type_error!("not an IO"));
        };
        let fd = convert::to_index(fd)?;
        if unsafe { libc::fcntl(fd as libc::c_int, libc::F_GETFD) } < 0 {
            return Err(crate::dispatch::raise_error(
                "Errno::EBADF",
                "Bad file descriptor".to_string(),
            ));
        }
        // Same two arguments `IO.new` takes, read before the swap for the same
        // reason -- a refusal must leave the handle on its old descriptor.
        let o = open_opts(mode, opts)?;
        check_fd_access(fd, &o)?;
        // SAFETY: the fd was just confirmed open, and re-init adopts it.
        let f = unsafe { std::fs::File::from_raw_fd(fd as libc::c_int) };
        {
            let mut b = io.backend.lock();
            let was_file = matches!(&*b, IoBackend::File(_));
            b.release_file();
            *b = if was_file {
                IoBackend::File(Some(f))
            } else {
                IoBackend::Pipe(Some(f))
            };
        }
        reset_handle_state(io);
        if let Some(p) = &o.path {
            *io.path.lock() = Some(p.clone());
        }
        apply_open_opts(recv, &o)?;
        Ok(recv.clone())
    }

    // `#initialize_copy` -- adopt a `dup(2)` of the other handle's descriptor
    // (the two share a file position, as CRuby's `IO#dup` pair does). A
    // std-stream source stays a std-stream handle: zeo's std streams are
    // positionless globals, not descriptors to duplicate.
    private def "initialize_copy"(recv, other) {
        use std::os::fd::{AsRawFd, FromRawFd};
        let Some(io) = as_rio(recv) else {
            return Err(crate::builtins::type_error!("not an IO"));
        };
        let Some(src) = as_rio(other) else {
            return Err(crate::builtins::type_error!(
                "initialize_copy should take same class object"
            ));
        };
        let dup_raw = |fd: libc::c_int| -> Result<std::fs::File, Signal> {
            let fd = unsafe { libc::dup(fd) };
            if fd < 0 {
                return Err(crate::builtins::file::raise_errno(
                    &std::io::Error::last_os_error(),
                    "dup",
                    "",
                ));
            }
            // SAFETY: `dup(2)` just handed us this descriptor to own.
            Ok(unsafe { std::fs::File::from_raw_fd(fd) })
        };
        let dup_slot = |slot: &Option<std::fs::File>| -> Result<Option<std::fs::File>, Signal> {
            let Some(f) = slot else {
                return Err(crate::builtins::io_error!("closed stream"));
            };
            dup_raw(f.as_raw_fd()).map(Some)
        };
        let new_backend = match &*src.backend.lock() {
            // A std stream is a NUMBER, not a descriptor this process owns, so
            // copying the backend produced a second handle that WAS fd 1 --
            // and `$stdout.dup` exists precisely to save the old description
            // before a `reopen` replaces it. `orgout = $stdout.dup;
            // $stdout.reopen(log); $stdout.reopen(orgout)` then restored fd 1
            // from itself, `dup2(1, 1)`, and every later write stayed in the
            // log. mkmf's `Logging.open` is that exact sequence, which is why
            // a gem's build progress vanished into its own mkmf.log.
            //
            // `dup(2)` gives what ruby gives: a plain IO on a fresh descriptor
            // over the same open file description. `Pipe` is the backend for
            // that -- a real fd, `IO` for `#class`, and no path.
            IoBackend::Std(s) => {
                let fd = match s {
                    StdStream::Stdin => 0,
                    StdStream::Stdout => 1,
                    StdStream::Stderr => 2,
                };
                IoBackend::Pipe(Some(dup_raw(fd)?))
            }
            IoBackend::File(slot) => IoBackend::File(dup_slot(slot)?),
            IoBackend::Pipe(slot) => IoBackend::Pipe(dup_slot(slot)?),
            IoBackend::Uninit => IoBackend::Uninit,
        };
        {
            let mut b = io.backend.lock();
            b.release_file();
            *b = new_backend;
        }
        reset_handle_state(io);
        io.lineno.store(
            src.lineno.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        io.binmode.store(
            src.binmode.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        io.sync.store(
            src.sync.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        *io.encodings.lock() = *src.encodings.lock();
        Ok(recv.clone())
    }

    // `io/console`'s additions to IO. The rows are declared here, with the rest
    // of IO's surface, because one class owns one table; the termios work they
    // stand on earns its own file.
    //
    // The `require` is ceremony -- the rows stand whether or not a program
    // writes one, which is what `gated` says. The `cfg` is a different
    // question: it asks whether the ext was COMPILED IN, and without it
    // `crate::ext::io_console` does not exist to call.

    #[cfg(feature = "ext-io-console")]
    def "winsize=" gated "io/console" (recv, _size) {
        crate::ext::io_console::winsize_set(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "raw" gated "io/console" (recv, *_args) {
        crate::ext::io_console::raw(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "raw!" gated "io/console" (recv, *_args) {
        crate::ext::io_console::raw_bang(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "cooked" gated "io/console" (recv) {
        crate::ext::io_console::cooked(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "cooked!" gated "io/console" (recv) {
        crate::ext::io_console::cooked_bang(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "echo?" gated "io/console" (recv) {
        crate::ext::io_console::echo_p(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "echo=" gated "io/console" (recv, _echo) {
        crate::ext::io_console::echo_set(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "noecho" gated "io/console" (recv) {
        crate::ext::io_console::noecho(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "getch" gated "io/console" (recv, *_args) {
        crate::ext::io_console::getch(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "getpass" gated "io/console" (recv, *_args) {
        crate::ext::io_console::getpass(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "input_pending?" gated "io/console" (recv) {
        crate::ext::io_console::input_pending_p(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "iflush" gated "io/console" (recv) {
        crate::ext::io_console::iflush(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "oflush" gated "io/console" (recv) {
        crate::ext::io_console::oflush(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "ioflush" gated "io/console" (recv) {
        crate::ext::io_console::ioflush(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "ttyname" gated "io/console" (recv) {
        crate::ext::io_console::ttyname(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "console_mode" gated "io/console" (recv) {
        crate::ext::io_console::console_mode(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "console_mode=" gated "io/console" (recv, _mode) {
        crate::ext::io_console::console_mode_set(recv, __args, __block)
    }

    // `rb_f_notimplement` raises before any arity check, so BOTH of these
    // take whatever they are given and always refuse. Measured: `pressed?`
    // with 0, 1 and 2 arguments is NotImplementedError every time, and
    // `respond_to?` answers false for both (see `reflect::responds_to_value`).
    #[cfg(feature = "ext-io-console")]
    def "pressed?" gated "io/console" (recv, *_args) {
        crate::ext::io_console::pressed_p(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "check_winsize_changed" gated "io/console" (recv, *_args) {
        crate::ext::io_console::check_winsize_changed(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "beep" gated "io/console" (recv) {
        crate::ext::io_console::beep(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "clear_screen" gated "io/console" (recv) {
        crate::ext::io_console::clear_screen(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "erase_line" gated "io/console" (recv, _mode) {
        crate::ext::io_console::erase_line(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "erase_screen" gated "io/console" (recv, _mode) {
        crate::ext::io_console::erase_screen(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "goto" gated "io/console" (recv, _line, _column) {
        crate::ext::io_console::goto(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "goto_column" gated "io/console" (recv, _column) {
        crate::ext::io_console::goto_column(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "cursor" gated "io/console" (recv) {
        crate::ext::io_console::cursor(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "cursor=" gated "io/console" (recv, _position) {
        crate::ext::io_console::cursor_set(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "cursor_up" gated "io/console" (recv, _n) {
        crate::ext::io_console::cursor_up(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "cursor_down" gated "io/console" (recv, _n) {
        crate::ext::io_console::cursor_down(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "cursor_left" gated "io/console" (recv, _n) {
        crate::ext::io_console::cursor_left(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "cursor_right" gated "io/console" (recv, _n) {
        crate::ext::io_console::cursor_right(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "scroll_forward" gated "io/console" (recv, _n) {
        crate::ext::io_console::scroll_forward(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def "scroll_backward" gated "io/console" (recv, _n) {
        crate::ext::io_console::scroll_backward(recv, __args, __block)
    }

    // `IO.console` -- the controlling terminal, from `io/console`. Variadic:
    // `IO.console(:close)` and `IO.console(meth, *args)` are both real forms.
    #[cfg(feature = "ext-io-console")]
    def self."console" cfunc gated "io/console" (recv, *_args) {
        crate::ext::io_console::io_class_console(recv, __args, __block)
    }

    // `io/console/size`'s two rows -- a separate require in ruby, so a
    // separate gate here. irb, debug and power_assert all reach for them.
    #[cfg(feature = "ext-io-console")]
    def self."console_size" cfunc gated "io/console/size" (recv) {
        crate::ext::io_console::io_class_console_size(recv, __args, __block)
    }

    #[cfg(feature = "ext-io-console")]
    def self."default_console_size" cfunc gated "io/console/size" (recv) {
        crate::ext::io_console::io_class_default_console_size(recv, __args, __block)
    }

    // The whole-file family is identical to `File`'s -- run File's own rows so
    // the two class methods can never drift, in shape or in behavior.

    def self."read" cfunc (recv, _path, _length?, _offset?, _opt?) {
        file_class_row("read", recv, __args, __block)
    }

    def self."write" cfunc (recv, _path, _data, _offset?) {
        file_class_row("write", recv, __args, __block)
    }

    def self."binread" cfunc (recv, _path, _length?, _offset?) {
        file_class_row("binread", recv, __args, __block)
    }

    def self."binwrite" cfunc (recv, _path, _data, _offset?) {
        file_class_row("binwrite", recv, __args, __block)
    }

    def self."readlines" cfunc (recv, _path, _sep?, _opt?) {
        file_class_row("readlines", recv, __args, __block)
    }

    def self."foreach" cfunc (recv, _path, _sep?, _opt?) {
        file_class_row("foreach", recv, __args, __block)
    }
}

/// The `IO::SEEK_*` and `IO::READABLE`/`WRITABLE`/`PRIORITY` constants --
/// seeded from generated `main()` beside the stdio ones. The open/lock flags
/// are NOT here: those are `File::Constants`, which `IO` includes.
pub fn seed_io_constants() {
    let io = zeo_abi::IO_CLASS.0;
    crate::constants::const_set(io, "SEEK_SET", RubyValue::Int(0));
    crate::constants::const_set(io, "SEEK_CUR", RubyValue::Int(1));
    crate::constants::const_set(io, "SEEK_END", RubyValue::Int(2));
    // Sparse-file seeks. Darwin has no `SEEK_HOLE`/`SEEK_DATA` in libc, so the
    // values are CRuby's own (`io.c` defines them unconditionally).
    crate::constants::const_set(io, "SEEK_HOLE", RubyValue::Int(3));
    crate::constants::const_set(io, "SEEK_DATA", RubyValue::Int(4));
    // The `IO#wait` event mask.
    crate::constants::const_set(io, "READABLE", RubyValue::Int(1));
    crate::constants::const_set(io, "PRIORITY", RubyValue::Int(2));
    crate::constants::const_set(io, "WRITABLE", RubyValue::Int(4));
}
