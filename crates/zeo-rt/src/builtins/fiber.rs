//! `Fiber`'s Path-2 (runtime `send`) rows. The static Path-1 fast path in
//! `codegen::call` emits `fiber_resume`/`fiber_alive` calls directly and
//! never comes here; these rows serve dynamically-typed receivers (a fiber
//! held in an ivar/Hash/`send` target), mirroring that codegen arm exactly,
//! error messages included.
//!
//! zeo has no fiber SCHEDULER, and the scheduler quartet says so rather than
//! pretending: `.scheduler`/`.current_scheduler` answer nil, `.set_scheduler`
//! accepts nil alone, and `.schedule` raises the same `RuntimeError` CRuby
//! raises when no scheduler is installed. `#backtrace` answers the frames of
//! the CURRENT fiber and `[]` for any other -- a suspended fiber keeps its own
//! in a saved context this walk cannot enter.

use crate::Symbol;
use crate::builtins::{arg_error, type_error};
use crate::dispatch::raise_error;
use crate::fiber::{self, FiberResume, fiber_alive, fiber_resume, fiber_transfer};
use crate::signal::Signal;
use crate::value::RubyValue;
use zeo_macros::ruby_class;

/// A Symbol/String storage-key argument as a `Symbol`.
fn key_sym(v: &RubyValue) -> Result<Symbol, Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(*s),
        RubyValue::Str(s) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            other.inspect_string()
        )),
    }
}

/// One outcome mapping for `resume`/`transfer`/`raise` -- every error
/// variant becomes its CRuby-verbatim `FiberError` (the same messages the
/// static Path-1 codegen arm emits).
fn outcome(result: FiberResume) -> Result<RubyValue, Signal> {
    match result {
        FiberResume::Value(v) => Ok(v),
        FiberResume::RubyError(sig) => Err(sig),
        FiberResume::Dead => Err(raise_error(
            "FiberError",
            "attempt to resume a terminated fiber".to_string(),
        )),
        FiberResume::Uninitialized => {
            Err(raise_error("FiberError", "uninitialized fiber".to_string()))
        }
        FiberResume::DoubleResume => Err(raise_error(
            "FiberError",
            "attempt to resume a resumed fiber (double resume)".to_string(),
        )),
        FiberResume::CrossThread => Err(raise_error(
            "FiberError",
            "fiber called across threads".to_string(),
        )),
        FiberResume::Unborn => Err(raise_error(
            "FiberError",
            "cannot raise exception on unborn fiber".to_string(),
        )),
    }
}

/// CRuby restricts `#storage`/`#storage=` to the fiber they belong to.
fn require_current(handle: &fiber::RFiber) -> Result<(), Signal> {
    if !fiber::fiber_is_current(handle) {
        return Err(arg_error!(
            "Fiber storage can only be accessed from the Fiber it belongs to"
        ));
    }
    Ok(())
}

/// Resolve `Fiber#raise`'s arguments to the exception value to inject, mirroring
/// `Kernel#raise`: no args -> `RuntimeError ""`; a class (optionally + message)
/// -> an instance of it; a String -> `RuntimeError`; an Exception -> itself.
fn resolve_raise_exc(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match args.first() {
        None => Ok(exc_of("RuntimeError", String::new())),
        Some(RubyValue::Class(cid)) => {
            let name =
                crate::dispatch::class_name(*cid).unwrap_or_else(|| "RuntimeError".to_string());
            let msg = args
                .get(1)
                .map(|m| m.to_display_string())
                .unwrap_or_default();
            Ok(exc_of(&name, msg))
        }
        Some(v) => crate::dispatch::coerce_raise_arg(v.clone()),
    }
}

/// Build an exception value by class name + message (unwrapping the Signal the
/// runtime raise channel produces).
fn exc_of(class_name: &str, msg: String) -> RubyValue {
    match raise_error(class_name, msg) {
        Signal::Raise(exc) => exc,
        _ => RubyValue::Nil,
    }
}

/// The frames a `#backtrace`-family row may report. Only the CURRENT fiber's
/// are reachable: a suspended one keeps its own in a saved context this walk
/// cannot enter, and a terminated one has none left -- which is the empty
/// array CRuby answers for it too.
fn own_frames(handle: &fiber::RFiber) -> Vec<(&'static str, u32, &'static str)> {
    match fiber::fiber_is_current(handle) {
        true => crate::frames::caller_frames(0),
        false => Vec::new(),
    }
}

ruby_class! {
    Fiber = zeo_abi::FIBER_CLASS < zeo_abi::OBJECT_CLASS;

    receiver f = crate::RubyValue::Fiber;

    // Codegen intercepts the literal `Fiber.new { ... }`; this row serves a
    // `send`, and is what a `class F < Fiber` reaches through `super`. The
    // block IS the fiber's body, so a blockless call is the same
    // ArgumentError CRuby raises out of `Proc.new`.
    def self."new" cfunc (_recv, &block) {
        match block {
            Some(b) => Ok(fiber::fiber_new(b.clone())),
            None => Err(raise_error(
                "ArgumentError",
                "tried to create Proc object without a block".to_string(),
            )),
        }
    }
    def self."current"(_recv) {
        Ok(fiber::fiber_current())
    }
    def self."[]"(_recv, key) {
        Ok(fiber::fiber_storage_get(key_sym(key)?))
    }
    def self."[]="(_recv, key, value) {
        fiber::fiber_storage_set(key_sym(key)?, value.clone());
        Ok(value.clone())
    }
    // `Fiber.yield(*args)` -- suspend back to whoever resumed us. Codegen
    // intercepts the literal call; this row serves a `send` and is what makes
    // the name appear in `Fiber.singleton_methods`.
    def self."yield" cfunc (_recv, *args) {
        match fiber::fiber_yield(args.to_vec()) {
            crate::fiber::FiberYield::Value(v) => Ok(v),
            crate::fiber::FiberYield::Raise(e) => Err(Signal::Raise(e)),
            crate::fiber::FiberYield::Root => Err(raise_error(
                "FiberError",
                "attempt to yield on a not resumed fiber".to_string(),
            )),
        }
    }
    // The scheduler quartet. There is no fiber scheduler here, so every query
    // answers nil, `set_scheduler` takes only the nil that installs none, and
    // `schedule` raises the very error CRuby raises without one.
    def self."scheduler" | "current_scheduler" (_recv) {
        Ok(RubyValue::Nil)
    }
    def self."set_scheduler"(_recv, scheduler) {
        if !scheduler.is_nil() {
            return Err(raise_error(
                "NotImplementedError",
                "zeo has no fiber scheduler".to_string(),
            ));
        }
        Ok(RubyValue::Nil)
    }
    def self."schedule" cfunc (_recv, *_args, &_block) {
        Err(raise_error("RuntimeError", "No scheduler is available!".to_string()))
    }
    // `Fiber.blocking?` -- non-false while the ROOT fiber runs, which is the
    // one no scheduler created. CRuby answers the nesting count, and with no
    // scheduler that is always 1.
    def self."blocking?"(_recv) {
        let here = fiber::fiber_current().as_fiber_unchecked();
        Ok(match fiber::fiber_is_root(&here) {
            true => RubyValue::Int(1),
            false => RubyValue::Bool(false),
        })
    }
    // `Fiber.blocking { |fiber| ... }` -- run the block with the current fiber
    // forced into blocking mode. Every fiber here is already blocking, so the
    // whole method is the yield.
    def self."blocking"(_recv, &block) {
        let Some(block) = block else {
            return Err(raise_error("LocalJumpError", "no block given".to_string()));
        };
        block.as_proc_unchecked().call(&[fiber::fiber_current()])
    }

    def "resume" cfunc (_recv, *args) {
        outcome(fiber_resume(f, args.to_vec()))
    }
    def "transfer" cfunc (_recv, *args) {
        outcome(fiber_transfer(f, args.to_vec()))
    }
    // A reachable Fiber always carries its block -- CRuby's refusal.
    private def "initialize" cfunc (_recv, *_args, &_block) {
        Err(crate::builtins::runtime_error!("cannot initialize twice"))
    }
    def "alive?"(_recv) {
        Ok(RubyValue::Bool(fiber_alive(f)))
    }
    def "kill"(recv) {
        fiber::fiber_kill(f);
        // CRuby answers the (now terminated) fiber itself.
        Ok(recv.clone())
    }
    def "raise" cfunc (_recv, *args) {
        let exc = resolve_raise_exc(args)?;
        outcome(fiber::fiber_raise(f, exc))
    }
    def "storage"(_recv) {
        require_current(f)?;
        Ok(fiber::fiber_storage_hash(f))
    }
    def "storage="(_recv, value) {
        require_current(f)?;
        let pairs = match value {
            RubyValue::Nil => Vec::new(),
            RubyValue::Hash(h) => {
                let mut out = Vec::new();
                for (k, v) in h.lock().values() {
                    out.push((key_sym(k)?, v.clone()));
                }
                out
            }
            // NOT an implicit-conversion site: CRuby's fiber storage requires a
            // literal Hash ("storage must be a hash", oracle-verified).
            _ => return Err(type_error!("storage must be a hash")),
        };
        fiber::fiber_set_storage(f, pairs);
        Ok(value.clone())
    }
    // A fiber `Fiber.new` created is NON-blocking, whatever the scheduler
    // situation; only a thread's root fiber is blocking.
    def "blocking?"(_recv) {
        Ok(RubyValue::Bool(fiber::fiber_is_root(f)))
    }
    def "backtrace" cfunc (_recv, *_args) {
        let lines = own_frames(f)
            .iter()
            .map(|(file, line, m)| {
                RubyValue::Str(crate::string_new(format!("{file}:{line}:in '{m}'")))
            })
            .collect();
        Ok(RubyValue::Array(crate::array_new(lines)))
    }
    def "backtrace_locations" cfunc (_recv, *_args) {
        let locations = own_frames(f)
            .iter()
            .map(|(file, line, m)| crate::builtins::backtrace_location::location_new(file, *line, m))
            .collect();
        Ok(RubyValue::Array(crate::array_new(locations)))
    }
    def "inspect" | "to_s"(_recv) {
        Ok(RubyValue::Str(crate::string_new(fiber::fiber_inspect(f))))
    }
    // Two Fiber objects are equal iff they are the same fiber (identity).
    def "==" | "eql?" | "equal?" (_recv, other) {
        let same = matches!(other, RubyValue::Fiber(o) if std::sync::Arc::ptr_eq(f, o));
        Ok(RubyValue::Bool(same))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fiber::fiber_new;

    /// The `ruby_class!`-generated rows are reachable only through the dispatch
    /// tables (their Rust fn names are mangled), so the tests call them the way
    /// real dispatch does -- through Fiber's registered instance lookup.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::FIBER_CLASS)
            .expect("Fiber is a registered builtin table")
            .instance
            .as_ref()
            .expect("Fiber has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Fiber#{name} is defined"))
    }

    /// These rows exist so a fiber held in an ivar/collection -- i.e. any
    /// receiver whose type isn't statically known -- can still be resumed;
    /// the static Path-1 fast path in codegen never comes here.
    #[test]
    fn the_table_covers_the_dynamic_fiber_surface() {
        let tbl = crate::builtins::registered_table(zeo_abi::FIBER_CLASS).unwrap();
        let inst = tbl.instance.as_ref().unwrap();
        assert!((inst.lookup)("resume").is_some());
        assert!((inst.lookup)("alive?").is_some());
        assert!((inst.lookup)("nope").is_none());
        assert!((tbl.class.as_ref().unwrap().lookup)("current").is_some());
    }

    #[test]
    fn resume_drives_the_fiber_and_alive_p_tracks_it() {
        let body = RubyValue::Proc(crate::RProc::new(|_args: &[RubyValue]| {
            crate::fiber::fiber_yield(vec![RubyValue::Int(1)]);
            Ok(RubyValue::Int(2))
        }));
        let f = fiber_new(body);

        assert_eq!(
            imethod("alive?")(&f, &[], None).unwrap().inspect_string(),
            "true"
        );
        assert_eq!(
            imethod("resume")(&f, &[], None).unwrap().inspect_string(),
            "1"
        );
        // The body's final value, after which the fiber is dead.
        assert_eq!(
            imethod("resume")(&f, &[], None).unwrap().inspect_string(),
            "2"
        );
        assert_eq!(
            imethod("alive?")(&f, &[], None).unwrap().inspect_string(),
            "false"
        );
    }

    /// Resuming a finished fiber is a FiberError -- surfacing as a panic
    /// only because these tests run without a `ClassRegistry` installed.
    #[test]
    fn resuming_a_dead_fiber_is_a_fiber_error() {
        let body = RubyValue::Proc(crate::RProc::new(|_| Ok(RubyValue::Nil)));
        let f = fiber_new(body);
        imethod("resume")(&f, &[], None).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            imethod("resume")(&f, &[], None)
        }));
        assert!(r.is_err());
    }

    /// A resume argument reaches the fiber's block.
    #[test]
    fn resume_passes_its_arguments_to_the_block() {
        let body = RubyValue::Proc(crate::RProc::new(|args: &[RubyValue]| {
            Ok(args.first().cloned().unwrap_or(RubyValue::Nil))
        }));
        let f = fiber_new(body);
        let out = imethod("resume")(&f, &[RubyValue::Int(7)], None).unwrap();
        assert_eq!(out.inspect_string(), "7");
    }
}
