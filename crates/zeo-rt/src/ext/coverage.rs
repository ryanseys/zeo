//! The `Coverage` module -- line coverage over the AOT line instrumentation.
//!
//! CRuby's coverage extension hooks the VM's line events on iseqs compiled
//! while measurement is set up. zeo has no VM: when a program `require`s
//! `coverage`, the COMPILER emits the instrumentation instead --
//! [`cov_line`] calls beside every statement's `set_line` stamp,
//! [`cov_file_loaded`] marks where a spliced file's top level begins in the
//! main statement stream, and [`coverage_install`] receives the per-file
//! coverable-line table (total lines, statement lines, `def` lines). A
//! program that doesn't require `coverage` carries NONE of this -- the
//! emission is gated on the feature's activation, so the cost isn't a
//! branch per line, it's zero.
//!
//! Semantics mirror CRuby's, oracle-verified: a file is reported only if its
//! top level began executing while measurement was set up (the main script
//! never qualifies -- it starts before `Coverage.start` runs); statement
//! lines report live hit counts; non-executable lines report nil. `def`
//! lines are the one static approximation: a definition executes exactly
//! once, when its file loads, so a covered file's `def` lines report 1
//! (CRuby's value) without a runtime event. Lines only --
//! `supported?(:branches)`/`(:methods)`/`(:oneshot_lines)` answer false.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::builtins::{runtime_error, type_error};
use crate::collections::{array_new, hash_get, hash_has_key, hash_new};
use crate::{RubyValue, Signal, Symbol};
use zeo_macros::ruby_module;

/// One file's compiler-emitted coverable-line row:
/// `(path, total_lines, statement_lines, def_lines)`.
type FileRow = (&'static str, u32, &'static [u32], &'static [u32]);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Idle,
    Suspended,
    Running,
}

struct CovState {
    mode: Mode,
    /// Live hit counts, `file -> line -> count`.
    counts: HashMap<&'static str, HashMap<u32, u64>>,
    /// Files whose top level began while measurement was set up, in load
    /// order (CRuby reports files in the order they were compiled).
    files: Vec<&'static str>,
    /// The installed coverable-line table.
    table: &'static [FileRow],
}

/// The fast gate [`cov_line`] checks -- true exactly while `mode == Running`.
static ENABLED: AtomicBool = AtomicBool::new(false);

static STATE: LazyLock<Mutex<CovState>> = LazyLock::new(|| {
    Mutex::new(CovState {
        mode: Mode::Idle,
        counts: HashMap::new(),
        files: Vec::new(),
        table: &[],
    })
});

/// Install the compiler-emitted coverable-line table (generated `main`
/// calls this once, before the program body runs).
pub fn coverage_install(table: &'static [FileRow]) {
    STATE.lock().expect("coverage state lock").table = table;
}

/// One statement hit -- emitted beside every `set_line` stamp of a
/// coverage-activated program.
pub fn cov_line(file: &'static str, line: u32) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut st = STATE.lock().expect("coverage state lock");
    *st.counts.entry(file).or_default().entry(line).or_insert(0) += 1;
}

/// A spliced file's top level is beginning -- the file is reported iff
/// measurement is set up at this moment (CRuby: a file is covered iff it was
/// compiled while coverage was set up, running or suspended).
pub fn cov_file_loaded(file: &'static str) {
    let mut st = STATE.lock().expect("coverage state lock");
    if st.mode != Mode::Idle && !st.files.contains(&file) {
        st.files.push(file);
    }
}

/// Reject a `start`/`setup` argument naming a measurement mode the AOT
/// instrumentation doesn't implement.
fn check_modes(arg: &RubyValue) -> Result<(), Signal> {
    let unsupported = |name: &str| -> Signal {
        runtime_error!("coverage measurement mode {name} is not supported by zeo")
    };
    match arg {
        RubyValue::Hash(h) => {
            for name in ["branches", "methods", "oneshot_lines", "eval"] {
                let key = RubyValue::Symbol(Symbol::intern(name));
                if hash_get(h, &key).truthy() {
                    return Err(unsupported(&format!(":{name}")));
                }
            }
            Ok(())
        }
        RubyValue::Symbol(s) if s.name() == "all" => Err(unsupported(":all")),
        _ => Ok(()),
    }
}

/// `setup`'s core: Idle -> Suspended with cleared state.
fn do_setup(args: &[RubyValue]) -> Result<(), Signal> {
    if let Some(arg) = args.first() {
        check_modes(arg)?;
    }
    let mut st = STATE.lock().expect("coverage state lock");
    if st.mode != Mode::Idle {
        return Err(runtime_error!("coverage measurement is already setup"));
    }
    st.mode = Mode::Suspended;
    st.counts.clear();
    st.files.clear();
    Ok(())
}

fn do_resume() -> Result<(), Signal> {
    let mut st = STATE.lock().expect("coverage state lock");
    match st.mode {
        Mode::Idle => Err(runtime_error!("coverage measurement is not set up yet")),
        Mode::Running => Err(runtime_error!("coverage measurement is already running")),
        Mode::Suspended => {
            st.mode = Mode::Running;
            ENABLED.store(true, Ordering::Relaxed);
            Ok(())
        }
    }
}

/// Build the result Hash for the current counts: one entry per covered
/// file, an Array with an element per source line -- nil off the coverable
/// table, live counts on statement lines, the static 1 on `def` lines.
fn build_result(st: &CovState) -> RubyValue {
    let pairs = st
        .files
        .iter()
        .filter_map(|file| {
            let &(_, total, stmt_lines, def_lines) = st.table.iter().find(|(f, ..)| f == file)?;
            let empty = HashMap::new();
            let counts = st.counts.get(file).unwrap_or(&empty);
            let mut lines = vec![RubyValue::Nil; total as usize];
            for &l in stmt_lines {
                lines[(l - 1) as usize] =
                    RubyValue::Int(counts.get(&l).copied().unwrap_or(0) as i64);
            }
            // A definition executes once, at load; a `def` sharing a line
            // with a counted statement adds its execution to that count.
            for &l in def_lines {
                let slot = &mut lines[(l - 1) as usize];
                *slot = match slot {
                    RubyValue::Int(n) => RubyValue::Int(*n + 1),
                    _ => RubyValue::Int(1),
                };
            }
            let key = RubyValue::Str(crate::string_new((*file).to_string()));
            Some((key, RubyValue::Array(array_new(lines))))
        })
        .collect();
    RubyValue::Hash(hash_new(pairs))
}

/// A `result`/`start` keyword read off the trailing Hash argument,
/// defaulting to `default` when absent.
fn kwarg_bool(opts: Option<&RubyValue>, name: &str, default: bool) -> bool {
    let Some(RubyValue::Hash(h)) = opts else {
        return default;
    };
    let key = RubyValue::Symbol(Symbol::intern(name));
    if hash_has_key(h, &key) {
        hash_get(h, &key).truthy()
    } else {
        default
    }
}

ruby_module! {
    Coverage = zeo_abi::COVERAGE_MODULE;

    // `start` / `start(lines: true)` -- setup + resume. Modes beyond line
    // coverage raise (the AOT instrumentation has no branch/method events).
    def self."start"(_recv, _opt?) {
        do_setup(__args)?;
        do_resume()?;
        Ok(RubyValue::Nil)
    }
    def self."setup"(_recv, _opt?) {
        do_setup(__args)?;
        Ok(RubyValue::Nil)
    }
    def self."resume"(_recv) {
        do_resume()?;
        Ok(RubyValue::Nil)
    }
    def self."suspend"(_recv) {
        let mut st = STATE.lock().expect("coverage state lock");
        if st.mode != Mode::Running {
            return Err(runtime_error!("coverage measurement is not running"));
        }
        st.mode = Mode::Suspended;
        ENABLED.store(false, Ordering::Relaxed);
        Ok(RubyValue::Nil)
    }
    def self."running?"(_recv) {
        let st = STATE.lock().expect("coverage state lock");
        Ok(RubyValue::Bool(st.mode == Mode::Running))
    }
    def self."state"(_recv) {
        let st = STATE.lock().expect("coverage state lock");
        Ok(RubyValue::Symbol(Symbol::intern(match st.mode {
            Mode::Idle => "idle",
            Mode::Suspended => "suspended",
            Mode::Running => "running",
        })))
    }
    // `result(stop: true, clear: true)` -- the measured coverage; by default
    // ends measurement (a second `result` raises "not enabled").
    def self."result"(_recv, **opts) {
        let stop = kwarg_bool(opts, "stop", true);
        let clear = kwarg_bool(opts, "clear", true);
        let mut st = STATE.lock().expect("coverage state lock");
        if st.mode == Mode::Idle {
            return Err(runtime_error!("coverage measurement is not enabled"));
        }
        let result = build_result(&st);
        if clear {
            st.counts.clear();
        }
        if stop {
            st.mode = Mode::Idle;
            st.counts.clear();
            st.files.clear();
            ENABLED.store(false, Ordering::Relaxed);
        }
        Ok(result)
    }
    def self."peek_result"(_recv) {
        let st = STATE.lock().expect("coverage state lock");
        if st.mode == Mode::Idle {
            return Err(runtime_error!("coverage measurement is not enabled"));
        }
        Ok(build_result(&st))
    }
    // Line coverage is the one mode the AOT instrumentation implements.
    def self."supported?"(_recv, arg) {
        let RubyValue::Symbol(s) = arg else {
            return Err(type_error!(
                "wrong argument type {} (expected Symbol)",
                crate::builtins::class_name_of(arg)
            ));
        };
        Ok(RubyValue::Bool(s.name() == "lines"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The measurement state is process-global; tests serialize on this so
    // the parallel test runner can't interleave lifecycles.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset() {
        let mut st = STATE.lock().unwrap();
        st.mode = Mode::Idle;
        st.counts.clear();
        st.files.clear();
        st.table = &[];
        ENABLED.store(false, Ordering::Relaxed);
    }

    #[test]
    fn lifecycle_counts_and_reports() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        coverage_install(&[("t.rb", 4, &[2, 3], &[1])]);
        do_setup(&[]).unwrap();
        do_resume().unwrap();
        cov_file_loaded("t.rb");
        cov_line("t.rb", 2);
        cov_line("t.rb", 2);
        let st = STATE.lock().unwrap();
        let RubyValue::Hash(h) = build_result(&st) else {
            panic!("result is a Hash")
        };
        let key = RubyValue::Str(crate::string_new("t.rb".to_string()));
        let RubyValue::Array(a) = hash_get(&h, &key) else {
            panic!("per-file value is an Array")
        };
        let lines: Vec<RubyValue> = a.lock().iter().cloned().collect();
        assert!(matches!(lines[0], RubyValue::Int(1))); // def line: static 1
        assert!(matches!(lines[1], RubyValue::Int(2))); // stamped twice
        assert!(matches!(lines[2], RubyValue::Int(0))); // coverable, unhit
        assert!(matches!(lines[3], RubyValue::Nil)); // off the table
    }

    #[test]
    fn files_loaded_while_idle_stay_unreported() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        coverage_install(&[("early.rb", 1, &[1], &[])]);
        cov_file_loaded("early.rb"); // before any setup
        do_setup(&[]).unwrap();
        do_resume().unwrap();
        cov_line("early.rb", 1); // a method from it runs under coverage
        let st = STATE.lock().unwrap();
        let RubyValue::Hash(h) = build_result(&st) else {
            panic!("result is a Hash")
        };
        assert_eq!(h.lock().len(), 0);
    }

    #[test]
    fn disabled_stamps_do_not_count() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        coverage_install(&[("t.rb", 1, &[1], &[])]);
        cov_line("t.rb", 1); // idle: ignored
        do_setup(&[]).unwrap();
        cov_line("t.rb", 1); // suspended: ignored
        do_resume().unwrap();
        cov_file_loaded("t.rb");
        cov_line("t.rb", 1);
        let st = STATE.lock().unwrap();
        assert_eq!(st.counts["t.rb"][&1], 1);
    }
}
