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
type Key = (String, u32, Vec<String>);

static CACHE: Mutex<Option<HashMap<Key, &'static Compiled>>> = Mutex::new(None);

fn compiled_for(req: &EvalRequest<'_>) -> Result<&'static Compiled, String> {
    // The names the caller's scope already carries: a snippet that only
    // READS one of them arrives as a vcall (prism parsed it alone), so the
    // cell has to exist whether or not the source assigns it.
    let scope_names = req.binding.map(|b| b.local_names()).unwrap_or_default();
    let key: Key = (req.src.to_string(), req.box_id, scope_names.clone());
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

fn build(req: &EvalRequest<'_>, scope_names: &[String]) -> Result<Compiled, String> {
    if req.mode != zeo_rt::eval::EvalMode::Caller {
        return Err("only a plain `Kernel#eval` compiles yet".to_string());
    }
    // A Binding captured inside a class or module carries that cref, and
    // every constant the source reads resolves against it -- knowledge the
    // fresh compiler below does not have, because the class was minted by
    // a compile that is already over. `Object` is the exception, and the
    // common case: its chain IS the top level, which is what a snippet
    // compiled with no cref at all already searches.
    if req
        .binding
        .is_some_and(|b| b.cref.is_some_and(|c| c != zeo_abi::OBJECT_CLASS))
    {
        return Err("the caller's cref is a run-time class".to_string());
    }
    // CRuby offsets a snippet's own line numbers by the `line` argument,
    // which reaches `__LINE__`, every backtrace row and every
    // `source_location` -- the front end has no such offset, so a snippet
    // that carries one still belongs to the interpreter.
    if req.line != 1 {
        return Err("the eval names a starting line".to_string());
    }
    // `__FILE__` and every frame the snippet raises from name the file the
    // CALLER gave (`(eval at f.rb:14)` when it gave none), so the snippet
    // is lowered under that name rather than the program's.
    let opts = crate::CompileOptions {
        file_name: Some(std::path::PathBuf::from(req.file)),
        ..crate::CompileOptions::default()
    };
    let analyzed = crate::analyze_snippet(req.src, &opts).map_err(|e| e.to_string())?;
    refusals(&analyzed)?;

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
fn refusals(analyzed: &crate::analyze::Analyzed) -> Result<(), String> {
    let compiler = &analyzed.compiler;
    // A top-level `def`/`class` is HOISTED out of the statements into the
    // class table, so the walk below never sees it -- and this compile
    // emits no registration tables at all, which is what made an eval'd
    // `def` vanish instead of installing. Both belong on the runtime's
    // overlay (G6-1's own next step).
    if compiler.classes[0]
        .methods
        .iter()
        .any(|e| !compiler.scope(e.def).native_default)
    {
        return Err("the source has a `def`".to_string());
    }
    if compiler
        .classes
        .iter()
        .enumerate()
        .any(|(i, c)| i != 0 && !c.is_builtin && !c.is_bootstrap)
    {
        return Err("the source has a `class`/`module`".to_string());
    }
    let hir = &compiler.hir;
    let mut refused = None;
    let mut stack: Vec<crate::hir::NodeId> = analyzed.main_statements.clone();
    while let Some(id) = stack.pop() {
        if refused.is_some() {
            break;
        }
        hir[id].for_each_child(&mut |c| stack.push(c));
        refused = match &hir[id] {
            // Registration: analyze put these in a class table nothing
            // registers, so the method would simply vanish. They belong on
            // the runtime's overlay instead.
            HirNode::DefMethod { .. } => Some("a `def`"),
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
            // A home the snippet does not have: `super` resumes from the
            // class the body was written in, `yield`/`return` belong to
            // the enclosing method.
            HirNode::SuperCall { .. } => Some("a `super`"),
            HirNode::Yield { .. } | HirNode::BlockGiven => Some("a `yield`"),
            HirNode::Return(..) => Some("a `return`"),
            // An owner the fresh compiler resolves statically.
            HirNode::ClassVarRead(..) | HirNode::ClassVarWrite { .. } => Some("a class variable"),
            HirNode::ConstWrite { .. } | HirNode::DynConstWrite { .. } => {
                Some("a constant assignment")
            }
            // `defined?` classifies its operand at compile time, and a
            // bare name in a snippet may be the caller's local.
            HirNode::Defined(..) => Some("a `defined?`"),
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
