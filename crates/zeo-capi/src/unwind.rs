//! Crossing between an extension's non-local exit and `Result<_, Signal>`.
//!
//! An extension raises by not returning. zeo raises by returning an `Err`.
//! The bridge is Rust's own unwinder: [`raise`] starts an unwind that carries
//! the `Signal`, and [`protect`] -- the only door into extension C -- catches
//! it and answers the `Err`. Every Rust function C can call is `extern
//! "C-unwind"` (see [`crate::cext_fn`]), and every C function zeo calls is
//! declared so, which is what lets the unwind travel through the extension's
//! own frames.
//!
//! # What this asks of the extension
//!
//! Its objects must carry unwind tables. zeo's `RbConfig` cflags add
//! `-fexceptions -fasynchronous-unwind-tables`, `mkmf` drops a gem's own
//! `-fno-*` twins with a warning, and the build refuses a Makefile that
//! still carries one. A C++ extension that wraps a Ruby call in `catch (...)`
//! intercepts the unwind; `rb_protect` is the spelling for that.
//!
//! # What this promises
//!
//! * Only a `Signal` travels this way. A genuine Rust panic inside a C frame
//!   is rethrown by `protect` exactly as it arrived, never turned into a
//!   Ruby exception.
//! * There is never an `extern "C"` Rust frame between a `raise` and the
//!   `protect` that catches it, and no zeo binary builds with
//!   `panic = "abort"`.
//!
//! # Fibers
//!
//! A protected frame is counted on the stack it is on (`zeo_rt::cframes`),
//! and a coroutine switch carries the count with the stack.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use zeo_rt::{Signal, cframes};

/// The unwind payload: a raise on its way to the innermost [`protect`].
struct Raised(Signal);

/// Leave for the innermost [`protect`], carrying `sig`.
///
/// Never returns. Every Rust frame on the way is `extern "C-unwind"` or
/// plain Rust, so its locals drop as the unwind passes.
pub fn raise(sig: Signal) -> ! {
    resume_unwind(Box::new(Raised(sig)))
}

/// Run `body` with a landing pad, turning a non-local exit back into an
/// `Err`. This is the only door into extension C.
pub fn protect<R>(body: impl FnOnce() -> R) -> Result<R, Signal> {
    cframes::enter();
    let out = catch_unwind(AssertUnwindSafe(body));
    cframes::leave();
    out.map_err(signal_of)
}

/// The `Signal` a payload carries. Anything else is a real Rust panic, and
/// it keeps travelling.
fn signal_of(payload: Box<dyn Any + Send>) -> Signal {
    match payload.downcast::<Raised>() {
        Ok(raised) => raised.0,
        Err(other) => resume_unwind(other),
    }
}

/// Wrap a Rust function C can call, so a `Signal` becomes an unwind.
///
/// The body runs in its own frame and hands back a `Result`. Only after it
/// has returned -- every local dropped, nothing live -- does the wrapper
/// raise. Nothing forces that ordering any more, but it keeps every entry
/// point's raise in one place, which is where the hundreds of them stay
/// readable.
#[macro_export]
macro_rules! cext_fn {
    ($(
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty $body:block
    )*) => {$(
        $(#[$meta])*
        #[unsafe(no_mangle)]
        #[allow(clippy::redundant_closure_call, reason = "the immediately-called closure gives the body a frame of its own, so the `Result` is matched with nothing of the body's alive")]
        #[allow(clippy::missing_safety_doc, reason = "one C ABI contract for every row -- C calls it with arguments matching the declared signature -- so 400 identical `# Safety` sections would say nothing this does not")]
        pub unsafe extern "C-unwind" fn $name($($arg : $ty),*) -> $ret {
            let outcome: ::std::result::Result<$ret, ::zeo_rt::Signal> = (|| $body)();
            match outcome {
                ::std::result::Result::Ok(v) => v,
                ::std::result::Result::Err(sig) => $crate::unwind::raise(sig),
            }
        }
    )*};
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Signal` a registry-less unit test can build. `runtime_error!`
    /// constructs a real exception object through the class registry, which
    /// no unit test installs -- it panics instead. What is on trial here is
    /// the unwind, and any `Signal` proves it.
    fn a_signal(tag: i64) -> Signal {
        Signal::Break(zeo_rt::RubyValue::Int(tag))
    }

    #[test]
    fn a_body_that_returns_answers_its_value() {
        assert_eq!(protect(|| 41 + 1).unwrap_or(0), 42);
        assert_eq!(cframes::depth(), 0, "the frame was not popped");
    }

    #[test]
    fn a_raise_lands_in_the_innermost_protect() {
        let err = protect(|| raise(a_signal(1)));
        assert!(err.is_err(), "the raise did not reach the landing pad");
        assert_eq!(cframes::depth(), 0);
    }

    #[test]
    fn the_innermost_frame_catches_and_the_outer_one_does_not() {
        let outer: Result<Result<(), Signal>, Signal> = protect(|| {
            let inner = protect(|| raise(a_signal(2)));
            assert!(inner.is_err());
            inner
        });
        let inner = outer.expect("the outer frame must not have caught anything");
        assert!(inner.is_err());
        assert_eq!(cframes::depth(), 0);
    }

    #[test]
    fn a_raise_past_two_frames_reaches_the_one_that_wrapped_it() {
        let r = protect(|| {
            // No inner `protect`: the raise below has to travel through this
            // closure's frame to the outer pad.
            let deeper = || raise(a_signal(3));
            deeper()
        });
        assert!(r.is_err());
        assert_eq!(cframes::depth(), 0);
    }

    /// A real Rust panic is not a Ruby exception: `protect` lets it through
    /// untouched.
    #[test]
    fn a_rust_panic_keeps_travelling() {
        let caught = catch_unwind(|| protect(|| -> () { panic!("a bug, not a raise") }));
        let payload = caught.expect_err("the panic was swallowed");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"a bug, not a raise"));
        assert_eq!(cframes::depth(), 0);
    }

    /// The unwind drops every `Scope` it passes, so the handles an
    /// extension touched before raising are released with it.
    #[test]
    fn a_raise_releases_the_scopes_it_flew_past() {
        let before = super::super::handles::live_count();
        let r = protect(|| {
            let _scope = super::super::scope::Scope::enter();
            let s = zeo_rt::builtins::string::str_value_in_enc(zeo_rt::encoding::UTF_8, "held");
            super::super::handles::pin(&s).expect("a String has a handle");
            assert_eq!(super::super::handles::live_count(), before + 1);
            raise(a_signal(4))
        });
        assert!(r.is_err());
        assert_eq!(
            super::super::scope::depth(),
            0,
            "a scope survived the unwind"
        );
        assert_eq!(
            super::super::handles::live_count(),
            before,
            "a handle stayed pinned across an unwind"
        );
    }

    // One entry point written through the macro, so the wrapper's two
    // edges are tested rather than trusted.
    crate::cext_fn! {
        fn zeo_cext_test_ok(n: i64) -> i64 { Ok(n * 2) }
    }
    crate::cext_fn! {
        fn zeo_cext_test_raises(n: i64) -> i64 { Err(a_signal(n)) }
    }

    #[test]
    fn a_cext_fn_returns_normally_and_raises_through_the_pad() {
        assert_eq!(protect(|| unsafe { zeo_cext_test_ok(21) }).ok(), Some(42));

        let err = protect(|| unsafe { zeo_cext_test_raises(7) });
        match err {
            Err(Signal::Break(zeo_rt::RubyValue::Int(7))) => {}
            other => panic!("the macro lost the signal: {other:?}"),
        }
        assert_eq!(cframes::depth(), 0);
    }

    /// A coroutine that starts while a protected frame is open must not
    /// inherit its count; the resumer gets its own back.
    #[test]
    fn a_switch_hands_the_new_stack_an_empty_count() {
        let seen = protect(|| {
            assert_eq!(cframes::depth(), 1);
            let inner = {
                let _guard = cframes::SwitchGuard::enter();
                cframes::depth()
            };
            assert_eq!(inner, 0, "the new stack inherited a frame count");
            cframes::depth()
        });
        assert_eq!(seen.ok(), Some(1), "the resumer's count was not restored");
        assert_eq!(cframes::depth(), 0);
    }
}
