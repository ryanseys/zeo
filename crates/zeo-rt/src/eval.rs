//! Runtime string `eval`, and the seam it reaches the compiler through.
//!
//! A snippet is COMPILED, by the same front end and the same Cranelift
//! emitter a whole program is (plan G6). The compiler cannot be a plain
//! dependency -- the runtime must not depend on it -- so the runtime
//! declares the seam here and the `zeo` library reaches down and fills it
//! through [`install`]. An AOT program carries the installer only when it
//! can eval at all, which is what lets the linker drop the compiler from
//! every program that cannot.
//!
//! There is no second implementation of Ruby behind this any more. A
//! prism-walking interpreter answered here until 2026-08-21, as the
//! differential oracle each widening of the compiled path was measured
//! against; what it declined, it declined LOUDLY, and so does this.

pub use crate::builtins::binding::RBinding;
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
    pub mode: EvalMode,
    /// The enclosing scope's frame label -- the snippet runs in the
    /// caller's name, not the entry cfunc's.
    pub label: &'static str,
    /// The Binding the source runs in, when it has one: its locals ARE the
    /// eval's locals (shared cells, so a write reaches the compiled
    /// frame), its `self` is the receiver, and its cref is what a constant
    /// resolves against.
    pub binding: Option<&'a RBinding>,
    /// The LEXICAL class chain a bare constant searches, innermost first
    /// and excluding the top level -- CRuby's cref list. Empty for an
    /// ordinary eval, whose one cref [`cref_of`] reads off the binding or
    /// the mode; a `class` body opened inside a snippet runs as an eval of
    /// its own and prepends its class to the chain it inherited, which is
    /// what lets `class Inside` written in `Wrap.class_eval` still see
    /// `Wrap::IN_WRAP`.
    pub cref_chain: &'a [zeo_abi::ClassId],
}

/// The classes a snippet resolves its constants against, and the name a
/// NameError qualifies with -- CRuby's rule: a Binding names its
/// capture's lexical class; otherwise only the two `*_eval` string forms
/// on a CLASS have one, and `instance_eval`'s is the singleton, which owns
/// no constants at all -- so the miss IS the answer, in the `#<Class:X>`
/// spelling. A `class` body opened inside a snippet prepends its own.
///
/// The compiler and the runtime must read this from ONE place, or a
/// resolves `K` differently depending on which one ran it.
pub fn cref_of(req: &EvalRequest<'_>) -> (Vec<zeo_abi::ClassId>, Option<String>) {
    if let Some(&head) = req.cref_chain.first() {
        return (req.cref_chain.to_vec(), crate::dispatch::class_name(head));
    }
    if let Some(b) = req.binding {
        return (
            b.cref.into_iter().collect(),
            b.cref.and_then(crate::dispatch::class_name),
        );
    }
    match (req.mode, &req.self_val) {
        (EvalMode::ClassEval, RubyValue::Class(cid)) => {
            (vec![*cid], crate::dispatch::class_name(*cid))
        }
        (EvalMode::InstanceEval, RubyValue::Class(cid)) => (
            Vec::new(),
            crate::dispatch::class_name(*cid).map(|n| format!("#<Class:{n}>")),
        ),
        _ => (Vec::new(), None),
    }
}

/// The block channel and `super` target a snippet reaches through.
///
/// `yield`, `block_given?` and a bare `super` written at a snippet's own
/// level belong to the method the `eval` was called from -- CRuby reads
/// them off the caller's control frame, which zeo has no equivalent of.
/// So a compiled scope that LEXICALLY contains a run-time `eval`
/// publishes its own home for the length of the call, and the snippet
/// reads the top of the stack. A block publishes the home it inherited
/// (`ProcEnv::lexical_blk`), which is what keeps an `eval` written inside
/// one aimed at the enclosing method rather than at whatever Ruby method
/// happens to be driving the block.
pub struct EvalHome {
    block: Option<RubyValue>,
    /// The enclosing method's own arguments and `super` target. `None` for
    /// a block's home -- a block carries neither, and a snippet that needs
    /// one says so rather than forwarding nothing.
    zsuper: Option<(Vec<RubyValue>, zeo_abi::ClassId, crate::Symbol)>,
}

thread_local! {
    static EVAL_HOMES: std::cell::RefCell<Vec<EvalHome>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// See [`crate::ec`] -- the stack is per-fiber, like every other
/// frame-shaped piece of ambient state.
pub(crate) fn swap_eval_homes(next: Vec<EvalHome>) -> Vec<EvalHome> {
    EVAL_HOMES.with(|h| std::mem::replace(&mut *h.borrow_mut(), next))
}

/// Publish this scope's block channel (and, for a method, what a bare
/// `super` written in a snippet would forward) for the length of the call.
pub fn home_push(
    block: Option<RubyValue>,
    zsuper: Option<(Vec<RubyValue>, zeo_abi::ClassId, crate::Symbol)>,
) {
    EVAL_HOMES.with(|h| h.borrow_mut().push(EvalHome { block, zsuper }));
}

pub fn home_pop() {
    EVAL_HOMES.with(|h| {
        h.borrow_mut().pop();
    });
}

/// The block a snippet's `yield`/`block_given?` means, or `None` where no
/// enclosing scope has one.
#[must_use]
pub fn home_block() -> Option<RubyValue> {
    EVAL_HOMES.with(|h| h.borrow().last().and_then(|e| e.block.clone()))
}

/// Whether any enclosing scope published a home at all. `false` means the
/// `eval` was called from a scope that can never have a block -- the top
/// level, or a class body -- which is what makes a `yield` written in the
/// snippet CRuby's compile-time `Invalid yield` rather than a run-time
/// `LocalJumpError`.
#[must_use]
pub fn has_home() -> bool {
    EVAL_HOMES.with(|h| !h.borrow().is_empty())
}

/// `yield` written at a snippet's own level.
pub fn home_yield(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match home_block() {
        Some(RubyValue::Proc(p)) => p.call(args),
        _ => Err(crate::builtins::local_jump_error!("no block given (yield)")),
    }
}

/// The `(defining class, name)` a `super` written at a snippet's own
/// level resumes from -- the enclosing method's, published for the call.
#[must_use]
pub fn home_super_target() -> Option<(zeo_abi::ClassId, crate::Symbol)> {
    EVAL_HOMES.with(|h| {
        h.borrow()
            .last()
            .and_then(|e| e.zsuper.as_ref().map(|(_, c, n)| (*c, *n)))
    })
}

/// `defined?(super)` written at a snippet's own level: the same walk,
/// without running it. Nil where no enclosing method published a target,
/// which is what CRuby answers for an `eval` at the top level.
#[must_use]
pub fn home_super_defined(recv: &RubyValue) -> bool {
    EVAL_HOMES
        .with(|h| h.borrow().last().and_then(|e| e.zsuper.clone()))
        .is_some_and(|(_, defining, name)| crate::dispatch::super_defined(recv, defining, name))
}

/// A bare `super` written at a snippet's own level: the enclosing
/// method's own arguments, resumed after the class that defined it.
pub fn home_super(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let home = EVAL_HOMES.with(|h| {
        h.borrow()
            .last()
            .and_then(|e| e.zsuper.clone().map(|z| (z, e.block.clone())))
    });
    match home {
        Some(((args, defining, name), block)) => {
            crate::dispatch::send_super_from(recv, defining, name, &args, block)
        }
        None => Err(crate::builtins::not_impl_error!(
            "a bare `super` inside an `eval` forwards the enclosing method's arguments, which \
             zeo hands a snippet only where the `eval` is written in the method itself"
        )),
    }
}

/// Reserve `n` flip-flop latch ids for one compiled snippet -- see
/// `flipflop::reserve`. A snippet is compiled by a fresh compiler whose
/// ids start at zero, which are the running program's own.
#[must_use]
pub fn reserve_flip_flops(n: u32) -> u32 {
    crate::flipflop::reserve(n)
}

/// One `using` site's activation slot: the refinements the module named
/// there holds, resolved when the `using` RUNS.
///
/// A snippet's `using` cannot be resolved at compile time -- the module is
/// a run-time constant, and what it refines lives in the running program's
/// registry, which the snippet's own fresh compiler has no entry for. So
/// the site reserves a slot (ids are global, exactly like a flip-flop
/// latch's), fills it when it runs, and every call site the `using` covers
/// reads it. A slot outlives the call, which is what lets a `def` written
/// after the `using` in the same snippet still see the refinement.
static USING_SLOTS: std::sync::LazyLock<
    std::sync::Mutex<Vec<Vec<(zeo_abi::ClassId, zeo_abi::ClassId, bool)>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

/// Reserve `n` consecutive activation slots for one compiled snippet.
#[must_use]
pub fn reserve_using_slots(n: u32) -> u32 {
    let mut slots = USING_SLOTS
        .lock()
        .expect("the using table is never poisoned");
    let base = slots.len();
    slots.resize(base + n as usize, Vec::new());
    base as u32
}

/// `using M` running in a snippet: record what `M` refines.
pub fn using_activate(slot: u32, module: &RubyValue) -> Result<(), Signal> {
    let RubyValue::Class(mid) = module else {
        return Err(crate::builtins::type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::class_name_of(module)
        ));
    };
    if crate::dispatch::class_is_module(*mid) != Some(true) {
        return Err(crate::builtins::type_error!(
            "wrong argument type Class (expected Module)"
        ));
    }
    // `(target, holder, singleton)`, the shape `refinement_home` matches.
    // The singleton form (`refine C.singleton_class`) is not distinguished:
    // the registry records a refinement's target class, not whether the
    // `refine` named its singleton, so a snippet activating one refines the
    // instance side. Recorded as a divergence rather than guessed.
    let candidates: Vec<(zeo_abi::ClassId, zeo_abi::ClassId, bool)> =
        crate::dispatch::refinements_of(*mid)
            .into_iter()
            .filter_map(|holder| {
                crate::dispatch::refinement_of(holder).map(|(_, target)| (target, holder, false))
            })
            .collect();
    let mut slots = USING_SLOTS
        .lock()
        .expect("the using table is never poisoned");
    if let Some(entry) = slots.get_mut(slot as usize) {
        *entry = candidates;
    }
    Ok(())
}

/// The refinements active at one call site: every slot the `using`s
/// covering it filled, innermost last (a later `using` wins).
#[must_use]
pub fn using_candidates(slots: &[u32]) -> Vec<(zeo_abi::ClassId, zeo_abi::ClassId, bool)> {
    let table = USING_SLOTS
        .lock()
        .expect("the using table is never poisoned");
    let mut out = Vec::new();
    for &slot in slots.iter().rev() {
        if let Some(entry) = table.get(slot as usize) {
            out.extend(entry.iter().copied());
        }
    }
    out
}

/// The `SyntaxError` a snippet that does not parse raises. Both
/// evaluators must build it here, or the same unparsable source raises a
/// different exception depending on which one ran it.
#[must_use]
pub fn syntax_error(message: String) -> Signal {
    crate::dispatch::raise_error("SyntaxError", message)
}

/// A `SyntaxError` CRuby raises while COMPILING the snippet rather than
/// while parsing it (`Invalid yield`): its backtrace is the eval's own
/// location alone, because no frame of the snippet ever ran.
#[must_use]
pub fn syntax_error_at(message: String, row: String) -> Signal {
    let signal = crate::dispatch::raise_error("SyntaxError", message);
    if let Signal::Raise(exc) = &signal {
        crate::builtins::exception::set_backtrace_lines(exc, vec![row]);
    }
    signal
}

/// A shape zeo declines to compile. Raised rather than answered
/// approximately: the compile contract is CRuby-identical or nothing, and
/// a snippet's compile is no different from a program's.
#[must_use]
pub fn not_compiled(what: String) -> Signal {
    crate::dispatch::raise_error(
        "NotImplementedError",
        format!("zeo cannot compile this `eval`: {what}"),
    )
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

/// Open a `class`/`module` written inside a run-time `eval`: reuse the
/// constant if it already names one, otherwise mint it, name it and bind
/// it -- the interpreter's `eval_class_or_module` half, which both
/// evaluators must reach through one function or the same source mints two
/// different classes depending on who ran it.
///
/// `owner` is the class the bare name belongs to (the snippet's cref, or
/// the top level), or the scope `A::B` named explicitly.
pub fn class_open(
    owner: &RubyValue,
    name: &str,
    superclass: Option<&RubyValue>,
    is_module: bool,
) -> Result<RubyValue, Signal> {
    let owner_id = match owner {
        RubyValue::Class(cid) => cid.0,
        other => {
            return Err(crate::builtins::type_error!(
                "{} is not a class/module",
                other.inspect_string()
            ));
        }
    };
    let kind = if is_module { "module" } else { "class" };
    let existing = match crate::constants::const_get(owner_id, name) {
        Some(RubyValue::Class(cid)) => Some(cid),
        // The constant is taken by something that is not a class at all --
        // ruby's own `TypeError`, raised before anything is minted.
        Some(_) => return Err(crate::builtins::type_error!("{name} is not a {kind}")),
        None if owner_id == zeo_abi::OBJECT_CLASS.0 => crate::dispatch::class_id_by_name(name),
        None => None,
    };
    if let Some(cid) = existing {
        return Ok(RubyValue::Class(cid));
    }
    let val = if is_module {
        crate::runtime_meta::runtime_module_new(None)?
    } else {
        crate::runtime_meta::runtime_class_new(superclass.cloned(), None)?
    };
    let RubyValue::Class(cid) = val else {
        unreachable!("runtime_class_new answers a Class")
    };
    let qualified = if owner_id == zeo_abi::OBJECT_CLASS.0 {
        name.to_string()
    } else {
        format!(
            "{}::{name}",
            crate::dispatch::class_name(zeo_abi::ClassId(owner_id)).unwrap_or_default()
        )
    };
    crate::runtime_meta::name_runtime_class_if_anonymous(cid, &qualified);
    crate::constants::const_set(owner_id, name, val.clone());
    Ok(val)
}

/// Run a `class`/`module` BODY written inside a run-time `eval`.
///
/// A class body is a scope of its own in CRuby -- its own cref, its own
/// locals, its own iseq -- and that is exactly what one more `eval` of it
/// is here: `class_eval` on the class the header just opened. Nothing in
/// the surrounding snippet is shared, because ruby shares nothing either,
/// and every question the body raises (where a `def` lands, what a bare
/// constant resolves against, what `@@x` owns) already has an answer on
/// this path.
pub fn class_body(
    class_val: &RubyValue,
    src: &str,
    file: &str,
    line: u32,
    label: &'static str,
    box_id: u32,
    outer_cref: &[zeo_abi::ClassId],
) -> Result<RubyValue, Signal> {
    if src.is_empty() {
        return Ok(RubyValue::Nil);
    }
    let RubyValue::Class(cid) = class_val else {
        unreachable!("the header answered a Class")
    };
    let mut chain = Vec::with_capacity(outer_cref.len() + 1);
    chain.push(*cid);
    chain.extend_from_slice(outer_cref);
    let req = EvalRequest {
        src,
        file,
        line,
        self_val: class_val.clone(),
        box_id,
        mode: EvalMode::ClassEval,
        label,
        binding: None,
        cref_chain: &chain,
    };
    compiler()?.eval(&req)
}

/// The evaluator the `zeo` library installs. A shape it declines raises --
/// there is nothing else to hand the snippet to.
pub trait EvalCompiler: Send + Sync {
    fn eval(&self, req: &EvalRequest<'_>) -> Result<RubyValue, Signal>;
}

static COMPILER: OnceLock<&'static dyn EvalCompiler> = OnceLock::new();

/// Called by `zeo_eval_install` -- from the `zeo` binary unconditionally,
/// and from an emitted program only when its `ProgramDesc` carries the
/// installer (a program that cannot `eval` does not link the compiler at
/// all, which is what lets `-dead_strip` drop it).
pub fn install(compiler: &'static dyn EvalCompiler) {
    let _ = COMPILER.set(compiler);
}

/// The installed compiler. Absent means this binary was linked without one
/// -- which is only reachable when the compiler's own
/// `Hir::uses_runtime_eval` missed the site, since every program that can
/// eval carries the installer.
fn compiler() -> Result<&'static dyn EvalCompiler, Signal> {
    COMPILER.get().copied().ok_or_else(|| {
        crate::builtins::not_impl_error!(
            "this program was compiled without the eval compiler, so it cannot evaluate a string \
             at run time (zeo links it only into a program it can see reach `eval`)"
        )
    })
}

// ---------------------------------------------------------------------------
// The runtime's `eval` entries -- what every dispatch site funnels through
// ---------------------------------------------------------------------------

/// Which surface invoked the eval -- it decides where a `def` inside the
/// source installs (the "default definee", CRuby's `cref`):
/// - `Caller` (`Kernel#eval`): an instance method on `self`'s class (a plain
///   top-level eval's `self` is the main object, so `def` lands on `Object`).
/// - `ClassEval` (`Module#class_eval`): an instance method on `self` (a Class).
/// - `InstanceEval` (`BasicObject#instance_eval`): a singleton method on
///   `self` (a class method when `self` is itself a Class).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EvalMode {
    Caller,
    ClassEval,
    InstanceEval,
}

/// The file name a snippet reports, which is CRuby's `(eval at f.rb:14)`
/// unless the caller named one: two evals in one program tell their
/// backtraces apart by where they were EVALUATED.
fn eval_path(file: Option<&str>) -> String {
    match file {
        Some(f) => f.to_string(),
        None => match crate::frames::current_location() {
            Some((f, l)) => format!("(eval at {f}:{l})"),
            None => EVAL_FILE.to_string(),
        },
    }
}

/// What a snippet with no caller location at all reports.
pub(crate) const EVAL_FILE: &str = "(eval)";

/// The cfunc frame that shows between the caller and the snippet -- how a
/// backtrace says where an eval was ENTERED as well as where it raised.
fn entry_label(mode: EvalMode) -> &'static str {
    match mode {
        EvalMode::Caller => "Kernel#eval",
        EvalMode::InstanceEval => "BasicObject#instance_eval",
        EvalMode::ClassEval => "Module#class_eval",
    }
}

/// Evaluate `src` as a standalone chunk of Ruby with `self` bound to
/// `self_val`, resolving constants/globals against `box_id`. A parse failure
/// becomes a catchable `SyntaxError`; the value of the last statement is
/// returned (`nil` for an empty program). `mode` decides where a `def`
/// inside the source installs -- see [`EvalMode`].
pub fn eval_string_mode(
    src: &str,
    self_val: RubyValue,
    box_id: u32,
    mode: EvalMode,
) -> Result<RubyValue, Signal> {
    eval_string_located(src, self_val, box_id, mode, None, None)
}

/// [`eval_string_mode`] with `eval`'s own filename/lineno arguments --
/// what `__FILE__`/`__LINE__` and every backtrace row inside the snippet
/// report. Absent, they are `("(eval at f.rb:14)", 1)`.
pub fn eval_string_located(
    src: &str,
    self_val: RubyValue,
    box_id: u32,
    mode: EvalMode,
    file: Option<&str>,
    line: Option<u32>,
) -> Result<RubyValue, Signal> {
    // The enclosing scope's label, read BEFORE the cfunc frame goes on --
    // the snippet runs in the caller's name, not the cfunc's.
    let label = crate::frames::current_frame_label().unwrap_or("<main>");
    let _c = crate::frames::synthetic_c_frame(entry_label(mode));
    let path = eval_path(file);
    let req = EvalRequest {
        src,
        file: &path,
        line: line.unwrap_or(1),
        self_val,
        box_id,
        mode,
        label,
        binding: None,
        cref_chain: &[],
    };
    compiler()?.eval(&req)
}

/// [`eval_string_mode`] in the default `Kernel#eval` mode -- the entry the
/// literal-splice fallback uses.
pub fn eval_string(src: &str, self_val: RubyValue, box_id: u32) -> Result<RubyValue, Signal> {
    eval_string_mode(src, self_val, box_id, EvalMode::Caller)
}

/// The dynamic-`eval` entry every dispatch site funnels through: coerce the
/// source argument to a String (Ruby raises `TypeError` for anything else,
/// even a Symbol) and evaluate it with `self` bound to `self_val`. Keeping the
/// coercion here means every caller -- `Kernel#eval`, `instance_eval`,
/// `class_eval` -- shares one definition of "what counts as evalable source".
pub fn eval_value_mode(
    src: RubyValue,
    self_val: RubyValue,
    box_id: u32,
    mode: EvalMode,
) -> Result<RubyValue, Signal> {
    eval_value_located(src, self_val, box_id, mode, None, None)
}

/// [`eval_value_mode`] carrying `eval`'s filename/lineno arguments.
pub fn eval_value_located(
    src: RubyValue,
    self_val: RubyValue,
    box_id: u32,
    mode: EvalMode,
    file: Option<&str>,
    line: Option<u32>,
) -> Result<RubyValue, Signal> {
    let code = crate::builtins::convert::to_rstr(&src)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    eval_string_located(&code, self_val, box_id, mode, file, line)
}

/// [`eval_value_mode`] in the default `Kernel#eval` mode.
pub fn eval_value(src: RubyValue, self_val: RubyValue, box_id: u32) -> Result<RubyValue, Signal> {
    eval_value_mode(src, self_val, box_id, EvalMode::Caller)
}

/// The compiled `eval(*args)` site: a splat means the argument list is a
/// run-time value, so the arity `Kernel#eval` declares is checked here
/// rather than by the shape of the call. Everything else is
/// [`eval_value_in_scope`], including the caller's own frame as `scope`.
pub fn eval_value_in_scope_argv(args: &[RubyValue], scope: RubyValue) -> Result<RubyValue, Signal> {
    if args.is_empty() || args.len() > 4 {
        return Err(crate::dispatch::wrong_arity(args.len(), "1..4"));
    }
    let at = |i: usize| args.get(i).cloned().unwrap_or(RubyValue::Nil);
    eval_value_in_scope(at(0), scope, at(1), at(2), at(3))
}

/// The compiled receiver-less `eval(src[, binding[, file[, line]]])` site.
/// CRuby runs a bare `eval` -- and one given a `nil` binding -- in the
/// CALLER's own frame, so the emitter hands that frame over as `scope`,
/// materialized right at the call site; that is what lets the source read
/// and write the caller's locals. An explicit non-nil binding wins over it,
/// and must be a `Binding`.
pub fn eval_value_in_scope(
    src: RubyValue,
    scope: RubyValue,
    binding: RubyValue,
    file: RubyValue,
    line: RubyValue,
) -> Result<RubyValue, Signal> {
    let chosen = if binding.is_nil() { &scope } else { &binding };
    let Some(b) = crate::builtins::binding::as_binding(chosen) else {
        return Err(crate::builtins::type_error!(
            "wrong argument type {} (expected binding)",
            crate::dispatch::class_name(binding.class_id()).unwrap_or_else(|| "Object".to_string())
        ));
    };
    // Without an explicit binding the source runs in a CHILD of the caller's
    // frame: it reads and writes the caller's locals, but a name it introduces
    // is its own and dies with the call. `Binding#eval` keeps them, because
    // there the Binding IS the scope.
    let implicit;
    let b = if binding.is_nil() {
        implicit = RBinding {
            self_val: b.self_val.clone(),
            scope: b.scope.child(),
            file: b.file.clone(),
            line: b.line,
            label: b.label,
            box_id: b.box_id,
            cref: b.cref,
            frozen: std::sync::atomic::AtomicBool::new(false),
        };
        &implicit
    } else {
        b
    };
    let file = match &file {
        RubyValue::Nil => None,
        v => Some(
            crate::builtins::convert::to_rstr(v)?
                .lock()
                .to_utf8_lossy()
                .into_owned(),
        ),
    };
    let line = match &line {
        RubyValue::Nil => None,
        v => Some(crate::builtins::convert::to_index(v)? as u32),
    };
    eval_with_binding(&src, b, file, line, "Kernel#eval")
}

/// `Binding#eval` and `Kernel#eval(src, binding, ...)` -- the source runs in
/// the captured scope: `b`'s locals ARE the eval's locals (shared cells, so a
/// write reaches the compiled frame), `b`'s `self` is the receiver, and `b`'s
/// cref is what a constant resolves against. `file`/`line` override what
/// `__FILE__`/`__LINE__` report, as CRuby's own 3rd/4th `eval` arguments do.
pub fn eval_with_binding(
    src: &RubyValue,
    b: &RBinding,
    file: Option<String>,
    line: Option<u32>,
    caller_label: &'static str,
) -> Result<RubyValue, Signal> {
    let code = crate::builtins::convert::to_rstr(src)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    let _c = crate::frames::synthetic_c_frame(caller_label);
    let path = eval_path(file.as_deref());
    let req = EvalRequest {
        src: &code,
        file: &path,
        // The snippet's own first line, which is 1 unless the caller named
        // one -- NOT the Binding's capture line, which is where `binding`
        // was written.
        line: line.unwrap_or(1),
        self_val: b.self_val.clone(),
        box_id: b.box_id,
        mode: EvalMode::Caller,
        label: b.label,
        binding: Some(b),
        cref_chain: &[],
    };
    compiler()?.eval(&req)
}
