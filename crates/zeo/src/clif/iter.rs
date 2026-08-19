//! Iterator fusion: the LITERAL `n.times { |i| }` and `(a..b).each { |i| }`
//! shapes (`analyze::fastpath`'s predicates -- a literal receiver cannot be
//! redefined at run time, so no gate and no dynamic fallback, exactly the
//! rustc emitter's rule). The typed `InlineIterKind` fusions (`ArrayEach`
//! and friends) need the dynamic-fallback arm's real `Proc`, so they ride
//! in with the blocks milestone (M0-14).

use super::ctx::{Fx, LoopCtl, VALUE_SIZE};
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
        && params.block_locals.is_empty()
        && params.implicit_block_locals.is_empty()
        && params.required.len() <= 1)
    {
        return fx.unsupported(site, "this block's parameter shape");
    }
    if matches!(counted, Counted::Range { .. }) && result.is_some() {
        return fx.unsupported(site, "a fused range-each in value position");
    }
    let param = params.required.first().cloned();
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
    // (rustc splices a fresh `let`); the shadow slot registers under a
    // synthetic key so the epilogue/landing releases it, and the visible
    // name maps to it only for the loop's extent.
    let shadow = param.as_ref().map(|name| {
        let ss = fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            VALUE_SIZE,
            3,
        ));
        let dst = fx.slot_addr(ss, 0);
        let z = fx.b.ins().iconst(types::I64, 0);
        for off in [0, 8, 16] {
            fx.b.ins().store(MemFlagsData::trusted(), z, dst, off);
        }
        let key = format!("{name}#blk{}", fx.locals.len());
        fx.locals.insert(key, ss);
        let old = fx.locals.insert(name.clone(), ss);
        (name.clone(), ss, old)
    });

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
    if let Some((_, ss, _)) = &shadow {
        let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
        let dst = fx.slot_addr(*ss, 0);
        ownership::write_assign(fx, &super::operand::Operand::Int(c), dst);
    }
    fx.loops.push(LoopCtl {
        exit,
        latch,
        body: body_blk,
        result,
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
        // Only `times` reaches here in value position: its value is the
        // receiver.
        let Counted::Times { n } = *counted else {
            unreachable!("range-each in value position refused above");
        };
        let n_v = fx.b.ins().iconst(types::I64, n);
        let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
        fx.b.ins().store(fl, tag, dst, 0);
        fx.b.ins().store(fl, n_v, dst, PAYLOAD_OFFSET as i32);
    }
    fx.b.ins().jump(exit, &[]);

    fx.b.switch_to_block(exit);
    fx.call("zeo_rt_pool_reset", &[mark]);
    // Lexical shadowing ends with the loop.
    if let Some((name, _, old)) = shadow {
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
