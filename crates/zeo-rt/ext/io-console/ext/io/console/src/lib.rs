//! `io/console` -- terminal modes (raw/cooked/echo), single-character reads,
//! and the cursor escapes, over `termios(3)`.
//!
//! CRuby ships this as `ext/io/console`, a require-gated extension, and this
//! directory keeps that path: `console.c` there, `src/lib.rs` here.
//!
//! The ROWS, though, cannot live here. `io/console` adds 34 methods to `IO`
//! itself, and one class owns one `ruby_class!` table -- so the declarations
//! sit in `io.rs` beside every other IO row, each marked `gated
//! "io/console"`, and forward to the bodies below. `require "io/console"`
//! opens them at its own document position. `io/console/size` is a second
//! file in ruby and carries a second gate. `IO#winsize` predates this module
//! and stays in `io.rs` whole.
//!
//! Every mode change goes through `in_mode`, which saves the terminal's
//! settings, applies a mutation, runs a body, and restores from a `Drop`
//! guard -- so a `raise` (or a `break`, or a panic) out of `io.raw { ... }`
//! can't leave the user's terminal in raw mode, which is the failure that
//! makes a shell unusable.
//!
//! A failed syscall raises with the errno IT set, through `sys_fail`, down to
//! CRuby's message split: the direct methods name the stream
//! (`"... - <STDIN>"`), the scoped ones don't, because CRuby reaches them
//! through a helper that has no name to report.
//!
//! Measured against io-console 0.8.2's own `console.c`, row by row.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::builtins::{io, local_jump_error, not_impl_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal, Symbol};
use zeo_macros::ruby_class;

/// io-console's `sys_fail`: the errno the failed call actually set, never a
/// fixed one. A tty ioctl against `/dev/null` sets `ENODEV`, against a closed
/// descriptor `EBADF`; reporting `ENOTTY` for all three told a caller the
/// stream was a pipe when it was not.
///
/// `name` is the stream label the direct methods report (`rb_syserr_fail_str`)
/// and the scoped ones omit (`rb_syserr_fail(error, 0)`) -- see the module
/// docs. A nameless stream (a pipe) reports no suffix either, so an empty
/// label reads as no label.
///
/// `method` labels the synthetic frame the raise is captured inside, so an
/// unrescued failure reads `in 'IO#raw!'` above `<main>` the way CRuby's does
/// -- these rows are C functions with no Ruby line of their own.
fn sys_fail(errno: i32, name: Option<&str>, method: &'static str) -> Signal {
    let _frame = crate::frames::synthetic_c_frame(method);
    let (class, text) =
        crate::builtins::file::errno_class_and_desc_of(&std::io::Error::from_raw_os_error(errno));
    let msg = match name.filter(|n| !n.is_empty()) {
        Some(n) => format!("{text} - {n}"),
        None => text,
    };
    raise_error(class, msg)
}

/// The errno the last libc call set.
fn last_errno() -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::ENOTTY)
}

/// The named form, for the direct methods -- `IO#winsize` (which lives in
/// `io.rs`) raises through here too.
pub(crate) fn not_a_terminal(recv: &RubyValue, method: &'static str) -> Signal {
    sys_fail(last_errno(), Some(&io::stream_label(recv)), method)
}

fn get_attr(fd: libc::c_int) -> Option<libc::termios> {
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` came from `#fileno` and `t` is a live, correctly-sized
    // termios for the call to fill.
    (unsafe { libc::tcgetattr(fd, &mut t) } == 0).then_some(t)
}

/// `TCSANOW` and a retry on `EINTR`, which is io-console's own `setattr`.
///
/// `TCSADRAIN` was wrong and it HUNG: it waits for the terminal's pending
/// output to be transmitted, so `pty.raw { ... }` blocked forever whenever
/// nobody was draining the other end.
fn set_attr(fd: libc::c_int, t: &libc::termios) -> bool {
    // SAFETY: `fd` came from `#fileno` and `t` is a live termios.
    while unsafe { libc::tcsetattr(fd, libc::TCSANOW, t) } != 0 {
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return false;
        }
    }
    true
}

/// Restores the terminal mode however the body leaves -- returned value,
/// Ruby exception, or panic.
struct ModeGuard {
    fd: libc::c_int,
    saved: libc::termios,
}

impl Drop for ModeGuard {
    fn drop(&mut self) {
        set_attr(self.fd, &self.saved);
    }
}

/// Run `body` with the terminal mode `apply` produces, then put the old mode
/// back. The `Errno::ENOTTY` this raises carries no stream name, matching
/// CRuby's scoped forms.
fn in_mode<T>(
    recv: &RubyValue,
    method: &'static str,
    apply: impl FnOnce(&mut libc::termios),
    body: impl FnOnce() -> Result<T, Signal>,
) -> Result<T, Signal> {
    let fd = io::raw_fd(recv)?;
    let saved = get_attr(fd).ok_or_else(|| sys_fail(last_errno(), None, method))?;
    let mut next = saved;
    apply(&mut next);
    if !set_attr(fd, &next) {
        return Err(sys_fail(last_errno(), None, method));
    }
    let _guard = ModeGuard { fd, saved };
    body()
}

/// Apply `mutate` to the stream's mode and leave it that way -- the `!`
/// methods and `echo=`. These DO name the stream in their `Errno::ENOTTY`.
fn set_mode(
    recv: &RubyValue,
    method: &'static str,
    mutate: impl FnOnce(&mut libc::termios),
) -> Result<RubyValue, Signal> {
    let fd = io::raw_fd(recv)?;
    let mut t = get_attr(fd).ok_or_else(|| not_a_terminal(recv, method))?;
    mutate(&mut t);
    if !set_attr(fd, &t) {
        return Err(not_a_terminal(recv, method));
    }
    Ok(recv.clone())
}

/// CRuby's `set_rawmode`: `cfmakeraw` minus the erase/kill echoes, then the
/// `min:`/`time:`/`intr:` options on top.
fn raw_mode(t: &mut libc::termios, opts: RawOpts) {
    // SAFETY: `t` is a live termios; `cfmakeraw` only rewrites its fields.
    unsafe { libc::cfmakeraw(t) };
    t.c_lflag &= !(libc::ECHOE | libc::ECHOK);
    // A NEGATIVE value is ignored, leaving `cfmakeraw`'s VMIN 1 / VTIME 0.
    // Assigning it wrapped to 255 through the `cc_t` cast, which made
    // `min: -1` a 255-byte read.
    if let Some(min) = opts.min.filter(|&m| m >= 0) {
        t.c_cc[libc::VMIN] = min as libc::cc_t;
    }
    if let Some(time) = opts.time.filter(|&s| s >= 0) {
        t.c_cc[libc::VTIME] = time as libc::cc_t;
    }
    if opts.intr {
        t.c_iflag |= libc::BRKINT;
        t.c_lflag |= libc::ISIG;
        t.c_oflag |= libc::OPOST;
    }
}

/// CRuby's `set_cookedmode` -- the canonical-line-discipline flags back on.
fn cooked_mode(t: &mut libc::termios) {
    t.c_iflag |= libc::BRKINT | libc::ISTRIP | libc::ICRNL | libc::IXON;
    t.c_oflag |= libc::OPOST;
    t.c_lflag |= libc::ECHO
        | libc::ECHOE
        | libc::ECHOK
        | libc::ECHONL
        | libc::ICANON
        | libc::ISIG
        | libc::IEXTEN;
}

fn echo_mode(t: &mut libc::termios, on: bool) {
    let bits = libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL;
    if on {
        t.c_lflag |= bits;
    } else {
        t.c_lflag &= !bits;
    }
}

/// `raw`/`raw!`/`getch`'s keyword options.
#[derive(Default, Clone, Copy)]
struct RawOpts {
    /// `VMIN` -- how many bytes a read must gather before returning.
    min: Option<i64>,
    /// `VTIME`, in DECIseconds. CRuby takes `time:` in seconds and multiplies
    /// by ten, so `time: 0.5` is five tenths.
    time: Option<i64>,
    /// Keep the signal-generating keys (`^C`) working inside raw mode.
    intr: bool,
}

/// Read `min:`/`time:`/`intr:` out of a trailing keyword Hash, and refuse
/// everything io-console's `rb_get_kwargs` refuses.
///
/// `min:` went through `num_to_f64_unchecked`, which PANICS on a String, so
/// `io.raw!(min: "x")` aborted the process where ruby raises TypeError. An
/// unknown key was silently dropped, and `intr:` accepted any value.
///
/// `method` names the ArgumentError's frame, as the other refusals here do.
fn raw_opts(args: &[RubyValue], method: &'static str) -> Result<RawOpts, Signal> {
    let mut opts = RawOpts::default();
    // `rb_check_arity(argc, 0, 0)` after the keywords come off: these rows
    // take NO positional argument. Dropping one silently sent `getch(:nope)`
    // into a blocking read where ruby refuses before touching the terminal.
    let positional = args.len() - usize::from(matches!(args.last(), Some(RubyValue::Hash(_))));
    if positional > 0 {
        let _frame = crate::frames::synthetic_c_frame(method);
        return Err(crate::builtins::arg_error!(
            "wrong number of arguments (given {positional}, expected 0)"
        ));
    }
    let Some(RubyValue::Hash(h)) = args.last() else {
        return Ok(opts);
    };
    let get = |name: &str| {
        let key = RubyValue::Symbol(Symbol::intern(name));
        match crate::hash_get(h, &key) {
            RubyValue::Nil => None,
            v => Some(v),
        }
    };
    // `rb_get_kwargs` with a positive optional count refuses a key it does
    // not name, reporting the FIRST one in the hash's own order.
    let RubyValue::Array(keys) = crate::collections::hash_keys(h) else {
        unreachable!("hash_keys answers an Array");
    };
    let unknown = keys.lock().iter().find(|k| {
        !matches!(k, RubyValue::Symbol(s)
            if matches!(s.name_str(), "min" | "time" | "intr"))
    }).cloned();
    if let Some(unknown) = unknown {
        let _frame = crate::frames::synthetic_c_frame(method);
        return Err(crate::builtins::arg_error!(
            "unknown keyword: {}",
            unknown.inspect_string()
        ));
    }
    // NUM2INT / NUM2DBL: a non-numeric raises TypeError, it does not abort.
    opts.min = get("min")
        .map(|v| int_arg(&v))
        .transpose()?;
    opts.time = get("time")
        .map(|v| {
            // `NUM2INT(rb_funcall(vtime, '*', 1, INT2FIX(10)))` -- a real
            // Ruby `*` send FIRST, so `time: "x"` multiplies the String and
            // then fails the integer conversion, which is where its message
            // comes from.
            let tenths =
                crate::dispatch::send_value(&v, Symbol::intern("*"), &[RubyValue::Int(10)], None)?;
            int_arg(&tenths)
        })
        .transpose()?;
    opts.intr = match get("intr") {
        None | Some(RubyValue::Bool(false)) => false,
        Some(RubyValue::Bool(true)) => true,
        Some(other) => {
            let _frame = crate::frames::synthetic_c_frame(method);
            return Err(crate::builtins::arg_error!(
                "true or false expected as intr: {}",
                other.to_display_string()
            ));
        }
    };
    Ok(opts)
}

/// Yield the receiver to the method's block, or raise as a bare `yield` in a
/// blockless method does -- CRuby's scoped console methods are `rb_yield`
/// calls and behave the same way.
fn yield_self(recv: &RubyValue, block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    match block {
        Some(RubyValue::Proc(p)) => p.call(std::slice::from_ref(recv)),
        // `rb_yield` with no block, which reports no `(yield)` suffix.
        _ => Err(local_jump_error!("no block given")),
    }
}

fn send0(recv: &RubyValue, name: &str) -> Result<RubyValue, Signal> {
    crate::dispatch::send_value(recv, Symbol::intern(name), &[], None)
}

/// Write `s` to the stream -- how every cursor/erase escape reaches the
/// terminal, and the reason they work on any IO rather than only a tty.
///
/// Answers the RECEIVER: every one of these rows ends `return io;` in
/// io-console, so `io.goto(1, 1).cursor_up(2)` chains.
fn write_str(recv: &RubyValue, s: String) -> Result<RubyValue, Signal> {
    let arg = RubyValue::Str(crate::collections::string_new(s));
    crate::dispatch::send_value(recv, Symbol::intern("write"), &[arg], None)?;
    Ok(recv.clone())
}

/// One `Integer` argument for the cursor/erase escapes.
fn int_arg(v: &RubyValue) -> Result<i64, Signal> {
    crate::builtins::convert::to_index(v)
}

/// `NUM2UINT`: an Integer that fits an `unsigned int`, refusing anything
/// outside `-2**31 ..= 2**32-1` the way io-console's coordinate rows do.
///
/// A plain `i64` arithmetic on the result panicked in debug and wrapped to a
/// malformed escape in release for `io.goto(2**63 - 1, 0)`.
fn num2uint(v: &RubyValue, method: &'static str) -> Result<u32, Signal> {
    let n = int_arg(v)?;
    let refuse = |side| {
        let _frame = crate::frames::synthetic_c_frame(method);
        crate::builtins::range_error!("integer {n} too {side} to convert to 'unsigned int'")
    };
    if n > i64::from(u32::MAX) {
        return Err(refuse("big"));
    }
    if n < i64::from(i32::MIN) {
        return Err(refuse("small"));
    }
    Ok(n as u32)
}

/// A ZERO-based coordinate as the escape's ONE-based one.
///
/// `NUM2UINT(y) + 1` in C, printed with `%d` -- so the sum wraps within
/// `unsigned int` and is then read back as SIGNED. `goto(-1, -1)` writes
/// `ESC[0;0H` and `cursor = [-2, -3]` writes `ESC[-1;-2H`, both of which this
/// reproduces exactly.
fn one_based(v: &RubyValue, method: &'static str) -> Result<i32, Signal> {
    Ok(num2uint(v, method)?.wrapping_add(1) as i32)
}

// -- the IO rows -------------------------------------------------------

pub fn raw(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let opts = raw_opts(args, "IO#raw")?;
    in_mode(
        recv,
        "IO#raw",
        |t| raw_mode(t, opts),
        || yield_self(recv, block),
    )
}

pub fn raw_bang(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let opts = raw_opts(args, "IO#raw!")?;
    set_mode(recv, "IO#raw!", |t| raw_mode(t, opts))
}

pub fn cooked(
    recv: &RubyValue,
    _args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    in_mode(recv, "IO#cooked", cooked_mode, || yield_self(recv, block))
}

pub fn cooked_bang(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    set_mode(recv, "IO#cooked!", cooked_mode)
}

pub fn echo_p(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let fd = io::raw_fd(recv)?;
    let t = get_attr(fd).ok_or_else(|| not_a_terminal(recv, "IO#echo?"))?;
    Ok(RubyValue::Bool(
        t.c_lflag & (libc::ECHO | libc::ECHONL) != 0,
    ))
}

pub fn echo_set(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let on = args[0].truthy();
    set_mode(recv, "IO#echo=", |t| echo_mode(t, on))?;
    // An assignment answers its right-hand side, not the receiver.
    Ok(args[0].clone())
}

pub fn noecho(
    recv: &RubyValue,
    _args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    in_mode(
        recv,
        "IO#noecho",
        |t| echo_mode(t, false),
        || yield_self(recv, block),
    )
}

/// `getch` -- one character, read with the terminal in raw mode. With
/// `min:`/`time:` it can answer nil, the read having timed out with nothing
/// gathered; that is the same nil `getc` answers at EOF.
pub fn getch(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let opts = raw_opts(args, "IO#getch")?;
    in_mode(
        recv,
        "IO#getch",
        |t| raw_mode(t, opts),
        || send0(recv, "getc"),
    )
}

/// `getpass(prompt = nil)` -- print the prompt, read a line with echo off,
/// then print the newline the user's Return didn't echo.
///
/// Three rules the first draft missed. The prompt and the closing newline go
/// to the WRITE half, and for `STDIN` that is `$stderr` -- writing them back
/// into stdin put a prompt where nobody could see it. The newline is written
/// from an `ensure`, so a failed read still ends the line. And the answer is
/// `chomp!(rb_default_rs)`, which strips a trailing `"\n"` IN PLACE and
/// leaves a `"\r"` alone; a plain `chomp` would make a new String and eat
/// `"\r\n"` too.
pub fn getpass(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    if args.len() > 1 {
        let _frame = crate::frames::synthetic_c_frame("IO#getpass");
        return Err(crate::builtins::arg_error!(
            "wrong number of arguments (given {}, expected 0..1)",
            args.len()
        ));
    }
    // `wio = rb_io_get_write_io(io); if (wio == io && io == rb_stdin) wio = rb_stderr;`
    let wio = match io::stream_of(recv) {
        Some(io::StdStream::Stdin) => io::stderr_value(),
        _ => recv.clone(),
    };
    if let Some(prompt) = args.first().filter(|v| !matches!(v, RubyValue::Nil)) {
        let s = crate::builtins::convert::to_rstr(prompt)?
            .lock()
            .to_utf8_lossy()
            .into_owned();
        write_str(&wio, s)?;
    }
    send0(&wio, "flush")?;
    let line = in_mode(
        recv,
        "IO#getpass",
        |t| echo_mode(t, false),
        || send0(recv, "gets"),
    );
    // The `rb_ensure`: the newline goes out however the read ended.
    let newline = write_str(&wio, "\n".to_string());
    let line = line?;
    newline?;
    match line {
        RubyValue::Nil => Ok(RubyValue::Nil),
        other => {
            let rs = RubyValue::Str(crate::collections::string_new("\n".to_string()));
            crate::dispatch::send_value(&other, Symbol::intern("chomp!"), &[rs], None)?;
            Ok(other)
        }
    }
}

fn flush_queue(
    recv: &RubyValue,
    method: &'static str,
    queue: libc::c_int,
) -> Result<RubyValue, Signal> {
    let fd = io::raw_fd(recv)?;
    // `if (tcflush(fd, ...)) sys_fail(io);` -- the flush's OWN failure is the
    // refusal. Discarding its result made a failed flush on a live terminal
    // silent.
    // SAFETY: a plain call on a descriptor this handle owns.
    if unsafe { libc::tcflush(fd, queue) } != 0 {
        return Err(not_a_terminal(recv, method));
    }
    Ok(recv.clone())
}

pub fn iflush(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    flush_queue(recv, "IO#iflush", libc::TCIFLUSH)
}

pub fn oflush(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    flush_queue(recv, "IO#oflush", libc::TCOFLUSH)
}

pub fn ioflush(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    flush_queue(recv, "IO#ioflush", libc::TCIOFLUSH)
}

/// `ttyname` -- the device path behind the stream, or nil when it isn't a
/// terminal.
///
/// The `isatty` test comes FIRST and is the only nil: past it, a failure is a
/// real one and raises with the errno and the call that set it. Reading a
/// NULL as nil swallowed every one of those. The answer is frozen, as
/// `rb_interned_str_cstr` makes it.
pub fn ttyname(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let fd = io::raw_fd(recv)?;
    // SAFETY: a plain query on a descriptor this handle owns.
    if unsafe { libc::isatty(fd) } != 1 {
        return Ok(RubyValue::Nil);
    }
    // `ttyname_r` into a fixed 1024-byte buffer, which is what io-console
    // starts with; the reentrant form is why this is not plain `ttyname`.
    // `c_char` is signed on x86-64 and aarch64 macOS and on x86-64 Linux, and
    // UNSIGNED on aarch64 Linux -- naming it explicitly keeps both building.
    let mut buf = [0 as libc::c_char; 1024];
    // SAFETY: `buf` is a live, correctly-sized array for the call to fill.
    let rc = unsafe { libc::ttyname_r(fd, buf.as_mut_ptr(), buf.len()) };
    if rc != 0 {
        return Err(sys_fail(rc, Some(&format!("ttyname_r({fd})")), "IO#ttyname"));
    }
    // SAFETY: `ttyname_r` answered 0, so `buf` holds a NUL-terminated path.
    let name = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    let s = crate::collections::string_new(name);
    let v = RubyValue::Str(s);
    crate::dispatch::send_value(&v, Symbol::intern("freeze"), &[], None)?;
    Ok(v)
}

/// `winsize = [rows, columns]`.
pub fn winsize_set(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // `rb_Array` first, then the length rule: io-console accepts only
    // `[rows, cols]` or `[rows, cols, xpixel, ypixel]`, and reports the count
    // it got. A String converts to a one-element Array and fails that rule,
    // which is why a non-Array reads as an arity error rather than a TypeError.
    let RubyValue::Array(dims) = crate::builtins::kernel::array_impl(&args[0..1])? else {
        unreachable!("Kernel#Array answers an Array");
    };
    let dims = dims.lock().clone();
    if dims.len() != 2 && dims.len() != 4 {
        return Err(crate::builtins::arg_error!(
            "wrong number of arguments (given {}, expected 2 or 4)",
            dims.len()
        ));
    }
    // `GetWriteFD` in io-console; zeo has no tied write half, so one fd
    // answers both (`rb_io_set_write_io`, `io.rs`).
    let fd = io::raw_fd(recv)?;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    // Each field is `NIL_P(m) ? 0 : NUM2UINT(m)`, truncated to the struct's
    // unsigned short.
    let field = |v: Option<&RubyValue>| -> Result<libc::c_ushort, Signal> {
        match v {
            None | Some(RubyValue::Nil) => Ok(0),
            // The narrowing to `unsigned short` is C's, but the REFUSAL is
            // `NUM2UINT`'s: `io.winsize = [2**33, 80]` silently set row 0.
            Some(v) => Ok(num2uint(v, "IO#winsize=")? as libc::c_ushort),
        }
    };
    ws.ws_row = field(dims.first())?;
    ws.ws_col = field(dims.get(1))?;
    ws.ws_xpixel = field(dims.get(2))?;
    ws.ws_ypixel = field(dims.get(3))?;
    // SAFETY: `fd` is this handle's descriptor and `ws` is a live winsize.
    if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &ws) } != 0 {
        return Err(not_a_terminal(recv, "IO#winsize="));
    }
    Ok(args[0].clone())
}

/// `console_mode` -- the current terminal settings, to be handed back to
/// `console_mode=` later.
pub fn console_mode(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let fd = io::raw_fd(recv)?;
    let t = get_attr(fd).ok_or_else(|| not_a_terminal(recv, "IO#console_mode"))?;
    Ok(RubyValue::Object(std::sync::Arc::new(ConsoleMode::new(t))))
}

pub fn console_mode_set(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mode = mode_of(&args[0])?;
    let saved = *mode.mode.lock();
    set_mode(recv, "IO#console_mode=", |t| *t = saved)?;
    Ok(args[0].clone())
}

/// `pressed?` and `check_winsize_changed` are Windows-only in CRuby too --
/// this is its own message, verbatim, not a zeo decline.
pub fn pressed_p(
    _recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let _frame = crate::frames::synthetic_c_frame("IO#pressed?");
    Err(not_impl_error!(
        "pressed?() function is unimplemented on this machine"
    ))
}

pub fn check_winsize_changed(
    _recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let _frame = crate::frames::synthetic_c_frame("IO#check_winsize_changed");
    Err(not_impl_error!(
        "check_winsize_changed() function is unimplemented on this machine"
    ))
}

// -- the cursor and erase escapes --------------------------------------
//
// These only WRITE, so they work on any stream and never raise ENOTTY -- the
// same as CRuby, where redirecting a program's output captures the escapes
// rather than failing.

/// `beep` -- a raw `write(2)` of the bell byte, not a buffered Ruby `write`.
///
/// io-console goes straight to the descriptor here, so the bell is not held
/// behind buffered output and the failure is the syscall's own errno. Sending
/// Ruby's `write` reported `IOError: not opened for writing` where ruby
/// reports `Errno::EBADF`.
pub fn beep(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let fd = io::raw_fd(recv)?;
    // SAFETY: one byte from a live local, to a descriptor this handle owns.
    if unsafe { libc::write(fd, c"\x07".as_ptr().cast(), 1) } < 0 {
        return Err(not_a_terminal(recv, "IO#beep"));
    }
    Ok(recv.clone())
}

pub fn clear_screen(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    write_str(recv, "\x1b[2J\x1b[1;1H".to_string())
}

/// CRuby's `mode_in_range`: `nil` is 0, and anything that is not an Integer
/// in `0..=high` is an ArgumentError naming the mode. Without this an
/// out-of-range number went out as a malformed escape.
fn mode_in_range(v: &RubyValue, high: i64, name: &str, method: &'static str) -> Result<i64, Signal> {
    let refuse = || {
        let _frame = crate::frames::synthetic_c_frame(method);
        crate::builtins::arg_error!("wrong {name} mode: {}", v.to_display_string())
    };
    match v {
        RubyValue::Nil => Ok(0),
        RubyValue::Int(n) if (0..=high).contains(n) => Ok(*n),
        _ => Err(refuse()),
    }
}

pub fn erase_line(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mode = mode_in_range(&args[0], 2, "line erase", "IO#erase_line")?;
    write_str(recv, format!("\x1b[{mode}K"))
}

pub fn erase_screen(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mode = mode_in_range(&args[0], 3, "screen erase", "IO#erase_screen")?;
    write_str(recv, format!("\x1b[{mode}J"))
}

/// `goto(line, column)` -- both zero-based, as CRuby's are, over an escape
/// whose own coordinates start at one.
pub fn goto(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let (row, col) = (
        one_based(&args[0], "IO#goto")?,
        one_based(&args[1], "IO#goto")?,
    );
    write_str(recv, format!("\x1b[{row};{col}H"))
}

pub fn goto_column(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    write_str(
        recv,
        format!("\x1b[{}G", one_based(&args[0], "IO#goto_column")?),
    )
}

/// The four relative moves, over CRuby's `console_move(io, y, x)`.
///
/// Two rules the plain `format!` got wrong: a ZERO distance writes NOTHING,
/// and a NEGATIVE one flips the direction rather than emitting a negative
/// number -- `cursor_up(-3)` moves three DOWN. `console_move` also flushes.
fn move_by(
    recv: &RubyValue,
    args: &[RubyValue],
    negative: char,
    positive: char,
) -> Result<RubyValue, Signal> {
    let n = int_arg(&args[0])?;
    if n == 0 {
        return Ok(recv.clone());
    }
    let letter = if n < 0 { negative } else { positive };
    write_str(recv, format!("\x1b[{}{letter}", n.unsigned_abs()))?;
    send0(recv, "flush")?;
    Ok(recv.clone())
}

pub fn cursor_up(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // `console_move(io, -n, 0)`: the sign is negated before the letter is
    // chosen, so a positive `n` goes UP.
    move_by(recv, args, 'B', 'A')
}

pub fn cursor_down(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    move_by(recv, args, 'A', 'B')
}

pub fn cursor_right(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    move_by(recv, args, 'D', 'C')
}

pub fn cursor_left(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    move_by(recv, args, 'C', 'D')
}

/// `console_scroll`: zero writes nothing, the sign picks the letter and the
/// magnitude is the count. No flush, unlike the four moves above.
fn scroll_by(
    recv: &RubyValue,
    args: &[RubyValue],
    negative: char,
    positive: char,
) -> Result<RubyValue, Signal> {
    let n = int_arg(&args[0])?;
    if n == 0 {
        return Ok(recv.clone());
    }
    let letter = if n < 0 { negative } else { positive };
    write_str(recv, format!("\x1b[{}{letter}", n.unsigned_abs()))
}

pub fn scroll_forward(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    scroll_by(recv, args, 'T', 'S')
}

pub fn scroll_backward(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // `console_scroll(io, -n)`: negated first, so a positive `n` scrolls back.
    scroll_by(recv, args, 'S', 'T')
}

/// `cursor` -- where the cursor is, as `[row, column]` zero-based. Asks the
/// terminal itself (the DSR escape) and parses the `ESC [ row ; col R` reply,
/// which is why it needs raw mode: the reply arrives on the INPUT side and
/// would otherwise be echoed and line-buffered.
pub fn cursor(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let opts = RawOpts {
        min: Some(1),
        ..RawOpts::default()
    };
    in_mode(
        recv,
        "IO#cursor",
        |t| raw_mode(t, opts),
        || {
            write_str(recv, "\x1b[6n".to_string())?;
            send0(recv, "flush")?;
            // `read_vt_response`: exactly two prefix BYTES, then digits and
            // `;` separators, ending at the first byte that is neither. A
            // stream that does not answer the escape is nil, not a guess
            // such as `[0, 0]`, "cursor at home", for a pipe.
            let byte = || -> Result<Option<u8>, Signal> {
                Ok(match send0(recv, "getbyte")? {
                    RubyValue::Int(b) => Some(b as u8),
                    _ => None,
                })
            };
            if byte()? != Some(0x1b) || byte()? != Some(b'[') {
                return Ok(RubyValue::Nil);
            }
            // The accumulator and the field list are BOUNDED. A Ruby program
            // can be the far end of the pty it queries, so the reply is
            // untrusted input: 20 digits overflowed the accumulator, and a
            // reply of nothing but `;` grew the Vec without limit.
            let mut fields: Vec<i64> = Vec::new();
            let mut num: i64 = 0;
            let mut term = None;
            while let Some(c) = byte()? {
                match c {
                    b';' if fields.len() < 8 => {
                        fields.push(num);
                        num = 0;
                    }
                    b';' => break,
                    b'0'..=b'9' => num = num.saturating_mul(10) + i64::from(c - b'0'),
                    _ => {
                        fields.push(num);
                        term = Some(c);
                        break;
                    }
                }
            }
            // Two coordinates and an `R` terminator, or nothing.
            if fields.len() != 2 || term != Some(b'R') {
                return Ok(RubyValue::Nil);
            }
            // The escape's coordinates are one-based; Ruby's are not.
            Ok(RubyValue::Array(crate::collections::array_new(vec![
                RubyValue::Int(fields[0] - 1),
                RubyValue::Int(fields[1] - 1),
            ])))
        },
    )
}

/// `cursor = [row, column]` -- `goto` by another spelling.
pub fn cursor_set(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // `rb_convert_type(cpos, T_ARRAY, "Array", "to_ary")` -- any object with
    // `to_ary` is a coordinate; a 1- or 3-element one is not.
    let RubyValue::Array(pos) = crate::builtins::convert::to_ary(&args[0])? else {
        unreachable!("to_ary answers an Array");
    };
    let pos = pos.lock().clone();
    if pos.len() != 2 {
        let _frame = crate::frames::synthetic_c_frame("IO#cursor=");
        return Err(crate::builtins::arg_error!("expected 2D coordinate"));
    }
    let (row, col) = (
        one_based(&pos[0], "IO#cursor=")?,
        one_based(&pos[1], "IO#cursor=")?,
    );
    write_str(recv, format!("\x1b[{row};{col}H"))
}

/// The one open `/dev/tty`, which `IO.console` hands back on every call --
/// io-console keeps it in `File::console` (or ractor-local storage) so
/// `IO.console.equal?(IO.console)` is true and a mode set through one call is
/// still there at the next.
static CONSOLE_DEV: std::sync::Mutex<Option<RubyValue>> = std::sync::Mutex::new(None);

/// `IO.console(*args)` -- the process's controlling terminal as a `File` on
/// `/dev/tty`, or nil when `/dev/tty` cannot be opened.
///
/// Three forms, all of them `console_dev`'s:
///   * `IO.console` answers the cached console, opening it on first use.
///   * `IO.console(:close)` closes it, drops the cache and answers nil.
///   * `IO.console(meth, *args)` sends `meth` to the console.
///
/// io-console's rule is to test the open alone -- not whether a standard
/// stream is a terminal -- so a program with piped stdio still reaches its
/// terminal, and one `/dev/tty` is cached rather than opened per call.
pub fn io_class_console(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // `Check_Type(argv[0], T_SYMBOL)` before anything else.
    let sym = match args.first() {
        None => None,
        Some(RubyValue::Symbol(s)) => Some(*s),
        Some(other) => {
            let _frame = crate::frames::synthetic_c_frame("IO.console");
            return Err(crate::builtins::type_error!(
                "wrong argument type {} (expected Symbol)",
                crate::builtins::convert_name_of(other)
            ));
        }
    };

    let mut slot = CONSOLE_DEV.lock().unwrap_or_else(|e| e.into_inner());
    // A console someone closed is dropped and reopened, as `console_dev` does.
    if slot.as_ref().is_some_and(io::io_is_closed) {
        *slot = None;
    }
    if sym.is_some_and(|s| s.name_str() == "close") && args.len() == 1 {
        if let Some(con) = slot.take() {
            drop(slot);
            crate::dispatch::send_value(&con, Symbol::intern("close"), &[], None)?;
        }
        return Ok(RubyValue::Nil);
    }
    if slot.is_none() {
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty");
        let Ok(f) = opened else {
            return Ok(RubyValue::Nil);
        };
        *slot = Some(io::file_value(f, Some("/dev/tty".to_string())));
    }
    let con = slot.clone().expect("opened just above");
    drop(slot);
    match sym {
        // `rb_f_send(argc, argv, con)` -- `IO.console(:winsize)`.
        Some(name) => crate::dispatch::send_value(&con, name, &args[1..], None),
        None => Ok(con),
    }
}

/// `IO.default_console_size` -- `io/console/size`'s fallback, straight out of
/// the environment: `[ENV["LINES"].to_i.nonzero? || 25, ENV["COLUMNS"]... || 80]`.
pub fn io_class_default_console_size(
    _recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // `String#to_i` stops at the first non-digit and answers 0 for junk, and
    // `nonzero?` turns that 0 into the default.
    // Read through the ENV object itself, so a program that assigned
    // `ENV["COLUMNS"]` this run is answered from the same place ruby reads.
    let from_env = |name: &str, fallback: i64| -> Result<i64, Signal> {
        let key = RubyValue::Str(crate::collections::string_new(name.to_string()));
        let got = crate::dispatch::send_value(
            &crate::builtins::env::env_value(),
            Symbol::intern("[]"),
            &[key],
            None,
        )?;
        let RubyValue::Str(s) = got else {
            return Ok(fallback);
        };
        let text = s.lock().to_utf8_lossy().into_owned();
        let digits: String = text
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        Ok(digits.parse::<i64>().ok().filter(|&n| n != 0).unwrap_or(fallback))
    };
    Ok(RubyValue::Array(crate::collections::array_new(vec![
        RubyValue::Int(from_env("LINES", 25)?),
        RubyValue::Int(from_env("COLUMNS", 80)?),
    ])))
}

/// `IO.console_size` -- `console.winsize`, falling back to the environment
/// when there is no console (`rescue NoMethodError`, which is what a nil
/// console raises).
pub fn io_class_console_size(
    recv: &RubyValue,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let con = io_class_console(recv, &[], None)?;
    if matches!(con, RubyValue::Nil) {
        return io_class_default_console_size(recv, args, blk);
    }
    crate::dispatch::send_value(&con, Symbol::intern("winsize"), &[], None)
}

// -- IO::ConsoleMode ---------------------------------------------------

/// A saved terminal mode. Not constructible from Ruby -- CRuby's has no
/// `initialize` either, so the only way to hold one is `IO#console_mode`.
pub struct ConsoleMode {
    mode: parking_lot::Mutex<libc::termios>,
    frozen: AtomicBool,
}

impl ConsoleMode {
    fn new(mode: libc::termios) -> ConsoleMode {
        ConsoleMode {
            mode: parking_lot::Mutex::new(mode),
            frozen: AtomicBool::new(false),
        }
    }
}

impl RubyObject for ConsoleMode {
    fn class_id(&self) -> zeo_abi::ClassId {
        zeo_abi::CONSOLE_MODE_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Acquire)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Release);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = ConsoleMode::new(*self.mode.lock());
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        std::sync::Arc::new(d)
    }
}

fn mode_of(v: &RubyValue) -> Result<&ConsoleMode, Signal> {
    match v {
        RubyValue::Object(o) => o.as_any().downcast_ref::<ConsoleMode>(),
        _ => None,
    }
    // `TypedData_Get_Struct`'s own wording, down to the wrap_struct_name.
    .ok_or_else(|| {
        crate::builtins::type_error!(
            "wrong argument type {} (expected console-mode)",
            crate::builtins::convert_name_of(v)
        )
    })
}

ruby_class! {
    ConsoleMode = zeo_abi::CONSOLE_MODE_CLASS < zeo_abi::OBJECT_CLASS;

    // The three editors CRuby gives a saved mode, so a caller can restore a
    // MODIFIED version of what it captured.
    //
    // `raw` and `raw!` are NOT the same row: `conmode_raw_new` applies the
    // mutation to a COPY and answers a new ConsoleMode, leaving the receiver
    // the mode it captured. Writing both as the in-place form destroyed the
    // saved mode the caller meant to restore.
    def "raw" (recv, *args, &_block) {
        let opts = raw_opts(args, "IO::ConsoleMode#raw")?;
        let mut t = *mode_of(recv)?.mode.lock();
        raw_mode(&mut t, opts);
        Ok(RubyValue::Object(std::sync::Arc::new(ConsoleMode::new(t))))
    }
    def "raw!" (recv, *args, &_block) {
        let opts = raw_opts(args, "IO::ConsoleMode#raw!")?;
        raw_mode(&mut mode_of(recv)?.mode.lock(), opts);
        Ok(recv.clone())
    }
    def "echo=" (recv, arg) {
        echo_mode(&mut mode_of(recv)?.mode.lock(), (*arg).truthy());
        Ok(recv.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pipe is never a terminal, so it is the portable stand-in for the
    /// not-a-tty path -- the one a test can rely on wherever it runs.
    fn a_pipe() -> RubyValue {
        let mut fds = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        use std::os::fd::FromRawFd;
        io::file_value(unsafe { std::fs::File::from_raw_fd(fds[0]) }, None)
    }

    /// Whether the row refused. Raising is registry-backed, and a bare unit
    /// test has no registry, so the raise arrives as a panic rather than an
    /// `Err` -- either one is the refusal being asserted.
    fn refuses(f: crate::builtins::BuiltinMethodFn, recv: &RubyValue) -> bool {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(recv, &[], None))) {
            Err(_) => true,
            Ok(r) => r.is_err(),
        }
    }

    #[test]
    fn a_non_terminal_raises_rather_than_pretending() {
        let pipe = a_pipe();
        for f in [raw_bang, cooked_bang, echo_p, iflush, oflush, ioflush] {
            assert!(refuses(f, &pipe), "a pipe has no terminal mode");
        }
    }

    #[test]
    fn ttyname_is_nil_for_a_non_terminal() {
        assert!(matches!(ttyname(&a_pipe(), &[], None), Ok(RubyValue::Nil)));
    }

    #[test]
    fn raw_mode_clears_canonical_input_and_echo() {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        t.c_lflag = libc::ICANON | libc::ECHO | libc::ISIG;
        raw_mode(&mut t, RawOpts::default());
        assert_eq!(t.c_lflag & libc::ICANON, 0);
        assert_eq!(t.c_lflag & libc::ECHO, 0);
        assert_eq!(t.c_lflag & libc::ISIG, 0, "signals are off unless intr:");
    }

    #[test]
    fn intr_keeps_the_signal_keys_alive() {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        raw_mode(
            &mut t,
            RawOpts {
                intr: true,
                ..RawOpts::default()
            },
        );
        assert_ne!(t.c_lflag & libc::ISIG, 0);
        assert_ne!(t.c_oflag & libc::OPOST, 0);
    }

    #[test]
    fn min_and_time_land_in_the_control_characters() {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        raw_mode(
            &mut t,
            RawOpts {
                min: Some(3),
                // `time:` is seconds; VTIME counts tenths.
                time: Some(5),
                intr: false,
            },
        );
        assert_eq!(t.c_cc[libc::VMIN], 3);
        assert_eq!(t.c_cc[libc::VTIME], 5);
    }

    #[test]
    fn raw_opts_reads_the_keyword_hash_and_scales_time() {
        let h = RubyValue::Hash(crate::collections::hash_new(vec![
            (RubyValue::Symbol(Symbol::intern("min")), RubyValue::Int(2)),
            (
                RubyValue::Symbol(Symbol::intern("time")),
                RubyValue::Float(0.5),
            ),
            (
                RubyValue::Symbol(Symbol::intern("intr")),
                RubyValue::Bool(true),
            ),
        ]));
        let opts = raw_opts(&[h], "IO#raw").expect("every key is a known one");
        assert_eq!(opts.min, Some(2));
        assert_eq!(opts.time, Some(5), "half a second is five tenths");
        assert!(opts.intr);
    }

    #[test]
    fn echo_mode_toggles_every_echo_bit() {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        echo_mode(&mut t, true);
        assert_ne!(t.c_lflag & libc::ECHO, 0);
        echo_mode(&mut t, false);
        assert_eq!(
            t.c_lflag & (libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL),
            0
        );
    }

    #[test]
    fn cooked_mode_restores_the_line_discipline() {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        cooked_mode(&mut t);
        assert_ne!(t.c_lflag & libc::ICANON, 0);
        assert_ne!(t.c_lflag & libc::ECHO, 0);
        assert_ne!(t.c_oflag & libc::OPOST, 0);
    }

    #[test]
    fn the_windows_only_methods_carry_crubys_own_message() {
        let pipe = a_pipe();
        for f in [pressed_p, check_winsize_changed] {
            assert!(refuses(f, &pipe));
        }
    }

    /// The `c_lflag` bits a program sets, as opposed to the status bits the
    /// kernel maintains there itself -- the only part of the flag word a
    /// save/restore round trip is required to reproduce.
    const OWNED: libc::tcflag_t = libc::ICANON
        | libc::ECHO
        | libc::ECHOE
        | libc::ECHOK
        | libc::ECHONL
        | libc::ISIG
        | libc::IEXTEN;

    /// A pseudo-terminal, the only terminal a test can rely on having. It is
    /// the SLAVE side that carries the line discipline -- `tcgetattr` on the
    /// master answers ENOTTY. `None` where the sandbox won't allocate one,
    /// which turns the tests below into no-ops rather than failures.
    fn a_pty() -> Option<RubyValue> {
        use std::os::fd::FromRawFd;
        // SAFETY: the standard `posix_openpt` handshake. The master fd is
        // leaked deliberately: closing it would revoke the slave, and this
        // process is a test binary about to exit.
        let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        if master < 0 || unsafe { libc::grantpt(master) } != 0 {
            return None;
        }
        if unsafe { libc::unlockpt(master) } != 0 {
            return None;
        }
        let name = unsafe { libc::ptsname(master) };
        if name.is_null() {
            return None;
        }
        let slave = unsafe { libc::open(name, libc::O_RDWR | libc::O_NOCTTY) };
        (slave >= 0).then(|| io::file_value(unsafe { std::fs::File::from_raw_fd(slave) }, None))
    }

    /// The core promise, on a real terminal: the mode changes, and it is
    /// back to what it was once the block returns.
    #[test]
    fn a_scoped_mode_change_is_undone_on_the_way_out() {
        let Some(pty) = a_pty() else { return };
        let fd = io::raw_fd(&pty).unwrap();
        let before = get_attr(fd).expect("a pty has a terminal mode");
        assert_ne!(before.c_lflag & libc::ICANON, 0, "a fresh pty is cooked");

        let seen = in_mode(
            &pty,
            "test",
            |t| raw_mode(t, RawOpts::default()),
            || Ok(get_attr(fd).expect("still a terminal").c_lflag),
        )
        .unwrap();
        assert_eq!(seen & libc::ICANON, 0, "the block ran in raw mode");
        // Only the flags a program owns: the kernel also keeps status bits
        // here (PENDIN, FLUSHO) that it sets on its own schedule.
        assert_eq!(
            get_attr(fd).unwrap().c_lflag & OWNED,
            before.c_lflag & OWNED,
            "and the old mode is back"
        );
    }

    /// The same, for a body that raises -- the failure mode that would
    /// otherwise leave a user's shell in raw mode.
    #[test]
    fn a_raise_through_the_block_still_restores_the_mode() {
        let Some(pty) = a_pty() else { return };
        let fd = io::raw_fd(&pty).unwrap();
        let before = get_attr(fd).unwrap();

        let out: Result<(), Signal> = in_mode(
            &pty,
            "test",
            |t| raw_mode(t, RawOpts::default()),
            || Err(Signal::Break(RubyValue::Nil)),
        );
        assert!(out.is_err());
        assert_eq!(
            get_attr(fd).unwrap().c_lflag & OWNED,
            before.c_lflag & OWNED
        );
    }

    /// A saved mode is a plain value: editing the copy leaves the terminal
    /// alone until `console_mode=` puts it back.
    #[test]
    fn a_saved_mode_is_a_detached_copy() {
        let Some(pty) = a_pty() else { return };
        let fd = io::raw_fd(&pty).unwrap();
        let RubyValue::Object(saved) = console_mode(&pty, &[], None).unwrap() else {
            panic!("console_mode answers an object")
        };
        let saved = RubyValue::Object(saved);
        raw_mode(
            &mut mode_of(&saved).unwrap().mode.lock(),
            RawOpts::default(),
        );
        assert_ne!(
            get_attr(fd).unwrap().c_lflag & libc::ICANON,
            0,
            "editing the copy left the terminal cooked"
        );

        console_mode_set(&pty, std::slice::from_ref(&saved), None).unwrap();
        assert_eq!(get_attr(fd).unwrap().c_lflag & libc::ICANON, 0, "now raw");
    }
}
