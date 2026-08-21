//! Per-function lowering context: the function builder, the one landing
//! block, local slots, temp-slot recycling, loop targets, and the lazy
//! import/rodata/symbol plumbing every lowering reaches through.

use super::emit::Emitter;
use crate::analyze::Analyzed;
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;
use std::collections::HashMap;

pub(crate) const VALUE_SIZE: u32 = zeo_abi::abi::VALUE_SIZE as u32;

/// One Ruby local's storage in the enclosing function.
#[derive(Clone, Copy)]
pub(crate) enum Local {
    /// An owned 24-byte value slot.
    Slot(ir::StackSlot),
    /// A captured local: the 8-byte slot holds the `*mut Cell` pointer.
    /// `owned` = this function created the cell (releases it at exit);
    /// a block body's env cells belong to the proc.
    Cell { ss: ir::StackSlot, owned: bool },
}

/// An enclosing native loop's jump targets. `result` is the loop's value
/// slot when the loop sits in value position: `break v` moves `v` there
/// (the normal exit writes the loop's own default first).
pub(crate) struct LoopCtl {
    pub exit: ir::Block,
    pub latch: ir::Block,
    pub body: ir::Block,
    pub result: Option<ir::Value>,
    /// The `ensure_depth` at loop entry: a `break`/`next`/`redo` may only
    /// jump when no `ensure` boundary sits between it and the loop.
    pub depth: usize,
    /// The `handling_depth` at loop entry: a jump out emits the
    /// difference in `handling_pop`s first (`$!` stays balanced).
    pub handling: usize,
}

pub(crate) struct Fx<'e, 'f> {
    pub em: &'e mut Emitter,
    pub an: &'e Analyzed,
    pub b: FunctionBuilder<'f>,
    /// The ONE landing block: releases the locals, pops the frame, returns
    /// `STATUS_SIGNAL` (the frame pop drains the release pool).
    pub land: ir::Block,
    /// The `zeo_rodata` base address, materialized once in the entry block.
    pub rodata_base: ir::Value,
    /// The `zeo_syms` base address, likewise.
    pub syms_base: ir::Value,
    /// Ruby local -> its storage (hoisted; captured names live in cells).
    /// Ordered: epilogues and capture walks iterate it, and emission must
    /// be a pure function of the source.
    pub locals: std::collections::BTreeMap<String, Local>,
    /// The names a `binding` taken in THIS scope reports, when the scope
    /// takes one (`analyze::captures::binding_scope_names`). Its presence is
    /// what promoted those locals to cells, which is the only storage a
    /// binding can share. `None` in a scope that never mentions `binding`.
    pub binding_names: Option<std::rc::Rc<Vec<String>>>,
    frefs: HashMap<&'static str, ir::FuncRef>,
    temp_free: Vec<ir::StackSlot>,
    temp_taken: Vec<ir::StackSlot>,
    pub loops: Vec<LoopCtl>,
    pub prev_line: Option<u32>,
    /// The stamped statement's FILE, tracked beside `prev_line` for line
    /// coverage: a statement that begins a spliced file is what marks the
    /// file covered.
    pub prev_file: Option<String>,
    /// The current `self` as a borrowed pointer: the method's first
    /// parameter, or the toplevel's pooled `main_object` copy.
    pub self_ptr: Option<ir::Value>,
    /// The enclosing class when lowering a method body -- what ivar slot
    /// resolution keys on.
    pub method_class: Option<zeo_abi::ClassId>,
    /// Lowering a CLASS-method body: `self` is the Class value, so `@x`
    /// is the class object's own ivar (a civar site, not lowered yet) and
    /// an implicit send resolves through the singleton chain.
    pub self_is_class: bool,
    /// This body's owner is NATIVE-BACKED (an exception subclass): ivars
    /// are name-keyed at runtime, not compiled slots.
    pub dyn_ivars: bool,
    /// The class this body's `def` was WRITTEN in (rustc's
    /// `cx.defining_class`) -- where a `super` walk resumes from. A module
    /// method's materialized copy keeps the MODULE here while
    /// `method_class` names the includer.
    pub defining_class: Option<zeo_abi::ClassId>,
    /// The singleton-class SURROGATE this body was lexically written in (a
    /// `def` in a constant-bearing `class << self` body). Bare constants and
    /// `Module.nesting` resolve through it; dispatch, ivars and `super` keep
    /// using the owner. `None` for every other body. See
    /// `Scope::lexical_home`.
    pub lexical_home: Option<zeo_abi::ClassId>,
    /// The enclosing method's name -- what a `super` re-sends.
    pub method_name: Option<String>,
    /// The enclosing method's parameter list -- what a bare `super`
    /// forwards by name.
    pub method_params: Option<crate::hir::Params>,
    /// This body is a RUNTIME-installed method (a `def`/`define_method`
    /// the analyzer could not register), so its defining class is minted
    /// at run time and a `super` reads it off the method-frame stack --
    /// rustc's `runtime_method_body_params` (`Some` = this flag).
    pub runtime_method_body: bool,
    /// ...and it came from a literal `define_method`, where ruby refuses
    /// a BARE `super` at dispatch: a block-shaped body has no parameter
    /// list to forward from, so ruby raises rather than guessing.
    pub define_method_body: bool,
    /// This scope's `self` is only known at run time -- a runtime-installed
    /// method body, or a block `instance_eval`/`instance_exec` re-homes. The
    /// class a `protected` check compares against is then whatever `self`
    /// turns out to be, not the lexically enclosing class (rustc's
    /// `Ctx::self_is_dynamic`).
    pub self_is_dynamic: bool,
    /// Lowering the statements of an AOT-spliced `eval("literal")`. prism
    /// parsed the snippet on its own, so a bare name that IS one of the
    /// enclosing scope's locals could only arrive as a vcall; ruby reads
    /// it as the local (`Ctx::in_eval_splice`).
    pub in_eval_splice: bool,
    /// Lowering a run-time `eval` snippet whose constants resolve against a
    /// class only the RUN TIME knows (`zeo::eval`): its id and its name.
    /// The fresh compiler a snippet is lowered by has no entry for it --
    /// class ids are the one thing both sides always agreed on, so the id
    /// travels as an immediate and every static fold stands down.
    pub eval_cref: Option<std::rc::Rc<(Option<u32>, String)>>,
    /// Lowering a run-time `eval` snippet: which surface invoked it
    /// (`eval_vm::EvalMode` as a byte). A `def` inside one installs where
    /// the RUN TIME says, because one snippet may be evaluated against any
    /// number of receivers.
    pub eval_mode: Option<u8>,
    /// The `Ruby::Box` this code runs in (`0` = main). Every dynamic send,
    /// global and constant owner is keyed by it, which is the AOT
    /// translation of CRuby's loading-box context (`Ctx::box_id`).
    pub box_id: u32,
    /// The name this body was DEFINED under when an alias reaches it by
    /// another: what `__method__` answers where `__callee__` answers
    /// [`Fx::method_name`].
    pub method_origin: Option<String>,
    /// A method body's `(out, ret_ok)`: `return` writes the value and
    /// jumps; `None` at the toplevel.
    pub ret: Option<(ir::Value, ir::Block)>,
    /// `retry` targets (a rescue clause's begin head), with their ensure
    /// and handling depths.
    pub retries: Vec<(ir::Block, usize, usize)>,
    /// How many `ensure` bodies enclose the current lowering point -- a
    /// DIRECT jump (`break`/`next`/`redo`/`return`/`retry`) may not cross
    /// one (it would skip the ensure), so it travels as a signal instead
    /// and the ensure-carrying `begin` settles it back onto its target.
    pub ensure_depth: usize,
    /// How many loop-targeted jumps have taken that signal route so far.
    /// A `begin` compares the count across its own lowering: a difference
    /// means one of its jumps needs its settle, and NO difference means
    /// nothing inside it can arm a `Break`/`Next`/`Redo`, so it must not
    /// claim one that merely passes through.
    pub ensure_jumps: usize,
    /// How many `$!` (`handling_push`) entries the current lexical point
    /// sits under -- a direct jump pops down to its target's depth.
    pub handling_depth: usize,
    /// The enclosing frame's label ("<main>", "Object#fib") -- what a
    /// block's own frame derives its "block in ..." label from. A block
    /// keeps the label of the scope it was WRITTEN in, never its own.
    pub frame_label: String,
    /// How many blocks deep this scope sits under `frame_label` -- ruby
    /// counts the nesting (`block (2 levels) in ...`), and a real `def`
    /// restarts the count.
    pub block_depth: usize,
    /// The method's borrowed block parameter (null = no block passed);
    /// `None` when the scope has no block slot at all (yield then passes
    /// null and raises the LocalJumpError).
    pub blk_ptr: Option<ir::Value>,
    /// Inside an escaping block body: where `next v` moves its value and
    /// jumps (the block's ok-exit).
    pub block_next: Option<(ir::Value, ir::Block)>,
    /// Names currently aliased to a fused-block shadow slot -- an escaping
    /// block may not capture one (the shadow dies with the loop).
    pub shadowed: std::collections::HashSet<String>,
    /// The ownership ledger `verify` checks: every owned-value emission
    /// site must be matched by exactly one consumption site.
    /// Drain the frame's release pool at every STATEMENT boundary, not
    /// only at frame pop. Set for the long-lived frames -- the top level
    /// and a required unit -- where a temporary otherwise lives for the
    /// whole program: CRuby's temporaries die with their statement, and a
    /// program that watches for collection (`ObjectSpace::WeakMap`, a
    /// finalizer) can SEE the difference.
    pub drain_temps: bool,
    pub owned_created: usize,
    pub owned_consumed: usize,
    /// The enclosing `def` carries the `ruby2_keywords` directive: a splat
    /// forwarding its `*rest` keeps the keyword mark on a trailing hash.
    pub ruby2_keywords: bool,
    /// Inside a block fn: the binding head `redo` jumps to (re-running the
    /// param bindings, ruby's rule) -- set once the bindings exist.
    pub block_redo: Option<ir::Block>,
}

impl<'e, 'f> Fx<'e, 'f> {
    pub fn new(
        em: &'e mut Emitter,
        an: &'e Analyzed,
        mut b: FunctionBuilder<'f>,
        rodata_base_of: impl FnOnce(&mut Emitter, &mut FunctionBuilder<'f>) -> (ir::Value, ir::Value),
    ) -> Fx<'e, 'f> {
        let (rodata_base, syms_base) = rodata_base_of(em, &mut b);
        let land = b.create_block();
        Fx {
            em,
            an,
            b,
            land,
            rodata_base,
            syms_base,
            locals: std::collections::BTreeMap::new(),
            frefs: HashMap::new(),
            temp_free: Vec::new(),
            temp_taken: Vec::new(),
            loops: Vec::new(),
            prev_line: None,
            prev_file: None,
            self_ptr: None,
            method_class: None,
            self_is_class: false,
            dyn_ivars: false,
            defining_class: None,
            lexical_home: None,
            binding_names: None,
            method_name: None,
            method_params: None,
            runtime_method_body: false,
            define_method_body: false,
            self_is_dynamic: false,
            in_eval_splice: false,
            eval_cref: None,
            eval_mode: None,
            box_id: 0,
            method_origin: None,
            ret: None,
            retries: Vec::new(),
            ensure_depth: 0,
            ensure_jumps: 0,
            handling_depth: 0,
            frame_label: String::new(),
            block_depth: 0,
            blk_ptr: None,
            block_next: None,
            shadowed: std::collections::HashSet::new(),
            drain_temps: false,
            owned_created: 0,
            owned_consumed: 0,
            ruby2_keywords: false,
            block_redo: None,
        }
    }

    /// The in-function reference for capi import `name`.
    pub fn fref(&mut self, name: &'static str) -> ir::FuncRef {
        if let Some(&f) = self.frefs.get(name) {
            return f;
        }
        let id = self.em.import(name);
        let f = self.em.module.declare_func_in_func(id, self.b.func);
        self.frefs.insert(name, f);
        f
    }

    /// Call capi `name`; the single result when it has one.
    pub fn call(&mut self, name: &'static str, args: &[ir::Value]) -> Option<ir::Value> {
        let f = self.fref(name);
        let inst = self.b.ins().call(f, args);
        self.b.func.dfg.inst_results(inst).first().copied()
    }

    /// A pointer to rodata offset `off`.
    pub fn rod(&mut self, off: u32) -> ir::Value {
        if off == 0 {
            self.rodata_base
        } else {
            self.b.ins().iadd_imm_u(self.rodata_base, i64::from(off))
        }
    }

    /// The runtime symbol id for `name` (an `i32` load from `zeo_syms`,
    /// filled by `zeo_unit_init`).
    pub fn sym_id(&mut self, name: &str) -> ir::Value {
        let idx = self.em.syms.intern(name);
        self.b.ins().load(
            types::I32,
            ir::MemFlagsData::trusted(),
            self.syms_base,
            (idx * 4) as i32,
        )
    }

    /// A fresh inline-cache slot in `zeo_callsites`, vetted against
    /// `caller` (`u32::MAX` = FCALL, no visibility question), as a pointer.
    ///
    /// One slot per SITE, never shared: the cache is monomorphic, so two
    /// sites sharing one would thrash each other. The base is
    /// re-materialised per site rather than hoisted to the entry block --
    /// it is an `adrp`/`add` pair the same shape rustc's `&__CS_n[i]`
    /// compiles to, and hoisting it would need the value to dominate every
    /// block that sends.
    pub fn callsite_ptr(&mut self, caller: u32) -> ir::Value {
        let idx = self.em.callsites.len();
        self.em.callsites.push(caller);
        let gv = self
            .em
            .module
            .declare_data_in_func(self.em.callsites_id, self.b.func);
        let base = self.b.ins().symbol_value(self.em.ptr, gv);
        let off = (idx * zeo_abi::abi::CALLSITE_SIZE) as i64;
        if off == 0 {
            base
        } else {
            self.b.ins().iadd_imm_u(base, off)
        }
    }

    /// A fresh class-method cache slot in `zeo_cm_sites`, as a pointer.
    /// One per SITE, like [`Fx::callsite_ptr`]; the slot carries no
    /// per-site constant (it is keyed by the caller passed per call).
    pub fn cm_site_ptr(&mut self) -> ir::Value {
        let idx = self.em.cm_sites;
        self.em.cm_sites += 1;
        let gv = self
            .em
            .module
            .declare_data_in_func(self.em.cm_sites_id, self.b.func);
        let base = self.b.ins().symbol_value(self.em.ptr, gv);
        let off = (idx * zeo_abi::abi::CLASSMETHOD_SITE_SIZE) as i64;
        if off == 0 {
            base
        } else {
            self.b.ins().iadd_imm_u(base, off)
        }
    }

    /// A 24-byte temp slot, recycled at statement boundaries.
    pub fn temp_slot(&mut self) -> ir::StackSlot {
        let ss = self.temp_free.pop().unwrap_or_else(|| {
            self.b.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                VALUE_SIZE,
                3,
            ))
        });
        self.temp_taken.push(ss);
        ss
    }

    /// A statement's temp watermark -- pair with [`Fx::end_stmt`].
    pub fn stmt_mark(&self) -> usize {
        self.temp_taken.len()
    }

    /// Statement boundary: every temp taken SINCE `mark` is dead (its
    /// value moved, pooled, or immediate), so those slots recycle. Scoped
    /// by the watermark because statements nest (an `if` arm's statements
    /// finish while the enclosing expression's temps are still borrowed).
    pub fn end_stmt(&mut self, mark: usize) {
        let tail = self.temp_taken.split_off(mark);
        self.temp_free.extend(tail);
    }

    /// This scope's box as the `u32` every runtime entry takes.
    pub fn box_v(&mut self) -> ir::Value {
        use cranelift_codegen::ir::InstBuilder;
        let b = self.box_id;
        self.b.ins().iconst(ir::types::I32, i64::from(b))
    }

    pub fn slot_addr(&mut self, ss: ir::StackSlot, offset: i32) -> ir::Value {
        let ptr = self.em.ptr;
        self.b.ins().stack_addr(ptr, ss, offset)
    }

    /// Branch to the landing when `status` is nonzero; lowering continues
    /// in a fresh block.
    pub fn fallible(&mut self, status: ir::Value) {
        let next = self.b.create_block();
        self.b.ins().brif(status, self.land, &[], next, &[]);
        self.b.switch_to_block(next);
    }

    /// After an unconditional jump: continue lowering in a fresh (possibly
    /// unreachable) block, so dead statements after `break`/`next` still
    /// lower without tripping the "block already terminated" rule.
    pub fn continue_unreachable(&mut self) {
        let next = self.b.create_block();
        self.b.switch_to_block(next);
    }

    /// The source location of `node`, for refusal messages and line stamps.
    pub fn location(&self, node: crate::hir::NodeId) -> Option<(&str, u32)> {
        crate::codegen::source_location(&self.an.compiler, node)
    }

    /// A fresh nil-initialized value slot.
    pub fn new_value_slot(&mut self) -> ir::StackSlot {
        let ss = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            VALUE_SIZE,
            3,
        ));
        let dst = self.slot_addr(ss, 0);
        let z = self.b.ins().iconst(types::I64, 0);
        for off in [0, 8, 16] {
            self.b.ins().store(MemFlagsData::trusted(), z, dst, off);
        }
        ss
    }

    /// The `*mut Cell` a captured local's pointer slot holds.
    pub fn cell_ptr(&mut self, ss: ir::StackSlot) -> ir::Value {
        let addr = self.slot_addr(ss, 0);
        let ptr = self.em.ptr;
        self.b.ins().load(ptr, MemFlagsData::trusted(), addr, 0)
    }

    /// Emit the `handling_pop`s a direct jump owes before leaving for a
    /// point at `target` handling depth (the lexical counter is untouched
    /// -- pops are per-path).
    pub fn pop_handling_to(&mut self, target: usize) {
        for _ in target..self.handling_depth {
            self.call("zeo_rt_handling_pop", &[]);
        }
    }

    /// A loud "the M0 slice cannot lower this" error, with the location.
    pub fn unsupported<T>(&self, node: crate::hir::NodeId, what: &str) -> Result<T, String> {
        let at = self
            .location(node)
            .map(|(f, l)| format!(" ({f}:{l})"))
            .unwrap_or_default();
        Err(format!("the CLIF backend cannot lower {what} yet{at}"))
    }
}
