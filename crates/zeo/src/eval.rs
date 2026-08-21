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
    fn eval(&self, req: &EvalRequest<'_>) -> Result<RubyValue, Signal> {
        match compiled_for(req) {
            Ok(c) => run(req, c),
            // Source prism refuses is the PROGRAM's error, not a compiler
            // limit: `eval("1 +")` raises a catchable `SyntaxError`.
            Err(Refusal::Syntax(msg)) => Err(zeo_rt::eval::syntax_error(msg)),
            Err(Refusal::SyntaxAt(msg, row)) => Err(zeo_rt::eval::syntax_error_at(msg, row)),
            // A shape zeo declines. It raises rather than answering
            // approximately -- CRuby-identical or nothing, which is the
            // same contract a program's compile has.
            Err(Refusal::NotCompiled(why)) => Err(zeo_rt::eval::not_compiled(why)),
        }
    }
}

/// Why a snippet did not run through this compiler.
enum Refusal {
    /// The source does not parse -- the snippet's own `SyntaxError`.
    Syntax(String),
    /// The source parses but does not COMPILE where it was called from
    /// (`Invalid yield`): a `SyntaxError` whose backtrace is the eval's
    /// own location, because no frame of the snippet ever ran.
    SyntaxAt(String, String),
    /// A shape zeo declines: a `NotImplementedError` naming it.
    NotCompiled(String),
}

impl From<String> for Refusal {
    fn from(why: String) -> Refusal {
        Refusal::NotCompiled(why)
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
type Key = (String, u32, Vec<String>, u8, Vec<u32>, bool);

static CACHE: Mutex<Option<HashMap<Key, &'static Compiled>>> = Mutex::new(None);

fn compiled_for(req: &EvalRequest<'_>) -> Result<&'static Compiled, Refusal> {
    // The names the caller's scope already carries: a snippet that only
    // READS one of them arrives as a vcall (prism parsed it alone), so the
    // cell has to exist whether or not the source assigns it.
    let scope_names = req.binding.map(|b| b.local_names()).unwrap_or_default();
    let key: Key = (
        req.src.to_string(),
        req.box_id,
        scope_names.clone(),
        mode_byte(req.mode),
        zeo_rt::eval::cref_of(req).0.iter().map(|c| c.0).collect(),
        // Whether a `yield` written here is legal at all -- a compile-time
        // question in CRuby, and a property of the CALLER, so the same
        // source compiled from a method and from the top level are two
        // different compiles.
        zeo_rt::eval::has_home(),
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

fn build(req: &EvalRequest<'_>, scope_names: &[String]) -> Result<Compiled, Refusal> {
    // The cref is a RUN-TIME class, so it travels as its id: the fresh
    // compiler below has no entry for it, and every static fold stands
    // down for that reason (`Fx::eval_cref`). `Object` needs none of it --
    // its table IS the top level, which a snippet already searches.
    let (chain, name) = zeo_rt::eval::cref_of(req);
    // `Object`'s table IS the top level, which every search ends at anyway.
    let chain: Vec<u32> = chain
        .iter()
        .map(|c| c.0)
        .filter(|&c| c != zeo_abi::OBJECT_CLASS.0)
        .collect();
    let cref = match (chain.is_empty(), name) {
        (true, None) => None,
        // No chain, but a NAME: `instance_eval` on a class resolves in its
        // SINGLETON, which owns no constants -- the miss is the answer,
        // and CRuby spells the miss `#<Class:X>::NAME`.
        (_, Some(name)) => Some(std::rc::Rc::new(crate::clif::ctx::EvalCref { chain, name })),
        (false, None) => return Err("the cref is a class with no name".to_string().into()),
    };
    // `__FILE__` and every frame the snippet raises from name the file the
    // CALLER gave (`(eval at f.rb:14)` when it gave none) and count from
    // the line it gave, so the snippet is lowered under both.
    let opts = crate::CompileOptions {
        file_name: Some(std::path::PathBuf::from(req.file)),
        line_offset: req.line.saturating_sub(1),
        mode: crate::CompileMode::Eval {
            cref: cref.is_some(),
        },
        ..crate::CompileOptions::default()
    };
    let analyzed =
        crate::analyze_snippet(req.src, &opts).map_err(|e| match e.syntax_message() {
            Some(msg) => Refusal::Syntax(msg.to_string()),
            None => Refusal::NotCompiled(e.to_string()),
        })?;
    refusals(&analyzed)?;
    if !zeo_rt::eval::has_home() {
        invalid_yield(&analyzed)?;
    }

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
        flip_flop_base: match analyzed.compiler.hir.flip_flops {
            0 => 0,
            n => zeo_rt::eval::reserve_flip_flops(n),
        },
        using_base: match analyzed.compiler.eval_activations.len() {
            0 => 0,
            n => zeo_rt::eval::reserve_using_slots(n as u32),
        },
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
    scope_refusals(analyzed)
}

/// A `yield` at the snippet's own level, where the `eval` was called from
/// a scope that can never have a block: CRuby compiles the snippet before
/// it runs a statement of it and refuses with `Invalid yield`, which is a
/// catchable `SyntaxError` naming the yield's own place. The walk stops at
/// a `def` -- one written in the snippet has a block channel of its own.
fn invalid_yield(analyzed: &crate::analyze::Analyzed) -> Result<(), Refusal> {
    let hir = &analyzed.compiler.hir;
    let mut stack: Vec<crate::hir::NodeId> = analyzed.main_statements.clone();
    while let Some(id) = stack.pop() {
        // `defined?(yield)` asks a question rather than yielding, and
        // CRuby answers it with nil rather than refusing the compile.
        if matches!(hir[id], HirNode::DefMethod { .. } | HirNode::Defined(_)) {
            continue;
        }
        hir[id].for_each_child(&mut |c| stack.push(c));
        if matches!(hir[id], HirNode::Yield(_)) {
            let (file, line) = crate::analyze::source::source_location(&analyzed.compiler, id)
                .unwrap_or(("(eval)", 1));
            return Err(Refusal::SyntaxAt(
                format!("{file}:{line}: Invalid yield"),
                file.to_string(),
            ));
        }
    }
    Ok(())
}

/// The shapes that are wrong ANYWHERE in a snippet, `def` bodies
/// included: their lowering reads a decision only a whole-program compile
/// makes. Each is a widening this compiler owes (G6-1/G6-2).
fn scope_refusals(analyzed: &crate::analyze::Analyzed) -> Result<(), String> {
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
            // Analyze registers nothing in a snippet, so neither of these
            // can be synthesized here -- they are refused because a
            // silently wrong answer is the alternative, not because the
            // shape is expected.
            HirNode::MethodRedefine { .. } => Some("a redefinition analyze resolved"),
            HirNode::DefHook { .. } => Some("a definition hook analyze spliced"),
            HirNode::ClassMethodPrepend { .. } => Some("a singleton `prepend`"),
            // Compile-time-only surfaces.
            // The FFI surface is assembled by analyze from markers a
            // whole-program compile consumes.
            HirNode::Ffi(..) => Some("an `FFI::Library` declaration"),
            HirNode::BoxScope { .. } | HirNode::BoxHandle(..) => Some("a `Ruby::Box`"),
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
