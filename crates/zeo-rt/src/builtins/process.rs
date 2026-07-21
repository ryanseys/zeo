//! `Process` (CRuby process.c) -- the identity and clock surface. Spawning
//! (`Process.spawn`/`Kernel#system`/backticks) is a later slice; this one
//! covers what programs read rather than what they start.
//!
//! `clock_gettime` answers a Float of SECONDS (CRuby's default unit), which
//! is what `Process.clock_gettime(Process::CLOCK_MONOTONIC)` benchmark
//! idioms subtract. The clock CONSTANTS are seeded as ordinary constants
//! under the Process module (see `seed_process`), matching how a program
//! writes them: `Process::CLOCK_MONOTONIC`.

use std::cell::RefCell;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::sync::Arc;

use crate::builtins::{arity, builtin_methods};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, PROCESS_STATUS_CLASS, PROCESS_TMS_CLASS};

builtin_methods! {
    pub(crate) fn lookup_class;

    "pid" => fn pid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(std::process::id() as i64))
    }
    // `Process.ppid` has no portable std equivalent; libc's getppid is the
    // honest answer rather than a fabricated one.
    "ppid" => fn ppid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getppid() } as i64))
    }
    "clock_gettime" => fn clock_gettime(_recv, args, _block) {
        arity!(args, 1..=2);
        let clock = int_arg(&args[0])?;
        clock_in_unit(clock_seconds(clock)?, args.get(1))
    }
    // `Process.clock_getres(clock_id [, unit])` -- the clock's resolution, in the
    // same units `clock_gettime` accepts (default a Float of seconds).
    "clock_getres" => fn clock_getres(_recv, args, _block) {
        arity!(args, 1..=2);
        let clock = int_arg(&args[0])?;
        clock_in_unit(clock_res_seconds(clock)?, args.get(1))
    }
    // Real/effective user and group ids (libc getuid/geteuid/getgid/getegid).
    "uid" => fn uid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getuid() } as i64))
    }
    "euid" => fn euid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::geteuid() } as i64))
    }
    "gid" => fn gid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getgid() } as i64))
    }
    "egid" => fn egid(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getegid() } as i64))
    }
    // `Process.getpgrp` -- the current process group id.
    "getpgrp" => fn getpgrp(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(unsafe { libc::getpgrp() } as i64))
    }
    // `Process.getsid([pid])` -- the session id of `pid` (0/none = this process).
    "getsid" => fn getsid(_recv, args, _block) {
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
    "getpriority" => fn getpriority(_recv, args, _block) {
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
    "groups" => fn groups(_recv, args, _block) {
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
    "times" => fn times(_recv, args, _block) {
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
}

/// One clock's current value in seconds. The ids are the OS's own
/// (`libc::CLOCK_*`), which is exactly what `seed_process` publishes as
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
            other => Err(raise_error(
                "ArgumentError",
                format!("unexpected unit: {other}"),
            )),
        },
        Some(other) => Err(raise_error(
            "ArgumentError",
            format!("unexpected unit: {}", other.inspect_string()),
        )),
    }
}

/// Coerce an argument to an `i64`, raising CRuby's TypeError otherwise.
fn int_arg(v: &RubyValue) -> Result<i64, crate::Signal> {
    match v {
        RubyValue::Int(i) => Ok(*i),
        other => Err(raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into Integer",
                crate::builtins::convert_name_of(other)
            ),
        )),
    }
}

/// Installs the `Process` clock constants -- called once from generated
/// `main()`. Values are the OS's own ids (see `clock_seconds`).
pub fn seed_process() {
    let cid = zeo_abi::PROCESS_CLASS.0;
    let set = |name: &str, v: libc::clockid_t| {
        crate::constants::const_set(cid, name, RubyValue::Int(v as i64));
    };
    set("CLOCK_REALTIME", libc::CLOCK_REALTIME);
    set("CLOCK_MONOTONIC", libc::CLOCK_MONOTONIC);
    set("CLOCK_PROCESS_CPUTIME_ID", libc::CLOCK_PROCESS_CPUTIME_ID);
    set("CLOCK_THREAD_CPUTIME_ID", libc::CLOCK_THREAD_CPUTIME_ID);
    // `Process.getpriority`/`setpriority`'s `which` selectors.
    let seti = |name: &str, v: i64| crate::constants::const_set(cid, name, RubyValue::Int(v));
    seti("PRIO_PROCESS", libc::PRIO_PROCESS as i64);
    seti("PRIO_PGRP", libc::PRIO_PGRP as i64);
    seti("PRIO_USER", libc::PRIO_USER as i64);
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
    match v {
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        other => Err(raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into String",
                crate::builtins::convert_name_of(other)
            ),
        )),
    }
}

/// Build the `Command` for a `system`/backtick argument list. A single string
/// picks the shell-or-direct path per `needs_shell`; multiple arguments always
/// exec directly (`system("prog", "arg", ...)`). `Ok(None)` is an empty
/// command (a blank single string), which the callers turn into their own
/// "nothing ran" answer.
fn build_command(args: &[RubyValue]) -> Result<Option<Command>, Signal> {
    if args.is_empty() {
        return Err(raise_error(
            "ArgumentError",
            "wrong number of arguments (given 0, expected 1+)".to_string(),
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
    let status = child
        .wait()
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
    if let Some(mut so) = child.stdout.take() {
        use std::io::Read;
        so.read_to_end(&mut out)
            .map_err(|e| raise_error("IOError", e.to_string()))?;
    }
    let status = child
        .wait()
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
