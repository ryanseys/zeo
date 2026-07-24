//! `Process` (CRuby process.c) -- the identity and clock surface. Spawning
//! (`Process.spawn`/`Kernel#system`/backticks) is a later slice; this one
//! covers what programs read rather than what they start.
//!
//! `clock_gettime` answers a Float of SECONDS (CRuby's default unit), which
//! is what `Process.clock_gettime(Process::CLOCK_MONOTONIC)` benchmark
//! idioms subtract. The clock CONSTANTS are declared as ordinary constants
//! under the Process module (the `const` rows in the `ruby_module!` below),
//! matching how a program writes them: `Process::CLOCK_MONOTONIC`.

use std::cell::RefCell;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use crate::builtins::{arg_error, arity, builtin_methods, type_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, PROCESS_STATUS_CLASS, PROCESS_TMS_CLASS};
use zeo_macros::ruby_module;

ruby_module! {
    Process = zeo_abi::PROCESS_CLASS;

    // The clock ids `Process.clock_gettime` accepts, and the
    // `getpriority`/`setpriority` `which` selectors -- the OS's own values (see
    // `clock_seconds`), published as `Process::CLOCK_*` / `Process::PRIO_*`.
    const CLOCK_REALTIME = RubyValue::Int(libc::CLOCK_REALTIME as i64);
    const CLOCK_MONOTONIC = RubyValue::Int(libc::CLOCK_MONOTONIC as i64);
    const CLOCK_PROCESS_CPUTIME_ID = RubyValue::Int(libc::CLOCK_PROCESS_CPUTIME_ID as i64);
    const CLOCK_THREAD_CPUTIME_ID = RubyValue::Int(libc::CLOCK_THREAD_CPUTIME_ID as i64);
    const CLOCK_MONOTONIC_RAW = RubyValue::Int(libc::CLOCK_MONOTONIC_RAW as i64);
    const CLOCK_MONOTONIC_RAW_APPROX = RubyValue::Int(libc::CLOCK_MONOTONIC_RAW_APPROX as i64);
    const CLOCK_UPTIME_RAW = RubyValue::Int(libc::CLOCK_UPTIME_RAW as i64);
    const CLOCK_UPTIME_RAW_APPROX = RubyValue::Int(libc::CLOCK_UPTIME_RAW_APPROX as i64);
    const PRIO_PROCESS = RubyValue::Int(libc::PRIO_PROCESS as i64);
    const PRIO_PGRP = RubyValue::Int(libc::PRIO_PGRP as i64);
    const PRIO_USER = RubyValue::Int(libc::PRIO_USER as i64);
    // `waitpid`/`wait` flags.
    const WNOHANG = RubyValue::Int(libc::WNOHANG as i64);
    const WUNTRACED = RubyValue::Int(libc::WUNTRACED as i64);
    // `getrlimit`/`setrlimit` resources and the "no limit" sentinel. macOS has
    // no RLIM_SAVED_CUR/MAX distinct from RLIM_INFINITY, so all three coincide.
    const RLIMIT_AS = RubyValue::Int(libc::RLIMIT_AS as i64);
    const RLIMIT_CORE = RubyValue::Int(libc::RLIMIT_CORE as i64);
    const RLIMIT_CPU = RubyValue::Int(libc::RLIMIT_CPU as i64);
    const RLIMIT_DATA = RubyValue::Int(libc::RLIMIT_DATA as i64);
    const RLIMIT_FSIZE = RubyValue::Int(libc::RLIMIT_FSIZE as i64);
    const RLIMIT_MEMLOCK = RubyValue::Int(libc::RLIMIT_MEMLOCK as i64);
    const RLIMIT_NOFILE = RubyValue::Int(libc::RLIMIT_NOFILE as i64);
    const RLIMIT_NPROC = RubyValue::Int(libc::RLIMIT_NPROC as i64);
    const RLIMIT_RSS = RubyValue::Int(libc::RLIMIT_RSS as i64);
    const RLIMIT_STACK = RubyValue::Int(libc::RLIMIT_STACK as i64);
    const RLIM_INFINITY = RubyValue::Int(libc::RLIM_INFINITY as i64);
    const RLIM_SAVED_CUR = RubyValue::Int(libc::RLIM_INFINITY as i64);
    const RLIM_SAVED_MAX = RubyValue::Int(libc::RLIM_INFINITY as i64);

    def self.pid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(std::process::id() as i64))
    }
    // `Process._fork` (process.c) -- the low-level primitive `Process.fork` /
    // `Kernel#fork` build on. Gems hook fork by prepending a module onto
    // `Process.singleton_class` and overriding `_fork` (connection_pool's
    // `ForkTracker`, whose `super` lands here). Performs the real fork(2) and
    // answers the child pid (0 in the child), matching CRuby on a fork-capable
    // platform; it only ever runs if the program actually forks.
    def self._fork(_recv, args, _block) {
        arity!(args, 0);
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(raise_error("SystemCallError", std::io::Error::last_os_error().to_string()));
        }
        Ok(RubyValue::Int(pid as i64))
    }
    // `Process.fork [{ block }]` -- the high-level fork, defined in terms of the
    // `_fork` primitive: it DISPATCHES `_fork` through the receiver, so a
    // prepended override (connection_pool's `ForkTracker`, whose `super` reaches
    // the native `_fork` above) is honored -- which is the whole reason gems
    // hook `_fork` rather than `fork`. Parent: the child pid. Child WITH a
    // block: run it, then exit through the SAME at_exit/finalizer epilogue the
    // top level uses (a `SystemExit` status is respected; any other uncaught
    // exception is reported and exits 1). Child with NO block: `nil`, so the
    // caller drives the child itself.
    def self.fork(recv, args, block) {
        arity!(args, 0);
        let pid = crate::dispatch::send_value(recv, crate::Symbol::intern("_fork"), &[], None)?;
        if matches!(pid, RubyValue::Int(0)) {
            if let Some(RubyValue::Proc(p)) = block {
                let outcome = p.call(&[]);
                crate::exec::run_at_exit();
                crate::builtins::weak::run_finalizers();
                match outcome {
                    Ok(_) => std::process::exit(0),
                    Err(Signal::Raise(exc)) => {
                        if let Some(code) = crate::builtins::kernel::system_exit_status(&exc) {
                            std::process::exit(code);
                        }
                        crate::builtins::exception::report_uncaught(&exc);
                        std::process::exit(1);
                    }
                    Err(_) => std::process::exit(1),
                }
            }
            return Ok(RubyValue::Nil);
        }
        Ok(pid)
    }
    // `Process.kill(sig, *pids)` -- resolve the signal, deliver to each pid,
    // answer how many were signaled. Self-delivery of a signal whose `trap`
    // registered a Proc runs that handler synchronously INSTEAD of a real
    // OS signal: zeo's `trap` installs no OS handler (see
    // `builtins::signal`), so `libc::kill` to ourselves would take the
    // signal's default disposition and terminate the very program that
    // trapped it. Signal 0 (existence probe) and other-process delivery go
    // through the real syscall.
    def self.kill(_recv, args, _block) {
        let Some((sig, pids)) = args.split_first() else {
            return Err(arg_error!("wrong number of arguments (given 0, expected at least 1)"));
        };
        let no = crate::builtins::signal::resolve_signal_arg(sig)?;
        let me = std::process::id() as i64;
        for pv in pids {
            let pid = int_arg(pv)?;
            if pid == me && no != 0 {
                match crate::builtins::signal::trap_action(no) {
                    Some(RubyValue::Proc(p)) => {
                        p.call(&[RubyValue::Int(no as i64)])?;
                        continue;
                    }
                    // An "IGNORE" action swallows the signal entirely.
                    Some(RubyValue::Str(s)) if &*s.lock().to_utf8_lossy() == "IGNORE" => continue,
                    // DEFAULT (or never trapped): fall through to the real
                    // delivery -- the default disposition IS the behavior.
                    _ => {}
                }
            }
            if unsafe { libc::kill(pid as libc::pid_t, no as libc::c_int) } != 0 {
                let err = std::io::Error::last_os_error();
                return Err(match err.raw_os_error() {
                    Some(libc::ESRCH) => raise_error("Errno::ESRCH", "No such process".to_string()),
                    Some(libc::EPERM) => {
                        raise_error("Errno::EPERM", "Operation not permitted".to_string())
                    }
                    _ => raise_error("SystemCallError", err.to_string()),
                });
            }
        }
        Ok(RubyValue::Int(pids.len() as i64))
    }
    // `Process.ppid` has no portable std equivalent; libc's getppid is the
    // honest answer rather than a fabricated one.
    def self.ppid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getppid() } as i64))
    }
    def self.clock_gettime(_recv, args, _block) {
        arity!(args, 1..=2);
        let clock = int_arg(&args[0])?;
        clock_in_unit(clock_seconds(clock)?, args.get(1))
    }
    // `Process.clock_getres(clock_id [, unit])` -- the clock's resolution, in the
    // same units `clock_gettime` accepts (default a Float of seconds).
    def self.clock_getres(_recv, args, _block) {
        arity!(args, 1..=2);
        let clock = int_arg(&args[0])?;
        clock_in_unit(clock_res_seconds(clock)?, args.get(1))
    }
    // Real/effective user and group ids (libc getuid/geteuid/getgid/getegid).
    def self.uid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getuid() } as i64))
    }
    def self.euid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::geteuid() } as i64))
    }
    def self.gid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getgid() } as i64))
    }
    def self.egid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getegid() } as i64))
    }
    // `Process.getpgrp` -- the current process group id.
    def self.getpgrp(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getpgrp() } as i64))
    }
    // `Process.getsid([pid])` -- the session id of `pid` (0/none = this process).
    def self.getsid(_recv, args, _block) {
        arity!(args, 0..=1);
        let pid = match args.first() {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => int_arg(v)? as libc::pid_t,
        };
        let sid = unsafe { libc::getsid(pid) };
        if sid < 0 {
            return Err(raise_error("Errno::ESRCH", "No such process".to_string()));
        }
        Ok(RubyValue::Int(sid as i64))
    }
    // `Process.getpriority(which, who)` -- the scheduling priority. `getpriority`
    // returns -1 both for a real -1 priority and on error, so errno is cleared
    // first and checked after (CRuby does the same).
    def self.getpriority(_recv, args, _block) {
        arity!(args, 2);
        let which = int_arg(&args[0])? as libc::c_int;
        let who = int_arg(&args[1])? as libc::id_t;
        unsafe { *libc::__error() = 0 };
        let prio = unsafe { libc::getpriority(which, who) };
        if prio == -1 && unsafe { *libc::__error() } != 0 {
            return Err(raise_error("Errno::ESRCH", "No such process".to_string()));
        }
        Ok(RubyValue::Int(prio as i64))
    }
    // `Process.groups` -- the supplementary group ids, as an Array of Integer.
    def self.groups(_recv, args, _block) {
        arity!(args, 0);
        let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
        let mut buf = vec![0 as libc::gid_t; count.max(0) as usize];
        let n = unsafe { libc::getgroups(buf.len() as libc::c_int, buf.as_mut_ptr()) };
        let list = buf
            .iter()
            .take(n.max(0) as usize)
            .map(|g| RubyValue::Int(*g as i64))
            .collect();
        Ok(RubyValue::Array(crate::array_new(list)))
    }
    // `Process.times` -- a Process::Tms of CPU seconds, from `getrusage` for
    // this process (utime/stime) and its reaped children (cutime/cstime).
    def self.times(_recv, args, _block) {
        arity!(args, 0);
        // SAFETY: each `getrusage` fully initializes its zeroed out-param.
        let rusage = |who: libc::c_int| -> libc::rusage {
            let mut u: libc::rusage = unsafe { std::mem::zeroed() };
            unsafe { libc::getrusage(who, &mut u) };
            u
        };
        let secs = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
        let me = rusage(libc::RUSAGE_SELF);
        let kids = rusage(libc::RUSAGE_CHILDREN);
        Ok(new_tms(secs(me.ru_utime), secs(me.ru_stime), secs(kids.ru_utime), secs(kids.ru_stime)))
    }
    // `Process.getpgid(pid)` -- the process group id of `pid` (0 = this process).
    def self.getpgid(_recv, args, _block) {
        arity!(args, 1);
        let pid = int_arg(&args[0])? as libc::pid_t;
        let pgid = unsafe { libc::getpgid(pid) };
        if pgid < 0 {
            return Err(errno_fail("getpgid"));
        }
        Ok(RubyValue::Int(pgid as i64))
    }
    // `Process.setpgid(pid, pgrp)` -- put `pid` into process group `pgrp`.
    def self.setpgid(_recv, args, _block) {
        arity!(args, 2);
        let pid = int_arg(&args[0])? as libc::pid_t;
        let pgrp = int_arg(&args[1])? as libc::pid_t;
        if unsafe { libc::setpgid(pid, pgrp) } != 0 {
            return Err(errno_fail("setpgid"));
        }
        Ok(RubyValue::Int(0))
    }
    // `Process.setpgrp` -- make this process a group leader (setpgid(0, 0)).
    def self.setpgrp(_recv, args, _block) {
        arity!(args, 0);
        if unsafe { libc::setpgid(0, 0) } != 0 {
            return Err(errno_fail("setpgrp"));
        }
        Ok(RubyValue::Int(0))
    }
    // `Process.setsid` -- start a new session; answers the new session id.
    def self.setsid(_recv, args, _block) {
        arity!(args, 0);
        let sid = unsafe { libc::setsid() };
        if sid < 0 {
            return Err(errno_fail("setsid"));
        }
        Ok(RubyValue::Int(sid as i64))
    }
    // `Process.setpriority(which, who, prio)` -- set a scheduling priority.
    def self.setpriority(_recv, args, _block) {
        arity!(args, 3);
        let which = int_arg(&args[0])? as libc::c_int;
        let who = int_arg(&args[1])? as libc::id_t;
        let prio = int_arg(&args[2])? as libc::c_int;
        if unsafe { libc::setpriority(which, who, prio) } != 0 {
            return Err(errno_fail("setpriority"));
        }
        Ok(RubyValue::Int(0))
    }
    // Real/effective id setters -- each answers its Integer argument (CRuby's shape).
    def self."uid="(_recv, args, _block) {
        arity!(args, 1);
        let id = int_arg(&args[0])?;
        if unsafe { libc::setuid(id as libc::uid_t) } != 0 {
            return Err(errno_fail("setuid"));
        }
        Ok(RubyValue::Int(id))
    }
    def self."gid="(_recv, args, _block) {
        arity!(args, 1);
        let id = int_arg(&args[0])?;
        if unsafe { libc::setgid(id as libc::gid_t) } != 0 {
            return Err(errno_fail("setgid"));
        }
        Ok(RubyValue::Int(id))
    }
    def self."euid="(_recv, args, _block) {
        arity!(args, 1);
        let id = int_arg(&args[0])?;
        if unsafe { libc::seteuid(id as libc::uid_t) } != 0 {
            return Err(errno_fail("seteuid"));
        }
        Ok(RubyValue::Int(id))
    }
    def self."egid="(_recv, args, _block) {
        arity!(args, 1);
        let id = int_arg(&args[0])?;
        if unsafe { libc::setegid(id as libc::gid_t) } != 0 {
            return Err(errno_fail("setegid"));
        }
        Ok(RubyValue::Int(id))
    }
    // `Process.groups = [gid, ...]` -- replace the supplementary groups; answers the arg.
    def self."groups="(_recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Array(a) = &args[0] else {
            return Err(type_error!(
                "no implicit conversion of {} into Array",
                crate::builtins::class_name_of(&args[0])
            ));
        };
        let gids: Vec<libc::gid_t> = a
            .lock()
            .iter()
            .map(|v| int_arg(v).map(|n| n as libc::gid_t))
            .collect::<Result<_, _>>()?;
        if unsafe { libc::setgroups(gids.len() as libc::c_int, gids.as_ptr()) } != 0 {
            return Err(errno_fail("setgroups"));
        }
        Ok(args[0].clone())
    }
    // `Process.getrlimit(resource)` -> `[soft, hard]`.
    def self.getrlimit(_recv, args, _block) {
        arity!(args, 1);
        let res = int_arg(&args[0])?;
        // SAFETY: getrlimit fully initializes the zeroed out-param on success.
        let mut lim: libc::rlimit = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrlimit(res as _, &mut lim) } != 0 {
            return Err(errno_fail("getrlimit"));
        }
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Int(lim.rlim_cur as i64),
            RubyValue::Int(lim.rlim_max as i64),
        ])))
    }
    // `Process.setrlimit(resource, soft [, hard])` -- hard defaults to soft.
    def self.setrlimit(_recv, args, _block) {
        arity!(args, 2..=3);
        let res = int_arg(&args[0])?;
        let cur = int_arg(&args[1])?;
        let max = match args.get(2) {
            Some(v) => int_arg(v)?,
            None => cur,
        };
        let lim = libc::rlimit {
            rlim_cur: cur as libc::rlim_t,
            rlim_max: max as libc::rlim_t,
        };
        if unsafe { libc::setrlimit(res as _, &lim) } != 0 {
            return Err(errno_fail("setrlimit"));
        }
        Ok(RubyValue::Nil)
    }
    // `Process.maxgroups` / `maxgroups=` -- a settable ceiling CRuby keeps for
    // `getgroups`; not a syscall, just a stored bound.
    def self.maxgroups(_recv, args, _block) {
        arity!(args, 0);
        // CRuby caps the reported ceiling at the system NGROUPS_MAX (16 on
        // macOS), so a larger stored bound reads back clamped.
        let stored = PROCESS_MAXGROUPS.load(Ordering::Relaxed);
        let cap = unsafe { libc::sysconf(libc::_SC_NGROUPS_MAX) };
        Ok(RubyValue::Int(if cap > 0 { stored.min(cap) } else { stored }))
    }
    def self."maxgroups="(_recv, args, _block) {
        arity!(args, 1);
        let n = int_arg(&args[0])?;
        PROCESS_MAXGROUPS.store(n, Ordering::Relaxed);
        Ok(RubyValue::Int(n))
    }
    // `Process.argv0` -- the program name (`$0`) at startup.
    def self.argv0(_recv, args, _block) {
        arity!(args, 0);
        Ok(crate::globals::global_get(0, "$0"))
    }
    // `Process.setproctitle(str)` -- answers the title. macOS has no portable
    // setproctitle, so the cosmetic title itself is a best-effort no-op.
    def self.setproctitle(_recv, args, _block) {
        arity!(args, 1);
        crate::builtins::convert::to_str(&args[0])
    }
    // `Process.warmup` -- a JIT/heap warmup hint; nothing to warm here.
    def self.warmup(_recv, args, _block) {
        arity!(args, 0..=1);
        Ok(RubyValue::Bool(true))
    }
    // `Process.initgroups(username, gid)` -- set the supplementary group list
    // from the group database for `username` plus `gid`; answers the new
    // groups. Typically root-only.
    def self.initgroups(_recv, args, _block) {
        arity!(args, 2);
        let RubyValue::Str(user) = &args[0] else {
            return Err(type_error!(
                "no implicit conversion of {} into String",
                crate::builtins::class_name_of(&args[0])
            ));
        };
        let gid = int_arg(&args[1])? as libc::c_int;
        let cuser = std::ffi::CString::new(user.lock().to_utf8_lossy().into_owned())
            .map_err(|_| arg_error!("string contains null byte"))?;
        if unsafe { libc::initgroups(cuser.as_ptr(), gid) } != 0 {
            return Err(errno_fail("initgroups"));
        }
        Ok(current_groups())
    }
    // `Process.daemon(nochdir = nil, noclose = nil)` -- detach into the
    // background; answers 0.
    def self.daemon(_recv, args, _block) {
        arity!(args, 0..=2);
        let truthy =
            |v: Option<&RubyValue>| matches!(v, Some(x) if !x.is_nil() && !matches!(x, RubyValue::Bool(false)));
        let nochdir = if truthy(args.first()) { 1 } else { 0 };
        let noclose = if truthy(args.get(1)) { 1 } else { 0 };
        #[allow(deprecated)]
        let ret = unsafe { libc::daemon(nochdir, noclose) };
        if ret != 0 {
            return Err(errno_fail("daemon"));
        }
        Ok(RubyValue::Int(0))
    }
}

/// The stored `Process.maxgroups` bound. Defaults to CRuby's `RB_MAX_GROUPS`;
/// the reader clamps it to the runtime `NGROUPS_MAX`.
static PROCESS_MAXGROUPS: AtomicI64 = AtomicI64::new(65536);

/// Raise the current `errno` as the matching `Errno::*` (via File's mapper, the
/// single place errno -> exception-class lives).
fn errno_fail(syscall: &str) -> crate::Signal {
    crate::builtins::file::raise_errno(&std::io::Error::last_os_error(), syscall, "")
}

/// The current supplementary groups as an Array of Integer -- shared by the
/// `groups` reader and `initgroups`'s answer.
fn current_groups() -> RubyValue {
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    let mut buf = vec![0 as libc::gid_t; count.max(0) as usize];
    let n = unsafe { libc::getgroups(buf.len() as libc::c_int, buf.as_mut_ptr()) };
    let list = buf
        .iter()
        .take(n.max(0) as usize)
        .map(|g| RubyValue::Int(*g as i64))
        .collect();
    RubyValue::Array(crate::array_new(list))
}

/// One clock's current value in seconds. The ids are the OS's own
/// (`libc::CLOCK_*`), which is exactly what the `CLOCK_*` constants publish as
/// `Process::CLOCK_*` -- so a program that passes the constant through gets
/// the clock it named, and one that passes a bare integer gets whatever that
/// integer means to this OS, as in CRuby.
fn clock_seconds(clock: i64) -> Result<f64, crate::Signal> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid, fully-initialized out-param for the duration
    // of the call; `clock_gettime` writes it and touches nothing else.
    let rc = unsafe { libc::clock_gettime(clock as libc::clockid_t, &mut ts) };
    if rc != 0 {
        return Err(crate::dispatch::raise_error(
            "Errno::EINVAL",
            format!("Invalid argument - unknown clock id: {clock}"),
        ));
    }
    Ok(ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9)
}

/// A single clock's RESOLUTION in seconds (`clock_getres`), the companion of
/// [`clock_seconds`].
fn clock_res_seconds(clock: i64) -> Result<f64, crate::Signal> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid out-param; `clock_getres` writes it and nothing else.
    let rc = unsafe { libc::clock_getres(clock as libc::clockid_t, &mut ts) };
    if rc != 0 {
        return Err(raise_error(
            "Errno::EINVAL",
            format!("Invalid argument - unknown clock id: {clock}"),
        ));
    }
    Ok(ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9)
}

/// Render a clock value given in SECONDS as the unit the optional argument
/// requests (default `:float_second`) -- shared by `clock_gettime`/`clock_getres`.
fn clock_in_unit(secs: f64, unit: Option<&RubyValue>) -> Result<RubyValue, crate::Signal> {
    match unit {
        None => Ok(RubyValue::Float(secs)),
        Some(RubyValue::Symbol(s)) => match s.name().as_str() {
            "float_second" => Ok(RubyValue::Float(secs)),
            "float_millisecond" => Ok(RubyValue::Float(secs * 1e3)),
            "float_microsecond" => Ok(RubyValue::Float(secs * 1e6)),
            "second" => Ok(RubyValue::Int(secs as i64)),
            "millisecond" => Ok(RubyValue::Int((secs * 1e3) as i64)),
            "microsecond" => Ok(RubyValue::Int((secs * 1e6) as i64)),
            "nanosecond" => Ok(RubyValue::Int((secs * 1e9) as i64)),
            other => Err(arg_error!("unexpected unit: {other}")),
        },
        Some(other) => Err(arg_error!("unexpected unit: {}", other.inspect_string())),
    }
}

/// Coerce an argument to an `i64` through the `to_int` protocol.
fn int_arg(v: &RubyValue) -> Result<i64, crate::Signal> {
    crate::builtins::convert::to_index(v)
}

// ---------------------------------------------------------------------------
// `Process::Status` -- the object `$?` holds after a `system` or a backtick.
// ---------------------------------------------------------------------------

/// A finished child's wait status. `raw` is the platform `wait()` status word
/// (what `#to_i` answers, and what `ExitStatus` decodes); `pid` is the child
/// that produced it.
pub struct RProcessStatus {
    pid: i64,
    raw: i32,
}

impl RProcessStatus {
    /// The normal-exit code (`nil` when terminated by a signal instead) --
    /// `ExitStatus` owns the platform bit-layout so we don't re-derive it.
    fn exitstatus(&self) -> Option<i32> {
        std::process::ExitStatus::from_raw(self.raw).code()
    }
    /// The terminating signal number, when the child died from one.
    fn termsig(&self) -> Option<i32> {
        std::process::ExitStatus::from_raw(self.raw).signal()
    }
}

impl RubyObject for RProcessStatus {
    fn class_id(&self) -> ClassId {
        PROCESS_STATUS_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    // Immutable once built (a status word never changes), so `freeze` has
    // nothing to guard.
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RProcessStatus {
            pid: self.pid,
            raw: self.raw,
        })
    }
}

fn new_status(pid: i64, raw: i32) -> RubyValue {
    RubyValue::Object(Arc::new(RProcessStatus { pid, raw }))
}

fn recv_status(recv: &RubyValue) -> &RProcessStatus {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RProcessStatus>()
            .expect("Process::Status table row dispatched on a non-Status receiver"),
        _ => panic!("Process::Status table row dispatched on a non-Object receiver"),
    }
}

/// The body of `#to_s` (and, wrapped, `#inspect`): CRuby renders a normal exit
/// as `pid N exit C` and a signal death as `pid N signal S`.
fn status_describe(s: &RProcessStatus) -> String {
    if let Some(code) = s.exitstatus() {
        format!("pid {} exit {}", s.pid, code)
    } else if let Some(sig) = s.termsig() {
        format!("pid {} signal {}", s.pid, sig)
    } else {
        format!("pid {}", s.pid)
    }
}

builtin_methods! {
    pub(crate) fn lookup_status;

    "exitstatus" => fn status_exitstatus(recv, args, _block) {
        arity!(args, 0);
        Ok(match recv_status(recv).exitstatus() {
            Some(code) => RubyValue::Int(code as i64),
            None => RubyValue::Nil,
        })
    }
    // `nil` (not `false`) when the child was signalled rather than exiting --
    // CRuby's own three-valued answer.
    "success?" => fn status_success(recv, args, _block) {
        arity!(args, 0);
        Ok(match recv_status(recv).exitstatus() {
            Some(code) => RubyValue::Bool(code == 0),
            None => RubyValue::Nil,
        })
    }
    "pid" => fn status_pid(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_status(recv).pid))
    }
    "to_i" => fn status_to_i(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_status(recv).raw as i64))
    }
    "exited?" => fn status_exited(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_status(recv).exitstatus().is_some()))
    }
    "signaled?" => fn status_signaled(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_status(recv).termsig().is_some()))
    }
    "termsig" => fn status_termsig(recv, args, _block) {
        arity!(args, 0);
        Ok(match recv_status(recv).termsig() {
            Some(sig) => RubyValue::Int(sig as i64),
            None => RubyValue::Nil,
        })
    }
    "stopped?" => fn status_stopped(recv, args, _block) {
        arity!(args, 0);
        // A reaped child is never in the stopped state (that needs WUNTRACED,
        // which `wait()` doesn't set), so this is always false here.
        let _ = recv_status(recv);
        Ok(RubyValue::Bool(false))
    }
    // `$? == 0` (an Integer) and `$? == other_status` both compare the raw
    // status word, CRuby's rule.
    "==" => fn status_eq(recv, args, _block) {
        arity!(args, 1);
        let me = recv_status(recv).raw as i64;
        Ok(RubyValue::Bool(match &args[0] {
            RubyValue::Int(i) => *i == me,
            RubyValue::Object(o) => o
                .as_any()
                .downcast_ref::<RProcessStatus>()
                .is_some_and(|s| s.raw as i64 == me),
            _ => false,
        }))
    }
    "to_s" => fn status_to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(status_describe(recv_status(recv)))))
    }
    "inspect" => fn status_inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(format!(
            "#<Process::Status: {}>",
            status_describe(recv_status(recv))
        ))))
    }
}

// ---------------------------------------------------------------------------
// `Process::Tms` -- the CPU-times struct `Process.times` answers (utime/stime/
// cutime/cstime, all Float seconds). CRuby makes it an actual Struct; here it
// is a small payload object carrying the four values with matching accessors.
// ---------------------------------------------------------------------------

pub struct RTms {
    utime: f64,
    stime: f64,
    cutime: f64,
    cstime: f64,
}

impl RubyObject for RTms {
    fn class_id(&self) -> ClassId {
        PROCESS_TMS_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RTms {
            utime: self.utime,
            stime: self.stime,
            cutime: self.cutime,
            cstime: self.cstime,
        })
    }
}

fn new_tms(utime: f64, stime: f64, cutime: f64, cstime: f64) -> RubyValue {
    RubyValue::Object(Arc::new(RTms {
        utime,
        stime,
        cutime,
        cstime,
    }))
}

fn recv_tms(recv: &RubyValue) -> &RTms {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RTms>()
            .expect("Process::Tms table row dispatched on a non-Tms receiver"),
        _ => panic!("Process::Tms table row dispatched on a non-Object receiver"),
    }
}

builtin_methods! {
    pub(crate) fn lookup_tms;

    "utime" => fn tms_utime(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(recv_tms(recv).utime))
    }
    "stime" => fn tms_stime(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(recv_tms(recv).stime))
    }
    "cutime" => fn tms_cutime(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(recv_tms(recv).cutime))
    }
    "cstime" => fn tms_cstime(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(recv_tms(recv).cstime))
    }
    "to_a" | "values" => fn tms_to_a(recv, args, _block) {
        arity!(args, 0);
        let t = recv_tms(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Float(t.utime),
            RubyValue::Float(t.stime),
            RubyValue::Float(t.cutime),
            RubyValue::Float(t.cstime),
        ])))
    }
    "to_s" | "inspect" => fn tms_inspect(recv, args, _block) {
        arity!(args, 0);
        let t = recv_tms(recv);
        // Ruby renders a whole-valued Float as `1.0`; `inspect_string` on a
        // Float value is exactly that formatter, so the struct line matches.
        let f = |v: f64| RubyValue::Float(v).inspect_string();
        Ok(RubyValue::Str(crate::string_new(format!(
            "#<struct Process::Tms utime={}, stime={}, cutime={}, cstime={}>",
            f(t.utime), f(t.stime), f(t.cutime), f(t.cstime),
        ))))
    }
}

// ---------------------------------------------------------------------------
// `$?` -- the last child status, thread-local like CRuby's own special global.
// ---------------------------------------------------------------------------

thread_local! {
    static LAST_CHILD_STATUS: RefCell<RubyValue> = const { RefCell::new(RubyValue::Nil) };
}

/// `$?` -- the `Process::Status` of the last `system`/backtick child, or nil.
pub fn last_child_status() -> RubyValue {
    LAST_CHILD_STATUS.with(|c| c.borrow().clone())
}

fn set_last_child_status(v: RubyValue) {
    LAST_CHILD_STATUS.with(|c| *c.borrow_mut() = v);
}

// ---------------------------------------------------------------------------
// Running a command -- shared by `Kernel#system` and the backtick.
// ---------------------------------------------------------------------------

/// CRuby's shell-metacharacter set (`process.c`, `rb_proc_exec`): a
/// single-string command containing any of these runs through `/bin/sh -c`;
/// otherwise it is whitespace-split and exec'd directly. A plain space is NOT
/// a metacharacter -- `system("echo hi")` execs `["echo","hi"]` directly.
const SHELL_META: &[char] = &[
    '*', '?', '{', '}', '[', ']', '<', '>', '(', ')', '~', '&', '|', '\\', '$', ';', '\'', '"',
    '`', '\n',
];

fn needs_shell(cmd: &str) -> bool {
    cmd.chars().any(|c| SHELL_META.contains(&c))
}

fn cmd_str(v: &RubyValue) -> Result<String, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned())
}

/// Build the `Command` for a `system`/backtick argument list. A single string
/// picks the shell-or-direct path per `needs_shell`; multiple arguments always
/// exec directly (`system("prog", "arg", ...)`). `Ok(None)` is an empty
/// command (a blank single string), which the callers turn into their own
/// "nothing ran" answer.
fn build_command(args: &[RubyValue]) -> Result<Option<Command>, Signal> {
    if args.is_empty() {
        return Err(arg_error!(
            "wrong number of arguments (given 0, expected 1+)"
        ));
    }
    if args.len() == 1 {
        let s = cmd_str(&args[0])?;
        if needs_shell(&s) {
            let mut c = Command::new("/bin/sh");
            c.arg("-c").arg(&s);
            return Ok(Some(c));
        }
        let words: Vec<&str> = s.split_whitespace().collect();
        let Some((prog, rest)) = words.split_first() else {
            return Ok(None);
        };
        let mut c = Command::new(prog);
        c.args(rest);
        Ok(Some(c))
    } else {
        let prog = cmd_str(&args[0])?;
        let mut c = Command::new(prog);
        for a in &args[1..] {
            c.arg(cmd_str(a)?);
        }
        Ok(Some(c))
    }
}

/// `Kernel#system` -- runs the command with stdout/stderr inherited, sets `$?`,
/// and answers `true` (exit 0) / `false` (any other exit or a signal) / `nil`
/// (the command could not be executed). Does not raise on a nonzero exit.
pub fn system(
    _recv: &RubyValue,
    args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    set_last_child_status(RubyValue::Nil);
    let Some(mut cmd) = build_command(args)? else {
        return Ok(RubyValue::Bool(false));
    };
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        // Couldn't even start it (e.g. ENOENT on a direct exec) -> nil.
        Err(_) => return Ok(RubyValue::Nil),
    };
    let pid = child.id() as i64;
    // Gvl-released: the child can run arbitrarily long, and an armed
    // (`ZEO_GVL=1`) holder parked in wait(2) must not stall its siblings.
    let status = crate::gvl::without_gvl(|| child.wait())
        .map_err(|e| raise_error("SystemCallError", e.to_string()))?;
    set_last_child_status(new_status(pid, status.into_raw()));
    Ok(RubyValue::Bool(status.success()))
}

/// `Kernel#\`` -- runs the command, captures its stdout (stderr inherited),
/// sets `$?`, and answers the captured output as a String. A nonzero exit does
/// NOT raise; a command that cannot be started raises `Errno::ENOENT`, CRuby's
/// own behaviour.
pub fn backquote(
    _recv: &RubyValue,
    args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    set_last_child_status(RubyValue::Nil);
    let raw_cmd = cmd_str(&args[0])?;
    let Some(mut cmd) = build_command(args)? else {
        return Err(raise_error(
            "Errno::ENOENT",
            "No such file or directory - ".to_string(),
        ));
    };
    cmd.stdout(std::process::Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(raise_error(
                "Errno::ENOENT",
                format!("No such file or directory - {raw_cmd}"),
            ));
        }
        Err(e) => return Err(raise_error("SystemCallError", e.to_string())),
    };
    let pid = child.id() as i64;
    let mut out = Vec::new();
    // Gvl-released like `system`: draining the child's stdout and waiting
    // for its exit both block until the child decides to finish.
    if let Some(mut so) = child.stdout.take() {
        use std::io::Read;
        crate::gvl::without_gvl(|| so.read_to_end(&mut out))
            .map_err(|e| raise_error("IOError", e.to_string()))?;
    }
    let status = crate::gvl::without_gvl(|| child.wait())
        .map_err(|e| raise_error("SystemCallError", e.to_string()))?;
    set_last_child_status(new_status(pid, status.into_raw()));
    // Tagged with the default external encoding, as CRuby's backtick output is.
    Ok(RubyValue::Str(crate::string_from_bytes(
        out,
        crate::encoding::default_external(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process_module() -> RubyValue {
        RubyValue::Class(zeo_abi::PROCESS_CLASS)
    }

    #[test]
    fn pid_answers_this_process() {
        let got = pid(&process_module(), &[], None).unwrap();
        assert!(matches!(got, RubyValue::Int(p) if p == std::process::id() as i64));
    }

    #[test]
    fn ppid_is_a_positive_integer() {
        let got = ppid(&process_module(), &[], None).unwrap();
        assert!(matches!(got, RubyValue::Int(p) if p > 0));
    }

    /// The monotonic clock advances and never goes backwards -- the one
    /// property the benchmark idiom depends on.
    #[test]
    fn the_monotonic_clock_advances() {
        let m = RubyValue::Int(libc::CLOCK_MONOTONIC as i64);
        let RubyValue::Float(a) =
            clock_gettime(&process_module(), std::slice::from_ref(&m), None).unwrap()
        else {
            panic!("expected a Float")
        };
        std::thread::sleep(std::time::Duration::from_millis(2));
        let RubyValue::Float(b) = clock_gettime(&process_module(), &[m], None).unwrap() else {
            panic!("expected a Float")
        };
        assert!(b > a, "monotonic clock went backwards: {a} -> {b}");
    }

    /// The unit argument scales the answer and picks Int vs Float, CRuby's
    /// own contract.
    #[test]
    fn the_unit_argument_scales_and_types_the_answer() {
        let m = RubyValue::Int(libc::CLOCK_MONOTONIC as i64);
        let ms = clock_gettime(
            &process_module(),
            &[
                m.clone(),
                RubyValue::Symbol(crate::Symbol::intern("millisecond")),
            ],
            None,
        )
        .unwrap();
        assert!(matches!(ms, RubyValue::Int(_)));

        let fs = clock_gettime(
            &process_module(),
            &[m, RubyValue::Symbol(crate::Symbol::intern("float_second"))],
            None,
        )
        .unwrap();
        assert!(matches!(fs, RubyValue::Float(_)));
    }

    /// An unknown unit raises rather than silently answering seconds.
    #[test]
    fn an_unknown_unit_raises() {
        let r = std::panic::catch_unwind(|| {
            clock_gettime(
                &process_module(),
                &[
                    RubyValue::Int(libc::CLOCK_MONOTONIC as i64),
                    RubyValue::Symbol(crate::Symbol::intern("fortnights")),
                ],
                None,
            )
        });
        // Registry-less, `raise_error` surfaces as a panic.
        assert!(r.is_err());
    }

    #[test]
    fn lookup_finds_the_process_names() {
        assert!(lookup_class("pid").is_some());
        assert!(lookup_class("clock_gettime").is_some());
        assert!(lookup_class("nope").is_none());
    }
}
