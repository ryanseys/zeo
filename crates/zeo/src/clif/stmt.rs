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
        let mark = fx.stmt_mark();
        lower_stmt(fx, stmt)?;
        fx.end_stmt(mark);
    }
    Ok(())
}

/// A body in VALUE position (a method body, an `if`-expression arm): the
/// leading statements run as statements, the tail's value MOVES into the
/// caller's `dst` slot. An empty body is nil.
pub(crate) fn lower_value_body_into(
    fx: &mut Fx,
    stmts: &[NodeId],
    dst: cranelift_codegen::ir::Value,
) -> Result<(), String> {
    let Some((&tail, init)) = stmts.split_last() else {
        ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
        return Ok(());
    };
    for &stmt in init {
        let mark = fx.stmt_mark();
        lower_stmt(fx, stmt)?;
        fx.end_stmt(mark);
    }
    stamp_line(fx, tail);
    let op = lower_tail_expr(fx, tail)?;
    ownership::write_move_into(fx, &op, dst);
    Ok(())
}

/// `@name = value`: the runtime slot write (frozen check inside); the
/// value MOVES in.
fn lower_ivar_write(fx: &mut Fx, site: NodeId, name: &str, value: NodeId) -> Result<(), String> {
    let slot = ivar_slot_of(fx, site, name)?;
    let op = lower_expr(fx, value)?;
    let ptr = ownership::move_ptr(fx, &op);
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
    let status = fx
        .call("zeo_rt_ivar_set_slot", &[self_ptr, slot_v, ptr])
        .expect("ivar_set_slot returns a status");
    fx.fallible(status);
    Ok(())
}

fn ivar_slot_of(fx: &Fx, site: NodeId, name: &str) -> Result<usize, String> {
    let Some(class) = fx.method_class else {
        return fx.unsupported(site, "an ivar outside a compiled method");
    };
    crate::analyze::class_query::slot_of(&fx.an.compiler, class, name)
        .ok_or(())
        .or_else(|()| fx.unsupported(site, "a dynamic (slotless) ivar"))
}

/// A tail position accepts a few statement-shaped nodes whose value Ruby
/// defines: an assignment answers the assigned value, a loop answers nil.
#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: the refusal arm IS the default -- an unlisted node kind must refuse loudly, which is exactly what a new HirNode should do here until its lowering lands"
)]
fn lower_tail_expr(fx: &mut Fx, tail: NodeId) -> Result<super::operand::Operand, String> {
    use super::operand::{Operand, TagInfo};
    match &fx.an.compiler.hir[tail] {
        HirNode::LocalWrite(name, _) => {
            let name = name.clone();
            lower_stmt(fx, tail)?;
            let &ss = fx.locals.get(&name).expect("just assigned");
            let addr = fx.slot_addr(ss, 0);
            Ok(Operand::Ptr {
                addr,
                owned: false,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::IvarWrite(name, value) => {
            let (name, value) = (name.clone(), *value);
            let slot = ivar_slot_of(fx, tail, &name)?;
            lower_ivar_write(fx, tail, &name, value)?;
            let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
            fx.call("zeo_rt_ivar_get_slot", &[self_ptr, slot_v, out]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::While {
            cond,
            body,
            negate,
            post,
        } => {
            let (cond, body, negate, post) = (*cond, body.clone(), *negate, *post);
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            lower_loop(fx, Some((cond, negate)), &body, post, Some(dst))?;
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::Loop { body } => {
            let body = body.clone();
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            lower_loop(fx, None, &body, false, Some(dst))?;
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::Break(..) | HirNode::Next(..) | HirNode::Redo => {
            // The jump leaves this block unreachable; the nil is never read.
            lower_stmt(fx, tail)?;
            Ok(Operand::Nil)
        }
        HirNode::If { .. }
        | HirNode::IntegerLit(..)
        | HirNode::FloatLit(..)
        | HirNode::BoolLit(..)
        | HirNode::NilLit
        | HirNode::StringLit(..)
        | HirNode::LocalRead(..)
        | HirNode::IvarRead(..)
        | HirNode::Or(..)
        | HirNode::And(..)
        | HirNode::ClassRef(..)
        | HirNode::New { .. }
        | HirNode::SelfRef
        | HirNode::Call { .. } => lower_expr(fx, tail),
        other => {
            let what = format!("this tail expression ({})", statement_kind(other));
            fx.unsupported(tail, &what)
        }
    }
}

#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: the refusal arm IS the default -- an unlisted node kind must refuse loudly, which is exactly what a new HirNode should do here until its lowering lands"
)]
pub(crate) fn lower_stmt(fx: &mut Fx, stmt: NodeId) -> Result<(), String> {
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
            lower_loop(fx, Some((cond, negate)), &body, post, None)
        }
        HirNode::Loop { body } => {
            let body = body.clone();
            lower_loop(fx, None, &body, false, None)
        }
        HirNode::Break(value) => {
            let value = *value;
            if fx.loops.is_empty() {
                return fx.unsupported(stmt, "`break` outside a loop");
            }
            let result = fx.loops.last().expect("checked above").result;
            match (value, result) {
                // `break v` in a value-position loop: v IS the loop's value.
                (Some(v), Some(dst)) => {
                    let op = lower_expr(fx, v)?;
                    ownership::write_move_into(fx, &op, dst);
                }
                (Some(v), None) => {
                    let op = lower_expr(fx, v)?;
                    ownership::discard(fx, op);
                }
                (None, Some(dst)) => {
                    ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
                }
                (None, None) => {}
            }
            let exit = fx.loops.last().expect("checked above").exit;
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
        HirNode::IvarWrite(name, value) => {
            let (name, value) = (name.clone(), *value);
            lower_ivar_write(fx, stmt, &name, value)
        }
        HirNode::Return(value) => {
            let value = *value;
            let Some((out, ret_ok)) = fx.ret else {
                return fx.unsupported(stmt, "a top-level `return`");
            };
            match value {
                Some(v) => {
                    let op = lower_expr(fx, v)?;
                    ownership::write_move_into(fx, &op, out);
                }
                None => {
                    ownership::write_move_into(fx, &super::operand::Operand::Nil, out);
                }
            }
            fx.b.ins().jump(ret_ok, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Seq(stmts) => {
            let stmts = stmts.clone();
            lower_stmts(fx, &stmts)
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block: Some(blk),
            block_arg: None,
            safe: false,
        } if args.is_empty() => {
            let (receiver, name, kwargs_empty, blk) =
                (*receiver, name.clone(), kwargs.is_empty(), *blk);
            match super::iter::counted_of(fx, receiver, &name, kwargs_empty) {
                Some(counted) => super::iter::lower_counted(fx, stmt, &counted, blk, None),
                None => fx.unsupported(stmt, "a block argument"),
            }
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
        | HirNode::IvarRead(..)
        | HirNode::Or(..)
        | HirNode::And(..)
        | HirNode::ClassRef(..)
        | HirNode::New { .. }
        | HirNode::SelfRef
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
    result: Option<cranelift_codegen::ir::Value>,
) -> Result<(), String> {
    let mark = fx
        .call("zeo_rt_pool_mark", &[])
        .expect("pool_mark returns the watermark");
    let head = fx.b.create_block();
    let body_blk = fx.b.create_block();
    let latch = fx.b.create_block();
    let exit_normal = fx.b.create_block();
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
            fx.b.ins().brif(t, body_blk, &[], exit_normal, &[]);
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
        result,
    });
    lower_stmts(fx, body)?;
    fx.loops.pop();
    fx.b.ins().jump(latch, &[]);

    fx.b.switch_to_block(latch);
    fx.call("zeo_rt_pool_reset", &[mark]);
    fx.b.ins().jump(head, &[]);

    // Ran-to-completion (condition went false): a loop's own value is nil;
    // `break v` bypasses this write.
    fx.b.switch_to_block(exit_normal);
    if let Some(dst) = result {
        ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
    }
    fx.b.ins().jump(exit, &[]);

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
