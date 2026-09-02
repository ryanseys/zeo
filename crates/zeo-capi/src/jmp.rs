//! Crossing between `longjmp` and `Result<_, Signal>`.
//!
//! An extension raises by not returning. zeo raises by returning an `Err`.
//! Bridging them is the one part of the C surface that cannot be written in
//! safe Rust and cannot be written in Rust at all: a `longjmp` past a live
//! Rust frame skips its destructors, which is undefined behaviour rather than
//! a leak. So the `setjmp` lives in `csrc/cext_jmp.c` and this module is the
//! Rust half of the same mechanism.
//!
//! # The rule
//!
//! **The longjmp only ever unwinds C frames of the extension.** Two things
//! hold it:
//!
//! * [`protect`] is the only way into C, and the frame it establishes sits
//!   immediately below the extension's frames and above the Rust that called
//!   in. Nothing of zeo's is in between.
//! * Every Rust function C can call is written through [`crate::cext_fn`],
//!   which runs its body in an inner frame, takes the `Result` back, and only
//!   then stores the `Signal` and jumps. By the time [`raise`] is reached the
//!   body's locals are already dropped.
//!
//! # Fibers
//!
//! The chain names addresses on the CURRENT stack, and zeo's fibers are real
//! stackful coroutines with a userspace stack switch. A `Fiber.yield` out of
//! an extension callback would otherwise leave the chain pointing at a stack
//! that is no longer running, and the next `rb_raise` would land in it.
//!
//! So the chain travels with the stack, exactly as `coroutine`'s `CURRENT`
//! does and through the same two hooks: the resumer saves and restores its
//! own around `resume`, and a waking coroutine re-establishes its own after
//! `suspend`. That is why the head lives in Rust rather than in C -- and in
//! the runtime (`zeo_rt::cframes`) rather than here, because the switch
//! happens whether or not this crate is linked.

use std::cell::Cell;
use std::ffi::c_void;
use zeo_rt::{Signal, cframes};

/// The tag `rb_raise` and friends leave with. MRI's `ruby_tag_type` numbers
/// these; only `RUBY_TAG_RAISE` is minted here, because zeo carries the rest
/// of the reasons inside the `Signal` itself.
const TAG_RAISE: i32 = 6;

unsafe extern "C" {
    fn zeo_cext_call_protected(
        body: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32;
    fn zeo_cext_jump_tag(tag: i32) -> !;
}

thread_local! {
    /// The `Signal` a raise left behind, read by whoever catches the tag.
    static PENDING: Cell<Option<Box<Signal>>> = const { Cell::new(None) };
}

/// The innermost `zeo_cext_jmp` frame, or null: C reads and writes it
/// through these two, so the chain can follow a fiber's stack.
#[unsafe(no_mangle)]
extern "C" fn zeo_cext_jmp_head() -> *mut c_void {
    cframes::head()
}

#[unsafe(no_mangle)]
extern "C" fn zeo_cext_jmp_set_head(head: *mut c_void) {
    cframes::set_head(head);
}

/// Leave for the innermost [`protect`], carrying `sig`.
///
/// Never returns. Call it only from a `cext_fn!` body's tail, where no Rust
/// destructor is live -- that is the whole contract, and the macro is what
/// makes it true rather than a comment.
pub fn raise(sig: Signal) -> ! {
    PENDING.set(Some(Box::new(sig)));
    // SAFETY: the C side aborts with a diagnostic rather than jumping when no
    // frame is open, so this either lands in a `setjmp` or dies saying why.
    unsafe { zeo_cext_jump_tag(TAG_RAISE) }
}

/// Run `body` with a landing pad, turning a non-local exit back into an
/// `Err`.
///
/// This is the only door into extension C. `body` is `FnOnce`, but it is
/// invoked through a raw pointer across a `setjmp`, so anything it captures
/// is leaked if the extension jumps out -- exactly as an MRI extension's own
/// C locals are. Keep captures to handles the caller still owns.
pub fn protect<R>(body: impl FnOnce() -> R) -> Result<R, Signal> {
    // `Option` so the trampoline can move the closure out; a jump leaves it
    // in place and the whole cell is dropped by the caller's frame.
    let mut slot: Option<Box<dyn FnOnce() -> R>> = Some(Box::new(body));
    let mut result: Option<R> = None;
    let mut pair = (&mut slot, &mut result);

    unsafe extern "C" fn trampoline<R>(arg: *mut c_void) -> *mut c_void {
        // SAFETY: `arg` is the `pair` below, and nothing else reaches here.
        let pair =
            unsafe { &mut *arg.cast::<(&mut Option<Box<dyn FnOnce() -> R>>, &mut Option<R>)>() };
        if let Some(f) = pair.0.take() {
            *pair.1 = Some(f());
        }
        std::ptr::null_mut()
    }

    cframes::enter();
    // Where the scope stack stood before C could touch it. A longjmp skips
    // every `Scope::drop` it flies past, so the unwind below is the only
    // thing that releases those pins.
    let scopes = super::scope::depth();
    // SAFETY: `trampoline::<R>` matches the C prototype and `pair` outlives
    // the call. `out` is null because the trampoline answers through `pair`.
    let tag = unsafe {
        zeo_cext_call_protected(
            trampoline::<R>,
            std::ptr::from_mut(&mut pair).cast(),
            std::ptr::null_mut(),
        )
    };
    cframes::leave();
    super::scope::unwind_to(scopes);

    if tag == 0 {
        return Ok(result.expect("a protected body that returned left its value"));
    }
    // A tag with no pending signal means C jumped for a reason zeo did not
    // mint. There is no honest value to answer, so say so rather than
    // inventing one.
    Err(*PENDING
        .take()
        .unwrap_or_else(|| Box::new(unexpected_tag(tag))))
}

fn unexpected_tag(tag: i32) -> Signal {
    zeo_rt::builtins::runtime_error!(
        "a C extension left through tag {tag} with no exception; \
         this is a gem calling rb_jump_tag outside a protected frame"
    )
}

/// Wrap a Rust function C can call, so a `Signal` becomes a `longjmp`.
///
/// The body runs in its own frame and hands back a `Result`. Only after it
/// has returned -- every local dropped, nothing live -- does the wrapper
/// store the signal and jump. That ordering is the reason the macro exists;
/// writing the same two statements by hand at 400 entry points is where the
/// one that leaks a `String` across a `longjmp` would come from.
#[macro_export]
macro_rules! cext_fn {
    ($(
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty $body:block
    )*) => {$(
        $(#[$meta])*
        #[unsafe(no_mangle)]
        // The immediately-called closure is the POINT, not a clumsy block:
        // it gives the body a frame that ends before `raise` longjmps out.
        // Inlining it would leave the body's locals live across the jump,
        // which is the leak this macro exists to prevent.
        #[allow(clippy::redundant_closure_call)]
        // Every one of these is a C ABI entry point with one contract --
        // C calls it with arguments matching the declared signature -- so
        // 400 identical `# Safety` sections would say nothing this does not.
        #[allow(clippy::missing_safety_doc)]
        pub unsafe extern "C" fn $name($($arg : $ty),*) -> $ret {
            let outcome: ::std::result::Result<$ret, ::zeo_rt::Signal> = (|| $body)();
            match outcome {
                ::std::result::Result::Ok(v) => v,
                ::std::result::Result::Err(sig) => $crate::jmp::raise(sig),
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
    /// the jump, and any `Signal` proves it.
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

    /// A longjmp skips every `Scope::drop` on the way out, so `protect` has
    /// to release those pins itself. Without this the handles an extension
    /// touched before raising would stay pinned for the life of the process.
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
        assert_eq!(super::super::scope::depth(), 0, "a scope survived the jump");
        assert_eq!(
            super::super::handles::live_count(),
            before,
            "a handle stayed pinned across a longjmp"
        );
    }

    // The macro is what makes the ordering true at 400 entry points rather
    // than being a comment, so one entry point written through it is tested.
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

    /// A `jmp_buf` names one stack. A coroutine that starts while a
    /// protected frame is open must not inherit it, or a raise on the new
    /// stack jumps into a stack that is not running.
    #[test]
    fn a_switch_hands_the_new_stack_an_empty_chain() {
        let seen = protect(|| {
            assert_eq!(cframes::depth(), 1);
            let inner = {
                let _guard = cframes::SwitchGuard::enter();
                (cframes::depth(), cframes::head().is_null())
            };
            assert_eq!(inner, (0, true), "the new stack inherited a chain");
            cframes::depth()
        });
        assert_eq!(seen.ok(), Some(1), "the resumer's chain was not restored");
        assert_eq!(cframes::depth(), 0);
    }
}
