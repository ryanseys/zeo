//! The Proc -> Dynamic bridge and runtime method frames -- what a `super`
//! in a runtime-defined method resolves against.

use super::*;

/// Wrap a compiled block as an instance method: the method's receiver becomes
/// the block's `self` (`instance_exec`-style rebinding, which `RProc` supports
/// by taking self as a parameter), and the block the METHOD is called with is
/// forwarded into the body, so `yield`/`&blk` inside a `define_method` body
/// see the method's caller's block -- CRuby's `invoke_bmethod` specval, not
/// the closure env (see `ProcData::f`).
///
/// `defining` is the class this method is installed on and `name` its name; the
/// wrapper pushes them as the current method frame for the duration of the call
/// so a `super` in the body (which has no compile-time defining class -- the
/// class was minted at runtime) can resume the receiver's MRO walk after
/// `defining`. See [`send_super_dynamic`].
pub fn dynamic_from_proc(defining: ClassId, name: Symbol, body: RProc) -> MethodImpl {
    MethodImpl::Dynamic(Arc::new(move |recv: &RObj, args: &[RubyValue], block| {
        let self_val = RubyValue::Object(recv.clone());
        push_method_frame(defining, name);
        // Control flow here is `Result<_, Signal>`, never an unwinding panic, so
        // this pop runs on every exit (value OR signal) without a guard type.
        let out = body.call_with_self_and_block(&self_val, args, block);
        pop_method_frame();
        out
    }))
}

/// Run a BARE VALUE's singleton method inside a method frame.
///
/// [`dynamic_from_proc`] does this for an `Object` receiver by wrapping the
/// proc in a `MethodImpl`. A bare value has no `RObj` to bind, so its
/// singleton stays an `RProc` in its own table and the call site is the only
/// place the frame can be pushed -- without it, a `super` in
/// `class << "str"; def to_s = "x" + super; end` has no seat to walk up from
/// and raises "super called outside of method".
pub fn call_value_singleton(
    body: &RProc,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    push_method_frame(SINGLETON_DEFINING, name);
    // `Result<_, Signal>` throughout, never an unwinding panic, so the pop
    // runs on every exit without a guard type -- same as `dynamic_from_proc`.
    let out = body.call_with_self_and_block(recv, args, block);
    pop_method_frame();
    out
}

// ---------------------------------------------------------------------------
// Runtime method frames -- what a `super` in a runtime-defined method resolves
// against
// ---------------------------------------------------------------------------

thread_local! {
    /// The stack of runtime-defined methods currently executing on this thread,
    /// each `(defining class, method name)`. `dynamic_from_proc`'s wrapper
    /// pushes on entry and pops on exit, so the top frame is always the
    /// innermost runtime method -- exactly what a bare `super` there needs.
    static METHOD_FRAMES: RefCell<Vec<(ClassId, Symbol)>> = const { RefCell::new(Vec::new()) };

    /// The stack of runtime class BODIES currently executing -- a
    /// `Class.new`/`Module.new` block or a `class_eval`. Carries the running
    /// default visibility and `module_function` mode a bare directive sets,
    /// as CRuby's cref does, and dies with the body like a cref too.
    static BODY_FRAMES: RefCell<Vec<BodyFrame>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy)]
pub(super) struct BodyFrame {
    pub(super) class: ClassId,
    pub(super) vis: crate::dispatch::MethodVisibility,
    pub(super) module_function: bool,
}

/// Runs `f` with a fresh body frame for `id`, so bare directives inside it
/// have somewhere to record themselves.
pub(crate) fn with_body_frame<T>(
    id: ClassId,
    f: impl FnOnce() -> Result<T, Signal>,
) -> Result<T, Signal> {
    BODY_FRAMES.with(|s| {
        s.borrow_mut().push(BodyFrame {
            class: id,
            vis: crate::dispatch::MethodVisibility::Public,
            module_function: false,
        })
    });
    // A class body's definee is the class, even inside an enclosing
    // `instance_eval` -- so suspend those frames for this body.
    let outer = SINGLETON_DEFINEE.with(|s| std::mem::take(&mut *s.borrow_mut()));
    let out = f();
    SINGLETON_DEFINEE.with(|s| *s.borrow_mut() = outer);
    BODY_FRAMES.with(|s| {
        s.borrow_mut().pop();
    });
    out
}

// The receivers of the `instance_eval`/`instance_exec` frames open on this
// thread. Inside one, a `def` whose self IS that receiver installs on the
// singleton class -- Ruby's rule, and what `SingleForwardable` relies on.
// The receiver is recorded rather than a bare depth so that a `def` reached
// through some OTHER object's method, from inside the block, still follows
// the ordinary rule.
std::thread_local!(static SINGLETON_DEFINEE: std::cell::RefCell<Vec<RubyValue>> =
    const { std::cell::RefCell::new(Vec::new()) });

/// Run `f` with `recv`'s singleton class as the default definee --
/// `instance_eval`/`instance_exec`'s rule.
pub(crate) fn with_singleton_definee<T>(
    recv: &RubyValue,
    f: impl FnOnce() -> Result<T, Signal>,
) -> Result<T, Signal> {
    SINGLETON_DEFINEE.with(|s| s.borrow_mut().push(recv.clone()));
    let out = f();
    SINGLETON_DEFINEE.with(|s| {
        s.borrow_mut().pop();
    });
    out
}

/// The class an enclosing `class_eval`/`class_exec`/`Class.new`/`Module.new`
/// block made the default definee -- [`singleton_definee`]'s module twin, and
/// what makes a `def` in such a block land on the new class rather than on
/// the cref where the block was written.
///
/// `slf` has to still BE that class: a `def` reached through some other
/// object's method, from inside the block, follows the ordinary rule.
pub(crate) fn module_definee(slf: &RubyValue) -> Option<ClassId> {
    let RubyValue::Class(cid) = slf else {
        return None;
    };
    BODY_FRAMES.with(|s| s.borrow().last().map(|f| f.class).filter(|c| c == cid))
}

/// Whether a `def` running with `recv` as its self installs on the singleton
/// class -- true exactly inside an `instance_eval`/`instance_exec` of `recv`.
pub(crate) fn singleton_definee(recv: &RubyValue) -> bool {
    SINGLETON_DEFINEE.with(|s| {
        s.borrow().last().is_some_and(|open| match (open, recv) {
            (RubyValue::Class(a), RubyValue::Class(b)) => a == b,
            _ => crate::builtins::basic_object::value_identity(open, recv),
        })
    })
}

pub(super) fn current_frame_for(id: ClassId) -> Option<BodyFrame> {
    BODY_FRAMES.with(|s| s.borrow().iter().rev().find(|f| f.class == id).copied())
}

pub(super) fn update_frame_for(id: ClassId, f: impl FnOnce(&mut BodyFrame)) {
    BODY_FRAMES.with(|s| {
        if let Some(frame) = s.borrow_mut().iter_mut().rev().find(|fr| fr.class == id) {
            f(frame);
        }
    });
}

/// A sentinel "defining class" for a per-object singleton method: it is never a
/// real class id (`u32::MAX` is above every runtime id), so `send_super_from`'s
/// ancestor lookup misses it and resumes from the TOP of the receiver's own
/// ancestry -- which is exactly where a singleton method's `super` belongs (the
/// conceptual singleton class sits ahead of the object's real class).
pub const SINGLETON_DEFINING: ClassId = ClassId(u32::MAX);

pub(super) fn push_method_frame(defining: ClassId, name: Symbol) {
    METHOD_FRAMES.with(|f| f.borrow_mut().push((defining, name)));
}

pub(super) fn pop_method_frame() {
    METHOD_FRAMES.with(|f| {
        f.borrow_mut().pop();
    });
}

/// `super` from inside a RUNTIME-defined method (a `def` in a `Class.new` /
/// `Struct.new` / `Data.define` body, or a `define_method`) whose defining class
/// is not known at compile time. Reads this thread's current method frame --
/// pushed by [`dynamic_from_proc`] on entry -- and resumes the receiver's MRO
/// walk after that class, exactly like the compile-time `send_super_from`.
/// Raises `RuntimeError` when there is no active runtime frame (a `super`
/// written outside any method), matching CRuby's runtime error rather than a
/// compile-time rejection.
/// The raise for a BARE `super` in a scope with no compile-time method of its
/// own. A block that BECAME a method at run time (`define_method`, and the
/// same call through `send`) gets ruby's own refusal -- a zsuper forwards the
/// method's parameters and a block-shaped body has none to forward. Anywhere
/// else there is no method at all.
pub fn bare_super_outside_a_method() -> Signal {
    let in_method = METHOD_FRAMES.with(|f| !f.borrow().is_empty())
        || crate::eval::home_super_target().is_some();
    if in_method {
        runtime_error!(
            "implicit argument passing of super from method defined by \
             define_method() is not supported. Specify all arguments explicitly."
        )
    } else {
        crate::dispatch::raise_error(
            "NoMethodError",
            "super called outside of method".to_string(),
        )
    }
}

/// [`send_super_dynamic`]'s `defined?(super)` twin: whether a target exists,
/// asked without calling. A body with no method frame and no eval home is not
/// in a method at all, which is ruby's `nil`.
pub fn super_defined_dynamic(recv: &RubyValue) -> bool {
    let seat = METHOD_FRAMES
        .with(|f| f.borrow().last().copied())
        .or_else(crate::eval::home_super_target);
    match seat {
        Some((defining, name)) => crate::dispatch::super_defined(recv, defining, name),
        None => false,
    }
}

pub fn send_super_dynamic(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match METHOD_FRAMES.with(|f| f.borrow().last().copied()) {
        Some((defining, name)) => send_super_from(recv, defining, name, args, block),
        // A snippet's own level has no method of its own, and yet the
        // `eval` may sit inside one -- whose target it published for the
        // call (`zeo_rt::eval::EvalHome`).
        None => match crate::eval::home_super_target() {
            Some((defining, name)) => send_super_from(recv, defining, name, args, block),
            // Ruby's own class for it (`vm_insnhelper.c` raises through
            // `rb_vm_call_super` with no cref), and the same one the
            // emitter's static twin uses when it can see there is no method.
            None => Err(crate::dispatch::raise_error(
                "NoMethodError",
                "super called outside of method".to_string(),
            )),
        },
    }
}
