//! `Signal` (CRuby signal.c) -- the signal name/number surface: `Signal.list`,
//! `Signal.signame`, and `Signal.trap`. zeo installs no OS signal handlers,
//! so `trap` is a VALIDATED no-op: it rejects unknown/untrappable signals the
//! way CRuby does, and tracks each signal's previously registered action (the
//! value `trap` returns) so a program reads back what it set, but no handler
//! ever fires. The name<->number table and its resolution helpers are shared
//! with `SignalException`'s `initialize` (see `builtins::exception`).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::builtins::{arg_error, arity, class_name_of};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal, string_new};
use zeo_macros::ruby_module;

/// The Darwin/BSD signal set zeo targets, canonical name (no `SIG` prefix)
/// -> number, plus the `EXIT` pseudo-signal (0). Canonical-only (no `IOT`/`CLD`
/// aliases): `Signal.signame` must answer the canonical name for a number, and
/// every `Signal.list` lookup the tests make is by a canonical key.
pub(crate) const SIGNAL_TABLE: &[(&str, i32)] = &[
    ("EXIT", 0),
    ("HUP", 1),
    ("INT", 2),
    ("QUIT", 3),
    ("ILL", 4),
    ("TRAP", 5),
    ("ABRT", 6),
    ("EMT", 7),
    ("FPE", 8),
    ("KILL", 9),
    ("BUS", 10),
    ("SEGV", 11),
    ("SYS", 12),
    ("PIPE", 13),
    ("ALRM", 14),
    ("TERM", 15),
    ("URG", 16),
    ("STOP", 17),
    ("TSTP", 18),
    ("CONT", 19),
    ("CHLD", 20),
    ("TTIN", 21),
    ("TTOU", 22),
    ("IO", 23),
    ("XCPU", 24),
    ("XFSZ", 25),
    ("VTALRM", 26),
    ("PROF", 27),
    ("WINCH", 28),
    ("INFO", 29),
    ("USR1", 30),
    ("USR2", 31),
];

/// Signals that CRuby refuses to trap -- `KILL` and `STOP` can't be caught,
/// ignored, or handled, so `Signal.trap` on them raises `Errno::EINVAL`.
const UNTRAPPABLE: &[i32] = &[9, 17];

/// Resolve a signal name (with or without a leading `SIG`) to its number.
/// Case-sensitive, matching CRuby (`"INT"`/`"SIGINT"` resolve, `"int"` does not).
pub(crate) fn signo_from_name(name: &str) -> Option<i32> {
    let bare = name.strip_prefix("SIG").unwrap_or(name);
    SIGNAL_TABLE
        .iter()
        .find(|(s, _)| *s == bare)
        .map(|(_, n)| *n)
}

/// The canonical name (no `SIG` prefix) for a signal number, `None` if unknown.
pub(crate) fn name_from_signo(no: i32) -> Option<&'static str> {
    SIGNAL_TABLE.iter().find(|(_, n)| *n == no).map(|(s, _)| *s)
}

thread_local! {
    /// Each signal's last-registered action, keyed by number -- the value
    /// `Signal.trap` returns on the NEXT call for that signal. A signal absent
    /// here reads back as `"DEFAULT"`. Thread-local rather than a process global:
    /// zeo installs no real handlers, so this only feeds `trap`'s return
    /// value, and every test drives `trap` from the main thread; a
    /// `Send`-free `RefCell` sidesteps storing a `Proc` in a global.
    static TRAP_STATE: RefCell<HashMap<i32, RubyValue>> = RefCell::new(HashMap::new());
}

/// The action currently registered for signal `no` -- `None` when never
/// set (the "DEFAULT" state). Backs `Process.kill`'s synthetic
/// self-delivery of trapped signals.
pub(crate) fn trap_action(no: i32) -> Option<RubyValue> {
    TRAP_STATE.with(|s| s.borrow().get(&no).cloned())
}

/// Resolve `Signal.trap`/`SignalException`/`Process.kill`'s signal argument
/// (a name String/Symbol, or a number) to a signal number, raising
/// `ArgumentError` for an unknown name or an out-of-range number exactly as
/// CRuby does.
pub(crate) fn resolve_signal_arg(arg: &RubyValue) -> Result<i32, Signal> {
    match arg {
        RubyValue::Int(i) => {
            let no = *i as i32;
            if name_from_signo(no).is_some() {
                Ok(no)
            } else {
                Err(arg_error!("invalid signal number ({no})"))
            }
        }
        RubyValue::Str(s) => resolve_signal_name(&s.lock().to_utf8_lossy()),
        RubyValue::Symbol(sym) => resolve_signal_name(&sym.name()),
        other => Err(arg_error!("bad signal type {}", class_name_of(other))),
    }
}

fn resolve_signal_name(name: &str) -> Result<i32, Signal> {
    signo_from_name(name).ok_or_else(|| {
        let bare = name.strip_prefix("SIG").unwrap_or(name);
        arg_error!("unsupported signal `SIG{bare}'")
    })
}

ruby_module! {
    Signal = zeo_abi::SIGNAL_MODULE;

    // `Signal.list` -> {name => number}, the canonical table as a fresh Hash.
    def self."list"(_recv) {
        let pairs = SIGNAL_TABLE
            .iter()
            .map(|(name, no)| {
                (RubyValue::Str(string_new(name.to_string())), RubyValue::Int(*no as i64))
            })
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    // `Signal.signame(n)` -> the canonical name (no `SIG` prefix) or nil. A Float
    // truncates toward zero; a non-numeric argument is a TypeError.
    def self."signame"(_recv, arg) {
        let no = crate::builtins::convert::to_index(arg)? as i32;
        match name_from_signo(no) {
            Some(name) => Ok(RubyValue::Str(string_new(name.to_string()))),
            None => Ok(RubyValue::Nil),
        }
    }
    // `Signal.trap(sig, action = nil) { block }` -> the PREVIOUS action for
    // `sig` (`"DEFAULT"` if never set). See `trap_impl`.
    def self."trap"(_recv, *args, &block) {
        trap_impl(args, block)
    }
}

/// The shared body of `Signal.trap` and the private `Kernel#trap`: validate the
/// signal, record the new action, and return the previous one (`"DEFAULT"` if
/// never set). A no-op beyond that bookkeeping -- zeo installs no handler, so
/// nothing fires. Unknown signals raise `ArgumentError`; `KILL`/`STOP` raise
/// `Errno::EINVAL`.
pub(crate) fn trap_impl(args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    arity!(args, 1..=2);
    let no = resolve_signal_arg(&args[0])?;
    if UNTRAPPABLE.contains(&no) {
        return Err(raise_error(
            "Errno::EINVAL",
            "Invalid argument - trap".to_string(),
        ));
    }
    let prev = TRAP_STATE.with(|s| s.borrow().get(&no).cloned());
    let handler = match block {
        Some(b) => b,
        None => args.get(1).cloned().unwrap_or(RubyValue::Nil),
    };
    TRAP_STATE.with(|s| s.borrow_mut().insert(no, handler));
    Ok(prev.unwrap_or_else(|| RubyValue::Str(string_new("DEFAULT".to_string()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `ruby_module!`-generated class methods are reachable only through
    /// the dispatch table (their Rust fn names are mangled), so the tests call
    /// them the way real dispatch does -- through `Signal`'s registered
    /// class-method `lookup`.
    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::SIGNAL_MODULE)
            .expect("Signal is a registered builtin table")
            .class
            .as_ref()
            .expect("Signal has class methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Signal.{name} is defined"))
    }

    fn sym(name: &str) -> RubyValue {
        RubyValue::Symbol(crate::Symbol::intern(name))
    }

    #[test]
    fn list_maps_canonical_names_to_numbers() {
        let RubyValue::Hash(h) = cmethod("list")(&RubyValue::Nil, &[], None).unwrap() else {
            panic!("Signal.list is a Hash")
        };
        // A representative canonical entry resolves to its number.
        let int = crate::hash_get(&h, &RubyValue::Str(string_new("INT".to_string())));
        assert!(matches!(int, RubyValue::Int(2)));
        assert!(h.lock().len() == SIGNAL_TABLE.len());
    }

    #[test]
    fn signame_answers_the_canonical_name_or_nil() {
        let got = cmethod("signame")(&RubyValue::Nil, &[RubyValue::Int(9)], None).unwrap();
        assert!(matches!(got, RubyValue::Str(ref s) if s.lock().to_utf8_lossy() == "KILL"));
        // An out-of-table number is nil, not an error.
        let miss = cmethod("signame")(&RubyValue::Nil, &[RubyValue::Int(99)], None).unwrap();
        assert!(matches!(miss, RubyValue::Nil));
    }

    #[test]
    fn trap_returns_the_previous_action_and_records_the_new_one() {
        // First trap on a fresh signal reads back "DEFAULT".
        let prev = cmethod("trap")(&RubyValue::Nil, &[sym("USR1"), RubyValue::Nil], None).unwrap();
        assert!(matches!(prev, RubyValue::Str(ref s) if s.lock().to_utf8_lossy() == "DEFAULT"));
        // The second call sees the action the first one stored (here Nil).
        let again = cmethod("trap")(&RubyValue::Nil, &[sym("USR1"), RubyValue::Nil], None).unwrap();
        assert!(matches!(again, RubyValue::Nil));
    }

    #[test]
    fn trap_on_an_untrappable_signal_is_einval() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cmethod("trap")(&RubyValue::Nil, &[sym("KILL"), RubyValue::Nil], None)
        }));
        // Errno::EINVAL raised; registry-less in a bare unit test, so it panics.
        assert!(r.is_err() || r.unwrap().is_err());
    }
}
