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
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::builtins::{arg_error, arity, type_error};
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

    def self.pid(_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(std::process::id() as i64))
    }
    // `Process._fork` (process.c) -- the low-level primitive `Process.fork` /
    // `Kernel#fork` build on. Gems hook fork by prepending a module onto
    // `Process.singleton_class` and overriding `_fork` (connection_pool's
    // `ForkTracker`, whose `super` lands here). Performs the real fork(2) and
    // answers the child pid (0 in the child), matching CRuby on a fork-capable
    // platform; it only ever runs if the program actually forks.
    def self._fork(_recv, *args, &_block) {
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
    def self.fork(recv, *args, &block) {
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
    def self.kill(_recv, *args, &_block) {
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
    // `Process.exit`, `Process.exit!`, `Process.abort` are the module-function
    // twins of the `Kernel#` forms and share their exact semantics, so they
    // delegate rather than reimplement: `exit` raises a catchable `SystemExit`
    // that unwinds `ensure`/runs `at_exit`; `exit!` is the uncatchable immediate
    // `_exit(2)` (no unwinding, no `at_exit`); `abort` writes its message to
    // stderr first, then raises `SystemExit` with status 1.
    def self.exit(_recv, *args, &_block) {
        arity!(args, 0..=1);
        Err(crate::builtins::kernel::kernel_exit(args))
    }
    def self."exit!"(_recv, *args, &_block) {
        arity!(args, 0..=1);
        crate::builtins::kernel::kernel_exit_bang(args)
    }
    def self.abort(_recv, *args, &_block) {
        arity!(args, 0..=1);
        Err(crate::builtins::kernel::kernel_abort(args))
    }
    // `Process.ppid` has no portable std equivalent; libc's getppid is the
    // honest answer rather than a fabricated one.
    def self.ppid(_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getppid() } as i64))
    }
    def self.clock_gettime(_recv, *args, &_block) {
        arity!(args, 1..=2);
        let clock = int_arg(&args[0])?;
        clock_in_unit(clock_seconds(clock)?, args.get(1))
    }
    // `Process.clock_getres(clock_id [, unit])` -- the clock's resolution, in the
    // same units `clock_gettime` accepts (default a Float of seconds).
    def self.clock_getres(_recv, *args, &_block) {
        arity!(args, 1..=2);
        let clock = int_arg(&args[0])?;
        clock_in_unit(clock_res_seconds(clock)?, args.get(1))
    }
    // Real/effective user and group ids (libc getuid/geteuid/getgid/getegid).
    def self.uid(_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getuid() } as i64))
    }
    def self.euid(_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::geteuid() } as i64))
    }
    def self.gid(_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getgid() } as i64))
    }
    def self.egid(_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getegid() } as i64))
    }
    // `Process.getpgrp` -- the current process group id.
    def self.getpgrp(_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getpgrp() } as i64))
    }
    // `Process.getsid([pid])` -- the session id of `pid` (0/none = this process).
    def self.getsid(_recv, *args, &_block) {
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
    def self.getpriority(_recv, *args, &_block) {
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
    def self.groups(_recv, *args, &_block) {
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
    def self.times(_recv, *args, &_block) {
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
    def self.getpgid(_recv, *args, &_block) {
        arity!(args, 1);
        let pid = int_arg(&args[0])? as libc::pid_t;
        let pgid = unsafe { libc::getpgid(pid) };
        if pgid < 0 {
            return Err(errno_fail("getpgid"));
        }
        Ok(RubyValue::Int(pgid as i64))
    }
    // `Process.setpgid(pid, pgrp)` -- put `pid` into process group `pgrp`.
    def self.setpgid(_recv, *args, &_block) {
        arity!(args, 2);
        let pid = int_arg(&args[0])? as libc::pid_t;
        let pgrp = int_arg(&args[1])? as libc::pid_t;
        if unsafe { libc::setpgid(pid, pgrp) } != 0 {
            return Err(errno_fail("setpgid"));
        }
        Ok(RubyValue::Int(0))
    }
    // `Process.setpgrp` -- make this process a group leader (setpgid(0, 0)).
    def self.setpgrp(_recv, *args, &_block) {
        arity!(args, 0);
        if unsafe { libc::setpgid(0, 0) } != 0 {
            return Err(errno_fail("setpgrp"));
        }
        Ok(RubyValue::Int(0))
    }
    // `Process.setsid` -- start a new session; answers the new session id.
    def self.setsid(_recv, *args, &_block) {
        arity!(args, 0);
        let sid = unsafe { libc::setsid() };
        if sid < 0 {
            return Err(errno_fail("setsid"));
        }
        Ok(RubyValue::Int(sid as i64))
    }
    // `Process.setpriority(which, who, prio)` -- set a scheduling priority.
    def self.setpriority(_recv, *args, &_block) {
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
    def self."uid="(_recv, *args, &_block) {
        arity!(args, 1);
        let id = int_arg(&args[0])?;
        if unsafe { libc::setuid(id as libc::uid_t) } != 0 {
            return Err(errno_fail("setuid"));
        }
        Ok(RubyValue::Int(id))
    }
    def self."gid="(_recv, *args, &_block) {
        arity!(args, 1);
        let id = int_arg(&args[0])?;
        if unsafe { libc::setgid(id as libc::gid_t) } != 0 {
            return Err(errno_fail("setgid"));
        }
        Ok(RubyValue::Int(id))
    }
    def self."euid="(_recv, *args, &_block) {
        arity!(args, 1);
        let id = int_arg(&args[0])?;
        if unsafe { libc::seteuid(id as libc::uid_t) } != 0 {
            return Err(errno_fail("seteuid"));
        }
        Ok(RubyValue::Int(id))
    }
    def self."egid="(_recv, *args, &_block) {
        arity!(args, 1);
        let id = int_arg(&args[0])?;
        if unsafe { libc::setegid(id as libc::gid_t) } != 0 {
            return Err(errno_fail("setegid"));
        }
        Ok(RubyValue::Int(id))
    }
    // `Process.groups = [gid, ...]` -- replace the supplementary groups; answers the arg.
    def self."groups="(_recv, *args, &_block) {
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
    def self.getrlimit(_recv, *args, &_block) {
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
    def self.setrlimit(_recv, *args, &_block) {
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
    def self.maxgroups(_recv, *args, &_block) {
        arity!(args, 0);
        // CRuby caps the reported ceiling at the system NGROUPS_MAX (16 on
        // macOS), so a larger stored bound reads back clamped.
        let stored = PROCESS_MAXGROUPS.load(Ordering::Relaxed);
        let cap = unsafe { libc::sysconf(libc::_SC_NGROUPS_MAX) };
        Ok(RubyValue::Int(if cap > 0 { stored.min(cap) } else { stored }))
    }
    def self."maxgroups="(_recv, *args, &_block) {
        arity!(args, 1);
        let n = int_arg(&args[0])?;
        PROCESS_MAXGROUPS.store(n, Ordering::Relaxed);
        Ok(RubyValue::Int(n))
    }
    // `Process.argv0` -- the program name (`$0`) at startup.
    def self.argv0(_recv, *args, &_block) {
        arity!(args, 0);
        Ok(crate::globals::global_get(0, "$0"))
    }
    // `Process.setproctitle(str)` -- answers the title. macOS has no portable
    // setproctitle, so the cosmetic title itself is a best-effort no-op.
    def self.setproctitle(_recv, *args, &_block) {
        arity!(args, 1);
        crate::builtins::convert::to_str(&args[0])
    }
    // `Process.warmup` -- a JIT/heap warmup hint; nothing to warm here.
    def self.warmup(_recv, *args, &_block) {
        arity!(args, 0..=1);
        Ok(RubyValue::Bool(true))
    }
    // `Process.initgroups(username, gid)` -- set the supplementary group list
    // from the group database for `username` plus `gid`; answers the new
    // groups. Typically root-only.
    def self.initgroups(_recv, *args, &_block) {
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
    def self.daemon(_recv, *args, &_block) {
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
    // `Process.wait([pid = -1, flags = 0])` / its alias `Process.waitpid` --
    // reap one child, answer its pid, and set `$?` to its `Process::Status`.
    // A default pid of -1 waits for any child; `WNOHANG` makes a not-yet-exited
    // child answer `nil` (with `$?` cleared) instead of blocking. No children
    // at all raises `Errno::ECHILD`, CRuby's own behaviour.
    def self."wait" | "waitpid"(_recv, *args, &_block) {
        arity!(args, 0..=2);
        let pid = args.first().map(int_arg).transpose()?.unwrap_or(-1);
        let flags = args.get(1).map(int_arg).transpose()?.unwrap_or(0);
        match raw_waitpid(pid, flags)? {
            Some((reaped, raw)) => {
                set_last_child_status(new_status(reaped, raw));
                Ok(RubyValue::Int(reaped))
            }
            None => {
                set_last_child_status(RubyValue::Nil);
                Ok(RubyValue::Nil)
            }
        }
    }
    // `Process.wait2` / `Process.waitpid2` -- like `wait`, but the answer is the
    // `[pid, Process::Status]` pair (still setting `$?`), or `nil` under
    // `WNOHANG` with no ready child.
    def self."wait2" | "waitpid2"(_recv, *args, &_block) {
        arity!(args, 0..=2);
        let pid = args.first().map(int_arg).transpose()?.unwrap_or(-1);
        let flags = args.get(1).map(int_arg).transpose()?.unwrap_or(0);
        match raw_waitpid(pid, flags)? {
            Some((reaped, raw)) => {
                let status = new_status(reaped, raw);
                set_last_child_status(status.clone());
                Ok(RubyValue::Array(crate::array_new(vec![
                    RubyValue::Int(reaped),
                    status,
                ])))
            }
            None => {
                set_last_child_status(RubyValue::Nil);
                Ok(RubyValue::Nil)
            }
        }
    }
    // `Process.waitall` -- reap EVERY child, answering an Array of
    // `[pid, Process::Status]` pairs (empty when there were none). `$?` ends as
    // the last reaped child's status, matching CRuby.
    def self.waitall(_recv, *args, &_block) {
        arity!(args, 0);
        let mut pairs = Vec::new();
        loop {
            match raw_waitpid(-1, 0) {
                Ok(Some((reaped, raw))) => {
                    let status = new_status(reaped, raw);
                    set_last_child_status(status.clone());
                    pairs.push(RubyValue::Array(crate::array_new(vec![
                        RubyValue::Int(reaped),
                        status,
                    ])));
                }
                // ECHILD is the normal "no more children" terminator, not an
                // error here -- every other errno propagates.
                Ok(None) => break,
                Err(_) => break,
            }
        }
        Ok(RubyValue::Array(crate::array_new(pairs)))
    }
    // `Process.last_status` -- the `$?` of the current thread: the
    // `Process::Status` of the last child this thread waited on, or `nil`.
    def self.last_status(_recv, *args, &_block) {
        arity!(args, 0);
        Ok(last_child_status())
    }
    // `Process.detach(pid)` -- reap `pid` in the BACKGROUND so it never lingers
    // as a zombie, answering a Thread whose `#value` is the child's
    // `Process::Status`. CRuby returns a `Process::Waiter` (a Thread subclass
    // exposing `#pid`); zeo returns a plain Thread, which covers the usual
    // `detach(pid).join` / `.value` contract. The reaper runs on its own
    // thread, so it does not touch the caller's `$?`.
    def self.detach(_recv, *args, &_block) {
        arity!(args, 1);
        Ok(detach_thread(int_arg(&args[0])?))
    }
    // `Process.spawn([env,] command... [,options])` -- start a child WITHOUT
    // waiting (unlike `system`), answering its pid; the child is reapable with
    // `Process.wait`. `build_spawn_command` handles the leading env hash, the
    // command forms (shell string / direct argv / `[cmdname, argv0]`), and the
    // trailing options hash (`:chdir`, `:unsetenv_others`, `:in`/`:out`/`:err`
    // redirects to files or to each other). Std fds are inherited unless
    // redirected. See that helper for the options not yet honored.
    def self.spawn(_recv, *args, &_block) {
        spawn_pid(args)
    }
    // `Process.exec([env,] command... [,options])` -- REPLACE the current
    // process image (execvp), never returning on success. Shares spawn's
    // argument parsing, so env/chdir/redirects apply to THIS process in the
    // instant before the exec. A failure to exec raises the matching Errno.
    def self.exec(_recv, *args, &_block) {
        let mut cmd = build_spawn_command(args)?;
        // `CommandExt::exec` returns only on failure (it diverges on success).
        Err(spawn_error(&cmd.exec()))
    }

    // `Process::Status` -- the object `$?` holds after a wait/system/backtick.
    // Its instances are the `RProcessStatus` payload (defined below); the
    // accessors read the raw wait-status word it carries. Nested here so the one
    // process.rs owns the whole `Process` namespace, Ruby-style.
    class Status = zeo_abi::PROCESS_STATUS_CLASS < zeo_abi::OBJECT_CLASS {
        def "exitstatus"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(match recv_status(recv).exitstatus() {
                Some(code) => RubyValue::Int(code as i64),
                None => RubyValue::Nil,
            })
        }
        // `nil` (not `false`) when the child was signalled rather than exiting --
        // CRuby's own three-valued answer.
        def "success?"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(match recv_status(recv).exitstatus() {
                Some(code) => RubyValue::Bool(code == 0),
                None => RubyValue::Nil,
            })
        }
        def "pid"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Int(recv_status(recv).pid))
        }
        def "to_i"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Int(recv_status(recv).raw as i64))
        }
        def "exited?"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Bool(recv_status(recv).exitstatus().is_some()))
        }
        def "signaled?"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Bool(recv_status(recv).termsig().is_some()))
        }
        def "termsig"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(match recv_status(recv).termsig() {
                Some(sig) => RubyValue::Int(sig as i64),
                None => RubyValue::Nil,
            })
        }
        def "stopped?"(recv, *args, &_block) {
            arity!(args, 0);
            // A reaped child is never in the stopped state (that needs WUNTRACED,
            // which `wait()` doesn't set), so this is always false here.
            let _ = recv_status(recv);
            Ok(RubyValue::Bool(false))
        }
        // `$? == 0` (an Integer) and `$? == other_status` both compare the raw
        // status word, CRuby's rule.
        def "=="(recv, *args, &_block) {
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
        def "to_s"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Str(crate::string_new(status_describe(recv_status(recv)))))
        }
        def "inspect"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Str(crate::string_new(format!(
                "#<Process::Status: {}>",
                status_describe(recv_status(recv))
            ))))
        }
    }

    // `Process::Tms` -- the CPU-times struct `Process.times` answers. Instances
    // are the `RTms` payload (defined below) carrying utime/stime/cutime/cstime
    // as Float seconds.
    class Tms = zeo_abi::PROCESS_TMS_CLASS < zeo_abi::OBJECT_CLASS {
        include zeo_abi::COMPARABLE_CLASS;

        def "utime"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Float(recv_tms(recv).utime))
        }
        def "stime"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Float(recv_tms(recv).stime))
        }
        def "cutime"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Float(recv_tms(recv).cutime))
        }
        def "cstime"(recv, *args, &_block) {
            arity!(args, 0);
            Ok(RubyValue::Float(recv_tms(recv).cstime))
        }
        def "to_a" | "values"(recv, *args, &_block) {
            arity!(args, 0);
            let t = recv_tms(recv);
            Ok(RubyValue::Array(crate::array_new(vec![
                RubyValue::Float(t.utime),
                RubyValue::Float(t.stime),
                RubyValue::Float(t.cutime),
                RubyValue::Float(t.cstime),
            ])))
        }
        def "to_s" | "inspect"(recv, *args, &_block) {
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
}

/// The stored `Process.maxgroups` bound. Defaults to CRuby's `RB_MAX_GROUPS`;
/// the reader clamps it to the runtime `NGROUPS_MAX`.
static PROCESS_MAXGROUPS: AtomicI64 = AtomicI64::new(65536);

/// Raise the current `errno` as the matching `Errno::*` (via File's mapper, the
/// single place errno -> exception-class lives).
fn errno_fail(syscall: &str) -> crate::Signal {
    crate::builtins::file::raise_errno(&std::io::Error::last_os_error(), syscall, "")
}

/// One `waitpid(2)`, in zeo's terms: `Ok(Some((pid, raw)))` for a reaped child,
/// `Ok(None)` when `WNOHANG` finds no child ready yet (`ret == 0`), and an
/// `Err` otherwise. No children left is `Errno::ECHILD` ("No child processes"),
/// which `raise_errno` does not name, so it is raised explicitly; `EINTR` is
/// retried, as CRuby's own wait loop does. The syscall runs GVL-released so a
/// blocking wait cannot stall sibling threads.
pub(crate) fn raw_waitpid(pid: i64, flags: i64) -> Result<Option<(i64, i32)>, Signal> {
    loop {
        let mut raw: libc::c_int = 0;
        let ret = crate::gvl::without_gvl(|| unsafe {
            libc::waitpid(pid as libc::pid_t, &mut raw, flags as libc::c_int)
        });
        if ret > 0 {
            return Ok(Some((ret as i64, raw)));
        }
        if ret == 0 {
            return Ok(None);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::ECHILD) => {
                return Err(raise_error(
                    "Errno::ECHILD",
                    "No child processes".to_string(),
                ));
            }
            _ => return Err(errno_fail("waitpid")),
        }
    }
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

pub(crate) fn new_status(pid: i64, raw: i32) -> RubyValue {
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

pub(crate) fn set_last_child_status(v: RubyValue) {
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

// ---------------------------------------------------------------------------
// `Process.spawn`/`Process.exec` -- the fuller launcher: an optional leading
// `env` hash and trailing `options` hash bracket the command words.
// ---------------------------------------------------------------------------

/// Whether an options-hash symbol key (`:unsetenv_others`) is present with a
/// truthy value.
fn hash_truthy(h: &crate::collections::RHash, key: &str) -> bool {
    let v = crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern(key)));
    !v.is_nil() && !matches!(v, RubyValue::Bool(false))
}

/// The background reaper `Process.detach` answers: a Thread whose value is the
/// child's `Process::Status` (nil if the pid was already gone). Also what
/// `PTY.spawn`'s block form leaves behind, CRuby's own detach call there.
pub(crate) fn detach_thread(pid: i64) -> RubyValue {
    let reaper = crate::RProc::new(move |_args: &[RubyValue]| {
        match raw_waitpid(pid, 0) {
            Ok(Some((reaped, raw))) => Ok(new_status(reaped, raw)),
            // A pid that is already gone (or never ours) yields nil rather
            // than propagating ECHILD out of the detached thread.
            _ => Ok(RubyValue::Nil),
        }
    });
    crate::thread::thread_new(RubyValue::Proc(reaper), Vec::new())
}

/// Start a child without waiting, answering its pid -- the shared engine of
/// `Process.spawn` and `Kernel#spawn` (Open3 calls the latter receiverless).
pub(crate) fn spawn_pid(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let mut cmd = build_spawn_command(args)?;
    match cmd.spawn() {
        Ok(child) => Ok(RubyValue::Int(child.id() as i64)),
        Err(e) => Err(spawn_error(&e)),
    }
}

/// Turn a `spawn`/`exec` argument list into a ready `Command`: peel an optional
/// leading `env` Hash and trailing `options` Hash (CRuby keys on POSITION, not
/// key type), build the command from what's left, then layer env + options on.
pub(crate) fn build_spawn_command(args: &[RubyValue]) -> Result<Command, Signal> {
    let mut rest = args;

    // A leading Hash is the environment; a trailing Hash is the options. Order,
    // not key type, is what tells them apart (`spawn(env, cmd, opts)`).
    let env = match rest.first() {
        Some(v @ RubyValue::Hash(_)) => {
            let v = v.clone();
            rest = &rest[1..];
            Some(v)
        }
        _ => None,
    };
    let opts = match rest.last() {
        Some(v @ RubyValue::Hash(_)) => {
            let v = v.clone();
            rest = &rest[..rest.len() - 1];
            Some(v)
        }
        _ => None,
    };
    if rest.is_empty() {
        return Err(arg_error!("no command given"));
    }

    let mut cmd = build_exec_argv(rest)?;

    // `:unsetenv_others` wipes the inherited environment before the explicit
    // pairs are applied; a nil value unsets a single variable.
    if let Some(RubyValue::Hash(o)) = &opts {
        if hash_truthy(o, "unsetenv_others") {
            cmd.env_clear();
        }
    }
    if let Some(RubyValue::Hash(e)) = &env {
        for (k, v) in crate::collections::hash_pairs(e) {
            let key = cmd_str(&k)?;
            if v.is_nil() {
                cmd.env_remove(&key);
            } else {
                cmd.env(&key, cmd_str(&v)?);
            }
        }
    }
    if let Some(RubyValue::Hash(o)) = &opts {
        apply_spawn_options(&mut cmd, o)?;
    }
    Ok(cmd)
}

/// Build the `Command` for the command WORDS (env/options already stripped). A
/// lone string runs through the shell when it holds a metacharacter, else it is
/// exec'd directly; a `[cmdname, argv0]` first element sets an explicit argv[0];
/// multiple arguments always exec directly.
fn build_exec_argv(args: &[RubyValue]) -> Result<Command, Signal> {
    // A `[cmdname, argv0]` pair anywhere a program name is expected.
    let program = |v: &RubyValue| -> Result<Command, Signal> {
        if let RubyValue::Array(a) = v {
            let a = a.lock();
            if a.len() != 2 {
                return Err(arg_error!("wrong first argument"));
            }
            let mut c = Command::new(cmd_str(&a[0])?);
            c.arg0(cmd_str(&a[1])?);
            return Ok(c);
        }
        Ok(Command::new(cmd_str(v)?))
    };

    if args.len() == 1 {
        if let RubyValue::Str(_) = &args[0] {
            let s = cmd_str(&args[0])?;
            if needs_shell(&s) {
                let mut c = Command::new("/bin/sh");
                c.arg("-c").arg(&s);
                return Ok(c);
            }
            let words: Vec<&str> = s.split_whitespace().collect();
            let Some((prog, rest)) = words.split_first() else {
                return Err(arg_error!("empty command"));
            };
            let mut c = Command::new(prog);
            c.args(rest);
            return Ok(c);
        }
        // A single `[cmdname, argv0]` with no further arguments.
        return program(&args[0]);
    }

    let mut c = program(&args[0])?;
    for a in &args[1..] {
        c.arg(cmd_str(a)?);
    }
    Ok(c)
}

/// Apply the `options` hash to `cmd`: `:chdir`, and `:in`/`:out`/`:err` (or the
/// `[:out, :err]` pair) redirects to a file, or to each other
/// (`:err => :out` / `[:child, :out]`). File targets are collected first and
/// wired up after the loop so an `:err => :out` written before `:out => file`
/// still lands on the file. Options this does not model yet (`:pgroup`,
/// `:umask`, `:close_others`, bare-fd-number keys, IO-object targets) are
/// ignored rather than raising, keeping more programs runnable.
fn apply_spawn_options(cmd: &mut Command, opts: &crate::collections::RHash) -> Result<(), Signal> {
    use std::process::Stdio;

    let mut out: Option<std::fs::File> = None;
    let mut err: Option<std::fs::File> = None;
    let mut err_to_out = false;
    let mut out_to_err = false;

    for (k, v) in crate::collections::hash_pairs(opts) {
        match option_key(&k) {
            OptKey::UnsetenvOthers => {} // applied in build_spawn_command
            OptKey::Chdir => {
                cmd.current_dir(cmd_str(&v)?);
            }
            OptKey::In => {
                let f = match crate::builtins::io::dup_fd_file(&v) {
                    Some(f) => f,
                    None => open_redirect_file(&v, false)?,
                };
                cmd.stdin(Stdio::from(f));
            }
            // `merge_fd` catches `:out => :err` / `[:child, :err]`; an IO value
            // (Open3's pipe ends) hands its fd over; anything else is a file
            // (or `[name, mode]`) target.
            OptKey::Out => match merge_fd(&v) {
                Some(2) => out_to_err = true,
                Some(_) => {}
                None => {
                    out = Some(match crate::builtins::io::dup_fd_file(&v) {
                        Some(f) => f,
                        None => open_redirect_file(&v, true)?,
                    })
                }
            },
            OptKey::Err => match merge_fd(&v) {
                Some(1) => err_to_out = true,
                Some(_) => {}
                None => {
                    err = Some(match crate::builtins::io::dup_fd_file(&v) {
                        Some(f) => f,
                        None => open_redirect_file(&v, true)?,
                    })
                }
            },
            OptKey::OutErr => {
                let f = match crate::builtins::io::dup_fd_file(&v) {
                    Some(f) => f,
                    None => open_redirect_file(&v, true)?,
                };
                // A dup (try_clone) shares the file offset, so the two streams
                // interleave into one file as CRuby's shared-fd redirect does.
                out = Some(f.try_clone().map_err(|e| spawn_error(&e))?);
                err = Some(f);
            }
            OptKey::Unsupported => {}
        }
    }

    // fd->fd merges resolve against whatever the other stream became, whichever
    // order the keys appeared in.
    if err_to_out {
        if let Some(f) = &out {
            err = Some(f.try_clone().map_err(|e| spawn_error(&e))?);
        }
    }
    if out_to_err {
        if let Some(f) = &err {
            out = Some(f.try_clone().map_err(|e| spawn_error(&e))?);
        }
    }
    if let Some(f) = out {
        cmd.stdout(Stdio::from(f));
    }
    if let Some(f) = err {
        cmd.stderr(Stdio::from(f));
    }
    Ok(())
}

/// The recognized `options`-hash keys.
enum OptKey {
    Chdir,
    UnsetenvOthers,
    In,
    Out,
    Err,
    OutErr,
    Unsupported,
}

/// Classify an options-hash KEY: the `:chdir`/`:unsetenv_others` symbols, the
/// `:in`/`:out`/`:err` redirect symbols (or their fd-number equals 0/1/2), and
/// the `[:out, :err]` pair. Everything else is `Unsupported` (ignored).
fn option_key(k: &RubyValue) -> OptKey {
    match k {
        RubyValue::Symbol(s) => match s.name().as_str() {
            "chdir" => OptKey::Chdir,
            "unsetenv_others" => OptKey::UnsetenvOthers,
            "in" => OptKey::In,
            "out" => OptKey::Out,
            "err" => OptKey::Err,
            _ => OptKey::Unsupported,
        },
        RubyValue::Int(0) => OptKey::In,
        RubyValue::Int(1) => OptKey::Out,
        RubyValue::Int(2) => OptKey::Err,
        RubyValue::Array(a) => {
            // `[:out, :err] => target` -- both streams to one destination.
            let a = a.lock();
            let names: Vec<String> = a
                .iter()
                .filter_map(|e| match e {
                    RubyValue::Symbol(s) => Some(s.name()),
                    _ => None,
                })
                .collect();
            if names.len() == 2
                && names.contains(&"out".to_string())
                && names.contains(&"err".to_string())
            {
                OptKey::OutErr
            } else {
                OptKey::Unsupported
            }
        }
        _ => OptKey::Unsupported,
    }
}

/// If a redirect VALUE names another standard stream (`:in`/`:out`/`:err`, an
/// fd number 0/1/2, or a `[:child, :out]`-style pair), answer that fd -- an
/// fd->fd merge rather than a file target.
fn merge_fd(v: &RubyValue) -> Option<i32> {
    let fd_of = |name: &str| match name {
        "in" => Some(0),
        "out" => Some(1),
        "err" => Some(2),
        _ => None,
    };
    match v {
        RubyValue::Symbol(s) => fd_of(&s.name()),
        RubyValue::Int(n @ (0..=2)) => Some(*n as i32),
        RubyValue::Array(a) => {
            // `[:child, :out]` / `[:child, 1]` -- the child's own fd.
            let a = a.lock();
            match (a.first(), a.get(1)) {
                (Some(RubyValue::Symbol(c)), Some(target)) if c.name() == "child" => match target {
                    RubyValue::Symbol(s) => fd_of(&s.name()),
                    RubyValue::Int(n @ (0..=2)) => Some(*n as i32),
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

/// Open a redirect target for `spawn`/`exec`. The spec is a filename String, or
/// a `[name, mode]` / `[name, mode, perm]` Array; `write` picks the default mode
/// (`"w"` create+truncate vs `"r"`) when none is given.
fn open_redirect_file(spec: &RubyValue, write: bool) -> Result<std::fs::File, Signal> {
    let (path, mode) = match spec {
        RubyValue::Array(a) => {
            let a = a.lock();
            let path = cmd_str(a.first().ok_or_else(|| arg_error!("empty redirect"))?)?;
            let mode = match a.get(1) {
                Some(m) => cmd_str(m)?,
                None => String::new(),
            };
            (path, mode)
        }
        _ => (cmd_str(spec)?, String::new()),
    };

    let mut o = std::fs::OpenOptions::new();
    match mode.as_str() {
        "r" => o.read(true),
        "w" => o.write(true).create(true).truncate(true),
        "a" => o.append(true).create(true),
        "r+" => o.read(true).write(true),
        "w+" => o.read(true).write(true).create(true).truncate(true),
        "a+" => o.read(true).append(true).create(true),
        // No explicit mode: read for `:in`, create+truncate-for-write otherwise.
        _ if write => o.write(true).create(true).truncate(true),
        _ => o.read(true),
    };
    o.open(&path)
        .map_err(|e| crate::builtins::file::raise_errno(&e, "open", &path))
}

/// Map a `spawn`/`exec` failure (`Command::spawn`/`exec`) to the matching
/// `Errno` exception, CRuby's own behaviour (a missing program is
/// `Errno::ENOENT`).
pub(crate) fn spawn_error(e: &std::io::Error) -> Signal {
    crate::builtins::file::raise_errno(e, "exec", "")
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

    /// The `ruby_module!`-generated class methods are reachable only through
    /// the dispatch table (their Rust fn names are mangled), so the tests call
    /// them the way real dispatch does -- through `Process`'s registered
    /// class-method `lookup`.
    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::PROCESS_CLASS)
            .expect("Process is a registered builtin table")
            .class
            .as_ref()
            .expect("Process has class methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Process.{name} is defined"))
    }

    #[test]
    fn pid_answers_this_process() {
        let got = cmethod("pid")(&process_module(), &[], None).unwrap();
        assert!(matches!(got, RubyValue::Int(p) if p == std::process::id() as i64));
    }

    #[test]
    fn ppid_is_a_positive_integer() {
        let got = cmethod("ppid")(&process_module(), &[], None).unwrap();
        assert!(matches!(got, RubyValue::Int(p) if p > 0));
    }

    /// The monotonic clock advances and never goes backwards -- the one
    /// property the benchmark idiom depends on.
    #[test]
    fn the_monotonic_clock_advances() {
        let m = RubyValue::Int(libc::CLOCK_MONOTONIC as i64);
        let RubyValue::Float(a) =
            cmethod("clock_gettime")(&process_module(), std::slice::from_ref(&m), None).unwrap()
        else {
            panic!("expected a Float")
        };
        std::thread::sleep(std::time::Duration::from_millis(2));
        let RubyValue::Float(b) = cmethod("clock_gettime")(&process_module(), &[m], None).unwrap()
        else {
            panic!("expected a Float")
        };
        assert!(b > a, "monotonic clock went backwards: {a} -> {b}");
    }

    /// The unit argument scales the answer and picks Int vs Float, CRuby's
    /// own contract.
    #[test]
    fn the_unit_argument_scales_and_types_the_answer() {
        let m = RubyValue::Int(libc::CLOCK_MONOTONIC as i64);
        let ms = cmethod("clock_gettime")(
            &process_module(),
            &[
                m.clone(),
                RubyValue::Symbol(crate::Symbol::intern("millisecond")),
            ],
            None,
        )
        .unwrap();
        assert!(matches!(ms, RubyValue::Int(_)));

        let fs = cmethod("clock_gettime")(
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
            cmethod("clock_gettime")(
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
        let tbl = crate::builtins::registered_table(zeo_abi::PROCESS_CLASS)
            .unwrap()
            .class
            .as_ref()
            .unwrap();
        assert!((tbl.lookup)("pid").is_some());
        assert!((tbl.lookup)("clock_gettime").is_some());
        assert!((tbl.lookup)("wait").is_some());
        assert!((tbl.lookup)("nope").is_none());
    }

    /// Reach a NESTED class's (`Process::Status`/`Process::Tms`) instance method
    /// through its own registered instance table -- the same dispatch path real
    /// code takes, and proof the nested `ruby_class!` blocks register at all.
    fn imethod(id: ClassId, name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(id)
            .unwrap_or_else(|| panic!("class {id:?} is registered"))
            .instance
            .as_ref()
            .expect("class has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("#{name} is defined"))
    }

    fn call(
        f: crate::builtins::BuiltinMethodFn,
        recv: &RubyValue,
        args: &[RubyValue],
    ) -> RubyValue {
        f(recv, args, None).expect("method should not raise")
    }

    // `RubyValue` has no Rust `PartialEq` (equality is dispatched), so the tests
    // project each result to a primitive to assert on.
    fn int(v: &RubyValue) -> i64 {
        match v {
            RubyValue::Int(i) => *i,
            other => panic!("expected an Integer, got {}", other.inspect_string()),
        }
    }
    fn float(v: &RubyValue) -> f64 {
        match v {
            RubyValue::Float(f) => *f,
            other => panic!("expected a Float, got {}", other.inspect_string()),
        }
    }
    fn boolean(v: &RubyValue) -> bool {
        match v {
            RubyValue::Bool(b) => *b,
            other => panic!("expected a Boolean, got {}", other.inspect_string()),
        }
    }
    fn text(v: &RubyValue) -> String {
        match v {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected a String, got {}", other.inspect_string()),
        }
    }

    // -- Process::Status (nested class) -------------------------------------

    /// A normal exit: `raw = code << 8` on every Unix, so `WEXITSTATUS` reads
    /// the code back and `WIFSIGNALED` is false.
    #[test]
    fn status_reads_a_normal_exit() {
        let s = new_status(4242, 7 << 8);
        assert_eq!(
            int(&call(imethod(PROCESS_STATUS_CLASS, "exitstatus"), &s, &[])),
            7
        );
        assert!(!boolean(&call(
            imethod(PROCESS_STATUS_CLASS, "success?"),
            &s,
            &[]
        )));
        assert!(boolean(&call(
            imethod(PROCESS_STATUS_CLASS, "exited?"),
            &s,
            &[]
        )));
        assert!(!boolean(&call(
            imethod(PROCESS_STATUS_CLASS, "signaled?"),
            &s,
            &[]
        )));
        assert!(call(imethod(PROCESS_STATUS_CLASS, "termsig"), &s, &[]).is_nil());
        assert!(!boolean(&call(
            imethod(PROCESS_STATUS_CLASS, "stopped?"),
            &s,
            &[]
        )));
        assert_eq!(
            int(&call(imethod(PROCESS_STATUS_CLASS, "pid"), &s, &[])),
            4242
        );
        assert_eq!(
            int(&call(imethod(PROCESS_STATUS_CLASS, "to_i"), &s, &[])),
            (7 << 8) as i64
        );
        assert_eq!(
            text(&call(imethod(PROCESS_STATUS_CLASS, "to_s"), &s, &[])),
            "pid 4242 exit 7"
        );
    }

    /// Exit code 0 is the only `success? == true` case.
    #[test]
    fn status_zero_exit_is_the_only_success() {
        let ok = new_status(1, 0);
        assert_eq!(
            int(&call(imethod(PROCESS_STATUS_CLASS, "exitstatus"), &ok, &[])),
            0
        );
        assert!(boolean(&call(
            imethod(PROCESS_STATUS_CLASS, "success?"),
            &ok,
            &[]
        )));
    }

    /// A signal death: `raw = signo`, so `exitstatus`/`success?` are nil (the
    /// three-valued answer) and `termsig` carries the number.
    #[test]
    fn status_reads_a_signal_death() {
        let s = new_status(5, 9); // SIGKILL
        assert!(call(imethod(PROCESS_STATUS_CLASS, "exitstatus"), &s, &[]).is_nil());
        assert!(call(imethod(PROCESS_STATUS_CLASS, "success?"), &s, &[]).is_nil());
        assert!(boolean(&call(
            imethod(PROCESS_STATUS_CLASS, "signaled?"),
            &s,
            &[]
        )));
        assert_eq!(
            int(&call(imethod(PROCESS_STATUS_CLASS, "termsig"), &s, &[])),
            9
        );
        assert!(!boolean(&call(
            imethod(PROCESS_STATUS_CLASS, "exited?"),
            &s,
            &[]
        )));
    }

    /// `$? == int` and `$? == other_status` both compare the raw status word.
    #[test]
    fn status_equality_compares_the_raw_word() {
        let s = new_status(1, 7 << 8);
        let eq = imethod(PROCESS_STATUS_CLASS, "==");
        assert!(boolean(&call(eq, &s, &[RubyValue::Int((7 << 8) as i64)])));
        assert!(!boolean(&call(eq, &s, &[RubyValue::Int(0)])));
        // A different pid but the same raw word still compares equal.
        assert!(boolean(&call(eq, &s, &[new_status(999, 7 << 8)])));
        assert!(!boolean(&call(eq, &s, &[RubyValue::Nil])));
    }

    // -- Process::Tms (nested class) ----------------------------------------

    #[test]
    fn tms_exposes_the_four_cpu_times() {
        let t = new_tms(1.0, 2.0, 3.0, 4.0);
        assert_eq!(
            float(&call(imethod(PROCESS_TMS_CLASS, "utime"), &t, &[])),
            1.0
        );
        assert_eq!(
            float(&call(imethod(PROCESS_TMS_CLASS, "stime"), &t, &[])),
            2.0
        );
        assert_eq!(
            float(&call(imethod(PROCESS_TMS_CLASS, "cutime"), &t, &[])),
            3.0
        );
        assert_eq!(
            float(&call(imethod(PROCESS_TMS_CLASS, "cstime"), &t, &[])),
            4.0
        );
        // `to_a`/`values` share one body -- both answer the four in order.
        for name in ["to_a", "values"] {
            let RubyValue::Array(a) = call(imethod(PROCESS_TMS_CLASS, name), &t, &[]) else {
                panic!("{name} is an Array")
            };
            let a = a.lock();
            assert_eq!(a.len(), 4);
            assert_eq!(float(&a[0]), 1.0);
            assert_eq!(float(&a[3]), 4.0);
        }
        assert_eq!(
            text(&call(imethod(PROCESS_TMS_CLASS, "inspect"), &t, &[])),
            "#<struct Process::Tms utime=1.0, stime=2.0, cutime=3.0, cstime=4.0>"
        );
    }

    /// `Process.times` answers a live `Process::Tms` whose accessors work.
    #[test]
    fn times_answers_a_tms_struct() {
        let t = call(cmethod("times"), &process_module(), &[]);
        assert!(matches!(&t, RubyValue::Object(o) if o.class_id() == PROCESS_TMS_CLASS));
        assert!(matches!(
            call(imethod(PROCESS_TMS_CLASS, "utime"), &t, &[]),
            RubyValue::Float(_)
        ));
    }

    // -- resource limits / groups / pgrp ------------------------------------

    /// `getrlimit` answers a `[soft, hard]` Integer pair with `soft <= hard`
    /// (RLIM_INFINITY comparing greater than any finite soft limit).
    #[test]
    fn getrlimit_answers_a_soft_hard_pair() {
        let lim = call(
            cmethod("getrlimit"),
            &process_module(),
            &[RubyValue::Int(libc::RLIMIT_NOFILE as i64)],
        );
        let RubyValue::Array(a) = lim else {
            panic!("getrlimit is an Array")
        };
        let a = a.lock();
        assert_eq!(a.len(), 2);
        let (soft, hard) = (int(&a[0]), int(&a[1]));
        assert!(soft <= hard, "soft {soft} should not exceed hard {hard}");
    }

    /// `maxgroups=` stores the request but the reader clamps it to the OS
    /// `NGROUPS_MAX`, so a huge write never reads back larger than the ceiling,
    /// and a small write round-trips.
    #[test]
    fn maxgroups_write_is_clamped_on_read() {
        let ceiling = unsafe { libc::sysconf(libc::_SC_NGROUPS_MAX) };
        call(
            cmethod("maxgroups="),
            &process_module(),
            &[RubyValue::Int(1_000_000)],
        );
        let capped = int(&call(cmethod("maxgroups"), &process_module(), &[]));
        assert!(
            capped > 0 && capped <= ceiling,
            "{capped} should be within (0, {ceiling}]"
        );

        call(
            cmethod("maxgroups="),
            &process_module(),
            &[RubyValue::Int(8)],
        );
        assert_eq!(int(&call(cmethod("maxgroups"), &process_module(), &[])), 8);
    }

    /// `getpgrp`/`getpgid(0)` agree and are positive.
    #[test]
    fn process_group_queries_agree() {
        let pgrp = int(&call(cmethod("getpgrp"), &process_module(), &[]));
        let pgid = int(&call(
            cmethod("getpgid"),
            &process_module(),
            &[RubyValue::Int(0)],
        ));
        assert_eq!(pgrp, pgid);
        assert!(pgrp > 0);
    }

    // -- constants ----------------------------------------------------------

    /// The `install_constants` thunk seeds every declared `Process::*` constant
    /// under `PROCESS_CLASS` with the OS's own value.
    #[test]
    fn constants_install_with_the_os_values() {
        let install = crate::builtins::registered_table(zeo_abi::PROCESS_CLASS)
            .unwrap()
            .install_constants
            .expect("Process seeds constants");
        install();
        let owner = zeo_abi::PROCESS_CLASS.0;
        let get = |n: &str| crate::constants::const_get(owner, n).expect("constant seeded");
        assert_eq!(int(&get("WNOHANG")), libc::WNOHANG as i64);
        assert_eq!(int(&get("RLIMIT_NOFILE")), libc::RLIMIT_NOFILE as i64);
        assert_eq!(int(&get("CLOCK_MONOTONIC")), libc::CLOCK_MONOTONIC as i64);
        assert_eq!(int(&get("PRIO_PROCESS")), libc::PRIO_PROCESS as i64);
    }

    // -- surface completeness -----------------------------------------------

    /// Every method this batch series added is present on the class-method
    /// table (a regression guard against a `def` being dropped in a refactor),
    /// and the nested classes register under their own ids.
    #[test]
    fn the_class_method_surface_is_complete() {
        let names = crate::builtins::registered_table(zeo_abi::PROCESS_CLASS)
            .unwrap()
            .class
            .as_ref()
            .unwrap()
            .names;
        let present: std::collections::HashSet<&str> = names().iter().copied().collect();
        for expected in [
            "pid",
            "fork",
            "_fork",
            "kill",
            "ppid",
            "exit",
            "exit!",
            "abort",
            "wait",
            "waitpid",
            "wait2",
            "waitpid2",
            "waitall",
            "detach",
            "last_status",
            "getpgid",
            "setpgid",
            "getrlimit",
            "setrlimit",
            "maxgroups",
            "maxgroups=",
            "setproctitle",
            "warmup",
            "daemon",
            "getpriority",
            "setpriority",
            "clock_gettime",
            "clock_getres",
            "times",
            "spawn",
            "exec",
        ] {
            assert!(
                present.contains(expected),
                "Process.{expected} missing from the surface"
            );
        }
        // The nested classes are registered as their own tables.
        assert!(crate::builtins::registered_table(PROCESS_STATUS_CLASS).is_some());
        assert!(crate::builtins::registered_table(PROCESS_TMS_CLASS).is_some());
    }

    // -- spawn/exec argument parsing ----------------------------------------

    fn rstr(s: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(s.to_string()))
    }
    fn sym(s: &str) -> RubyValue {
        RubyValue::Symbol(crate::Symbol::intern(s))
    }
    fn hash(pairs: Vec<(RubyValue, RubyValue)>) -> RubyValue {
        RubyValue::Hash(crate::collections::hash_new(pairs))
    }
    fn program_of(c: &Command) -> String {
        c.get_program().to_string_lossy().into_owned()
    }
    fn args_of(c: &Command) -> Vec<String> {
        c.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// Multiple arguments always exec directly: the first is the program, the
    /// rest its argv.
    #[test]
    fn exec_argv_direct_multi_arg() {
        let c = build_exec_argv(&[rstr("echo"), rstr("hi"), rstr("there")]).unwrap();
        assert_eq!(program_of(&c), "echo");
        assert_eq!(args_of(&c), ["hi", "there"]);
    }

    /// A lone string execs directly when plain, but runs through `/bin/sh -c`
    /// the moment it holds a shell metacharacter.
    #[test]
    fn exec_argv_single_string_splits_or_shells() {
        let plain = build_exec_argv(&[rstr("echo hi")]).unwrap();
        assert_eq!(program_of(&plain), "echo");
        assert_eq!(args_of(&plain), ["hi"]);

        let shelled = build_exec_argv(&[rstr("echo *")]).unwrap();
        assert_eq!(program_of(&shelled), "/bin/sh");
        assert_eq!(args_of(&shelled), ["-c", "echo *"]);
    }

    /// A `[cmdname, argv0]` first element runs `cmdname` -- the explicit argv[0]
    /// is not observable through `Command`'s getters, so we assert the program.
    #[test]
    fn exec_argv_cmdname_argv0_pair() {
        let pair = RubyValue::Array(crate::array_new(vec![rstr("/bin/ls"), rstr("myname")]));
        let c = build_exec_argv(&[pair, rstr("-l")]).unwrap();
        assert_eq!(program_of(&c), "/bin/ls");
        assert_eq!(args_of(&c), ["-l"]);
    }

    /// `build_spawn_command` peels a LEADING env hash and a TRAILING options
    /// hash off the command words (by position), applying both.
    #[test]
    fn spawn_command_peels_env_and_options() {
        let env = hash(vec![(rstr("FOO"), rstr("bar"))]);
        let opts = hash(vec![(sym("chdir"), rstr("/tmp"))]);
        let cmd = build_spawn_command(&[env, rstr("echo"), rstr("hi"), opts]).unwrap();
        assert_eq!(program_of(&cmd), "echo");
        assert_eq!(args_of(&cmd), ["hi"]);
        assert!(
            cmd.get_envs().any(|(k, v)| k.to_str() == Some("FOO")
                && v.map(|x| x.to_string_lossy().into_owned()) == Some("bar".to_string())),
            "FOO=bar should be set"
        );
        assert_eq!(cmd.get_current_dir().unwrap().to_string_lossy(), "/tmp");
    }

    /// A nil env value unsets that variable rather than setting it.
    #[test]
    fn spawn_command_nil_env_value_unsets() {
        let env = hash(vec![(rstr("DROP"), RubyValue::Nil)]);
        let cmd = build_spawn_command(&[env, rstr("true")]).unwrap();
        assert!(
            cmd.get_envs()
                .any(|(k, v)| k.to_str() == Some("DROP") && v.is_none()),
            "DROP should be marked for removal"
        );
    }

    /// No command at all (just an env/options hash) is an ArgumentError --
    /// registry-less in a unit test, `arg_error!` surfaces as a panic.
    #[test]
    fn spawn_command_requires_a_command() {
        let r = std::panic::catch_unwind(|| build_spawn_command(&[hash(vec![])]));
        assert!(r.is_err());
    }

    #[test]
    fn option_key_classifies_redirect_keys() {
        assert!(matches!(option_key(&sym("chdir")), OptKey::Chdir));
        assert!(matches!(
            option_key(&sym("unsetenv_others")),
            OptKey::UnsetenvOthers
        ));
        assert!(matches!(option_key(&sym("in")), OptKey::In));
        assert!(matches!(option_key(&sym("out")), OptKey::Out));
        assert!(matches!(option_key(&RubyValue::Int(2)), OptKey::Err));
        assert!(matches!(option_key(&sym("pgroup")), OptKey::Unsupported));
        let pair = RubyValue::Array(crate::array_new(vec![sym("out"), sym("err")]));
        assert!(matches!(option_key(&pair), OptKey::OutErr));
    }

    #[test]
    fn merge_fd_reads_stream_targets() {
        assert_eq!(merge_fd(&sym("out")), Some(1));
        assert_eq!(merge_fd(&sym("err")), Some(2));
        assert_eq!(merge_fd(&RubyValue::Int(0)), Some(0));
        // `[:child, :out]` and `[:child, 1]` both name fd 1.
        let child_sym = RubyValue::Array(crate::array_new(vec![sym("child"), sym("out")]));
        assert_eq!(merge_fd(&child_sym), Some(1));
        let child_int = RubyValue::Array(crate::array_new(vec![sym("child"), RubyValue::Int(1)]));
        assert_eq!(merge_fd(&child_int), Some(1));
        // A filename is a file target, not a merge.
        assert_eq!(merge_fd(&rstr("log.txt")), None);
    }

    /// `open_redirect_file` creates+truncates for write and honours a
    /// `[name, mode]` append form.
    #[test]
    fn open_redirect_file_write_and_append_modes() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("zeo_spawn_redir_{}.txt", std::process::id()));
        let p = path.to_string_lossy().into_owned();

        let f = open_redirect_file(&rstr(&p), true).unwrap();
        (&f).write_all(b"first").unwrap();
        drop(f);
        // A second write mode ("w") truncates.
        let f = open_redirect_file(&rstr(&p), true).unwrap();
        (&f).write_all(b"hi").unwrap();
        drop(f);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hi");

        // `[name, "a"]` appends rather than truncating.
        let append = RubyValue::Array(crate::array_new(vec![rstr(&p), rstr("a")]));
        let f = open_redirect_file(&append, true).unwrap();
        (&f).write_all(b"!").unwrap();
        drop(f);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hi!");

        std::fs::remove_file(&path).ok();
    }
}
