//! Readiness: `IO#wait`, `IO.select`, and the poll both are built on.

use super::*;

/// One `IO.select` operand's descriptor: the object's own when it is an IO,
/// otherwise its `to_io`'s.
///
/// The conversion is what makes a WRAPPER selectable (`rb_io_get_io`), and the
/// refusal is load-bearing: `raw_fd` answers 0 for anything it does not
/// recognise, so a plain object silently watched STDIN and a select that
/// should have raised TypeError reported whatever the terminal was doing.
pub(super) fn select_fd(v: &RubyValue) -> Result<libc::c_int, Signal> {
    if as_rio(v).is_some() {
        return raw_fd(v);
    }
    let sym = crate::Symbol::intern("to_io");
    if crate::dispatch::responds_to_value(v, sym, true) {
        let io = crate::dispatch::send_value(v, sym, &[], None)?;
        if as_rio(&io).is_some() {
            return raw_fd(&io);
        }
    }
    Err(crate::builtins::type_error!(
        "no implicit conversion of {} into IO",
        crate::builtins::convert_name_of(v)
    ))
}

/// Shared body of `IO#wait_readable`/`#wait_writable` (from `require "io/wait"`)
/// -- block until the stream is ready for `events` (`POLLIN`/`POLLOUT`) or the
/// optional `timeout` (seconds; `nil`/absent = block indefinitely) elapses.
/// Answers `self` when ready, `nil` on timeout, via real `poll(2)` over the fd
/// -- the same libc-over-`io_fileno` shape as `io_winsize`.
pub(super) fn io_wait_for(
    recv: &RubyValue,
    timeout: Option<&RubyValue>,
    events: libc::c_short,
) -> Result<RubyValue, Signal> {
    let timeout_ms = wait_timeout_ms(timeout)?;
    let mut pfd = libc::pollfd {
        fd: raw_fd(recv)?,
        events,
        revents: 0,
    };
    unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    Ok(if poll_ready(pfd.revents, events) {
        recv.clone()
    } else {
        RubyValue::Nil
    })
}

/// Whether one `poll(2)` result counts as ready for `events`. A hangup or an
/// error makes a stream readable (at EOF) and writable (the write reports the
/// error), but never PRIORITY-readable: only out-of-band data is that.
/// A readiness timeout in seconds as milliseconds. Absent or nil blocks
/// forever (`-1`); anything at or below zero polls and returns at once.
///
/// A non-numeric RAISES. It went through `num_to_f64_unchecked`, which is a
/// `panic!` for anything that is not a number, so `io.wait(:read)` -- the
/// documented spelling, where the Symbol lands in the timeout slot -- took
/// the whole process down instead of reporting a TypeError.
pub(super) fn wait_timeout_ms(timeout: Option<&RubyValue>) -> Result<libc::c_int, Signal> {
    let Some(v) = timeout.filter(|v| !v.is_nil()) else {
        return Ok(-1);
    };
    // `rb_time_interval`'s own refusal, message included.
    let secs = match v {
        RubyValue::Int(_) | RubyValue::Float(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
            crate::builtins::numeric::num_to_f64_unchecked(v)
        }
        other => {
            return Err(crate::builtins::type_error!(
                "can't convert {} into time interval",
                crate::builtins::convert_name_of(other)
            ));
        }
    };
    if secs <= 0.0 {
        return Ok(0);
    }
    // A timeout larger than `c_int` blocks rather than wrapping to a poll.
    Ok(((secs * 1000.0).min(f64::from(libc::c_int::MAX))) as libc::c_int)
}

pub(super) fn poll_ready(revents: libc::c_short, events: libc::c_short) -> bool {
    if revents & events != 0 {
        return true;
    }
    events != libc::POLLPRI && revents & (libc::POLLHUP | libc::POLLERR) != 0
}

/// One `select(2)` answer: a ready flag per descriptor, per set, in the order
/// the sets were given.
pub(super) type Readiness = (Vec<bool>, Vec<bool>, Vec<bool>);

/// Which of `fds` are ready, one `select(2)` per set. `poll(2)` cannot answer
/// this: on macOS it reports `POLLPRI` for any readable pipe, so an
/// exception-set query there would claim ordinary bytes are out-of-band data.
/// `select` is also what CRuby's own `IO.select` calls.
///
/// A descriptor at or past `FD_SETSIZE` cannot be named in an `fd_set` and is
/// reported not-ready rather than corrupting the set.
pub(super) fn select_ready(
    read: &[libc::c_int],
    write: &[libc::c_int],
    except: &[libc::c_int],
    timeout_ms: libc::c_int,
) -> Result<Readiness, Signal> {
    const LIMIT: libc::c_int = libc::FD_SETSIZE as libc::c_int;
    // SAFETY: `fd_set` is a plain bitmap; zeroed is the empty set, which is
    // exactly what `FD_ZERO` writes.
    let mut sets: [libc::fd_set; 3] = unsafe { std::mem::zeroed() };
    let mut nfds = 0;
    for (set, fds) in sets.iter_mut().zip([read, write, except]) {
        for &fd in fds {
            if (0..LIMIT).contains(&fd) {
                unsafe { libc::FD_SET(fd, set) };
                nfds = nfds.max(fd + 1);
            }
        }
    }
    let mut tv = libc::timeval {
        tv_sec: (timeout_ms / 1000) as libc::time_t,
        tv_usec: (timeout_ms % 1000 * 1000) as libc::suseconds_t,
    };
    let deadline = if timeout_ms < 0 {
        std::ptr::null_mut()
    } else {
        &mut tv
    };
    let n = unsafe { libc::select(nfds, &mut sets[0], &mut sets[1], &mut sets[2], deadline) };
    if n < 0 {
        return Err(crate::builtins::file::raise_errno(
            &std::io::Error::last_os_error(),
            "select",
            "",
        ));
    }
    let mut out = Vec::new();
    for (set, fds) in sets.iter().zip([read, write, except]) {
        out.push(
            fds.iter()
                .map(|&fd| (0..LIMIT).contains(&fd) && unsafe { libc::FD_ISSET(fd, set) })
                .collect::<Vec<bool>>(),
        );
    }
    let mut it = out.into_iter();
    Ok((
        it.next().unwrap_or_default(),
        it.next().unwrap_or_default(),
        it.next().unwrap_or_default(),
    ))
}
