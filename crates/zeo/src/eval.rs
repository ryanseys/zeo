//! The real compiler behind a run-time `eval` (plan G6).
//!
//! `zeo_rt` cannot depend on the compiler, so it declares the seam
//! (`zeo_rt::eval::EvalCompiler`) and this module reaches down into it.
//! One snippet = one front-end run + one `JITModule`, kept for the
//! process's life exactly as CRuby keeps an eval's iseq.
//!
//! **What it accepts is deliberately narrow, and grows.** `eval` answers
//! `None` for every shape this compiler does not yet lower CORRECTLY, and
//! the interpreter answers those -- which keeps the interpreter as the
//! differential oracle for each shape until it takes over (G6-4). The
//! refusals are shapes that would COMPILE and be wrong, not shapes the
//! emitter rejects: a rejection is already an error the caller falls back
//! on. `ZEO_EVAL_DEBUG=1` prints why one fell back.

use crate::hir::HirNode;
use std::collections::HashMap;
use std::sync::Mutex;
use zeo_rt::capi::procs::Cell;
use zeo_rt::eval::{EvalCompiler, EvalFn, EvalRequest};
use zeo_rt::{RubyValue, Signal};

/// Install this compiler as the runtime's evaluator. Called by the `zeo`
/// binary at startup, and (G6-3) by an emitted program whose
/// `ProgramDesc` carries the installer.
pub fn install() {
    zeo_rt::eval::install(&Jit);
}

/// The installer an emitted program's `ProgramDesc.eval_install` names.
///
/// # Safety
/// Called once, from `zeo_rt_main`'s bootstrap, before any Ruby runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_eval_install() {
    install();
}

struct Jit;

impl EvalCompiler for Jit {
    fn eval(&self, req: &EvalRequest<'_>) -> Option<Result<RubyValue, Signal>> {
        match compiled_for(req) {
            Ok(c) => Some(run(req, c)),
            Err(why) => {
                if std::env::var_os("ZEO_EVAL_DEBUG").is_some() {
                    eprintln!("zeo: eval falls back to the interpreter: {why}");
                }
                None
            }
        }
    }
}

/// One compiled snippet: the entry's address, and the locals it wants a
/// cell for, in the order its entry loads them.
struct Compiled {
    entry: usize,
    cells: Vec<String>,
}

/// Keyed by everything the lowering depends on. The cell list is part of
/// it because the entry loads its locals BY INDEX: the same source under a
/// Binding with different names is a different function.
type Key = (String, u32, Vec<String>, u8, u32);

static CACHE: Mutex<Option<HashMap<Key, &'static Compiled>>> = Mutex::new(None);

fn compiled_for(req: &EvalRequest<'_>) -> Result<&'static Compiled, String> {
    // The names the caller's scope already carries: a snippet that only
    // READS one of them arrives as a vcall (prism parsed it alone), so the
    // cell has to exist whether or not the source assigns it.
    let scope_names = req.binding.map(|b| b.local_names()).unwrap_or_default();
    let key: Key = (
        req.src.to_string(),
        req.box_id,
        scope_names.clone(),
        mode_byte(req.mode),
        zeo_rt::eval::cref_of(req).0.map_or(u32::MAX, |c| c.0),
    );
    if let Some(&c) = CACHE
        .lock()
        .expect("the eval cache is never poisoned")
        .get_or_insert_with(HashMap::default)
        .get(&key)
    {
        return Ok(c);
    }
    let compiled: &'static Compiled = Box::leak(Box::new(build(req, &scope_names)?));
    CACHE
        .lock()
        .expect("the eval cache is never poisoned")
        .get_or_insert_with(HashMap::default)
        .insert(key, compiled);
    Ok(compiled)
}

/// `EvalMode` as the byte the emitter and `zeo_rt_eval_define` speak.
fn mode_byte(mode: zeo_rt::eval::EvalMode) -> u8 {
    match mode {
        zeo_rt::eval::EvalMode::Caller => 0,
        zeo_rt::eval::EvalMode::ClassEval => 1,
        zeo_rt::eval::EvalMode::InstanceEval => 2,
    }
}

fn build(req: &EvalRequest<'_>, scope_names: &[String]) -> Result<Compiled, String> {
    // The cref is a RUN-TIME class, so it travels as its id: the fresh
    // compiler below has no entry for it, and every static fold stands
    // down for that reason (`Fx::eval_cref`). `Object` needs none of it --
    // its table IS the top level, which a snippet already searches.
    let cref = match zeo_rt::eval::cref_of(req) {
        (Some(cid), _) if cid == zeo_abi::OBJECT_CLASS => None,
        (Some(cid), Some(name)) => Some(std::rc::Rc::new((Some(cid.0), name))),
        (Some(_), None) => return Err("the cref is a class with no name".to_string()),
        // No cref, but a NAME: `instance_eval` on a class resolves in its
        // SINGLETON, which owns no constants -- the miss is the answer,
        // and CRuby spells the miss `#<Class:X>::NAME`.
        (None, Some(name)) => Some(std::rc::Rc::new((None, name))),
        (None, None) => None,
    };
    // `__FILE__` and every frame the snippet raises from name the file the
    // CALLER gave (`(eval at f.rb:14)` when it gave none) and count from
    // the line it gave, so the snippet is lowered under both.
    let opts = crate::CompileOptions {
        file_name: Some(std::path::PathBuf::from(req.file)),
        line_offset: req.line.saturating_sub(1),
        mode: crate::CompileMode::Eval,
        ..crate::CompileOptions::default()
    };
    let analyzed = crate::analyze_snippet(req.src, &opts).map_err(|e| e.to_string())?;
    refusals(&analyzed, cref.as_ref().and_then(|c| c.0).is_none())?;

    // The snippet's own locals join the caller's: under a Binding every
    // one of them is a cell, so a name the source introduces lands in the
    // Binding (CRuby declares an eval's locals when it parses it) and a
    // name it assigns reaches the frame the Binding captured.
    let mut cells: Vec<String> = Vec::new();
    if req.binding.is_some() {
        cells.extend(scope_names.iter().cloned());
        let mut locals = crate::analyze::local_storage::Locals::default();
        for &stmt in &analyzed.main_statements {
            crate::analyze::local_storage::collect_locals(&analyzed.compiler, stmt, &mut locals);
        }
        for name in locals.names() {
            if !cells.contains(name) {
                cells.push(name.clone());
            }
        }
    }
    let spec = crate::clif::eval::EvalSpec {
        cells: &cells,
        box_id: req.box_id,
        label: req.label,
        cref,
        mode: mode_byte(req.mode),
    };
    let program = crate::clif::eval::compile(&analyzed, &spec)?;
    if let Some(init) = program.unit_init {
        let init: unsafe extern "C" fn() = unsafe { std::mem::transmute(init) };
        unsafe { init() };
    }
    let entry = program.entry as usize;
    // The module owns the code the entry lives in, and the entry outlives
    // this call by construction -- a Proc built inside the snippet can be
    // called at any later point.
    std::mem::forget(program);
    Ok(Compiled { entry, cells })
}

/// The shapes that would lower without complaint and be WRONG in a
/// snippet, because their lowering reads a decision only a whole-program
/// compile makes. Each is a widening this compiler owes (G6-1/G6-2).
fn refusals(analyzed: &crate::analyze::Analyzed, no_cref: bool) -> Result<(), String> {
    scope_refusals(analyzed, no_cref)?;
    home_refusals(analyzed)
}

/// The shapes that are wrong ANYWHERE in a snippet, `def` bodies
/// included: their lowering reads a decision only a whole-program compile
/// makes. Each is a widening this compiler owes (G6-1/G6-2).
fn scope_refusals(analyzed: &crate::analyze::Analyzed, no_cref: bool) -> Result<(), String> {
    let compiler = &analyzed.compiler;
    // `CompileMode::Eval` registers nothing, so nothing can be hoisted
    // past the walk below -- but a compile that DID register would emit
    // rows this snippet has no tables for, and the method would simply
    // vanish. Cheap to assert, and the assertion is the contract.
    debug_assert!(
        compiler.classes[0]
            .methods
            .iter()
            .all(|e| compiler.scope(e.def).native_default),
        "an eval snippet registered a method"
    );
    let hir = &compiler.hir;
    // `super`, `yield` and `return` all need a home the SNIPPET does not
    // have -- but a `def` written inside it does have one, and the
    // emitter's run-time-installed body already carries it (it reads the
    // defining class off the method-frame stack). So they are refused at
    // the snippet's own level and allowed inside a `def` there: this walk
    // stops at one.
    let mut refused = None;
    let mut stack: Vec<crate::hir::NodeId> = analyzed.main_statements.clone();
    while let Some(id) = stack.pop() {
        if refused.is_some() {
            break;
        }
        if matches!(hir[id], HirNode::DefMethod { .. }) {
            continue;
        }
        hir[id].for_each_child(&mut |c| stack.push(c));
        refused = match &hir[id] {
            // Registration: `CompileMode::Eval` leaves these unregistered,
            // and only a `def` has a run-time install path already (the
            // emitter's own arm for a `def` written where analyze could
            // not register one). The rest still belong to the interpreter.
            HirNode::ClassDef { .. } => Some("a `class`/`module`"),
            HirNode::MethodRedefine { .. } => Some("a redefinition"),
            HirNode::AliasMethod { .. } => Some("an `alias`"),
            HirNode::Undef(..) | HirNode::ClassMethodUndef { .. } => Some("an `undef`"),
            HirNode::MethodVisibility { .. }
            | HirNode::ClassMethodVisibility { .. }
            | HirNode::ModuleFunction { .. }
            | HirNode::ConstantVisibility { .. } => Some("a visibility statement"),
            HirNode::Include { .. }
            | HirNode::Extend { .. }
            | HirNode::Prepend { .. }
            | HirNode::ClassMethodPrepend { .. }
            | HirNode::DefHook { .. }
            | HirNode::Refine { .. }
            | HirNode::Using { .. } => Some("a definition-level statement"),
            // A class variable's owner is the cref, and a snippet's cref is
            // a run-time class only when it HAS one: a top-level eval's
            // `@@x` is ruby's own "class variable access from toplevel",
            // which this compiler does not raise yet.
            HirNode::ClassVarRead(..) | HirNode::ClassVarWrite { .. } if no_cref => {
                Some("a class variable with no cref")
            }
            // Compile-time-only surfaces.
            HirNode::Eval(..) | HirNode::Ffi(..) => Some("a nested compiler surface"),
            HirNode::BoxScope { .. } | HirNode::BoxHandle(..) => Some("a `Ruby::Box`"),
            HirNode::PreExec(..) => Some("a `BEGIN` block"),
            HirNode::FlipFlop { .. } => Some("a flip-flop"),
            _ => None,
        };
    }
    match refused {
        Some(what) => Err(format!("the source has {what}")),
        None => Ok(()),
    }
}

/// The shapes that need a HOME -- the enclosing method's identity, block
/// channel or return target. A snippet's own level has none; a `def`
/// written inside it does, so this walk stops at one.
fn home_refusals(analyzed: &crate::analyze::Analyzed) -> Result<(), String> {
    let hir = &analyzed.compiler.hir;
    let mut refused = None;
    let mut stack: Vec<crate::hir::NodeId> = analyzed.main_statements.clone();
    while let Some(id) = stack.pop() {
        if refused.is_some() {
            break;
        }
        if matches!(hir[id], HirNode::DefMethod { .. }) {
            continue;
        }
        hir[id].for_each_child(&mut |c| stack.push(c));
        refused = match &hir[id] {
            // `super` resumes from the class the body was written in;
            // `yield`/`return` belong to the method the eval sits inside,
            // whose block channel and return target the snippet's own
            // frame does not carry.
            HirNode::SuperCall { .. } => Some("a top-level `super`"),
            HirNode::Yield { .. } | HirNode::BlockGiven => Some("a top-level `yield`"),
            HirNode::Return(..) => Some("a top-level `return`"),
            _ => None,
        };
    }
    match refused {
        Some(what) => Err(format!("the source has {what}")),
        None => Ok(()),
    }
}

fn run(req: &EvalRequest<'_>, c: &Compiled) -> Result<RubyValue, Signal> {
    // One reference per cell, handed back below: the Binding owns them.
    let cells: Vec<*mut Cell> = match req.binding {
        Some(b) => c.cells.iter().map(|n| b.local_cell_ptr(n)).collect(),
        None => Vec::new(),
    };
    let f: EvalFn = unsafe { std::mem::transmute(c.entry) };
    let answer = unsafe { zeo_rt::eval::call(req, f, &cells) };
    for cell in cells {
        unsafe { zeo_rt::capi::procs::zeo_rt_cell_release(cell) };
    }
    answer
}
