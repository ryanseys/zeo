//! Lowering the forms that choose a block: `if`, `&&`/`||`, `case/when`
//! and the flip-flop's two-state machine.

use super::*;

/// Lower `cond` for a BRANCH: the result is an i8 truthiness value. A
/// comparison the operator fast path owns answers its condition bit
/// directly (no boxed Bool, no `zeo_rt_truthy` call); everything else
/// lowers normally and reduces through `ownership::truthy`. The flag is
/// keyed by node id and consumed only by the binop arm, so routing stays
/// in `lower_expr` and cannot drift.
pub(in crate::clif) fn lower_condition(
    fx: &mut Fx,
    cond: NodeId,
) -> CResult<cranelift_codegen::ir::Value> {
    let saved = fx.branch_cond.replace(cond);
    let r = lower_expr(fx, cond);
    fx.branch_cond = saved;
    Ok(ownership::truthy(fx, r?))
}

/// `if` in VALUE position: both arms move their value into one result
/// slot.
pub(super) fn if_expr(
    fx: &mut Fx,
    cond: NodeId,
    then_body: &[NodeId],
    else_body: &[NodeId],
) -> CResult<Operand> {
    let t = lower_condition(fx, cond)?;
    let ss = fx.temp_slot();
    // The result address is computed BEFORE the branch, so it dominates
    // both arms.
    let dst = fx.slot_addr(ss, 0);
    let b_then = fx.b.create_block();
    let b_else = fx.b.create_block();
    let join = fx.b.create_block();
    fx.b.ins().brif(t, b_then, &[], b_else, &[]);
    fx.b.switch_to_block(b_then);
    crate::clif::stmt::lower_value_body_into(fx, then_body, dst)?;
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(b_else);
    crate::clif::stmt::lower_value_body_into(fx, else_body, dst)?;
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A `true`/`false` immediate into `dst`.
pub(super) fn write_bool(fx: &mut Fx, dst: cranelift_codegen::ir::Value, v: bool) {
    let b = fx.b.ins().iconst(types::I8, i64::from(v));
    ownership::write_move_into(fx, &Operand::Bool(b), dst);
}

/// One flip-flop evaluation: the latch decides, and either operand may
/// raise, so both are ordinary lowered expressions guarded by the latch's
/// own branches.
pub(super) fn flip_flop(
    fx: &mut Fx,
    state: u32,
    left: NodeId,
    right: NodeId,
    exclusive: bool,
) -> CResult<Operand> {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let on_blk = fx.b.create_block();
    let test_left = fx.b.create_block();
    let turned_on = fx.b.create_block();
    let off_blk = fx.b.create_block();
    let join = fx.b.create_block();
    let state_v = fx.flip_flop_state_value(state);
    let on = fx.call_status("zeo_rt_flip_flop_on", &[state_v]);
    fx.b.ins().brif(on, on_blk, &[], test_left, &[]);

    // Already on: the right operand decides whether this is the last true.
    let turn_off = |fx: &mut Fx| -> CResult<()> {
        let op = lower_expr(fx, right)?;
        let t = ownership::truthy(fx, op);
        let clear = fx.b.create_block();
        let done = fx.b.create_block();
        fx.b.ins().brif(t, clear, &[], done, &[]);
        fx.b.switch_to_block(clear);
        let state_v = fx.flip_flop_state_value(state);
        let zero = fx.b.ins().iconst(types::I8, 0);
        fx.call("zeo_rt_flip_flop_set", &[state_v, zero]);
        fx.b.ins().jump(done, &[]);
        fx.b.switch_to_block(done);
        Ok(())
    };
    fx.b.switch_to_block(on_blk);
    turn_off(fx)?;
    write_bool(fx, dst, true);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(test_left);
    let op = lower_expr(fx, left)?;
    let t = ownership::truthy(fx, op);
    fx.b.ins().brif(t, turned_on, &[], off_blk, &[]);

    fx.b.switch_to_block(turned_on);
    let state_v = fx.flip_flop_state_value(state);
    let one = fx.b.ins().iconst(types::I8, 1);
    fx.call("zeo_rt_flip_flop_set", &[state_v, one]);
    if !exclusive {
        turn_off(fx)?;
    }
    write_bool(fx, dst, true);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(off_blk);
    write_bool(fx, dst, false);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(join);
    Ok(Operand::Ptr {
        addr: dst,
        owned: false,
        tag: TagInfo::Known(ValueTag::Bool as u8),
    })
}

/// `a || b` / `a && b`: keep `a` when its truthiness matches
/// `keep_truthy`, else evaluate and keep `b` -- the OPERAND is the value,
/// Ruby's rule.
pub(super) fn short_circuit(
    fx: &mut Fx,
    a: NodeId,
    b: NodeId,
    keep_truthy: bool,
) -> CResult<Operand> {
    let a_op = lower_expr(fx, a)?;
    let ptr = ownership::borrow_ptr(fx, &a_op);
    let a_tag = a_op.tag();
    if a_op.owned() {
        ownership::pool_owned(fx, ptr, a_tag);
    }
    let borrowed = Operand::Ptr {
        addr: ptr,
        owned: false,
        tag: a_tag,
    };
    let t = ownership::truthy(fx, borrowed);
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let keep_a = fx.b.create_block();
    let eval_b = fx.b.create_block();
    let join = fx.b.create_block();
    if keep_truthy {
        fx.b.ins().brif(t, keep_a, &[], eval_b, &[]);
    } else {
        fx.b.ins().brif(t, eval_b, &[], keep_a, &[]);
    }
    fx.b.switch_to_block(keep_a);
    let a_again = Operand::Ptr {
        addr: ptr,
        owned: false,
        tag: a_tag,
    };
    ownership::write_move_into(fx, &a_again, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(eval_b);
    let b_op = lower_expr(fx, b)?;
    ownership::write_move_into(fx, &b_op, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// `case`/`when` in VALUE position: the subject is evaluated once and
/// parked borrowed; each `when` value tests through the runtime's `===`
/// dispatch (`case_eq`; a splatted list through `case_eq_any`, which
/// short-circuits exactly as the listed form's `||` chain does); the first
/// hit's body moves its value into the one result slot; no hit runs the
/// else body (an absent one answers nil). Subjectless `case` tests each
/// value's truthiness, ruby's if-chain sugar.
pub(super) fn case_when(
    fx: &mut Fx,
    subject: Option<NodeId>,
    arms: &[(Vec<ArrayElem>, Vec<NodeId>)],
    else_body: &[NodeId],
) -> CResult<Operand> {
    let subj = match subject {
        Some(n) => {
            let op = lower_expr(fx, n)?;
            let tag = op.tag();
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, tag);
            }
            Some(ptr)
        }
        None => None,
    };
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    // The `===` hit flag's scratch byte (a whole value slot; only byte 0
    // is used).
    let hit_ss = fx.temp_slot();
    let hit_ptr = fx.slot_addr(hit_ss, 0);
    let join = fx.b.create_block();
    for (values, body) in arms {
        let body_block = fx.b.create_block();
        for elem in values {
            let hit = match (elem, subj) {
                (ArrayElem::Single(v), Some(s)) => {
                    let op = lower_expr(fx, *v)?;
                    let tag = op.tag();
                    let p = ownership::borrow_ptr(fx, &op);
                    if op.owned() {
                        ownership::pool_owned(fx, p, tag);
                    }
                    let status = fx.call_status("zeo_rt_case_eq", &[p, s, hit_ptr]);
                    fx.fallible(status);
                    fx.b.ins()
                        .load(types::I8, MemFlagsData::trusted(), hit_ptr, 0)
                }
                (ArrayElem::Splat(v), Some(s)) => {
                    let op = lower_expr(fx, *v)?;
                    let tag = op.tag();
                    let p = ownership::borrow_ptr(fx, &op);
                    if op.owned() {
                        ownership::pool_owned(fx, p, tag);
                    }
                    let status = fx.call_status("zeo_rt_case_eq_any", &[p, s, hit_ptr]);
                    fx.fallible(status);
                    fx.b.ins()
                        .load(types::I8, MemFlagsData::trusted(), hit_ptr, 0)
                }
                (ArrayElem::Single(v), None) => {
                    let op = lower_expr(fx, *v)?;
                    ownership::truthy(fx, op)
                }
                (ArrayElem::Splat(v), None) => {
                    let op = lower_expr(fx, *v)?;
                    let tag = op.tag();
                    let p = ownership::borrow_ptr(fx, &op);
                    if op.owned() {
                        ownership::pool_owned(fx, p, tag);
                    }
                    let status = fx.call_status("zeo_rt_splat_any_truthy", &[p, hit_ptr]);
                    fx.fallible(status);
                    fx.b.ins()
                        .load(types::I8, MemFlagsData::trusted(), hit_ptr, 0)
                }
            };
            let cont = fx.b.create_block();
            fx.b.ins().brif(hit, body_block, &[], cont, &[]);
            fx.b.switch_to_block(cont);
        }
        // The fall-through block (no value hit) is where the NEXT arm's
        // tests continue; remember it, fill this arm's body, come back.
        let fall = fx.b.current_block().expect("a block is under construction");
        fx.b.switch_to_block(body_block);
        crate::clif::stmt::lower_value_body_into(fx, body, dst)?;
        fx.b.ins().jump(join, &[]);
        fx.b.switch_to_block(fall);
    }
    crate::clif::stmt::lower_value_body_into(fx, else_body, dst)?;
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}
