//! The seam between a run-time `eval` and whatever evaluates it.
//!
//! Two things can: the prism-walking interpreter in [`crate::eval_vm`],
//! and the REAL compiler, which the `zeo` library installs here through
//! [`install`] (it cannot be a plain dependency -- the runtime must not
//! depend on the compiler, so the compiler reaches down instead).
//!
//! Both exist during the burn-down. `ZEO_EVAL=compiler` routes what the
//! installed compiler accepts through it and lets the rest fall back to
//! the interpreter, so the slice can widen one shape at a time with the
//! interpreter as the differential oracle for every one. The interpreter
//! is retired when nothing falls back (plan G6-4).

pub use crate::builtins::binding::RBinding;
pub use crate::eval_vm::EvalMode;
use crate::{RubyValue, Signal};
use std::sync::OnceLock;

/// Everything an evaluator needs about one `eval` call. Mirrors the
/// interpreter's own `Env`, which is what proves the list is complete.
pub struct EvalRequest<'a> {
    /// The source text.
    pub src: &'a str,
    /// What `__FILE__`/`__LINE__` report -- `eval`'s own 3rd/4th arguments
    /// when given, `("(eval)", 1)` otherwise.
    pub file: &'a str,
    pub line: u32,
    /// The `self` every implicit-receiver call and `@ivar` binds to.
    pub self_val: RubyValue,
    /// Defining box for constant and global resolution.
    pub box_id: u32,
    pub mode: crate::eval_vm::EvalMode,
    /// The enclosing scope's frame label -- the snippet runs in the
    /// caller's name, not the entry cfunc's.
    pub label: &'static str,
    /// The Binding the source runs in, when it has one: its locals ARE the
    /// eval's locals (shared cells, so a write reaches the compiled
    /// frame), its `self` is the receiver, and its cref is what a constant
    /// resolves against.
    pub binding: Option<&'a RBinding>,
}

/// The class a snippet resolves its constants against, and the name a
/// NameError qualifies with -- CRuby's rule, and the interpreter's own
/// (`eval_vm::imp::eval_string`'s `(cref, cref_name)` pair and
/// `RBinding::cref`): a Binding names its capture's lexical class;
/// otherwise only the two `*_eval` string forms on a CLASS have one, and
/// `instance_eval`'s is the singleton, which owns no constants at all --
/// so the miss IS the answer, in the `#<Class:X>` spelling.
///
/// Both evaluators must read this from ONE place, or the same snippet
/// resolves `K` differently depending on which one ran it.
pub fn cref_of(req: &EvalRequest<'_>) -> (Option<zeo_abi::ClassId>, Option<String>) {
    if let Some(b) = req.binding {
        return (b.cref, b.cref.and_then(crate::dispatch::class_name));
    }
    match (req.mode, &req.self_val) {
        (EvalMode::ClassEval, RubyValue::Class(cid)) => {
            (Some(*cid), crate::dispatch::class_name(*cid))
        }
        (EvalMode::InstanceEval, RubyValue::Class(cid)) => (
            None,
            crate::dispatch::class_name(*cid).map(|n| format!("#<Class:{n}>")),
        ),
        _ => (None, None),
    }
}

/// The `SyntaxError` a snippet that does not parse raises. Both
/// evaluators must build it here, or the same unparsable source raises a
/// different exception depending on which one ran it.
#[must_use]
pub fn syntax_error(message: String) -> Signal {
    crate::dispatch::raise_error("SyntaxError", message)
}

/// The C signature a compiled snippet's entry function has: the status
/// protocol every compiled function speaks, over the caller's cell array
/// (one `*mut Cell` per local the snippet binds, in the order the compiler
/// asked for) and a BORROWED `self`.
pub type EvalFn = unsafe extern "C" fn(
    cells: *const *mut crate::capi::procs::Cell,
    self_val: *const RubyValue,
    out: *mut RubyValue,
) -> i32;

/// Run one compiled snippet under `req`'s own frame -- the same file,
/// label and line the interpreter pushes, so a backtrace cannot tell the
/// two evaluators apart.
///
/// # Safety
/// `f` must be a live entry compiled for exactly `req` and `cells` (same
/// source, same cell order); its module must outlive the call.
pub unsafe fn call(
    req: &EvalRequest<'_>,
    f: EvalFn,
    cells: &[*mut crate::capi::procs::Cell],
) -> Result<RubyValue, Signal> {
    let _frame = crate::frames::FrameGuard::push(
        crate::frames::intern_path(req.file),
        req.label,
        req.line,
        0,
    );
    let enter = || {
        let mut out = std::mem::MaybeUninit::<RubyValue>::uninit();
        let status = unsafe { f(cells.as_ptr(), &req.self_val, out.as_mut_ptr()) };
        if status == zeo_abi::abi::STATUS_OK {
            let v = unsafe { out.assume_init() };
            crate::capi::leakcheck::consumed(&v);
            Ok(v)
        } else {
            Err(crate::signal::take_pending()
                .expect("a compiled eval answered STATUS_SIGNAL with an empty pending slot"))
        }
    };
    // `instance_eval`'s default definee is the receiver's SINGLETON, which
    // is a run-time fact the emitted `def` asks the runtime for -- the
    // block form marks it the same way (`BasicObject#instance_eval`).
    match (req.mode, &req.self_val) {
        (EvalMode::InstanceEval, _) => {
            crate::runtime_meta::with_singleton_definee(&req.self_val, enter)
        }
        // A `class_eval` STRING is a class body, so a bare
        // `private`/`module_function` written in it is a cursor every `def`
        // after it reads -- the same body frame the BLOCK form opens.
        (EvalMode::ClassEval, RubyValue::Class(cid)) => {
            crate::runtime_meta::with_body_frame(*cid, enter)
        }
        _ => enter(),
    }
}

/// An evaluator the `zeo` library installs. `None` from [`EvalCompiler::
/// eval`] means "this shape is not compiled yet" -- the caller falls back
/// to the interpreter, which is what keeps the burn-down incremental.
pub trait EvalCompiler: Send + Sync {
    fn eval(&self, req: &EvalRequest<'_>) -> Option<Result<RubyValue, Signal>>;
}

static COMPILER: OnceLock<&'static dyn EvalCompiler> = OnceLock::new();

/// Called by `zeo_eval_install` -- from the `zeo` binary unconditionally,
/// and from an emitted program only when its `ProgramDesc` carries the
/// installer (a program that cannot `eval` does not link the compiler at
/// all, which is what lets `-dead_strip` drop it).
pub fn install(compiler: &'static dyn EvalCompiler) {
    let _ = COMPILER.set(compiler);
}

/// The installed compiler, when one is installed AND selected. Reading the
/// switch here rather than at the call sites keeps "which evaluator" one
/// question with one answer.
pub(crate) fn selected() -> Option<&'static dyn EvalCompiler> {
    if !matches!(std::env::var("ZEO_EVAL").as_deref(), Ok("compiler")) {
        return None;
    }
    COMPILER.get().copied()
}
