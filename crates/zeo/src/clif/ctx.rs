//! Per-function lowering context: the function builder, the one landing
//! block, local slots, temp-slot recycling, loop targets, and the lazy
//! import/rodata/symbol plumbing every lowering reaches through.

use super::module::Emitter;
use crate::analyze::Analyzed;
use crate::codegen_error::CResult;
use cranelift_codegen::cursor::{Cursor, FuncCursor};
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
    /// Where the ITERATION's block value goes, for a fused loop whose
    /// accumulator consumes it (`arr.count { .. }`): the body's tail and
    /// every `next v` MOVE the value here, and the latch consumes it.
    /// `None` = the value is discarded, every plain loop's rule.
    pub next_value: Option<ir::Value>,
}

/// The lexical class chain a snippet's bare constant searches -- CRuby's
/// cref list, innermost first, with the top level left off (every search
/// ends there anyway).
///
/// The fresh compiler a snippet is lowered by has no entry for any of these
/// classes: they were minted while the program ran. Class ids are the one
/// thing both sides always agreed on, so the ids travel as immediates and
/// every static fold stands down beside them.
pub(crate) struct EvalCref {
    /// The chain, innermost first. EMPTY for an `instance_eval` on a class:
    /// its cref is the singleton, which owns no constants at all -- the
    /// miss IS the answer, and `name` is what CRuby qualifies it with.
    pub chain: Vec<u32>,
    /// How a `NameError` raised in this scope spells the miss.
    pub name: String,
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
    /// The next number a SYNTHETIC local key takes (`r#blk3`, `#acc4`). A
    /// monotone count, not `locals.len()`: two spliced loops in one scope
    /// can reach the same map size, and the second's key then replaced the
    /// first's -- which left the first's slot with no epilogue release.
    pub synthetic_locals: usize,
    /// The names a `binding` taken in THIS scope reports, when the scope
    /// takes one (`analyze::captures::binding_scope_names`). Its presence is
    /// what promoted those locals to cells, which is the only storage a
    /// binding can share. `None` in a scope that never mentions `binding`.
    pub binding_names: Option<std::rc::Rc<Vec<String>>>,
    frefs: HashMap<&'static str, ir::FuncRef>,
    temp_free: Vec<ir::StackSlot>,
    temp_taken: Vec<ir::StackSlot>,
    pub loops: Vec<LoopCtl>,
    /// The node a condition site is lowering RIGHT NOW (`lower_condition`).
    /// Consumed only by the operator fast path: a comparison that IS this
    /// node answers its condition bit directly instead of boxing a Bool.
    /// Keyed by id so routing stays in `lower_expr` and nothing can drift.
    pub branch_cond: Option<crate::hir::NodeId>,
    /// The thread's `FrameHot` header address (`zeo_rt_frame_hot`), fetched
    /// once in the prologue by `frames::fetch_frame_hot`; `None` in a
    /// function whose prologue pushed no frame -- the inline frame
    /// sequences then fall back to their capi calls (a mid-function fetch
    /// would not dominate its other users).
    pub frame_hot: Option<ir::Value>,
    /// The gates word's global value, declared at most once per function.
    /// See [`Fx::gates_base`].
    pub gates_gv: Option<ir::GlobalValue>,
    /// The id-translation table's GV, cached like `gates_gv`. Packaged id
    /// mode only.
    pub cids_gv: Option<ir::GlobalValue>,
    /// The reveal-group base cell's GV. Package emission only.
    pub unit_base_gv: Option<ir::GlobalValue>,
    /// [`Fx::gates_base`]'s twin for the patched-class bitmap.
    pub patched_bits_gv: Option<ir::GlobalValue>,
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
    /// The class this body's `def` was WRITTEN
    /// in -- where a `super` walk resumes from. A module
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
    /// at run time and a `super` reads it off the method-frame stack.
    pub runtime_method_body: bool,
    /// ...and it came from a literal `define_method`, where ruby refuses
    /// a BARE `super` at dispatch: a block-shaped body has no parameter
    /// list to forward from, so ruby raises rather than guessing.
    pub define_method_body: bool,
    /// This scope's `self` is only known at run time -- a runtime-installed
    /// method body, or a block `instance_eval`/`instance_exec` re-homes. The
    /// class a `protected` check compares against is then whatever `self`
    /// turns out to be, not the lexically enclosing class.
    pub self_is_dynamic: bool,
    /// Lowering a run-time `eval` snippet whose constants resolve against
    /// classes only the RUN TIME knows (`zeo::eval`) -- see [`EvalCref`].
    pub eval_cref: Option<std::rc::Rc<EvalCref>>,
    /// Lowering a run-time `eval` snippet: which surface invoked it
    /// (`zeo_rt::eval::EvalMode` as a byte). A `def` inside one installs where
    /// the RUN TIME says, because one snippet may be evaluated against any
    /// number of receivers.
    pub eval_mode: Option<u8>,
    /// What every flip-flop id in this function is offset by -- zero in a
    /// program, a reserved base in an `eval` snippet (see `EvalSpec`).
    pub flip_flop_base: u32,
    /// The base activation slot id this snippet's `using` sites reserved --
    /// global, like a flip-flop latch's, because a snippet's own compiler
    /// numbers from zero (`zeo_rt::eval::reserve_using_slots`).
    pub using_base: u32,
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
    /// Inside a block fn: the BODY head `redo` jumps to. Past the bindings,
    /// because ruby does not re-yield -- the parameters and block-locals
    /// keep whatever the body assigned to them.
    pub block_redo: Option<ir::Block>,
    /// The function's entry block. Every slot's initialization is emitted
    /// here by [`Fx::drain_slot_inits`], never where the slot was created.
    entry: ir::Block,
    /// Value slots owing their three nil words. See [`Fx::new_value_slot`].
    deferred_zero: Vec<ir::StackSlot>,
    /// Cell-pointer slots owing their null. See [`Fx::new_cell_slot`].
    deferred_null: Vec<ir::StackSlot>,
}

impl<'e, 'f> Fx<'e, 'f> {
    pub fn new(
        em: &'e mut Emitter,
        an: &'e Analyzed,
        mut b: FunctionBuilder<'f>,
        rodata_base_of: impl FnOnce(&mut Emitter, &mut FunctionBuilder<'f>) -> (ir::Value, ir::Value),
    ) -> Fx<'e, 'f> {
        let (rodata_base, syms_base) = rodata_base_of(em, &mut b);
        // Every caller creates the entry block inside `rodata_base_of` and
        // leaves it current, which is what makes it reachable here.
        let entry = b
            .current_block()
            .expect("rodata_base_of leaves the entry block current");
        let land = b.create_block();
        Fx {
            em,
            an,
            b,
            land,
            rodata_base,
            syms_base,
            locals: std::collections::BTreeMap::new(),
            synthetic_locals: 0,
            frefs: HashMap::new(),
            temp_free: Vec::new(),
            temp_taken: Vec::new(),
            loops: Vec::new(),
            branch_cond: None,
            frame_hot: None,
            gates_gv: None,
            cids_gv: None,
            unit_base_gv: None,
            patched_bits_gv: None,
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
            eval_cref: None,
            flip_flop_base: 0,
            using_base: 0,
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
            entry,
            deferred_zero: Vec::new(),
            deferred_null: Vec::new(),
        }
    }

    /// The in-function reference for capi import `name`.
    pub fn fref(&mut self, name: &'static str) -> ir::FuncRef {
        if let Some(&f) = self.frefs.get(name) {
            return f;
        }
        let id = self.em.import(name);
        let f = self.em.module.declare_func_in_func(id, self.b.func);
        // On the OBJECT path a capi call is direct (`bl` + linker
        // relocation, with the linker's own range veneers as the escape):
        // the import-linkage default went through the GOT -- an adrp+ldr
        // pair per call on every hot path. The JIT keeps the default; its
        // symbols resolve at runtime lookup, not by a static linker.
        if matches!(self.em.module, super::module::ClifModule::Object(_)) {
            self.b.func.dfg.ext_funcs[f].colocated = true;
        }
        self.frefs.insert(name, f);
        f
    }

    /// Call capi `name`; the single result when it has one.
    pub fn call(&mut self, name: &'static str, args: &[ir::Value]) -> Option<ir::Value> {
        let f = self.fref(name);
        let inst = self.b.ins().call(f, args);
        self.b.func.dfg.inst_results(inst).first().copied()
    }

    /// [`call`](Self::call) for a capi that returns a value (the status
    /// protocol and friends). A returning import with no result is a
    /// compiler bug -- the panic derives its message from `name`, so no
    /// call site hand-writes the symbol twice.
    pub fn call_status(&mut self, name: &'static str, args: &[ir::Value]) -> ir::Value {
        self.call(name, args)
            .unwrap_or_else(|| panic!("{name} returns a value"))
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
    /// it is a cheap `adrp`/`add`
    /// pair, and hoisting it would need the value to dominate every
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

    /// A fresh constant-read cache slot in `zeo_const_sites`, as a pointer.
    /// One per SITE, like [`Fx::callsite_ptr`]; the slot carries no
    /// per-site constant (the resolve chain rides in the call's own
    /// arguments).
    pub fn const_site_ptr(&mut self) -> ir::Value {
        let idx = self.em.const_sites;
        self.em.const_sites += 1;
        let gv = self
            .em
            .module
            .declare_data_in_func(self.em.const_sites_id, self.b.func);
        let base = self.b.ins().symbol_value(self.em.ptr, gv);
        let off = (idx * zeo_abi::abi::CONST_SITE_SIZE) as i64;
        if off == 0 {
            base
        } else {
            self.b.ins().iadd_imm_u(base, off)
        }
    }

    /// A fresh compiled-construction cache slot in `zeo_new_sites`, as a
    /// pointer. One per SITE, like [`Fx::callsite_ptr`]; the slot carries
    /// no per-site constant (the class id rides in the call's arguments).
    pub fn new_site_ptr(&mut self) -> ir::Value {
        let idx = self.em.new_sites;
        self.em.new_sites += 1;
        let gv = self
            .em
            .module
            .declare_data_in_func(self.em.new_sites_id, self.b.func);
        let base = self.b.ins().symbol_value(self.em.ptr, gv);
        let off = (idx * zeo_abi::abi::NEW_SITE_SIZE) as i64;
        if off == 0 {
            base
        } else {
            self.b.ins().iadd_imm_u(base, off)
        }
    }

    /// A fresh dynamic-caller cache slot in `zeo_dyn_sites`, as a
    /// pointer. One per SITE, like [`Fx::callsite_ptr`]; the slot carries
    /// no per-site constant (the caller rides in the call's arguments).
    pub fn dyn_site_ptr(&mut self) -> ir::Value {
        let idx = self.em.dyn_sites;
        self.em.dyn_sites += 1;
        let gv = self
            .em
            .module
            .declare_data_in_func(self.em.dyn_sites_id, self.b.func);
        let base = self.b.ins().symbol_value(self.em.ptr, gv);
        let off = (idx * zeo_abi::abi::DYNCALLER_SITE_SIZE) as i64;
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

    /// The gates word's address. Three sites test gate bits -- the frame
    /// sequences and the two dispatch fast paths -- and each declared the
    /// data in the function again, so one body carried three global values
    /// for one symbol and materialized its address three times.
    ///
    /// Only the DECLARATION is shared. Each site still emits its own
    /// `symbol_value` (so the value dominates its own use) and its own
    /// load: the gates word is a live latch, and a body that reads it once
    /// could pair an inline push with a traced pop.
    pub fn gates_base(&mut self) -> ir::Value {
        use cranelift_codegen::ir::InstBuilder;
        use cranelift_module::Module;
        let gv = match self.gates_gv {
            Some(gv) => gv,
            None => {
                let gv = self.em.module.declare_data_in_func(self.em.gates_id, self.b.func);
                self.gates_gv = Some(gv);
                gv
            }
        };
        let ptr = self.em.ptr;
        self.b.ins().symbol_value(ptr, gv)
    }

    /// The ONE place a compile-time class id becomes a machine value.
    ///
    /// `Immediate` mode emits the id as an iconst -- today's whole-program
    /// compile, byte for byte. `Packaged` mode reads an id in this
    /// object's own band from the id-translation table the HOST fills at
    /// link time, which is what makes a package object correct in any
    /// program. Sentinels (`u32::MAX`) and fixed builtin ids pass through
    /// as immediates in both modes, so any class-id-shaped value may
    /// route here. The load is readonly-flagged: the table is initialized
    /// data, so Cranelift may hoist and dedupe it freely.
    pub fn cid_value(&mut self, cid: u32) -> ir::Value {
        use cranelift_codegen::ir::{InstBuilder, MemFlagsData};
        if let super::module::IdMode::Packaged { first } = self.em.id_mode
            && cid >= first
            && cid != u32::MAX
        {
            let base = self.cids_base();
            let off = ((cid - first) * 4) as i32;
            let fl = MemFlagsData::trusted().with_readonly();
            return self.b.ins().load(ir::types::I32, fl, base, off);
        }
        self.b.ins().iconst(ir::types::I32, i64::from(cid))
    }

    /// The id-translation table's base address, GV-cached like
    /// [`Fx::gates_base`].
    fn cids_base(&mut self) -> ir::Value {
        use cranelift_codegen::ir::InstBuilder;
        use cranelift_module::Module;
        let gv = match self.cids_gv {
            Some(gv) => gv,
            None => {
                let id = self.em.cids_data_id();
                let gv = self.em.module.declare_data_in_func(id, self.b.func);
                self.cids_gv = Some(gv);
                gv
            }
        };
        let ptr = self.em.ptr;
        self.b.ins().symbol_value(ptr, gv)
    }

    /// A reveal-group id as a machine value. An ordinary compile bakes
    /// `unit_base + local` (packages merged into it own `[0, unit_base)`);
    /// a PACKAGE reads its base from the `{prefix}_unit_base` cell the
    /// host fills, plus the local offset -- its reveal ids are as
    /// position-independent as its class ids.
    pub fn reveal_group_value(&mut self, local: u32) -> ir::Value {
        use cranelift_codegen::ir::{InstBuilder, MemFlagsData};
        use cranelift_module::Module;
        if self.em.pkg.is_none() {
            return self
                .b
                .ins()
                .iconst(ir::types::I32, i64::from(self.em.unit_base + local));
        }
        let gv = match self.unit_base_gv {
            Some(gv) => gv,
            None => {
                let id = self.em.unit_base_data_id();
                let gv = self.em.module.declare_data_in_func(id, self.b.func);
                self.unit_base_gv = Some(gv);
                gv
            }
        };
        let ptr = self.em.ptr;
        let addr = self.b.ins().symbol_value(ptr, gv);
        let fl = MemFlagsData::trusted().with_readonly();
        let base = self.b.ins().load(ir::types::I32, fl, addr, 0);
        if local == 0 {
            return base;
        }
        self.b.ins().iadd_imm_u(base, i64::from(local))
    }

    /// The patched-class bitmap's base address, on [`Fx::gates_base`]'s
    /// declare-once/materialize-per-site rule.
    pub fn patched_bits_base(&mut self) -> ir::Value {
        use cranelift_codegen::ir::InstBuilder;
        use cranelift_module::Module;
        let gv = match self.patched_bits_gv {
            Some(gv) => gv,
            None => {
                let gv = self
                    .em
                    .module
                    .declare_data_in_func(self.em.patched_bits_id, self.b.func);
                self.patched_bits_gv = Some(gv);
                gv
            }
        };
        let ptr = self.em.ptr;
        self.b.ins().symbol_value(ptr, gv)
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

    /// The interruption checkpoint: an inline load of the runtime's
    /// pending counter, with the delivery call on a cold branch. A plain
    /// load matches the Rust fast path's Relaxed read -- a checkpoint that
    /// races a post sees it at the next checkpoint, same as the called
    /// form did.
    pub fn check_ints(&mut self) {
        let gv = self
            .em
            .module
            .declare_data_in_func(self.em.pending_id, self.b.func);
        let base = self.b.ins().symbol_value(self.em.ptr, gv);
        let pending = self
            .b
            .ins()
            .load(types::I32, MemFlagsData::trusted(), base, 0);
        let cold = self.b.create_block();
        let cont = self.b.create_block();
        self.b.set_cold_block(cold);
        self.b.ins().brif(pending, cold, &[], cont, &[]);
        self.b.switch_to_block(cold);
        let status = self.call_status("zeo_rt_check_ints", &[]);
        self.b.ins().brif(status, self.land, &[], cont, &[]);
        self.b.switch_to_block(cont);
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
        crate::analyze::source::source_location(&self.an.compiler, node)
    }

    /// A fresh nil-initialized value slot.
    ///
    /// The three nil words are NOT emitted here. They are deferred to the
    /// entry block, because a caller may be building a block that only one
    /// path reaches -- a fused loop's inline arm behind its guards -- while
    /// the epilogue and the landing release every slot in `locals`
    /// unconditionally. A slot initialized where it was created is then
    /// released uninitialised on every path that skipped that block, and
    /// `zeo_rt_release` runs `drop_in_place` over whatever the stack held.
    pub fn new_value_slot(&mut self) -> ir::StackSlot {
        let ss = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            VALUE_SIZE,
            3,
        ));
        self.deferred_zero.push(ss);
        ss
    }

    /// A fresh cell-pointer slot, nulled in the entry block.
    ///
    /// Null is the "no cell here" sentinel `zeo_rt_cell_release` skips, for
    /// the same reason [`Fx::new_value_slot`] defers its nil words: the
    /// creating block may not run.
    pub fn new_cell_slot(&mut self) -> ir::StackSlot {
        let ss =
            self.b
                .create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
        self.deferred_null.push(ss);
        ss
    }

    /// Emit every slot's initialization at the top of the entry block. Runs
    /// once, immediately before the builder is finalized, so that a slot
    /// created anywhere in the function is initialised on every path.
    ///
    /// Asserts the invariant the epilogue depends on: every slot in `locals`
    /// was created through [`Fx::new_value_slot`] or [`Fx::new_cell_slot`],
    /// so `release_locals` -- which walks `locals` unconditionally from both
    /// the normal exit and the landing -- can never be handed one that no
    /// path initialised.
    pub fn drain_slot_inits(&mut self) {
        let zero = std::mem::take(&mut self.deferred_zero);
        let null = std::mem::take(&mut self.deferred_null);
        #[cfg(debug_assertions)]
        {
            let init: std::collections::HashSet<ir::StackSlot> =
                zero.iter().chain(null.iter()).copied().collect();
            for (name, l) in &self.locals {
                let ss = match l {
                    Local::Slot(ss) | Local::Cell { ss, .. } => *ss,
                };
                assert!(
                    init.contains(&ss),
                    "ICE: local `{name}`'s slot is released by the epilogue but was \
                     not created through new_value_slot/new_cell_slot, so no path \
                     initialises it"
                );
            }
        }
        if zero.is_empty() && null.is_empty() {
            return;
        }
        let ptr = self.em.ptr;
        let mut c = FuncCursor::new(self.b.func).at_first_insertion_point(self.entry);
        let z = c.ins().iconst(types::I64, 0);
        for ss in zero {
            let dst = c.ins().stack_addr(ptr, ss, 0);
            for off in [0, 8, 16] {
                c.ins().store(MemFlagsData::trusted(), z, dst, off);
            }
        }
        if !null.is_empty() {
            let n = c.ins().iconst(ptr, 0);
            for ss in null {
                let dst = c.ins().stack_addr(ptr, ss, 0);
                c.ins().store(MemFlagsData::trusted(), n, dst, 0);
            }
        }
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

    /// A loud "a package cannot lower this" error; the location travels
    /// as the node's span.
    pub fn unsupported<T>(&self, node: crate::hir::NodeId, what: &str) -> CResult<T> {
        Err(crate::codegen_error::CodegenError::unsupported(
            format!("the CLIF backend cannot lower {what} yet"),
            self.an.compiler.hir.span(node),
        ))
    }
}
