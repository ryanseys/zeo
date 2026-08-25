//! One `eval` snippet, compiled and JITted on its own.
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
use super::module::{ClifModule, Emitter};
use super::ownership;
use super::{statics, verify};
use crate::analyze::Analyzed;
use crate::codegen_error::{CResult, CodegenError};
use crate::hir::{HirNode, NodeId};
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, MemFlagsData, UserFuncName, types};
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
pub fn compile(analyzed: &Analyzed, spec: &EvalSpec<'_>) -> CResult<EvalProgram> {
    let mut em = Emitter::new(true)?;
    em.eval_sites = true;
    let entry_id = define_entry(&mut em, analyzed, spec)?;
    let unit_init = statics::define_unit_init(&mut em)?;
    statics::define_syms(&mut em)?;
    statics::define_callsites(&mut em)?;
    statics::define_cm_sites(&mut em)?;
    statics::define_const_sites(&mut em)?;
    statics::define_new_sites(&mut em)?;
    statics::define_rodata(&mut em)?;
    let ClifModule::Jit(mut module) = em.module else {
        unreachable!("Emitter::new(true) builds a JIT module")
    };
    module
        .finalize_definitions()
        .map_err(|e| CodegenError::internal(format!("finalizing an eval: {e}")))?;
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
) -> CResult<cranelift_module::FuncId> {
    let stmts = &analyzed.main_statements;
    let mut sig = em.module.make_signature();
    for _ in 0..3 {
        sig.params.push(AbiParam::new(em.ptr));
    }
    sig.returns.push(AbiParam::new(types::I32));
    let func_id = em
        .module
        .declare_function(ENTRY, Linkage::Local, &sig)
        .map_err(|e| CodegenError::internal(format!("declaring {ENTRY}: {e}")))?;

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
    fx.eval_cref = spec.cref.clone();
    fx.eval_mode = Some(spec.mode);
    // A snippet has no defining class of its own, so a `super` written in
    // one resumes from the (class, name) pair the method-frame stack
    // recorded -- the same question a run-time-installed body asks, and
    // the same answer.
    fx.runtime_method_body = true;
    fx.flip_flop_base = spec.flip_flop_base;
    fx.using_base = spec.using_base;
    // A `binding` written in the snippet reports the same names its own
    // entry took cells for -- the caller's, plus whatever the source
    // introduces. Without this the call reached `Kernel#binding`'s row,
    // which refuses (a method row cannot see its caller's scope).
    fx.binding_names = Some(std::rc::Rc::new(spec.cells.to_vec()));

    // The caller's cells first: unowned (the caller holds the reference
    // and drops it when the call returns).
    let fl = MemFlagsData::trusted();
    let ptr_ty = fx.em.ptr;
    for (i, name) in spec.cells.iter().enumerate() {
        let cellp = fx.b.ins().load(ptr_ty, fl, cells_p, (i * 8) as i32);
        let ss = fx.new_cell_slot();
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
        None,
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
            super::body::init_cell_local(&mut fx, name, None);
        } else {
            let ss = fx.new_value_slot();
            fx.locals.insert(name, Local::Slot(ss));
        }
    }

    super::stmt::lower_value_body_into(&mut fx, stmts, out_ptr)?;

    let epilogue = |fx: &mut Fx, status: i64| {
        super::body::release_locals(fx);
        let code = fx.b.ins().iconst(types::I32, status);
        fx.b.ins().return_(&[code]);
    };
    epilogue(&mut fx, 0);
    let land = fx.land;
    fx.b.switch_to_block(land);
    epilogue(&mut fx, 1);

    fx.drain_slot_inits();
    verify::check(&fx, ENTRY)?;
    let Fx { mut b, .. } = fx;
    b.seal_all_blocks();
    b.finalize(cfg);

    em.record_clif(ENTRY, &func);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(func_id, &mut ctx)
        .map_err(|e| CodegenError::internal(format!("compiling {ENTRY}: {e}")))?;
    Ok(func_id)
}

// ---------------------------------------------------------------------
// Lowering rules for statements inside an eval snippet: class bodies,
// mixins, and the default definee, all resolved at run time.

/// `include M` inside an `eval`: the receiverless send, with the module
/// read as an ordinary constant. Receiverless because ruby's own
/// `Module#include` is public but `main.include` is not -- the same
/// FCALL barrier a bare `include` at the top level passes.
pub(super) fn eval_mixin_send(fx: &mut Fx, site: NodeId, module: &str, verb: &str) -> CResult<()> {
    let op = eval_mixin_send_value(fx, site, module, verb)?;
    ownership::discard(fx, op);
    Ok(())
}

pub(super) fn eval_mixin_send_value(
    fx: &mut Fx,
    site: NodeId,
    module: &str,
    verb: &str,
) -> CResult<super::operand::Operand> {
    let arg = super::consts::const_read(fx, site, module)?;
    let tag = arg.tag();
    let argv = ownership::borrow_ptr(fx, &arg);
    if arg.owned() {
        ownership::pool_owned(fx, argv, tag);
    }
    super::call::implicit_send_ptr(fx, verb, argv, 1)
}

/// `class Foo < Bar ... end` / `module M ... end` written inside a run-time
/// `eval`.
///
/// The HEADER runs here: the owner is the snippet's own cref (or the scope
/// `A::B` names), the superclass is an ordinary constant read in THIS
/// scope, and the runtime reuses or mints the class.
///
/// The BODY runs as one more `class_eval` of its own source text. That is
/// not a shortcut -- a class body in CRuby is a separate iseq with its own
/// cref and its own locals, sharing nothing with the scope around it, and
/// this compiler's `class_eval` path already answers every question such a
/// body raises: where a `def` lands, what a bare constant resolves
/// against, which class owns `@@x`. Lowering the body HERE would need all
/// of that a second time, against a class id no compile can know.
pub(crate) fn eval_class_def(fx: &mut Fx, stmt: NodeId) -> CResult<super::operand::Operand> {
    use super::operand::{Operand, TagInfo};
    let HirNode::ClassDef {
        name,
        superclass,
        body,
        is_module,
    } = &fx.an.compiler.hir[stmt]
    else {
        unreachable!("guarded by the ClassDef arm")
    };
    let (name, superclass, body, is_module) =
        (name.clone(), superclass.clone(), body.clone(), *is_module);
    if name == crate::compiler::SINGLETON_SURROGATE {
        return fx.unsupported(stmt, "a `class << self` inside an `eval`");
    }
    let (scope, leaf) = crate::hir::split_const_path(&name);

    // The owner of the bare name: the scope when one is written, else the
    // snippet's own cref -- and the top level when it has none.
    let owner = match scope.filter(|s| !s.is_empty()) {
        Some(s) => super::consts::const_path_read(fx, stmt, s)?,
        None => {
            // The snippet's own cref when it has one, else its TOP LEVEL --
            // which inside a box is the box's surrogate, not `Object`.
            // Binding on `Object` let main reach a class the box wrote.
            let cid = fx
                .eval_cref
                .as_ref()
                .and_then(|c| c.chain.first().copied())
                .map_or_else(|| super::boxes::box_top(fx), crate::compiler::ClassId);
            super::consts::class_immediate(fx, cid)
        }
    };
    let owner_ptr = ownership::borrow_ptr(fx, &owner);
    if owner.owned() {
        ownership::pool_owned(fx, owner_ptr, owner.tag());
    }
    let super_ptr = match &superclass {
        Some(sup) => {
            let op = super::consts::const_path_read(fx, stmt, sup)?;
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            p
        }
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    let (nptr, nlen) = super::expr::rodata_name(fx, leaf);
    let is_module_v = fx.b.ins().iconst(types::I8, i64::from(u8::from(is_module)));
    let class_ss = fx.temp_slot();
    let class_ptr = fx.slot_addr(class_ss, 0);
    let status = fx.call_status(
        "zeo_rt_eval_class_open",
        &[owner_ptr, nptr, nlen, super_ptr, is_module_v, class_ptr],
    );
    fx.fallible(status);
    fx.owned_created += 1;
    ownership::pool_owned(
        fx,
        class_ptr,
        TagInfo::Known(zeo_abi::abi::ValueTag::Class as u8),
    );

    let (src, file, line) = eval_body_source(fx, stmt, &body)?;
    let (sptr, slen) = super::expr::rodata_name(fx, &src);
    let (fptr, flen) = super::expr::rodata_name(fx, &file);
    let kind = if is_module { "module" } else { "class" };
    let (lptr, llen) = super::expr::rodata_name(fx, &format!("<{kind}:{leaf}>"));
    let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
    let bx = fx.box_v();
    // The chain THIS scope searches, which the body prepends its own class
    // to: `class Inside` written in `Wrap.class_eval` still sees
    // `Wrap::IN_WRAP`.
    let outer: Vec<u32> = fx
        .eval_cref
        .as_ref()
        .map(|c| c.chain.clone())
        .unwrap_or_default();
    let bytes: Vec<u8> = outer.iter().flat_map(|c| c.to_le_bytes()).collect();
    let outer_off = fx.em.intern_rodata_aligned(&bytes, 4);
    let outer_ptr = fx.rod(outer_off);
    let n_outer = fx.b.ins().iconst(fx.em.ptr, outer.len() as i64);
    let out_ss = fx.temp_slot();
    let out = fx.slot_addr(out_ss, 0);
    let status = fx.call_status(
        "zeo_rt_eval_class_body",
        &[
            class_ptr, sptr, slen, fptr, flen, line_v, lptr, llen, bx, outer_ptr, n_outer, out,
        ],
    );
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss: out_ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The SOURCE TEXT of a body written inside a snippet, sliced from the
/// statements' own spans -- plus the file and first line they report, so a
/// backtrace row raised inside it names the same place the snippet does.
fn eval_body_source(fx: &Fx, stmt: NodeId, body: &[NodeId]) -> CResult<(String, String, u32)> {
    let hir = &fx.an.compiler.hir;
    let Some((file, line)) = crate::analyze::source::source_location(&fx.an.compiler, stmt) else {
        return Err(CodegenError::unsupported(
            "a span-less `class` inside an `eval`",
            None,
        ));
    };
    let (file, mut line) = (file.to_string(), line);
    let (Some(first), Some(last)) = (body.first(), body.last()) else {
        return Ok((String::new(), file, line));
    };
    let (Some(a), Some(b)) = (hir.span(*first), hir.span(*last)) else {
        return Err(CodegenError::unsupported(
            "a span-less statement in a `class` inside an `eval`",
            None,
        ));
    };
    // The body's TEXT is what runs -- a class body inside a snippet is one
    // more `class_eval` of its own source -- so the usual case is one slice
    // of the snippet between the first and last statement, which keeps every
    // line number exactly where the snippet put it.
    let own = hir
        .span(stmt)
        .and_then(|s| s.known())
        .ok_or_else(|| CodegenError::unsupported("a span-less `class` inside an `eval`", None))?;
    if a.file == own.file && a.start >= own.start && b.end <= own.end {
        let src = hir
            .files
            .get(a.file.0 as usize)
            .ok_or_else(|| CodegenError::internal("a `class` body from an unknown file"))?;
        // To the class's own `end`, not to the last STATEMENT's end. A
        // `class << self` is not a statement -- `lower::defs` splices it into
        // the enclosing body as a surrogate reopen plus the retagged `def`s --
        // so the last statement stops short of the `end` that closes the
        // singleton body, and prism then reports an unterminated `class`.
        let closing = (own.end as usize)
            .checked_sub(3)
            .filter(|&at| src.source.get(at..at + 3) == Some("end"))
            .filter(|&at| at >= b.end as usize);
        let (start, end) = (
            a.start as usize,
            closing.unwrap_or(b.end as usize).min(src.source.len()),
        );
        if start > end {
            return Err(CodegenError::internal(
                "a `class` body whose statements run backwards",
            ));
        }
        line = src.line_at(a.start);
        return Ok((src.source[start..end].trim_end().to_string(), file, line));
    }
    // A body analyze REWROTE reaches here: some of its statements are
    // synthesized, and their spans point into a source of analyze's own
    // making rather than into the snippet. An `FFI::Struct` subclass is the
    // shape that does it -- its `layout` is replaced in place by the
    // accessors `lower::ffi::synthesize_ffi_struct` writes -- and that
    // synthesized text is real Ruby, so the body is still recoverable: slice
    // each RUN of same-file statements from its own file and join the runs.
    // What this cannot keep is the line numbering, since the runs come from
    // different files; the class reports its own line for the whole body,
    // which is where the compiled tier attributes a synthesized accessor
    // too.
    let mut parts: Vec<String> = Vec::new();
    let mut run: Option<(crate::hir::FileId, u32, u32)> = None;
    let flush =
        |run: Option<(crate::hir::FileId, u32, u32)>, parts: &mut Vec<String>| -> CResult<()> {
            let Some((f, start, end)) = run else {
                return Ok(());
            };
            let src = hir
                .files
                .get(f.0 as usize)
                .ok_or_else(|| CodegenError::internal("a `class` body from an unknown file"))?;
            let (start, end) = (start as usize, (end as usize).min(src.source.len()));
            if start > end {
                return Err(CodegenError::internal(
                    "a `class` body whose statements run backwards",
                ));
            }
            parts.push(src.source[start..end].to_string());
            Ok(())
        };
    for id in body {
        let span = hir.span(*id).and_then(|s| s.known()).ok_or_else(|| {
            CodegenError::unsupported("a span-less statement in a `class` inside an `eval`", None)
        })?;
        run = match run {
            Some((f, start, end)) if f == span.file && span.start >= end => {
                Some((f, start, span.end))
            }
            other => {
                flush(other, &mut parts)?;
                Some((span.file, span.start, span.end))
            }
        };
    }
    flush(run, &mut parts)?;
    Ok((parts.join("\n"), file, line))
}

/// The class a definition-level statement written in a snippet names --
/// `alias`, `undef`, `private`, `module_function`, `private_constant`. Only
/// the run time knows it (`zeo_rt_eval_definee`), and one compiled snippet
/// may be evaluated against any number of receivers.
fn eval_definee(fx: &mut Fx, singleton: bool) -> CResult<super::operand::Operand> {
    use super::operand::Operand;
    let mode = fx.eval_mode.expect("guarded by the eval arms");
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let mode_v = fx.b.ins().iconst(types::I8, i64::from(mode));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status("zeo_rt_eval_definee", &[mode_v, self_ptr, out]);
    fx.fallible(status);
    fx.owned_created += 1;
    let definee = Operand::Slot {
        ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    };
    if !singleton {
        return Ok(definee);
    }
    let ptr = ownership::borrow_ptr(fx, &definee);
    ownership::pool_owned(fx, ptr, definee.tag());
    let empty = fx.b.ins().iconst(fx.em.ptr, 0);
    eval_definee_call(fx, ptr, "singleton_class", empty, 0)
}

/// `definee.<verb>(:a, :b, ...)` -- ruby writes these as private methods of
/// `Module`, so the send lowers the visibility barrier the way an implicit
/// receiver does.
pub(super) fn eval_definee_send(
    fx: &mut Fx,
    verb: &str,
    names: &[String],
    singleton: bool,
) -> CResult<()> {
    let op = eval_definee_send_value(fx, verb, names, singleton)?;
    ownership::discard(fx, op);
    Ok(())
}

pub(crate) fn eval_definee_send_value(
    fx: &mut Fx,
    verb: &str,
    names: &[String],
    singleton: bool,
) -> CResult<super::operand::Operand> {
    let definee = eval_definee(fx, singleton)?;
    let recv = ownership::borrow_ptr(fx, &definee);
    if definee.owned() {
        ownership::pool_owned(fx, recv, definee.tag());
    }
    let argv =
        fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (names.len().max(1) * zeo_abi::abi::VALUE_SIZE) as u32,
            3,
        ));
    for (i, name) in names.iter().enumerate() {
        let sym = fx.sym_id(name);
        let slot = fx.slot_addr(argv, (i * zeo_abi::abi::VALUE_SIZE) as i32);
        fx.call("zeo_rt_sym_value", &[sym, slot]);
    }
    let a0 = fx.slot_addr(argv, 0);
    eval_definee_call(fx, recv, verb, a0, names.len())
}

pub(super) fn eval_definee_call(
    fx: &mut Fx,
    recv: cranelift_codegen::ir::Value,
    verb: &str,
    argv: cranelift_codegen::ir::Value,
    argc: usize,
) -> CResult<super::operand::Operand> {
    use super::operand::Operand;
    let sym = fx.sym_id(verb);
    let bx = fx.box_v();
    let argc_v = fx.b.ins().iconst(fx.em.ptr, argc as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status(
        "zeo_rt_send_value_in",
        &[bx, recv, sym, argv, argc_v, null, out],
    );
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    })
}
