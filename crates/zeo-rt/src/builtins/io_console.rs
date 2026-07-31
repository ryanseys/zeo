//! `io/console` -- terminal modes (raw/cooked/echo), single-character reads,
//! and the cursor escapes, over `termios(3)`.
//!
//! CRuby ships this as `ext/io/console`, a require-gated extension; zeo's rows
//! hang unconditionally off the IO table, so `require "io/console"` is
//! recognized ceremony (`docs/EXTENSIONS.md`) -- the same shape `io/wait`
//! already has. `IO#winsize` predates this module and stays in `io.rs`.
//!
//! Every mode change goes through `in_mode`, which saves the terminal's
//! settings, applies a mutation, runs a body, and restores from a `Drop`
//! guard -- so a `raise` (or a `break`, or a panic) out of `io.raw { ... }`
//! can't leave the user's terminal in raw mode, which is the failure that
//! makes a shell unusable.
//!
//! On a stream that isn't a terminal, everything here raises `Errno::ENOTTY`
//! the way CRuby does, down to the message split: the direct methods name the
//! stream (`"... - <STDIN>"`), the scoped ones don't, because CRuby reaches
//! them through a helper that has no name to report.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::builtins::{arity, io, local_jump_error, not_impl_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal, Symbol};
use zeo_macros::ruby_class;

/// CRuby's `Errno::ENOTTY`. `name` is the stream label the direct methods
/// report and the scoped ones omit -- see the module docs. A nameless stream
/// (a pipe) reports no suffix either, so an empty label reads as no label.
///
/// `method` labels the synthetic frame the raise is captured inside, so an
/// unrescued failure reads `in 'IO#raw!'` above `<main>` the way CRuby's does
/// -- these rows are C functions with no Ruby line of their own.
fn enotty(name: Option<&str>, method: &'static str) -> Signal {
    let _frame = crate::frames::synthetic_c_frame(method);
    let msg = match name.filter(|n| !n.is_empty()) {
        Some(n) => format!("Inappropriate ioctl for device - {n}"),
        None => "Inappropriate ioctl for device".to_string(),
    };
    raise_error("Errno::ENOTTY", msg)
}

/// The named form, for the direct methods -- `IO#winsize` (which lives in
/// `io.rs`) raises through here too.
pub(crate) fn not_a_terminal(recv: &RubyValue, method: &'static str) -> Signal {
    enotty(Some(&io::stream_label(recv)), method)
}

fn get_attr(fd: libc::c_int) -> Option<libc::termios> {
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` came from `#fileno` and `t` is a live, correctly-sized
    // termios for the call to fill.
    (unsafe { libc::tcgetattr(fd, &mut t) } == 0).then_some(t)
}

/// `TCSADRAIN` rather than `TCSANOW`, matching CRuby: pending output is
/// written out before the mode flips, so a prompt already `print`ed isn't
/// swallowed by the switch into raw mode.
fn set_attr(fd: libc::c_int, t: &libc::termios) -> bool {
    unsafe { libc::tcsetattr(fd, libc::TCSADRAIN, t) == 0 }
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
    let saved = get_attr(fd).ok_or_else(|| enotty(None, method))?;
    let mut next = saved;
    apply(&mut next);
    if !set_attr(fd, &next) {
        return Err(enotty(None, method));
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
    if let Some(min) = opts.min {
        t.c_cc[libc::VMIN] = min as libc::cc_t;
    }
    if let Some(time) = opts.time {
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

/// Read `min:`/`time:`/`intr:` out of a trailing keyword Hash. Anything else
/// in the Hash is ignored, as CRuby's own option scan ignores unknown keys.
fn raw_opts(args: &[RubyValue]) -> RawOpts {
    let mut opts = RawOpts::default();
    let Some(RubyValue::Hash(h)) = args.last() else {
        return opts;
    };
    let get = |name: &str| {
        let key = RubyValue::Symbol(Symbol::intern(name));
        match crate::hash_get(h, &key) {
            RubyValue::Nil => None,
            v => Some(v),
        }
    };
    opts.min = get("min").map(|v| crate::builtins::numeric::num_to_f64_unchecked(&v) as i64);
    opts.time =
        get("time").map(|v| (crate::builtins::numeric::num_to_f64_unchecked(&v) * 10.0) as i64);
    opts.intr = matches!(get("intr"), Some(RubyValue::Bool(true)));
    opts
}

/// Yield the receiver to the method's block, or raise as a bare `yield` in a
/// blockless method does -- CRuby's scoped console methods are `rb_yield`
/// calls and behave the same way.
fn yield_self(recv: &RubyValue, block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    match block {
        Some(RubyValue::Proc(p)) => p.call(std::slice::from_ref(recv)),
        _ => Err(local_jump_error!("no block given (yield)")),
    }
}

fn send0(recv: &RubyValue, name: &str) -> Result<RubyValue, Signal> {
    crate::dispatch::send_value(recv, Symbol::intern(name), &[], None)
}

/// Write `s` to the stream -- how every cursor/erase escape reaches the
/// terminal, and the reason they work on any IO rather than only a tty.
fn write_str(recv: &RubyValue, s: String) -> Result<RubyValue, Signal> {
    let arg = RubyValue::Str(crate::collections::string_new(s));
    crate::dispatch::send_value(recv, Symbol::intern("write"), &[arg], None)?;
    Ok(RubyValue::Nil)
}

/// One `Integer` argument for the cursor/erase escapes.
fn int_arg(v: &RubyValue) -> Result<i64, Signal> {
    crate::builtins::convert::to_index(v)
}

// -- the IO rows -------------------------------------------------------

pub fn raw(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let opts = raw_opts(args);
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
    let opts = raw_opts(args);
    set_mode(recv, "IO#raw!", |t| raw_mode(t, opts))
}

pub fn cooked(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    in_mode(recv, "IO#cooked", cooked_mode, || yield_self(recv, block))
}

pub fn cooked_bang(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    set_mode(recv, "IO#cooked!", cooked_mode)
}

pub fn echo_p(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
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
    arity!(args, 1);
    let on = args[0].truthy();
    set_mode(recv, "IO#echo=", |t| echo_mode(t, on))?;
    // An assignment answers its right-hand side, not the receiver.
    Ok(args[0].clone())
}

pub fn noecho(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
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
    let opts = raw_opts(args);
    in_mode(
        recv,
        "IO#getch",
        |t| raw_mode(t, opts),
        || send0(recv, "getc"),
    )
}

/// `getpass(prompt = nil)` -- print the prompt, read a line with echo off,
/// then print the newline the user's Return didn't echo. The answer is
/// chomped, as CRuby's is.
pub fn getpass(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0..=1);
    if let Some(prompt) = args.first().filter(|v| !matches!(v, RubyValue::Nil)) {
        let s = crate::builtins::convert::to_rstr(prompt)?
            .lock()
            .to_utf8_lossy()
            .into_owned();
        write_str(recv, s)?;
    }
    let line = in_mode(
        recv,
        "IO#getpass",
        |t| echo_mode(t, false),
        || send0(recv, "gets"),
    )?;
    write_str(recv, "\n".to_string())?;
    match line {
        RubyValue::Nil => Ok(RubyValue::Nil),
        other => crate::dispatch::send_value(&other, Symbol::intern("chomp"), &[], None),
    }
}

fn flush_queue(
    recv: &RubyValue,
    method: &'static str,
    queue: libc::c_int,
) -> Result<RubyValue, Signal> {
    let fd = io::raw_fd(recv)?;
    // A non-tty fails here exactly as `tcgetattr` would, so probe first for
    // the message CRuby reports.
    get_attr(fd).ok_or_else(|| not_a_terminal(recv, method))?;
    unsafe { libc::tcflush(fd, queue) };
    Ok(recv.clone())
}

pub fn iflush(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    flush_queue(recv, "IO#iflush", libc::TCIFLUSH)
}

pub fn oflush(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    flush_queue(recv, "IO#oflush", libc::TCOFLUSH)
}

pub fn ioflush(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    flush_queue(recv, "IO#ioflush", libc::TCIOFLUSH)
}

/// `ttyname` -- the device path behind the stream, or nil when it isn't a
/// terminal (CRuby answers nil rather than raising here).
pub fn ttyname(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    let fd = io::raw_fd(recv)?;
    // SAFETY: `ttyname` answers a pointer into thread-local storage, valid
    // until the next call on this thread -- copied out immediately below.
    let p = unsafe { libc::ttyname(fd) };
    if p.is_null() {
        return Ok(RubyValue::Nil);
    }
    let name = unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned();
    Ok(RubyValue::Str(crate::collections::string_new(name)))
}

/// `winsize = [rows, columns]`.
pub fn winsize_set(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    let RubyValue::Array(dims) = &args[0] else {
        return Err(crate::builtins::type_error!(
            "expected an Array of [rows, columns]"
        ));
    };
    let dims = dims.lock().clone();
    let fd = io::raw_fd(recv)?;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    ws.ws_row = dims.first().map(int_arg).transpose()?.unwrap_or(0) as libc::c_ushort;
    ws.ws_col = dims.get(1).map(int_arg).transpose()?.unwrap_or(0) as libc::c_ushort;
    if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &ws) } != 0 {
        return Err(not_a_terminal(recv, "IO#winsize="));
    }
    Ok(args[0].clone())
}

/// `console_mode` -- the current terminal settings, to be handed back to
/// `console_mode=` later.
pub fn console_mode(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    let fd = io::raw_fd(recv)?;
    let t = get_attr(fd).ok_or_else(|| not_a_terminal(recv, "IO#console_mode"))?;
    Ok(RubyValue::Object(std::sync::Arc::new(ConsoleMode::new(t))))
}

pub fn console_mode_set(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
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

pub fn beep(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    write_str(recv, "\x07".to_string())
}

pub fn clear_screen(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    write_str(recv, "\x1b[2J\x1b[1;1H".to_string())
}

pub fn erase_line(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    write_str(recv, format!("\x1b[{}K", int_arg(&args[0])?))
}

pub fn erase_screen(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    write_str(recv, format!("\x1b[{}J", int_arg(&args[0])?))
}

/// `goto(line, column)` -- both zero-based, as CRuby's are, over an escape
/// whose own coordinates start at one.
pub fn goto(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 2);
    let (row, col) = (int_arg(&args[0])? + 1, int_arg(&args[1])? + 1);
    write_str(recv, format!("\x1b[{row};{col}H"))
}

pub fn goto_column(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    write_str(recv, format!("\x1b[{}G", int_arg(&args[0])? + 1))
}

/// The four relative moves plus the two scrolls, which differ only in their
/// final letter.
fn move_by(recv: &RubyValue, args: &[RubyValue], letter: char) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    write_str(recv, format!("\x1b[{}{letter}", int_arg(&args[0])?))
}

pub fn cursor_up(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    move_by(recv, args, 'A')
}

pub fn cursor_down(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    move_by(recv, args, 'B')
}

pub fn cursor_right(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    move_by(recv, args, 'C')
}

pub fn cursor_left(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    move_by(recv, args, 'D')
}

pub fn scroll_forward(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    move_by(recv, args, 'S')
}

pub fn scroll_backward(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    move_by(recv, args, 'T')
}

/// `cursor` -- where the cursor is, as `[row, column]` zero-based. Asks the
/// terminal itself (the DSR escape) and parses the `ESC [ row ; col R` reply,
/// which is why it needs raw mode: the reply arrives on the INPUT side and
/// would otherwise be echoed and line-buffered.
pub fn cursor(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
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
            let mut reply = String::new();
            // The reply ends at 'R'; a stream that answers nothing (not a real
            // terminal, however it typed) ends the loop at EOF.
            while let RubyValue::Str(s) = send0(recv, "getc")? {
                let c = s.lock().to_utf8_lossy().into_owned();
                let done = c == "R";
                reply.push_str(&c);
                if done {
                    break;
                }
            }
            let digits: Vec<i64> = reply
                .trim_start_matches(['\x1b', '['])
                .trim_end_matches('R')
                .split(';')
                .filter_map(|p| p.parse::<i64>().ok())
                .collect();
            // The escape's coordinates are one-based; Ruby's are not.
            Ok(RubyValue::Array(crate::collections::array_new(vec![
                RubyValue::Int(digits.first().copied().unwrap_or(1) - 1),
                RubyValue::Int(digits.get(1).copied().unwrap_or(1) - 1),
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
    arity!(args, 1);
    let RubyValue::Array(pos) = &args[0] else {
        return Err(crate::builtins::type_error!(
            "expected an Array of [row, column]"
        ));
    };
    let pos = pos.lock().clone();
    let row = pos.first().map(int_arg).transpose()?.unwrap_or(0);
    let col = pos.get(1).map(int_arg).transpose()?.unwrap_or(0);
    write_str(recv, format!("\x1b[{};{}H", row + 1, col + 1))?;
    Ok(args[0].clone())
}

/// `IO.console` -- the process's controlling terminal as a `File` on
/// `/dev/tty`, or nil when none of the standard streams is a terminal (CRuby's
/// own test, and what makes this answer nil under a test harness).
pub fn io_class_console(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0);
    if (0..=2).all(|fd| unsafe { libc::isatty(fd) } != 1) {
        return Ok(RubyValue::Nil);
    }
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty");
    match opened {
        Ok(f) => Ok(io::file_value(f, Some("/dev/tty".to_string()))),
        Err(_) => Ok(RubyValue::Nil),
    }
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
    .ok_or_else(|| crate::builtins::type_error!("not an IO::ConsoleMode"))
}

ruby_class! {
    ConsoleMode = zeo_abi::CONSOLE_MODE_CLASS < zeo_abi::OBJECT_CLASS;

    // The three editors CRuby gives a saved mode, so a caller can restore a
    // MODIFIED version of what it captured.
    def "raw" (recv, *args, &_block) {
        let opts = raw_opts(args);
        raw_mode(&mut mode_of(recv)?.mode.lock(), opts);
        Ok(recv.clone())
    }
    def "raw!" (recv, *args, &_block) {
        let opts = raw_opts(args);
        raw_mode(&mut mode_of(recv)?.mode.lock(), opts);
        Ok(recv.clone())
    }
    def "echo=" (recv, arg) {
        echo_mode(&mut mode_of(recv)?.mode.lock(), (*arg).truthy());
        Ok((*arg).clone())
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
        let opts = raw_opts(&[h]);
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

    /// The wave's core promise, on a real terminal: the mode changes, and it
    /// is back to what it was once the block returns.
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
