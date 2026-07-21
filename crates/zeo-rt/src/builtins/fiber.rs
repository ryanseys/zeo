//! `Fiber`'s Path-2 (runtime `send`) rows. The static Path-1 fast path in
//! `codegen::call` emits `fiber_resume`/`fiber_alive` calls directly and
//! never comes here; these rows serve dynamically-typed receivers (a fiber
//! held in an ivar/Hash/`send` target), mirroring that codegen arm exactly,
//! error messages included.

use crate::Symbol;
use crate::builtins::{arg_error, type_error};
use crate::dispatch::raise_error;
use crate::fiber::{self, FiberResume, fiber_alive, fiber_resume, fiber_transfer};
use crate::signal::Signal;
use crate::value::RubyValue;

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

fn f_resume(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match fiber_resume(&recv.as_fiber_unchecked(), args.to_vec()) {
        FiberResume::Value(v) => Ok(v),
        FiberResume::RubyError(sig) => Err(sig),
        FiberResume::Dead => Err(raise_error(
            "FiberError",
            "attempt to resume a terminated fiber".to_string(),
        )),
        FiberResume::DoubleResume => Err(raise_error(
            "FiberError",
            "attempt to resume a resumed fiber (double resume)".to_string(),
        )),
        FiberResume::CrossThread => Err(raise_error(
            "FiberError",
            "fiber called across threads".to_string(),
        )),
    }
}

fn f_transfer(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match fiber_transfer(&recv.as_fiber_unchecked(), args.to_vec()) {
        FiberResume::Value(v) => Ok(v),
        FiberResume::RubyError(sig) => Err(sig),
        FiberResume::Dead => Err(raise_error(
            "FiberError",
            "attempt to resume a terminated fiber".to_string(),
        )),
        FiberResume::DoubleResume => Err(raise_error(
            "FiberError",
            "attempt to resume a resumed fiber (double resume)".to_string(),
        )),
        FiberResume::CrossThread => Err(raise_error(
            "FiberError",
            "fiber called across threads".to_string(),
        )),
    }
}

fn f_alive_p(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(fiber_alive(&recv.as_fiber_unchecked())))
}

fn f_kill(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    fiber::fiber_kill(&recv.as_fiber_unchecked());
    // CRuby answers the (now terminated) fiber itself.
    Ok(recv.clone())
}

/// CRuby restricts `#storage`/`#storage=` to the fiber they belong to.
fn require_current(recv: &RubyValue) -> Result<crate::fiber::RFiber, Signal> {
    let handle = recv.as_fiber_unchecked();
    if !fiber::fiber_is_current(&handle) {
        return Err(arg_error!(
            "Fiber storage can only be accessed from the Fiber it belongs to"
        ));
    }
    Ok(handle)
}

fn f_storage(
    recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(fiber::fiber_storage_hash(&require_current(recv)?))
}

fn f_set_storage(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let handle = require_current(recv)?;
    let pairs = match &args[0] {
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
    fiber::fiber_set_storage(&handle, pairs);
    Ok(args[0].clone())
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
        Some(v) => Ok(crate::dispatch::coerce_raise_arg(v.clone())),
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

fn f_raise(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let exc = resolve_raise_exc(args)?;
    match fiber::fiber_raise(&recv.as_fiber_unchecked(), exc) {
        FiberResume::Value(v) => Ok(v),
        FiberResume::RubyError(sig) => Err(sig),
        FiberResume::Dead => Err(raise_error(
            "FiberError",
            "attempt to resume a terminated fiber".to_string(),
        )),
        FiberResume::DoubleResume => Err(raise_error(
            "FiberError",
            "attempt to resume a resumed fiber (double resume)".to_string(),
        )),
        FiberResume::CrossThread => Err(raise_error(
            "FiberError",
            "fiber called across threads".to_string(),
        )),
    }
}

// Two Fiber objects are equal iff they are the same fiber (identity).
fn f_eq(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let same = matches!(&args[0], RubyValue::Fiber(o) if std::sync::Arc::ptr_eq(&recv.as_fiber_unchecked(), o));
    Ok(RubyValue::Bool(same))
}

/// `Method#arity` twin of `lookup` -- this hand-rolled table declares no
/// per-method argc, so every method it defines reports CRuby's
/// variadic-cfunc default (`-1`).
pub fn lookup_arity(name: &str) -> Option<i64> {
    lookup(name).map(|_| -1)
}

pub fn lookup(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "resume" => f_resume,
        "transfer" => f_transfer,
        "alive?" => f_alive_p,
        "kill" => f_kill,
        "raise" => f_raise,
        "storage" => f_storage,
        "storage=" => f_set_storage,
        "==" | "eql?" | "equal?" => f_eq,
        _ => return None,
    })
}

/// Reflection companion to `lookup` (hand-written table).
pub fn lookup_names() -> &'static [&'static str] {
    &[
        "resume", "transfer", "alive?", "kill", "raise", "storage", "storage=", "==", "eql?",
        "equal?",
    ]
}

// --- Class methods (`Fiber.current`, `Fiber[]`, `Fiber.[]=`) ---------------

fn c_current(
    _recv: &RubyValue,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(fiber::fiber_current())
}

fn c_aref(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(fiber::fiber_storage_get(key_sym(&args[0])?))
}

fn c_aset(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    fiber::fiber_storage_set(key_sym(&args[0])?, args[1].clone());
    Ok(args[1].clone())
}

pub fn lookup_class(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "current" => c_current,
        "[]" => c_aref,
        "[]=" => c_aset,
        _ => return None,
    })
}

pub fn lookup_class_names() -> &'static [&'static str] {
    &["current", "[]", "[]="]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fiber::fiber_new;

    /// These rows exist so a fiber held in an ivar/collection -- i.e. any
    /// receiver whose type isn't statically known -- can still be resumed;
    /// the static Path-1 fast path in codegen never comes here.
    #[test]
    fn the_table_covers_the_dynamic_fiber_surface() {
        assert!(lookup("resume").is_some());
        assert!(lookup("alive?").is_some());
        assert!(lookup("nope").is_none());
    }

    #[test]
    fn resume_drives_the_fiber_and_alive_p_tracks_it() {
        let body = RubyValue::Proc(crate::RProc::new(|_args: &[RubyValue]| {
            crate::fiber::fiber_yield(vec![RubyValue::Int(1)]);
            Ok(RubyValue::Int(2))
        }));
        let f = fiber_new(body);

        assert_eq!(f_alive_p(&f, &[], None).unwrap().inspect_string(), "true");
        assert_eq!(f_resume(&f, &[], None).unwrap().inspect_string(), "1");
        assert_eq!(f_alive_p(&f, &[], None).unwrap().inspect_string(), "true");
        // The body's final value, after which the fiber is dead.
        assert_eq!(f_resume(&f, &[], None).unwrap().inspect_string(), "2");
        assert_eq!(f_alive_p(&f, &[], None).unwrap().inspect_string(), "false");
    }

    /// Resuming a finished fiber is a FiberError -- surfacing as a panic
    /// only because these tests run without a `ClassRegistry` installed.
    #[test]
    fn resuming_a_dead_fiber_is_a_fiber_error() {
        let body = RubyValue::Proc(crate::RProc::new(|_| Ok(RubyValue::Nil)));
        let f = fiber_new(body);
        f_resume(&f, &[], None).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f_resume(&f, &[], None)));
        assert!(r.is_err());
    }

    /// A resume argument reaches the fiber's block.
    #[test]
    fn resume_passes_its_arguments_to_the_block() {
        let body = RubyValue::Proc(crate::RProc::new(|args: &[RubyValue]| {
            Ok(args.first().cloned().unwrap_or(RubyValue::Nil))
        }));
        let f = fiber_new(body);
        let out = f_resume(&f, &[RubyValue::Int(7)], None).unwrap();
        assert_eq!(out.inspect_string(), "7");
    }
}
