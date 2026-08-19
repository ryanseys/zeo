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
    frefs: HashMap<&'static str, ir::FuncRef>,
    temp_free: Vec<ir::StackSlot>,
    temp_taken: Vec<ir::StackSlot>,
    pub loops: Vec<LoopCtl>,
    pub prev_line: Option<u32>,
    /// The current `self` as a borrowed pointer: the method's first
    /// parameter, or the toplevel's pooled `main_object` copy.
    pub self_ptr: Option<ir::Value>,
    /// The enclosing class when lowering a method body -- what ivar slot
    /// resolution keys on.
    pub method_class: Option<zeo_abi::ClassId>,
    /// A method body's `(out, ret_ok)`: `return` writes the value and
    /// jumps; `None` at the toplevel.
    pub ret: Option<(ir::Value, ir::Block)>,
    /// `retry` targets (a rescue clause's begin head), with their ensure
    /// and handling depths.
    pub retries: Vec<(ir::Block, usize, usize)>,
    /// How many `ensure` bodies enclose the current lowering point -- a
    /// DIRECT jump (`break`/`next`/`redo`/`return`/`retry`) may not cross
    /// one (it would skip the ensure); such jumps refuse until the
    /// jump-through-ensure machinery lands.
    pub ensure_depth: usize,
    /// How many `$!` (`handling_push`) entries the current lexical point
    /// sits under -- a direct jump pops down to its target's depth.
    pub handling_depth: usize,
    /// The enclosing frame's label ("<main>", "Object#fib") -- what a
    /// block's own frame derives its "block in ..." label from.
    pub frame_label: String,
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
    pub owned_created: usize,
    pub owned_consumed: usize,
    /// The enclosing `def` carries the `ruby2_keywords` directive: a splat
    /// forwarding its `*rest` keeps the keyword mark on a trailing hash.
    pub ruby2_keywords: bool,
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
            self_ptr: None,
            method_class: None,
            ret: None,
            retries: Vec::new(),
            ensure_depth: 0,
            handling_depth: 0,
            frame_label: String::new(),
            blk_ptr: None,
            block_next: None,
            shadowed: std::collections::HashSet::new(),
            owned_created: 0,
            owned_consumed: 0,
            ruby2_keywords: false,
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
        Err(format!(
            "--backend aot is an M0 vertical slice: cannot lower {what} yet{at}"
        ))
    }
}
