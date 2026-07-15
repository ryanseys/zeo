//! `Fiber`'s Path-2 (runtime `send`) rows. The static Path-1 fast path in
//! `codegen::call` emits `fiber_resume`/`fiber_alive` calls directly and
//! never comes here; these rows serve dynamically-typed receivers (a fiber
//! held in an ivar/Hash/`send` target), mirroring that codegen arm exactly,
//! error messages included.

use crate::dispatch::raise_error;
use crate::fiber::{fiber_alive, fiber_resume, FiberResume};
use crate::signal::Signal;
use crate::value::RubyValue;

fn f_resume(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    match fiber_resume(&recv.as_fiber_unchecked(), args.to_vec()) {
        FiberResume::Value(v) => Ok(v),
        FiberResume::RubyError(sig) => Err(sig),
        FiberResume::Dead => Err(raise_error(
            "FiberError",
            "attempt to resume a terminated fiber".to_string(),
        )),
        FiberResume::DoubleResume => Err(raise_error(
            "FiberError",
            "attempt to resume the current fiber (double resume)".to_string(),
        )),
        FiberResume::CrossThread => Err(raise_error(
            "FiberError",
            "fiber called across threads".to_string(),
        )),
    }
}

fn f_alive_p(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(fiber_alive(&recv.as_fiber_unchecked())))
}

pub fn lookup(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "resume" => f_resume,
        "alive?" => f_alive_p,
        _ => return None,
    })
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
