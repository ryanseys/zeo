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
//! `suspend`. That is why the head lives in Rust rather than in C.

use crate::Signal;
use std::cell::Cell;
use std::ffi::c_void;

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
    /// The innermost `zeo_cext_jmp` frame, or null.
    ///
    /// Rust owns it rather than C so it can follow a fiber's stack later; for
    /// now [`assert_switchable`] keeps the two from disagreeing.
    static HEAD: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };

    /// The `Signal` a raise left behind, read by whoever catches the tag.
    static PENDING: Cell<Option<Box<Signal>>> = const { Cell::new(None) };

    /// How many protected frames are open on this stack.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

#[unsafe(no_mangle)]
extern "C" fn zeo_cext_jmp_head() -> *mut c_void {
    HEAD.get()
}

#[unsafe(no_mangle)]
extern "C" fn zeo_cext_jmp_set_head(head: *mut c_void) {
    HEAD.set(head);
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

    DEPTH.set(DEPTH.get() + 1);
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
    DEPTH.set(DEPTH.get() - 1);
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
    crate::builtins::runtime_error!(
        "a C extension left through tag {tag} with no exception; \
         this is a gem calling rb_jump_tag outside a protected frame"
    )
}

/// How many protected frames are open. `cext::scope` and the fiber switch
/// both read it.
pub fn depth() -> usize {
    DEPTH.get()
}

/// This stack's chain, for a coroutine switch to put back later.
///
/// A `jmp_buf` names addresses on one stack, so the chain has to travel with
/// it. Both halves of a switch use this: `coroutine::resume` through
/// [`SwitchGuard`], and `coroutine::yield_current` by hand around its
/// `suspend`.
#[derive(Clone, Copy)]
pub struct Saved {
    head: *mut c_void,
    depth: usize,
}

pub fn save() -> Saved {
    Saved {
        head: HEAD.get(),
        depth: DEPTH.get(),
    }
}

pub fn restore(s: Saved) {
    HEAD.set(s.head);
    DEPTH.set(s.depth);
}

/// Restores the resumer's chain when control comes back, however it comes
/// back -- by yield, by completion, or by unwind.
pub struct SwitchGuard(Saved);

impl SwitchGuard {
    pub fn enter() -> SwitchGuard {
        let saved = save();
        // The coroutine starts on a stack of its own with no protected frame
        // on it. Leaving the resumer's head in place would let a raise there
        // jump into a stack that is not running.
        restore(Saved {
            head: std::ptr::null_mut(),
            depth: 0,
        });
        SwitchGuard(saved)
    }
}

impl Drop for SwitchGuard {
    fn drop(&mut self) {
        restore(self.0);
    }
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
    (
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty $body:block
    ) => {
        $(#[$meta])*
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name($($arg : $ty),*) -> $ret {
            let outcome: ::std::result::Result<$ret, $crate::Signal> = (|| $body)();
            match outcome {
                ::std::result::Result::Ok(v) => v,
                ::std::result::Result::Err(sig) => $crate::cext::jmp::raise(sig),
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Signal` a registry-less unit test can build. `runtime_error!`
    /// constructs a real exception object through the class registry, which
    /// no unit test installs -- it panics instead. What is on trial here is
    /// the jump, and any `Signal` proves it.
    fn a_signal(tag: i64) -> Signal {
        Signal::Break(crate::RubyValue::Int(tag))
    }

    #[test]
    fn a_body_that_returns_answers_its_value() {
        assert_eq!(protect(|| 41 + 1).unwrap_or(0), 42);
        assert_eq!(depth(), 0, "the frame was not popped");
    }

    #[test]
    fn a_raise_lands_in_the_innermost_protect() {
        let err = protect(|| raise(a_signal(1)));
        assert!(err.is_err(), "the raise did not reach the landing pad");
        assert_eq!(depth(), 0);
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
        assert_eq!(depth(), 0);
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
        assert_eq!(depth(), 0);
    }

    /// A longjmp skips every `Scope::drop` on the way out, so `protect` has
    /// to release those pins itself. Without this the handles an extension
    /// touched before raising would stay pinned for the life of the process.
    #[test]
    fn a_raise_releases_the_scopes_it_flew_past() {
        let before = super::super::handles::live_count();
        let r = protect(|| {
            let _scope = super::super::scope::Scope::enter();
            let s = crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, "held");
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
            Err(Signal::Break(crate::RubyValue::Int(7))) => {}
            other => panic!("the macro lost the signal: {other:?}"),
        }
        assert_eq!(depth(), 0);
    }

    /// A `jmp_buf` names one stack. A coroutine that starts while a
    /// protected frame is open must not inherit it, or a raise on the new
    /// stack jumps into a stack that is not running.
    #[test]
    fn a_switch_hands_the_new_stack_an_empty_chain() {
        let seen = protect(|| {
            assert_eq!(depth(), 1);
            let inner = {
                let _guard = SwitchGuard::enter();
                (depth(), super::HEAD.get().is_null())
            };
            assert_eq!(inner, (0, true), "the new stack inherited a chain");
            depth()
        });
        assert_eq!(seen.ok(), Some(1), "the resumer's chain was not restored");
        assert_eq!(depth(), 0);
    }
}
