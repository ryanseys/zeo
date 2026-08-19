//! Statement lowering for the M0 slice: assignments, `puts`, `if`,
//! `while`/`until`/`loop`, `break`/`next`/`redo`, and expression
//! statements. Everything else refuses loudly with its source location.

use super::ctx::{Fx, LoopCtl, VALUE_SIZE};
use super::expr::lower_expr;
use super::ownership;
use crate::hir::{ArrayElem, HirNode, NodeId};
use cranelift_codegen::ir::{InstBuilder, StackSlotData, StackSlotKind, types};

pub(crate) fn lower_stmts(fx: &mut Fx, stmts: &[NodeId]) -> Result<(), String> {
    for &stmt in stmts {
        lower_stmt(fx, stmt)?;
        fx.end_stmt();
    }
    Ok(())
}

#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: the refusal arm IS the default -- an unlisted node kind must refuse loudly, which is exactly what a new HirNode should do here until its lowering lands"
)]
fn lower_stmt(fx: &mut Fx, stmt: NodeId) -> Result<(), String> {
    stamp_line(fx, stmt);
    match &fx.an.compiler.hir[stmt] {
        HirNode::LocalWrite(name, value) => {
            let name = name.clone();
            let value = *value;
            let op = lower_expr(fx, value)?;
            let &ss = fx
                .locals
                .get(&name)
                .unwrap_or_else(|| panic!("local `{name}` must be hoisted"));
            let dst = fx.slot_addr(ss, 0);
            ownership::write_assign(fx, &op, dst);
            Ok(())
        }
        HirNode::Call {
            receiver: None,
            name,
            args,
            kwargs,
            block: None,
            block_arg: None,
            safe: false,
        } if name == "puts" && kwargs.is_empty() => {
            let args = args.clone();
            lower_puts(fx, stmt, &args)
        }
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
            let c = lower_expr(fx, cond)?;
            let t = ownership::truthy(fx, c);
            let b_then = fx.b.create_block();
            let b_else = fx.b.create_block();
            let join = fx.b.create_block();
            fx.b.ins().brif(t, b_then, &[], b_else, &[]);
            fx.b.switch_to_block(b_then);
            lower_stmts(fx, &then_body)?;
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(b_else);
            lower_stmts(fx, &else_body)?;
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(join);
            Ok(())
        }
        HirNode::While {
            cond,
            body,
            negate,
            post,
        } => {
            let (cond, body, negate, post) = (*cond, body.clone(), *negate, *post);
            lower_loop(fx, Some((cond, negate)), &body, post)
        }
        HirNode::Loop { body } => {
            let body = body.clone();
            lower_loop(fx, None, &body, false)
        }
        HirNode::Break(value) => {
            let value = *value;
            if let Some(v) = value {
                let op = lower_expr(fx, v)?;
                ownership::discard(fx, op);
            }
            let Some(ctl) = fx.loops.last() else {
                return fx.unsupported(stmt, "`break` outside a loop");
            };
            let exit = ctl.exit;
            fx.b.ins().jump(exit, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Next(value) => {
            let value = *value;
            if let Some(v) = value {
                let op = lower_expr(fx, v)?;
                ownership::discard(fx, op);
            }
            let Some(ctl) = fx.loops.last() else {
                return fx.unsupported(stmt, "`next` outside a loop");
            };
            let latch = ctl.latch;
            fx.b.ins().jump(latch, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Redo => {
            let Some(ctl) = fx.loops.last() else {
                return fx.unsupported(stmt, "`redo` outside a loop");
            };
            let body = ctl.body;
            fx.b.ins().jump(body, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Seq(stmts) => {
            let stmts = stmts.clone();
            lower_stmts(fx, &stmts)
        }
        // Anything else in statement position: try the expression lowering
        // and discard the value (it refuses on its own for shapes outside
        // the slice).
        HirNode::IntegerLit(..)
        | HirNode::FloatLit(..)
        | HirNode::BoolLit(..)
        | HirNode::NilLit
        | HirNode::StringLit(..)
        | HirNode::LocalRead(..)
        | HirNode::Call { .. } => {
            let op = lower_expr(fx, stmt)?;
            ownership::discard(fx, op);
            Ok(())
        }
        other => {
            let what = format!("this statement ({})", statement_kind(other));
            fx.unsupported(stmt, &what)
        }
    }
}

/// A short label for the refusal message.
#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: a refusal-message label -- every future node kind is correctly 'unsupported' until its lowering lands"
)]
fn statement_kind(node: &HirNode) -> &'static str {
    match node {
        HirNode::ClassDef { .. } => "a class definition",
        HirNode::DefMethod { .. } => "a method definition",
        HirNode::Begin { .. } => "a begin/rescue/ensure",
        _ => "an unsupported node kind",
    }
}

/// `puts` with arbitrary slice-lowerable arguments: a contiguous argv
/// array of borrowed copies (owned temps hand their value to the pool
/// first), then the status-protocol call.
fn lower_puts(fx: &mut Fx, stmt: NodeId, args: &[ArrayElem]) -> Result<(), String> {
    let argc = args.len();
    let argv = (argc > 0).then(|| {
        fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            argc as u32 * VALUE_SIZE,
            3,
        ))
    });
    for (i, arg) in args.iter().enumerate() {
        let ArrayElem::Single(id) = arg else {
            return fx.unsupported(stmt, "a splat argument");
        };
        let op = lower_expr(fx, *id)?;
        if op.owned() {
            let tag = op.tag();
            let addr = ownership::addr_of(fx, &op);
            ownership::pool_owned(fx, addr, tag);
        }
        let argv = argv.expect("argc > 0 here");
        let dst = fx.slot_addr(argv, (i as u32 * VALUE_SIZE) as i32);
        ownership::write_borrow(fx, &op, dst);
    }
    let argv_ptr = match argv {
        Some(ss) => fx.slot_addr(ss, 0),
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    let argc_v = fx.b.ins().iconst(fx.em.ptr, argc as i64);
    let out_ss = fx.temp_slot();
    let out = fx.slot_addr(out_ss, 0);
    let status = fx
        .call("zeo_rt_kernel_puts", &[argv_ptr, argc_v, out])
        .expect("kernel_puts returns a status");
    fx.fallible(status);
    // `puts` answers nil -- an immediate, nothing to release.
    Ok(())
}

/// A native loop: `while`/`until` (pre- or post-test) and bare `loop`.
/// The release pool is bracketed -- marked at entry, reset at the latch
/// and at the exit -- so a long loop never accumulates temps.
fn lower_loop(
    fx: &mut Fx,
    cond: Option<(NodeId, bool)>,
    body: &[NodeId],
    post: bool,
) -> Result<(), String> {
    let mark = fx
        .call("zeo_rt_pool_mark", &[])
        .expect("pool_mark returns the watermark");
    let head = fx.b.create_block();
    let body_blk = fx.b.create_block();
    let latch = fx.b.create_block();
    let exit = fx.b.create_block();
    let first = if post { body_blk } else { head };
    fx.b.ins().jump(first, &[]);

    fx.b.switch_to_block(head);
    match cond {
        Some((cond_id, negate)) => {
            let c = lower_expr(fx, cond_id)?;
            let mut t = ownership::truthy(fx, c);
            if negate {
                // `until`: flip the low bit of the 0/1 truthiness.
                t =
                    fx.b.ins()
                        .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, t, 0);
            }
            fx.b.ins().brif(t, body_blk, &[], exit, &[]);
        }
        None => {
            fx.b.ins().jump(body_blk, &[]);
        }
    }

    fx.b.switch_to_block(body_blk);
    let status = fx
        .call("zeo_rt_check_ints", &[])
        .expect("check_ints status");
    fx.fallible(status);
    fx.loops.push(LoopCtl {
        exit,
        latch,
        body: body_blk,
    });
    lower_stmts(fx, body)?;
    fx.loops.pop();
    fx.b.ins().jump(latch, &[]);

    fx.b.switch_to_block(latch);
    fx.call("zeo_rt_pool_reset", &[mark]);
    fx.b.ins().jump(head, &[]);

    fx.b.switch_to_block(exit);
    fx.call("zeo_rt_pool_reset", &[mark]);
    Ok(())
}

/// Mirror the rustc backend's `stamp_line`: a `set_line` only when the
/// statement's line differs from the previous stamp.
fn stamp_line(fx: &mut Fx, stmt: NodeId) {
    let Some((_, line)) = fx.location(stmt) else {
        return;
    };
    if fx.prev_line == Some(line) {
        return;
    }
    fx.prev_line = Some(line);
    let v = fx.b.ins().iconst(types::I32, i64::from(line));
    fx.call("zeo_rt_set_line", &[v]);
}
