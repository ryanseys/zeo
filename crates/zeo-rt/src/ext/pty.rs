//! `pty` -- pseudo-terminal support (CRuby's bundled `pty` gem), an in-tree
//! require-gated extension. `PTY.open` allocates a master/slave pair over
//! `openpty(3)`; `PTY.spawn`/`PTY.getpty` runs a command with a fresh pty as
//! its controlling terminal; `PTY.check` polls a child's fate without
//! blocking. The `ChildExited` exception is the gem's Ruby half
//! (`gems/pty/lib/pty.rb`) and the native raise reaches it by name -- minus
//! the `#status` detail CRuby's C constructor attaches, since a by-name raise
//! carries only a message (see `docs/EXTENSIONS.md`).

use crate::builtins::{arity, convert};
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
    // SAFETY: `ttyname` answers a static buffer for the live fd just made.
    let name = match unsafe { libc::ttyname(slave) } {
        p if p.is_null() => String::new(),
        // SAFETY: non-null `ttyname` answers a NUL-terminated device path.
        p => unsafe { std::ffi::CStr::from_ptr(p) }
            .to_string_lossy()
            .into_owned(),
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
    arity!(args, 1..=3);

    let (master, slave, _name) = open_pair()?;
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
    let child = cmd
        .spawn()
        .map_err(|e| crate::builtins::process::spawn_error(&e))?;
    // `Command` retains the slave Stdio fds until it drops; while any copy of
    // the slave stays open here, the master never sees EOF (`IO.popen` has the
    // same note).
    drop(cmd);
    let pid = i64::from(child.id());

    // Two handles on the ONE master fd, CRuby's own shape: a reader that knows
    // the child (`#pid`), and a writer.
    let r = crate::builtins::io::popen_value(
        master
            .try_clone()
            .map_err(|e| crate::builtins::process::spawn_error(&e))?,
        pid,
    );
    let w = crate::builtins::io::pipe_value(master);
    let trio = vec![r.clone(), w.clone(), RubyValue::Int(pid)];
    let result = RubyValue::Array(crate::collections::array_new(trio));

    let Some(RubyValue::Proc(p)) = block else {
        return Ok(result);
    };
    let out = p.call(&[result]);
    // CRuby's block form leaves a detached reaper behind (not a close): the
    // block owns the IOs, but the child must still not linger as a zombie.
    crate::builtins::process::detach_thread(pid);
    out
}

ruby_module! {
    PTY = zeo_abi::PTY_MODULE;

    // `PTY.spawn([env,] command... [,options]) -> [r, w, pid]` (or yields the
    // trio). `getpty` is CRuby's older name for the same call.
    def self."spawn" arity -1 (_recv, args, block) {
        spawn_under_pty(args, block)
    }
    def self."getpty" arity -1 (_recv, args, block) {
        spawn_under_pty(args, block)
    }

    // `PTY.open -> [master, slave]` (or yields the pair and closes it after):
    // the raw pair with no child attached -- the master an IO, the slave a
    // File carrying its device path.
    def self."open" arity 0 (_recv, args, block) {
        arity!(args, 0);
        let (master, slave, name) = open_pair()?;
        let m = crate::builtins::io::pipe_value(master);
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
    def self."check" arity -1 (_recv, args, _block) {
        arity!(args, 1..=2);
        let pid = convert::to_index(&args[0])?;
        let do_raise = !matches!(args.get(1), None | Some(RubyValue::Nil) | Some(RubyValue::Bool(false)));
        match crate::builtins::process::raw_waitpid(pid, i64::from(libc::WNOHANG | libc::WUNTRACED))? {
            None => Ok(RubyValue::Nil),
            Some((reaped, raw)) => {
                if do_raise {
                    // CRuby words the state: exited / stopped / changed.
                    let state = if libc::WIFSTOPPED(raw) { "stopped" } else { "exited" };
                    return Err(raise_error("PTY::ChildExited", format!("pty - {state}: {reaped}")));
                }
                Ok(crate::builtins::process::new_status(reaped, raw))
            }
        }
    }
}
