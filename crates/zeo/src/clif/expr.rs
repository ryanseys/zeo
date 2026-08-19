//! Expression lowering for the M0 slice: literals, local reads, and the
//! numeric binary operators with their three-arm shape -- inline Int,
//! inline Float, dynamic `send_value_in` fallback (exactly the rustc
//! emitter's match).

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};
use zeo_abi::abi::{PAYLOAD_OFFSET, ValueTag};

/// `EncodingId(1)` = UTF-8, every plain source literal's encoding.
const ENC_UTF8: i64 = 1;

#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: the refusal arm IS the default -- an unlisted node kind must refuse loudly, which is exactly what a new HirNode should do here until its lowering lands"
)]
pub(crate) fn lower_expr(fx: &mut Fx, id: NodeId) -> Result<Operand, String> {
    match &fx.an.compiler.hir[id] {
        HirNode::IntegerLit(v) => {
            let v = *v;
            Ok(Operand::Int(fx.b.ins().iconst(types::I64, v)))
        }
        HirNode::FloatLit(v) => {
            let v = *v;
            Ok(Operand::Float(fx.b.ins().f64const(v)))
        }
        HirNode::BoolLit(v) => {
            let v = i64::from(*v);
            Ok(Operand::Bool(fx.b.ins().iconst(types::I8, v)))
        }
        HirNode::NilLit => Ok(Operand::Nil),
        HirNode::StringLit(parts) => {
            let Some(text) = pure_literal(parts) else {
                return fx.unsupported(id, "an interpolated or non-UTF-8 string literal");
            };
            let off = fx.em.intern_rodata(text.as_bytes());
            let len = text.len();
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            let ptr = fx.rod(off);
            let len_v = fx.b.ins().iconst(fx.em.ptr, len as i64);
            let enc = fx.b.ins().iconst(types::I8, ENC_UTF8);
            fx.call("zeo_rt_str_new", &[ptr, len_v, enc, dst]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Known(ValueTag::Str as u8),
            })
        }
        HirNode::LocalRead(name) => {
            let Some(&ss) = fx.locals.get(name) else {
                // A read before any write is nil in Ruby only via `defined?`
                // shapes the slice does not lower; a plain read of an
                // unhoisted name cannot reach here.
                return fx.unsupported(id, "a read of an unknown local");
            };
            let addr = fx.slot_addr(ss, 0);
            Ok(Operand::Ptr {
                addr,
                owned: false,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
            if_expr(fx, cond, &then_body, &else_body)
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block: None,
            block_arg: None,
            safe: false,
        } if kwargs.is_empty() => {
            let (receiver, name, args) = (*receiver, name.clone(), args.clone());
            match receiver {
                Some(recv) if BinOp::of(&name).is_some() && args.len() == 1 => {
                    let [ArrayElem::Single(arg)] = args.as_slice() else {
                        return fx.unsupported(id, "a splat operand");
                    };
                    binop(
                        fx,
                        BinOp::of(&name).expect("guarded above"),
                        &name,
                        recv,
                        *arg,
                    )
                }
                Some(recv) => super::call::dynamic_send(fx, id, recv, &name, &args),
                None => match fx.em.methods.get(&name) {
                    Some(decl) if decl.arity == args.len() => {
                        super::call::direct_call(fx, id, &name, &args)
                    }
                    Some(_) => fx.unsupported(id, "a compiled call with the wrong arity"),
                    None => {
                        let what = format!("a receiverless call to `{name}`");
                        fx.unsupported(id, &what)
                    }
                },
            }
        }
        other => {
            let what = format!("this expression ({})", node_kind(other));
            fx.unsupported(id, &what)
        }
    }
}

/// `if` in VALUE position: both arms move their value into one result
/// slot.
fn if_expr(
    fx: &mut Fx,
    cond: NodeId,
    then_body: &[NodeId],
    else_body: &[NodeId],
) -> Result<Operand, String> {
    let c = lower_expr(fx, cond)?;
    let t = ownership::truthy(fx, c);
    let ss = fx.temp_slot();
    // The result address is computed BEFORE the branch, so it dominates
    // both arms.
    let dst = fx.slot_addr(ss, 0);
    let b_then = fx.b.create_block();
    let b_else = fx.b.create_block();
    let join = fx.b.create_block();
    fx.b.ins().brif(t, b_then, &[], b_else, &[]);
    fx.b.switch_to_block(b_then);
    super::stmt::lower_value_body_into(fx, then_body, dst)?;
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(b_else);
    super::stmt::lower_value_body_into(fx, else_body, dst)?;
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A short human label for refusal messages.
#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: a refusal-message label -- every future node kind is correctly 'unsupported' until its lowering lands"
)]
fn node_kind(node: &HirNode) -> &'static str {
    // One `match` would be 80 arms of labels nothing else needs; the
    // refusal text only has to orient, not classify.
    match node {
        HirNode::Call { .. } => "a method call",
        HirNode::If { .. } => "an `if` in value position",
        HirNode::While { .. } => "a loop in value position",
        _ => "an unsupported node kind",
    }
}

/// The pure (single non-interpolated UTF-8 part) text of a string literal.
pub(crate) fn pure_literal(parts: &[StrPart]) -> Option<String> {
    match parts {
        [] => Some(String::new()),
        [StrPart::Lit(s)] => Some(s.clone()),
        [StrPart::Bytes(_) | StrPart::Interp(_)] | [_, _, ..] => None,
    }
}

/// The operator set the slice lowers inline.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl BinOp {
    fn of(name: &str) -> Option<BinOp> {
        match name {
            "+" => Some(BinOp::Add),
            "-" => Some(BinOp::Sub),
            "*" => Some(BinOp::Mul),
            "<" => Some(BinOp::Lt),
            "<=" => Some(BinOp::Le),
            ">" => Some(BinOp::Gt),
            ">=" => Some(BinOp::Ge),
            "==" => Some(BinOp::Eq),
            "!=" => Some(BinOp::Ne),
            _ => None,
        }
    }

    fn int_cc(self) -> Option<IntCC> {
        Some(match self {
            BinOp::Lt => IntCC::SignedLessThan,
            BinOp::Le => IntCC::SignedLessThanOrEqual,
            BinOp::Gt => IntCC::SignedGreaterThan,
            BinOp::Ge => IntCC::SignedGreaterThanOrEqual,
            BinOp::Eq => IntCC::Equal,
            BinOp::Ne => IntCC::NotEqual,
            BinOp::Add | BinOp::Sub | BinOp::Mul => return None,
        })
    }

    fn float_cc(self) -> Option<FloatCC> {
        Some(match self {
            BinOp::Lt => FloatCC::LessThan,
            BinOp::Le => FloatCC::LessThanOrEqual,
            BinOp::Gt => FloatCC::GreaterThan,
            BinOp::Ge => FloatCC::GreaterThanOrEqual,
            BinOp::Eq => FloatCC::Equal,
            BinOp::Ne => FloatCC::NotEqual,
            BinOp::Add | BinOp::Sub | BinOp::Mul => return None,
        })
    }

    /// The int-overflow slow path's capi symbol.
    fn slow(self) -> Option<&'static str> {
        Some(match self {
            BinOp::Add => "zeo_rt_int_add_slow",
            BinOp::Sub => "zeo_rt_int_sub_slow",
            BinOp::Mul => "zeo_rt_int_mul_slow",
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => return None,
        })
    }
}

/// `a op b`, the rustc emitter's exact three arms. Both operands are
/// materialized (owned ones handed to the pool -- the arms only borrow),
/// the result is a fresh owned slot.
fn binop(fx: &mut Fx, op: BinOp, name: &str, recv: NodeId, arg: NodeId) -> Result<Operand, String> {
    let a = lower_expr(fx, recv)?;
    let b_op = lower_expr(fx, arg)?;
    // Unboxed-both fast case: no memory, no tag tests. Everything else
    // takes the general boxed shape.
    if let (Operand::Int(av), Operand::Int(bv)) = (&a, &b_op) {
        return Ok(int_int(fx, op, *av, *bv));
    }
    boxed_binop(fx, op, name, a, b_op)
}

/// Both operands statically Int: pure SSA.
fn int_int(
    fx: &mut Fx,
    op: BinOp,
    a: cranelift_codegen::ir::Value,
    b: cranelift_codegen::ir::Value,
) -> Operand {
    if let Some(cc) = op.int_cc() {
        return Operand::Bool(fx.b.ins().icmp(cc, a, b));
    }
    // Arithmetic on two Int literals/values still needs the overflow arm;
    // route through the boxed shape's slow call only on overflow.
    let (v, ovf) = match op {
        BinOp::Add => fx.b.ins().sadd_overflow(a, b),
        BinOp::Sub => fx.b.ins().ssub_overflow(a, b),
        BinOp::Mul => fx.b.ins().smul_overflow(a, b),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
            unreachable!("comparisons returned above")
        }
    };
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let slow = fx.b.create_block();
    let ok = fx.b.create_block();
    let join = fx.b.create_block();
    fx.b.ins().brif(ovf, slow, &[], ok, &[]);
    fx.b.switch_to_block(ok);
    store_int(fx, v, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(slow);
    {
        // The slow path needs boxed operands.
        let sa = fx.temp_slot();
        let pa = fx.slot_addr(sa, 0);
        store_int(fx, a, pa);
        let sb = fx.temp_slot();
        let pb = fx.slot_addr(sb, 0);
        store_int(fx, b, pb);
        let f = op.slow().expect("arithmetic has a slow path");
        fx.call(f, &[pa, pb, dst]);
        fx.b.ins().jump(join, &[]);
    }
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }
}

fn store_int(fx: &mut Fx, v: cranelift_codegen::ir::Value, dst: cranelift_codegen::ir::Value) {
    let fl = MemFlagsData::trusted();
    let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
    fx.b.ins().store(fl, tag, dst, 0);
    fx.b.ins().store(fl, v, dst, PAYLOAD_OFFSET as i32);
}

fn store_bool_tag(fx: &mut Fx, v: cranelift_codegen::ir::Value, dst: cranelift_codegen::ir::Value) {
    let fl = MemFlagsData::trusted();
    let tag =
        fx.b.ins()
            .iconst(types::I8, i64::from(ValueTag::Bool as u8));
    fx.b.ins().store(fl, tag, dst, 0);
    fx.b.ins().store(fl, v, dst, PAYLOAD_OFFSET as i32);
}

/// The general three-arm shape over boxed operands.
fn boxed_binop(
    fx: &mut Fx,
    op: BinOp,
    name: &str,
    a: Operand,
    b_op: Operand,
) -> Result<Operand, String> {
    let fl = MemFlagsData::trusted();
    let payload = PAYLOAD_OFFSET as i32;
    // Owned operands hand ownership to the pool; every arm below only
    // borrows the bytes.
    let pa = ownership::borrow_ptr(fx, &a);
    if a.owned() {
        pool_operand(fx, &a, pa);
    }
    let pb = ownership::borrow_ptr(fx, &b_op);
    if b_op.owned() {
        pool_operand(fx, &b_op, pb);
    }
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);

    let b_int = fx.b.create_block();
    let not_int = fx.b.create_block();
    let b_float = fx.b.create_block();
    let b_dyn = fx.b.create_block();
    let join = fx.b.create_block();

    let ta = fx.b.ins().load(types::I8, fl, pa, 0);
    let tb = fx.b.ins().load(types::I8, fl, pb, 0);
    let int_tag = i64::from(ValueTag::Int as u8);
    let a_int = fx.b.ins().icmp_imm_u(IntCC::Equal, ta, int_tag);
    let b_int_p = fx.b.ins().icmp_imm_u(IntCC::Equal, tb, int_tag);
    let both_int = fx.b.ins().band(a_int, b_int_p);
    fx.b.ins().brif(both_int, b_int, &[], not_int, &[]);

    fx.b.switch_to_block(b_int);
    {
        let av = fx.b.ins().load(types::I64, fl, pa, payload);
        let bv = fx.b.ins().load(types::I64, fl, pb, payload);
        if let Some(cc) = op.int_cc() {
            let c = fx.b.ins().icmp(cc, av, bv);
            store_bool_tag(fx, c, dst);
            fx.b.ins().jump(join, &[]);
        } else {
            let (v, ovf) = match op {
                BinOp::Add => fx.b.ins().sadd_overflow(av, bv),
                BinOp::Sub => fx.b.ins().ssub_overflow(av, bv),
                BinOp::Mul => fx.b.ins().smul_overflow(av, bv),
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
                    unreachable!("comparisons handled above")
                }
            };
            let slow = fx.b.create_block();
            let ok = fx.b.create_block();
            fx.b.ins().brif(ovf, slow, &[], ok, &[]);
            fx.b.switch_to_block(ok);
            store_int(fx, v, dst);
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(slow);
            let f = op.slow().expect("arithmetic has a slow path");
            fx.call(f, &[pa, pb, dst]);
            fx.b.ins().jump(join, &[]);
        }
    }

    fx.b.switch_to_block(not_int);
    let float_tag = i64::from(ValueTag::Float as u8);
    let a_f = fx.b.ins().icmp_imm_u(IntCC::Equal, ta, float_tag);
    let b_f = fx.b.ins().icmp_imm_u(IntCC::Equal, tb, float_tag);
    let both_f = fx.b.ins().band(a_f, b_f);
    fx.b.ins().brif(both_f, b_float, &[], b_dyn, &[]);

    fx.b.switch_to_block(b_float);
    {
        let av = fx.b.ins().load(types::F64, fl, pa, payload);
        let bv = fx.b.ins().load(types::F64, fl, pb, payload);
        if let Some(cc) = op.float_cc() {
            let c = fx.b.ins().fcmp(cc, av, bv);
            store_bool_tag(fx, c, dst);
        } else {
            let v = match op {
                BinOp::Add => fx.b.ins().fadd(av, bv),
                BinOp::Sub => fx.b.ins().fsub(av, bv),
                BinOp::Mul => fx.b.ins().fmul(av, bv),
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
                    unreachable!("comparisons handled above")
                }
            };
            let tag =
                fx.b.ins()
                    .iconst(types::I8, i64::from(ValueTag::Float as u8));
            fx.b.ins().store(fl, tag, dst, 0);
            fx.b.ins().store(fl, v, dst, payload);
        }
        fx.b.ins().jump(join, &[]);
    }

    fx.b.switch_to_block(b_dyn);
    {
        let sym = fx.sym_id(name);
        let zero = fx.b.ins().iconst(types::I32, 0);
        let one = fx.b.ins().iconst(fx.em.ptr, 1);
        let null = fx.b.ins().iconst(fx.em.ptr, 0);
        let status = fx
            .call("zeo_rt_send_value_in", &[zero, pa, sym, pb, one, null, dst])
            .expect("send returns a status");
        fx.fallible(status);
        fx.b.ins().jump(join, &[]);
    }

    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// Hand an owned operand's value to the pool, keeping `addr` borrowable.
fn pool_operand(fx: &mut Fx, op: &Operand, addr: cranelift_codegen::ir::Value) {
    ownership::pool_owned(fx, addr, op.tag());
}
