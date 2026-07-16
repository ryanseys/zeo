//! `Process` (CRuby process.c) -- the identity and clock surface. Spawning
//! (`Process.spawn`/`Kernel#system`/backticks) is a later slice; this one
//! covers what programs read rather than what they start.
//!
//! `clock_gettime` answers a Float of SECONDS (CRuby's default unit), which
//! is what `Process.clock_gettime(Process::CLOCK_MONOTONIC)` benchmark
//! idioms subtract. The clock CONSTANTS are seeded as ordinary constants
//! under the Process module (see `seed_process`), matching how a program
//! writes them: `Process::CLOCK_MONOTONIC`.

use crate::builtins::{arity, builtin_methods};
use crate::RubyValue;

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
        let clock = match &args[0] {
            RubyValue::Int(i) => *i,
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into Integer",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        };
        let secs = clock_seconds(clock)?;
        // The optional second argument is a unit Symbol. Only the default
        // (:float_second) and the two common integer units are honored;
        // anything else raises rather than silently answering seconds.
        match args.get(1) {
            None => Ok(RubyValue::Float(secs)),
            Some(RubyValue::Symbol(s)) => match s.name().as_str() {
                "float_second" => Ok(RubyValue::Float(secs)),
                "float_millisecond" => Ok(RubyValue::Float(secs * 1e3)),
                "float_microsecond" => Ok(RubyValue::Float(secs * 1e6)),
                "second" => Ok(RubyValue::Int(secs as i64)),
                "millisecond" => Ok(RubyValue::Int((secs * 1e3) as i64)),
                "microsecond" => Ok(RubyValue::Int((secs * 1e6) as i64)),
                "nanosecond" => Ok(RubyValue::Int((secs * 1e9) as i64)),
                other => Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    format!("unexpected unit: {other}"),
                )),
            },
            Some(other) => Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("unexpected unit: {}", other.inspect_string()),
            )),
        }
    }
}

/// One clock's current value in seconds. The ids are the OS's own
/// (`libc::CLOCK_*`), which is exactly what `seed_process` publishes as
/// `Process::CLOCK_*` -- so a program that passes the constant through gets
/// the clock it named, and one that passes a bare integer gets whatever that
/// integer means to this OS, as in CRuby.
fn clock_seconds(clock: i64) -> Result<f64, crate::Signal> {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
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

/// Installs the `Process` clock constants -- called once from generated
/// `main()`. Values are the OS's own ids (see `clock_seconds`).
pub fn seed_process() {
    let cid = spinel_abi::PROCESS_CLASS.0;
    let set = |name: &str, v: libc::clockid_t| {
        crate::constants::const_set(cid, name, RubyValue::Int(v as i64));
    };
    set("CLOCK_REALTIME", libc::CLOCK_REALTIME);
    set("CLOCK_MONOTONIC", libc::CLOCK_MONOTONIC);
    set("CLOCK_PROCESS_CPUTIME_ID", libc::CLOCK_PROCESS_CPUTIME_ID);
    set("CLOCK_THREAD_CPUTIME_ID", libc::CLOCK_THREAD_CPUTIME_ID);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process_module() -> RubyValue {
        RubyValue::Class(spinel_abi::PROCESS_CLASS)
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
        let RubyValue::Float(a) = clock_gettime(&process_module(), &[m.clone()], None).unwrap()
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
            &[m.clone(), RubyValue::Symbol(crate::Symbol::intern("millisecond"))],
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
