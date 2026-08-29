//! `pty` -- pseudo-terminal support (CRuby's bundled `pty` gem), an in-tree
//! require-gated extension. `PTY.open` allocates a master/slave pair over
//! `openpty(3)`; `PTY.spawn`/`PTY.getpty` runs a command with a fresh pty as
//! its controlling terminal; `PTY.check` polls a child's fate without
//! blocking. The `ChildExited` exception is the gem's Ruby half
//! (`ext/pty/lib/pty.rb`) and the native raise reaches it by name -- minus
//! the `#status` detail CRuby's C constructor attaches, since a by-name raise
//! carries only a message (see `docs/EXTENSIONS.md`).

use crate::builtins::convert;
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

// `openpty(3)` sits in libutil on Linux (folded into glibc since 2.34, but
// the link line still wants the archive elsewhere); macOS carries it in
// libSystem. Declared here rather than through the `libc` crate so the
// `#[link]` rides with the one user.
#[cfg_attr(target_os = "linux", link(name = "util"))]
unsafe extern "C" {
    fn openpty(
        amaster: *mut libc::c_int,
        aslave: *mut libc::c_int,
        name: *mut libc::c_char,
        termp: *const libc::c_void,
        winp: *const libc::c_void,
    ) -> libc::c_int;
}

fn errno(op: &str) -> Signal {
    crate::builtins::file::raise_errno(&std::io::Error::last_os_error(), op, "")
}

/// A fresh master/slave pair (cloexec'd like every fd this runtime creates)
/// and the slave's device name (`/dev/ttysNNN` / `/dev/pts/N`).
fn open_pair() -> Result<(std::fs::File, std::fs::File, String), Signal> {
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (0 as libc::c_int, 0 as libc::c_int);
    // SAFETY: two out-params `openpty` fills; no termios/winsize preset.
    if unsafe {
        openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    } != 0
    {
        return Err(errno("openpty"));
    }
    // `ttyname_r`, not `ttyname`: the latter answers a per-PROCESS static
    // buffer, so two threads in `PTY.open` raced and one pair's slave took
    // the other's device name.
    let mut buf = [0 as libc::c_char; 1024];
    // SAFETY: `buf` is a live, correctly-sized array for the call to fill,
    // and `slave` is the fd `openpty` just made.
    let name = if unsafe { libc::ttyname_r(slave, buf.as_mut_ptr(), buf.len()) } == 0 {
        // SAFETY: a 0 return means `buf` holds a NUL-terminated device path.
        unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    } else {
        String::new()
    };
    crate::builtins::io::set_fd_cloexec(master);
    crate::builtins::io::set_fd_cloexec(slave);
    // SAFETY: `openpty` just handed us these two fresh, owned fds.
    Ok(unsafe {
        (
            std::fs::File::from_raw_fd(master),
            std::fs::File::from_raw_fd(slave),
            name,
        )
    })
}

fn close_quietly(io: &RubyValue) {
    let _ = crate::dispatch::send_value(io, crate::Symbol::intern("close"), &[], None);
}

/// The shared body of `spawn`/`getpty`: run the command with the slave as its
/// stdio AND its controlling terminal, answering `[reader, writer, pid]`.
fn spawn_under_pty(args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    crate::builtins::check_arity(args.len(), 0, Some(3))?;

    let (master, slave, name) = open_pair()?;
    // `PTY.spawn` with no command runs a login shell, `$SHELL` or `/bin/sh`.
    let shell;
    let args = if args.is_empty() {
        shell = [RubyValue::Str(crate::collections::string_new(
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
        ))];
        &shell[..]
    } else {
        args
    };
    let mut cmd = crate::builtins::process::build_spawn_command(args)?;
    let dup = |f: &std::fs::File| {
        f.try_clone()
            .map_err(|e| crate::builtins::process::spawn_error(&e))
    };
    cmd.stdin(Stdio::from(dup(&slave)?));
    cmd.stdout(Stdio::from(dup(&slave)?));
    cmd.stderr(Stdio::from(slave));
    // The child must adopt the slave as its CONTROLLING terminal: a session of
    // its own, then `TIOCSCTTY` on fd 0 (the slave, after the dup2s).
    // SAFETY: both calls are async-signal-safe, the pre_exec contract.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    // `pty.c` blames "fork failed" for a command it could not start, whatever
    // the errno; the errno itself still picks the Errno class.
    let child = cmd
        .spawn()
        .map_err(|e| crate::builtins::file::raise_bare_errno_named(&e, "fork failed"))?;
    // `Command` retains the slave Stdio fds until it drops; while any copy of
    // the slave stays open here, the master never sees EOF (`IO.popen` has the
    // same note).
    drop(cmd);
    let pid = i64::from(child.id());

    // Two handles on the ONE master fd, CRuby's own shape. Both are `File`s
    // named for the SLAVE device -- `rb_io_open_descriptor(rb_cFile, ...)` --
    // and neither carries the pid: `r.pid` is nil in ruby, and the caller has
    // the pid from the trio.
    let r = crate::builtins::io::file_value(
        master
            .try_clone()
            .map_err(|e| crate::builtins::process::spawn_error(&e))?,
        Some(name.clone()),
    );
    let w = crate::builtins::io::file_value(master, Some(name));
    let trio = vec![r.clone(), w.clone(), RubyValue::Int(pid)];
    let result = RubyValue::Array(crate::collections::array_new(trio));

    let Some(RubyValue::Proc(p)) = block else {
        return Ok(result);
    };
    p.call(&[result])?;
    // CRuby's block form leaves a detached reaper behind (not a close): the
    // block owns the IOs, but the child must still not linger as a zombie.
    // It answers NIL, not the block's value.
    crate::builtins::process::detach_thread(pid);
    Ok(RubyValue::Nil)
}

ruby_module! {
    PTY = zeo_abi::PTY_MODULE;

    // `PTY.spawn([env,] command... [,options]) -> [r, w, pid]` (or yields the
    // trio). `getpty` is CRuby's older name for the same call.
    // Variadic, so the NO-argument form reaches the body: `PTY.spawn` with
    // nothing to run starts a login shell.
    def self."spawn" | "getpty" cfunc (_recv, *_args, &block) {
        spawn_under_pty(__args, block)
    }

    // `PTY.open -> [master, slave]` (or yields the pair and closes it after):
    // the raw pair with no child attached -- the master an IO, the slave a
    // File carrying its device path.
    def self."open" (_recv, &block) {
        let (master, slave, name) = open_pair()?;
        // The master's path is the slave's device under a `masterpty:` prefix
        // -- it has no device node of its own, and this is the name CRuby
        // gives it, which is what `#inspect` and `#path` report.
        let m = crate::builtins::io::pipe_value_named(master, format!("masterpty:{name}"));
        let s = crate::builtins::io::file_value(slave, Some(name));
        let pair = RubyValue::Array(crate::collections::array_new(vec![m.clone(), s.clone()]));
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(pair);
        };
        let out = p.call(&[pair]);
        close_quietly(&s);
        close_quietly(&m);
        out
    }

    // `PTY.check(pid, raise = false)` -- a non-blocking look at the child:
    // nil while it runs, its `Process::Status` once it exited or stopped
    // (`WUNTRACED`, as CRuby polls). With `raise`, a finished child raises
    // `PTY::ChildExited` instead.
    def self."check" arity -1 (_recv, arg1, arg2?) {
        // `NUM2PIDT` first: an out-of-range pid TRUNCATED to `pid_t`, and
        // `PTY.check(2**32)` became `waitpid(0, ...)` -- "any child in my
        // process group" -- which reaped a child the caller never named and
        // stole its status from a pending `Process.wait`.
        let pid = convert::to_index(arg1)?;
        let pid = libc::pid_t::try_from(pid).map_err(|_| {
            crate::builtins::range_error!("integer {pid} too big to convert to 'int'")
        })?;
        let pid = i64::from(pid);
        let do_raise = !matches!(arg2, None | Some(RubyValue::Nil) | Some(RubyValue::Bool(false)));
        // A pid that is not this process's child is nil, never an error:
        // `pty.c` calls `rb_waitpid` and answers Qnil on -1, so a stale or
        // foreign pid reads as "nothing to report". Raising ECHILD made
        // `PTY.check` unusable as the poll it is meant to be.
        let reaped = crate::builtins::process::raw_waitpid(
            pid,
            i64::from(libc::WNOHANG | libc::WUNTRACED),
        );
        match reaped {
            Err(_) | Ok(None) => Ok(RubyValue::Nil),
            Ok(Some((reaped, raw))) => {
                let status = crate::builtins::process::new_status(reaped, raw);
                if !do_raise {
                    return Ok(status);
                }
                // CRuby words the state: exited / stopped / changed. The
                // exception carries the status in `@status`, which is what
                // `PTY::ChildExited#status` reads -- raising by name alone
                // left it nil.
                // `raise_from_check` words THREE states; a killed child read
                // as "exited".
                let state = if libc::WIFSTOPPED(raw) {
                    "stopped"
                } else if libc::WIFSIGNALED(raw) {
                    "signaled"
                } else {
                    "exited"
                };
                let signal = raise_error("PTY::ChildExited", format!("pty - {state}: {reaped}"));
                if let Signal::Raise(exc) = &signal {
                    // `ivar_set_dyn` writes the `@` itself.
                    crate::dispatch::ivar_set_dyn(exc, "status", status)?;
                }
                Err(signal)
            }
        }
    }
}
