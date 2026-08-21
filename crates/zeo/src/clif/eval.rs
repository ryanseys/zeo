//! One `eval` snippet, compiled and JITted on its own (plan G6).
//!
//! A whole program emits a `ProgramDesc` full of tables the runtime
//! registers at boot; a snippet emits ONE function and the statics it
//! reads. Everything the program's emitter decides statically -- who owns
//! a method, which class a receiver is, where a constant lives -- is
//! decided at RUN time here, because a snippet arrives after all of that
//! has already happened.
//!
//! The entry is `(cells, self, out) -> i32`, the status protocol every
//! compiled function speaks. `cells` is the caller's own array of `*mut
//! Cell` -- one per local the snippet may touch, taken from the Binding
//! the eval runs under, so a write reaches the compiled frame that
//! captured it. The frame is pushed by the CALLER (`zeo::eval`), which is
//! what keeps the snippet's backtrace identical to the interpreter's.

use super::ctx::{Fx, Local};
use super::emit::{ClifModule, Emitter};
use super::{statics, verify};
use crate::analyze::Analyzed;
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{Linkage, Module};

/// What the snippet needs to know about the scope it runs in -- all of it
/// run-time knowledge the caller reads off the `EvalRequest`.
pub struct EvalSpec<'a> {
    /// The locals the entry binds from `cells`, in the caller's array
    /// order. Empty when the eval has no Binding: the snippet's own locals
    /// are then ordinary slots that die with the call, exactly as CRuby's
    /// bare `instance_eval`/`class_eval` string forms do.
    pub cells: &'a [String],
    /// The `Ruby::Box` the source resolves globals and constants in.
    pub box_id: u32,
    /// The enclosing frame's label -- what a block written in the snippet
    /// names itself after.
    pub label: &'a str,
    /// The classes the source resolves constants against, when they are
    /// ones only the run time knows -- see `clif::ctx::EvalCref`.
    pub cref: Option<std::rc::Rc<super::ctx::EvalCref>>,
    /// Which surface invoked the eval (`zeo_rt::eval::EvalMode` as a byte).
    pub mode: u8,
    /// The first flip-flop latch id reserved for this snippet. A snippet is
    /// compiled by a FRESH compiler whose ids start at zero, which are the
    /// running program's own -- see `zeo_rt::eval::reserve_flip_flops`.
    pub flip_flop_base: u32,
    pub using_base: u32,
}

/// A JITted snippet. The module owns the code, so it must outlive every
/// call into `entry`; `zeo::eval` keeps both for the process's life, as
/// CRuby keeps an eval's iseq.
pub struct EvalProgram {
    #[allow(dead_code, reason = "owns the code memory `entry` points into")]
    module: JITModule,
    pub entry: *const u8,
    /// Interns the snippet's symbols and initialises its cache slots --
    /// called once, before the first `entry` call.
    pub unit_init: Option<*const u8>,
}

/// Lower an analyzed snippet into executable memory.
pub fn compile(analyzed: &Analyzed, spec: &EvalSpec<'_>) -> Result<EvalProgram, String> {
    let mut em = Emitter::new(true)?;
    let entry_id = define_entry(&mut em, analyzed, spec)?;
    let unit_init = statics::define_unit_init(&mut em)?;
    statics::define_syms(&mut em)?;
    statics::define_callsites(&mut em)?;
    statics::define_cm_sites(&mut em)?;
    statics::define_rodata(&mut em)?;
    let ClifModule::Jit(mut module) = em.module else {
        unreachable!("Emitter::new(true) builds a JIT module")
    };
    module
        .finalize_definitions()
        .map_err(|e| format!("finalizing an eval: {e}"))?;
    let entry = module.get_finalized_function(entry_id);
    let unit_init = unit_init.map(|f| module.get_finalized_function(f));
    Ok(EvalProgram {
        module,
        entry,
        unit_init,
    })
}

const ENTRY: &str = "zeo_eval_entry";

fn define_entry(
    em: &mut Emitter,
    analyzed: &Analyzed,
    spec: &EvalSpec<'_>,
) -> Result<cranelift_module::FuncId, String> {
    let stmts = &analyzed.main_statements;
    let mut sig = em.module.make_signature();
    for _ in 0..3 {
        sig.params.push(AbiParam::new(em.ptr));
    }
    sig.returns.push(AbiParam::new(types::I32));
    let func_id = em
        .module
        .declare_function(ENTRY, Linkage::Local, &sig)
        .map_err(|e| format!("declaring {ENTRY}: {e}"))?;

    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 0), sig);
    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let b = FunctionBuilder::new(&mut func, &mut fbc);
    let mut fx = Fx::new(em, analyzed, b, |em, b| {
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let rodata_gv = em.module.declare_data_in_func(em.rodata_id, b.func);
        let syms_gv = em.module.declare_data_in_func(em.syms_id, b.func);
        let rodata = b.ins().symbol_value(em.ptr, rodata_gv);
        let syms = b.ins().symbol_value(em.ptr, syms_gv);
        (rodata, syms)
    });
    let (cells_p, self_p, out_ptr) = {
        let entry = fx.b.current_block().expect("entry is current");
        let p = fx.b.block_params(entry);
        (p[0], p[1], p[2])
    };

    fx.self_ptr = Some(self_p);
    // Every question the program's emitter answers from the class it is
    // emitting INTO, a snippet answers at run time: it has no class of its
    // own, its `self` is whatever the caller had, and its ivars are
    // name-keyed for the same reason.
    fx.self_is_dynamic = true;
    fx.dyn_ivars = true;
    fx.box_id = spec.box_id;
    fx.frame_label = spec.label.to_string();
    // prism parsed the snippet on its own, so a name that IS one of the
    // caller's locals could only arrive as a vcall -- ruby reads it as the
    // local, and the cells below are its storage.
    fx.in_eval_splice = true;
    fx.eval_cref = spec.cref.clone();
    fx.eval_mode = Some(spec.mode);
    // A snippet has no defining class of its own, so a `super` written in
    // one resumes from the (class, name) pair the method-frame stack
    // recorded -- the same question a run-time-installed body asks, and
    // the same answer.
    fx.runtime_method_body = true;
    fx.flip_flop_base = spec.flip_flop_base;
    fx.using_base = spec.using_base;

    // The caller's cells first: unowned (the caller holds the reference
    // and drops it when the call returns).
    let fl = MemFlagsData::trusted();
    let ptr_ty = fx.em.ptr;
    for (i, name) in spec.cells.iter().enumerate() {
        let cellp = fx.b.ins().load(ptr_ty, fl, cells_p, (i * 8) as i32);
        let ss =
            fx.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
        let dst = fx.slot_addr(ss, 0);
        fx.b.ins().store(fl, cellp, dst, 0);
        fx.locals
            .insert(name.clone(), Local::Cell { ss, owned: false });
    }
    // ...then the snippet's own names, when it runs without a Binding.
    let empty_params = crate::hir::Params::default();
    let caps = crate::analyze::captures::collect_escaping_captures(
        &analyzed.compiler,
        stmts,
        &empty_params,
        crate::analyze::class_query::SelfClass::new(None, None),
    );
    let captured = caps.locals;
    let mut locals = crate::analyze::local_storage::Locals::default();
    for &stmt in stmts {
        crate::analyze::local_storage::collect_locals(&analyzed.compiler, stmt, &mut locals);
    }
    for name in locals.names().to_vec() {
        if fx.locals.contains_key(&name) {
            continue;
        }
        if captured.contains(&name) {
            super::emit::init_cell_local(&mut fx, name, None);
        } else {
            let ss = fx.new_value_slot();
            fx.locals.insert(name, Local::Slot(ss));
        }
    }

    super::stmt::lower_value_body_into(&mut fx, stmts, out_ptr)?;

    let epilogue = |fx: &mut Fx, status: i64| {
        super::emit::release_locals(fx);
        let code = fx.b.ins().iconst(types::I32, status);
        fx.b.ins().return_(&[code]);
    };
    epilogue(&mut fx, 0);
    let land = fx.land;
    fx.b.switch_to_block(land);
    epilogue(&mut fx, 1);

    verify::check(&fx, ENTRY);
    let Fx { mut b, .. } = fx;
    b.seal_all_blocks();
    b.finalize(cfg);

    em.record_clif(ENTRY, &func);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(func_id, &mut ctx)
        .map_err(|e| format!("compiling {ENTRY}: {e}"))?;
    Ok(func_id)
}
