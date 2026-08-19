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

/// A multiple assignment (`a, b = ...`, `a, *r, c = arr`, nested
/// groups): the runtime splits the (to_ary-coerced) value against the
/// before/splat/after shape into owned slots; each target consumes its
/// slot. `value_ptr` is a BORROW of the right-hand side.
pub(crate) fn lower_multi_group(
    fx: &mut Fx,
    site: NodeId,
    group: &crate::hir::MultiTargetGroup,
    value_ptr: cranelift_codegen::ir::Value,
) -> Result<(), String> {
    let n_before = group.before.len();
    let n_after = group.after.len();
    let has_splat = group.splat.is_some();
    let n_out = n_before + usize::from(has_splat) + n_after;
    let slots =
        fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (n_out.max(1) as u32) * super::ctx::VALUE_SIZE,
            3,
        ));
    let slots_ptr = fx.slot_addr(slots, 0);
    let nb = fx.b.ins().iconst(fx.em.ptr, n_before as i64);
    let hs = fx.b.ins().iconst(types::I8, i64::from(has_splat));
    let na = fx.b.ins().iconst(fx.em.ptr, n_after as i64);
    let status = fx
        .call("zeo_rt_multi_split", &[value_ptr, nb, hs, na, slots_ptr])
        .expect("multi_split returns a status");
    fx.fallible(status);
    fx.owned_created += n_out;
    let mut s = 0usize;
    let addr_of_slot = |fx: &mut Fx, s: usize| fx.slot_addr(slots, (s as u32 * 24) as i32);
    for t in &group.before {
        let addr = addr_of_slot(fx, s);
        write_multi_target(fx, site, t, addr)?;
        s += 1;
    }
    match &group.splat {
        Some(Some(t)) => {
            let addr = addr_of_slot(fx, s);
            write_multi_target(fx, site, t, addr)?;
            s += 1;
        }
        Some(None) => {
            // An anonymous `*` collects and discards.
            let addr = addr_of_slot(fx, s);
            fx.call("zeo_rt_release", &[addr]);
            fx.owned_consumed += 1;
            s += 1;
        }
        None => {}
    }
    for t in &group.after {
        let addr = addr_of_slot(fx, s);
        write_multi_target(fx, site, t, addr)?;
        s += 1;
    }
    debug_assert_eq!(s, n_out);
    Ok(())
}

/// One multi-assignment target's write; `addr` holds the OWNED slot value
/// the target consumes.
fn write_multi_target(
    fx: &mut Fx,
    site: NodeId,
    target: &crate::hir::MultiTarget,
    addr: cranelift_codegen::ir::Value,
) -> Result<(), String> {
    use crate::hir::MultiTarget;
    let op = super::operand::Operand::Ptr {
        addr,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    };
    match target {
        MultiTarget::Local(name) => {
            ownership::write_local(fx, name, &op);
            Ok(())
        }
        MultiTarget::Ivar(name) => {
            let slot = ivar_slot_of(fx, site, name)?;
            let ptr = ownership::move_ptr(fx, &op);
            let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
            let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
            let status = fx
                .call("zeo_rt_ivar_set_slot", &[self_ptr, slot_v, ptr])
                .expect("ivar_set_slot returns a status");
            fx.fallible(status);
            Ok(())
        }
        MultiTarget::Call {
            write_call,
            tmp_name,
        } => {
            // Bind the write's synthetic hidden local, then run the write
            // call itself through the ordinary expression path.
            ownership::write_local(fx, tmp_name, &op);
            let r = lower_expr(fx, *write_call)?;
            ownership::discard(fx, r);
            Ok(())
        }
        MultiTarget::Nested(group) => {
            // The nested group destructures the slot's value (borrowing it
            // for the split), then the slot itself is done.
            let group = group.clone();
            lower_multi_group(fx, site, &group, addr)?;
            fx.call("zeo_rt_release", &[addr]);
            fx.owned_consumed += 1;
            Ok(())
        }
        MultiTarget::ClassVar(_)
        | MultiTarget::Global(_)
        | MultiTarget::Const(_)
        | MultiTarget::ScopedConst { .. } => {
            fx.unsupported(site, "a cvar/global/constant multi-assignment target")
        }
    }
}

fn ivar_slot_of(fx: &Fx, site: NodeId, name: &str) -> Result<usize, String> {
    if fx.self_is_class {
        return fx.unsupported(site, "a class-level ivar");
    }
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
            Ok(ownership::read_local(fx, &name).expect("just assigned"))
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
        HirNode::Break(..)
        | HirNode::Next(..)
        | HirNode::Redo
        | HirNode::Raise(..)
        | HirNode::Return(..) => {
            // The jump/signal leaves this block unreachable; the nil is
            // never read.
            lower_stmt(fx, tail)?;
            Ok(Operand::Nil)
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            let (body, rescues, else_body, ensure_body) = (
                body.clone(),
                rescues.clone(),
                else_body.clone(),
                ensure_body.clone(),
            );
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            super::control::lower_begin(
                fx,
                tail,
                &body,
                &rescues,
                else_body.as_deref(),
                ensure_body.as_deref(),
                Some(dst),
            )?;
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
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
        | HirNode::Yield(..)
        | HirNode::BlockGiven
        | HirNode::MultiWrite { .. }
        | HirNode::Lambda { .. }
        | HirNode::ArrayLit(..)
        | HirNode::HashLit(..)
        | HirNode::SymbolLit(..)
        | HirNode::Seq(..)
        | HirNode::CaseWhen { .. }
        | HirNode::RangeLit { .. }
        | HirNode::GlobalRead(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ClassVarRead(..)
        | HirNode::ClassVarWrite(..)
        | HirNode::ConstWrite { .. }
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
            ownership::write_local(fx, &name, &op);
            Ok(())
        }
        HirNode::MultiWrite { targets, value } => {
            let (targets, value) = (targets.clone(), *value);
            let op = lower_expr(fx, value)?;
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, op.tag());
            }
            lower_multi_group(fx, stmt, &targets, ptr)
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
                // In an escaping block, `break` arms the Break signal the
                // send-site's catch_break receives.
                if fx.block_next.is_some() {
                    let op = match value {
                        Some(v) => lower_expr(fx, v)?,
                        None => super::operand::Operand::Nil,
                    };
                    let ptr = ownership::move_ptr(fx, &op);
                    let kind =
                        fx.b.ins()
                            .iconst(types::I8, i64::from(zeo_abi::abi::SignalKind::Break as u8));
                    fx.pop_handling_to(0);
                    fx.call("zeo_rt_signal_set", &[kind, ptr]);
                    let land = fx.land;
                    fx.b.ins().jump(land, &[]);
                    fx.continue_unreachable();
                    return Ok(());
                }
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
            let ctl = fx.loops.last().expect("checked above");
            if ctl.depth != fx.ensure_depth {
                return fx.unsupported(stmt, "a `break` across an `ensure` boundary");
            }
            let (exit, handling) = (ctl.exit, ctl.handling);
            fx.pop_handling_to(handling);
            fx.b.ins().jump(exit, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Next(value) => {
            let value = *value;
            if fx.loops.is_empty()
                && let Some((out, ret_ok)) = fx.block_next
            {
                // In an escaping block, `next v` IS the block's return.
                match value {
                    Some(v) => {
                        let op = lower_expr(fx, v)?;
                        ownership::write_move_into(fx, &op, out);
                    }
                    None => {
                        ownership::write_move_into(fx, &super::operand::Operand::Nil, out);
                    }
                }
                fx.pop_handling_to(0);
                fx.b.ins().jump(ret_ok, &[]);
                fx.continue_unreachable();
                return Ok(());
            }
            if let Some(v) = value {
                let op = lower_expr(fx, v)?;
                ownership::discard(fx, op);
            }
            let Some(ctl) = fx.loops.last() else {
                return fx.unsupported(stmt, "`next` outside a loop");
            };
            if ctl.depth != fx.ensure_depth {
                return fx.unsupported(stmt, "a `next` across an `ensure` boundary");
            }
            let (latch, handling) = (ctl.latch, ctl.handling);
            fx.pop_handling_to(handling);
            fx.b.ins().jump(latch, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Redo => {
            let Some(ctl) = fx.loops.last() else {
                // `redo` in a block re-runs the block from its binding head
                // (the bindings re-run too -- they sit inside the loop).
                let Some(head) = fx.block_redo else {
                    return fx.unsupported(stmt, "`redo` outside a loop");
                };
                if fx.ensure_depth != 0 {
                    return fx.unsupported(stmt, "a `redo` across an `ensure` boundary");
                }
                fx.pop_handling_to(0);
                fx.b.ins().jump(head, &[]);
                fx.continue_unreachable();
                return Ok(());
            };
            if ctl.depth != fx.ensure_depth {
                return fx.unsupported(stmt, "a `redo` across an `ensure` boundary");
            }
            let (body, handling) = (ctl.body, ctl.handling);
            fx.pop_handling_to(handling);
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
                // In an escaping block, `return` arms the Return signal;
                // the runtime resolves it against the proc's captured home
                // (dead home -> LocalJumpError) and the defining method's
                // boundary folds a targeted one.
                if fx.block_next.is_some() {
                    if fx.ensure_depth != 0 {
                        return fx.unsupported(stmt, "a `return` across an `ensure` boundary");
                    }
                    let op = match value {
                        Some(v) => lower_expr(fx, v)?,
                        None => super::operand::Operand::Nil,
                    };
                    let ptr = ownership::move_ptr(fx, &op);
                    let kind =
                        fx.b.ins()
                            .iconst(types::I8, i64::from(zeo_abi::abi::SignalKind::Return as u8));
                    fx.pop_handling_to(0);
                    fx.call("zeo_rt_signal_set", &[kind, ptr]);
                    let land = fx.land;
                    fx.b.ins().jump(land, &[]);
                    fx.continue_unreachable();
                    return Ok(());
                }
                return fx.unsupported(stmt, "a top-level `return`");
            };
            if fx.ensure_depth != 0 {
                return fx.unsupported(stmt, "a `return` across an `ensure` boundary");
            }
            fx.pop_handling_to(0);
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
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            let (body, rescues, else_body, ensure_body) = (
                body.clone(),
                rescues.clone(),
                else_body.clone(),
                ensure_body.clone(),
            );
            super::control::lower_begin(
                fx,
                stmt,
                &body,
                &rescues,
                else_body.as_deref(),
                ensure_body.as_deref(),
                None,
            )
        }
        HirNode::Raise(args, cause) => {
            if let crate::hir::RaiseCause::Explicit(_) = cause {
                return fx.unsupported(stmt, "a `raise` with an explicit cause:");
            }
            let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
            // `raise` IS `Kernel#raise` -- the builtin row constructs,
            // stamps, and signals; the Ok arm is unreachable.
            let op = super::call::implicit_send(fx, stmt, "raise", &elems)?;
            ownership::discard(fx, op);
            Ok(())
        }
        HirNode::Retry => {
            let Some(&(target, depth, handling)) = fx.retries.last() else {
                return fx.unsupported(stmt, "`retry` outside a rescue clause");
            };
            if depth != fx.ensure_depth {
                return fx.unsupported(stmt, "a `retry` across an `ensure` boundary");
            }
            fx.pop_handling_to(handling);
            fx.b.ins().jump(target, &[]);
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
        } if kwargs.is_empty() => {
            let (receiver, name, args, blk) = (*receiver, name.clone(), args.clone(), *blk);
            if args.is_empty()
                && let Some(counted) = super::iter::counted_of(fx, receiver, &name, true)
            {
                return super::iter::lower_counted(fx, stmt, &counted, blk, None);
            }
            let op = if receiver.is_none()
                && let Some(decl) = fx.em.methods.get(&name)
                && decl.arity == args.len()
                && !super::expr::method_class_shadows(fx, &name)
            {
                super::call::direct_call(fx, stmt, &name, &args, Some(blk))?
            } else {
                super::blocks::block_send(fx, stmt, receiver, &name, &args, blk)?
            };
            ownership::discard(fx, op);
            Ok(())
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
        | HirNode::Yield(..)
        | HirNode::BlockGiven
        | HirNode::CaseWhen { .. }
        | HirNode::RangeLit { .. }
        | HirNode::GlobalRead(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ClassVarRead(..)
        | HirNode::ClassVarWrite(..)
        | HirNode::ConstWrite { .. }
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
        depth: fx.ensure_depth,
        handling: fx.handling_depth,
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
