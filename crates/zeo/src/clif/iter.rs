//! Iterator fusion: the LITERAL `n.times { |i| }` and `(a..b).each { |i| }`
//! shapes (`analyze::fastpath`'s predicates -- a literal receiver cannot be
//! redefined at run time, so no gate and no dynamic fallback, exactly the
//! rustc emitter's rule). The typed `InlineIterKind` fusions (`ArrayEach`
//! and friends) need the dynamic-fallback arm's real `Proc`, so they ride
//! in with the blocks milestone (M0-14).

use super::ctx::{Fx, LoopCtl};
use super::ownership;
use crate::hir::{HirNode, NodeId};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, types};
use zeo_abi::abi::{PAYLOAD_OFFSET, ValueTag};

/// The two fused shapes' bounds.
pub(crate) enum Counted {
    /// `n.times`: 0 up to (exclusive) n; the loop's value is `n`.
    Times { n: i64 },
    /// `(a..b).each` / `(a...b).each`: a up to b; the loop's value is the
    /// range, which the slice only supports in DISCARDED position.
    Range {
        start: i64,
        end: i64,
        exclusive: bool,
    },
}

/// Lower one fused counted loop. `result` = the loop's value slot when in
/// value position (`None` = statement position, value discarded).
pub(crate) fn lower_counted(
    fx: &mut Fx,
    site: NodeId,
    counted: &Counted,
    block: NodeId,
    result: Option<ir::Value>,
) -> Result<(), String> {
    let HirNode::Block { params, body } = &fx.an.compiler.hir[block] else {
        return fx.unsupported(site, "a non-literal block");
    };
    if !(params.destructures.is_empty()
        && params.optional.is_empty()
        && params.rest.is_none()
        && !params.implicit_rest
        && params.post.is_empty()
        && params.keywords.is_empty()
        && params.keyword_rest.is_none()
        && params.block.is_none()
        && params.required.len() <= 1)
    {
        return fx.unsupported(site, "this block's parameter shape");
    }
    let param = params.required.first().cloned();
    // A name first-assigned INSIDE the block is fresh on every invocation
    // in ruby. This splice shares the enclosing scope, where the name was
    // hoisted once, so each iteration resets it -- a conditional first
    // assignment (`x = v if cond`) must not carry into the next.
    let implicit_locals = params.implicit_block_locals.clone();
    // `|i; n|`: names the block DECLARES as its own. They shadow any
    // enclosing local of the same name and rebind fresh per iteration,
    // exactly like the implicit ones.
    let block_locals = params.block_locals.clone();
    // What an ESCAPING closure inside the body captures. The splice has no
    // frame of its own, so those names cannot live in a plain slot: ruby
    // binds a block parameter per invocation, and two closures built in two
    // iterations must not share one storage. They get a cell each iteration
    // instead -- rustc's "cell-wrapped fresh" splice.
    let escaping = crate::analyze::captures::collect_escaping_captures(
        &fx.an.compiler,
        body,
        params,
        crate::analyze::class_query::SelfClass::new(fx.method_class, None),
    )
    .locals;
    let per_iteration_cells: Vec<String> = param
        .iter()
        .chain(implicit_locals.iter())
        .chain(block_locals.iter())
        .filter(|n| escaping.contains(*n))
        .cloned()
        .collect();
    let body = body.clone();

    let (start, end, end_cc) = match *counted {
        Counted::Times { n } => (0, n, IntCC::SignedGreaterThanOrEqual),
        Counted::Range {
            start,
            end,
            exclusive,
        } => (
            start,
            end,
            if exclusive {
                IntCC::SignedGreaterThanOrEqual
            } else {
                IntCC::SignedGreaterThan
            },
        ),
    };

    // The block parameter SHADOWS any enclosing local of the same name
    // (rustc splices a fresh `let`); the shadow registers under a synthetic
    // key so the epilogue/landing releases it, and the visible name maps to
    // it only for the loop's extent. A name a closure escapes with is a cell
    // (replaced per iteration below); everything else is a plain slot, which
    // an escaping block may not capture.
    let mut restore: Vec<(String, Option<super::ctx::Local>)> = Vec::new();
    let bind_shadow =
        |fx: &mut Fx, restore: &mut Vec<(String, Option<super::ctx::Local>)>, name: &str| {
            let local = match per_iteration_cells.iter().any(|n| n == name) {
                true => new_cell_local(fx),
                false => super::ctx::Local::Slot(fx.new_value_slot()),
            };
            let key = format!("{name}#blk{}", fx.locals.len());
            fx.locals.insert(key, local);
            let old = fx.locals.insert(name.to_string(), local);
            if matches!(local, super::ctx::Local::Slot(_)) {
                fx.shadowed.insert(name.to_string());
            }
            restore.push((name.to_string(), old));
        };
    if let Some(name) = param.clone() {
        bind_shadow(fx, &mut restore, &name);
    }
    for name in implicit_locals.iter().chain(block_locals.iter()) {
        if per_iteration_cells.iter().any(|n| n == name) || block_locals.contains(name) {
            bind_shadow(fx, &mut restore, name);
        }
    }

    let counter =
        fx.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let mark = fx
        .call("zeo_rt_pool_mark", &[])
        .expect("pool_mark returns the watermark");
    let fl = MemFlagsData::trusted();
    let start_v = fx.b.ins().iconst(types::I64, start);
    let counter_addr = fx.slot_addr(counter, 0);
    fx.b.ins().store(fl, start_v, counter_addr, 0);

    let head = fx.b.create_block();
    let body_blk = fx.b.create_block();
    let latch = fx.b.create_block();
    let exit_normal = fx.b.create_block();
    let exit = fx.b.create_block();
    fx.b.ins().jump(head, &[]);

    fx.b.switch_to_block(head);
    let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
    let done = fx.b.ins().icmp_imm_s(end_cc, c, end);
    fx.b.ins().brif(done, exit_normal, &[], body_blk, &[]);

    fx.b.switch_to_block(body_blk);
    let status = fx
        .call("zeo_rt_check_ints", &[])
        .expect("check_ints returns a status");
    fx.fallible(status);
    // Each iteration binds fresh storage for every escaping name, so a
    // closure built last time keeps the value it captured.
    for name in &per_iteration_cells {
        replace_cell(fx, name);
    }
    if let Some(name) = &param {
        let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
        ownership::write_local(fx, name, &super::operand::Operand::Int(c));
    }
    for name in implicit_locals.iter().chain(block_locals.iter()) {
        // A cell was just replaced; a plain slot resets to nil.
        if matches!(fx.locals.get(name), Some(super::ctx::Local::Slot(_))) {
            ownership::write_local(fx, name, &super::operand::Operand::Nil);
        }
    }
    fx.loops.push(LoopCtl {
        exit,
        latch,
        body: body_blk,
        result,
        depth: fx.ensure_depth,
        handling: fx.handling_depth,
    });
    super::stmt::lower_stmts(fx, &body)?;
    fx.loops.pop();
    fx.b.ins().jump(latch, &[]);

    fx.b.switch_to_block(latch);
    let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
    let c1 = fx.b.ins().iadd_imm_s(c, 1);
    fx.b.ins().store(fl, c1, counter_addr, 0);
    fx.call("zeo_rt_pool_reset", &[mark]);
    fx.b.ins().jump(head, &[]);

    fx.b.switch_to_block(exit_normal);
    if let Some(dst) = result {
        // The loop's value is its RECEIVER: the count for `times`, the range
        // itself for a range-`each`. The range is rebuilt here from the same
        // literal endpoints the bounds came from -- the receiver was a
        // literal, which is what made the fusion legal in the first place.
        match *counted {
            Counted::Times { n } => {
                let n_v = fx.b.ins().iconst(types::I64, n);
                let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
                fx.b.ins().store(fl, tag, dst, 0);
                fx.b.ins().store(fl, n_v, dst, PAYLOAD_OFFSET as i32);
            }
            Counted::Range {
                start,
                end,
                exclusive,
            } => {
                let int_slot = |fx: &mut Fx, v: i64| {
                    let ss = fx.b.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        zeo_abi::abi::VALUE_SIZE as u32,
                        3,
                    ));
                    let addr = fx.slot_addr(ss, 0);
                    let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
                    let n = fx.b.ins().iconst(types::I64, v);
                    fx.b.ins().store(fl, tag, addr, 0);
                    fx.b.ins().store(fl, n, addr, PAYLOAD_OFFSET as i32);
                    addr
                };
                let a = int_slot(fx, start);
                let b = int_slot(fx, end);
                let excl = fx.b.ins().iconst(types::I8, i64::from(exclusive));
                let status = fx
                    .call("zeo_rt_range_new", &[a, b, excl, dst])
                    .expect("range_new returns a status");
                fx.fallible(status);
            }
        }
    }
    fx.b.ins().jump(exit, &[]);

    fx.b.switch_to_block(exit);
    fx.call("zeo_rt_pool_reset", &[mark]);
    // Lexical shadowing ends with the loop.
    for (name, old) in restore {
        fx.shadowed.remove(&name);
        match old {
            Some(prev) => {
                fx.locals.insert(name, prev);
            }
            None => {
                fx.locals.remove(&name);
            }
        }
    }
    Ok(())
}

/// The fused shape's bounds when `site` is a literal `times`/range-`each`
/// call (the `analyze::fastpath` predicates), else `None`.
pub(crate) fn counted_of(
    fx: &Fx,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
) -> Option<Counted> {
    let compiler = &fx.an.compiler;
    if crate::analyze::fastpath::is_times_fast_path(compiler, receiver, name, kwargs_empty) {
        let HirNode::IntegerLit(n) = compiler.hir[receiver.expect("guarded")] else {
            unreachable!("is_times_fast_path proved a literal receiver");
        };
        return Some(Counted::Times { n });
    }
    if crate::analyze::fastpath::is_range_each_fast_path(compiler, receiver, name, kwargs_empty) {
        let HirNode::RangeLit {
            start: Some(s),
            end: Some(e),
            exclusive,
        } = compiler.hir[receiver.expect("guarded")]
        else {
            unreachable!("is_range_each_fast_path proved a literal range");
        };
        let (HirNode::IntegerLit(start), HirNode::IntegerLit(end)) =
            (&compiler.hir[s], &compiler.hir[e])
        else {
            unreachable!("is_range_each_fast_path proved literal bounds");
        };
        return Some(Counted::Range {
            start: *start,
            end: *end,
            exclusive,
        });
    }
    None
}

/// A fresh owned cell in a slot of its own, ready to hold one binding.
fn new_cell_local(fx: &mut Fx) -> super::ctx::Local {
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let cellp = fx
        .call("zeo_rt_cell_new", &[null])
        .expect("cell_new returns the cell");
    let ss =
        fx.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let dst = fx.slot_addr(ss, 0);
    fx.b.ins().store(MemFlagsData::trusted(), cellp, dst, 0);
    super::ctx::Local::Cell { ss, owned: true }
}

/// Point `name`'s cell slot at a NEW cell and drop this scope's reference to
/// the old one -- a closure that captured it holds its own. The slot always
/// holds a live cell, so the epilogue's release needs no guard.
fn replace_cell(fx: &mut Fx, name: &str) {
    let Some(super::ctx::Local::Cell { ss, .. }) = fx.locals.get(name).copied() else {
        unreachable!("an escaping name was bound as a cell");
    };
    let old = fx.cell_ptr(ss);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let fresh = fx
        .call("zeo_rt_cell_new", &[null])
        .expect("cell_new returns the cell");
    let dst = fx.slot_addr(ss, 0);
    fx.b.ins().store(MemFlagsData::trusted(), fresh, dst, 0);
    fx.call("zeo_rt_cell_release", &[old]);
}
