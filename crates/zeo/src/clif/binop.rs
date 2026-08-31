//! The numeric binary operators: the statically-shaped Int and Float
//! arms and the boxed three-arm fallback that tag-tests at run time.

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::codegen_error::CResult;
use crate::hir::NodeId;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};
use zeo_abi::abi::{PAYLOAD_OFFSET, TAG_OFFSET, ValueTag};

/// The operator set lowered inline (see `types.rs`'s
/// `INT_RESULT_BINARY_OPS`/`FLOAT_RESULT_BINARY_OPS`), which the boxed
/// three-arm shape below serves from ONE site each -- the tag test asks
/// at run time, so no statically known operand pair is needed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    BAnd,
    BOr,
    BXor,
    Shl,
    Shr,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Cmp,
}

/// How the Int arm shapes an operator over two `i64` payloads.
enum IntShape {
    /// Overflow-checked arithmetic; the overflow arm calls the named capi.
    Overflow(&'static str),
    /// Total bitwise arithmetic -- an `i64` pair's result is an `i64`.
    Bits,
    /// A comparison, answering `Bool`.
    Compare(IntCC),
    /// `<=>`, answering `Int` -1/0/1.
    Spaceship,
    /// Floored `/` or `%`: a zero divisor raises here, and the one
    /// overflowing pair (`i64::MIN op -1`) takes the named capi.
    Floored { modulo: bool, slow: &'static str },
    /// A fallible call on the boxed operands (bignum promotion lives in
    /// the runtime).
    Call(&'static str),
}

/// How the Float arm shapes an operator over two `f64` payloads.
enum FloatShape {
    /// Total arithmetic, answering `Float`.
    Arith,
    /// A comparison, answering `Bool`.
    Compare(FloatCC),
    /// `zeo_rt_float_mod_checked` / `float_pow_checked` -- fallible.
    Fallible(&'static str),
    /// `zeo_rt_float_cmp` -- total, but answers `nil` against a NaN.
    Spaceship,
    /// `Float` has no such operator; the arm is not emitted at all and a
    /// Float pair falls to the dynamic send, where the runtime raises.
    None,
}

impl BinOp {
    fn of(name: &str) -> Option<BinOp> {
        match name {
            "+" => Some(BinOp::Add),
            "-" => Some(BinOp::Sub),
            "*" => Some(BinOp::Mul),
            "/" => Some(BinOp::Div),
            "%" => Some(BinOp::Mod),
            "**" => Some(BinOp::Pow),
            "&" => Some(BinOp::BAnd),
            "|" => Some(BinOp::BOr),
            "^" => Some(BinOp::BXor),
            "<<" => Some(BinOp::Shl),
            ">>" => Some(BinOp::Shr),
            "<" => Some(BinOp::Lt),
            "<=" => Some(BinOp::Le),
            ">" => Some(BinOp::Gt),
            ">=" => Some(BinOp::Ge),
            "==" => Some(BinOp::Eq),
            "!=" => Some(BinOp::Ne),
            "<=>" => Some(BinOp::Cmp),
            _ => None,
        }
    }

    fn int_shape(self) -> IntShape {
        match self {
            BinOp::Add => IntShape::Overflow("zeo_rt_int_add_slow"),
            BinOp::Sub => IntShape::Overflow("zeo_rt_int_sub_slow"),
            BinOp::Mul => IntShape::Overflow("zeo_rt_int_mul_slow"),
            BinOp::Div => IntShape::Floored {
                modulo: false,
                slow: "zeo_rt_int_div",
            },
            BinOp::Mod => IntShape::Floored {
                modulo: true,
                slow: "zeo_rt_int_mod",
            },
            BinOp::Pow => IntShape::Call("zeo_rt_int_pow"),
            BinOp::BAnd | BinOp::BOr | BinOp::BXor => IntShape::Bits,
            BinOp::Shl => IntShape::Call("zeo_rt_int_shl"),
            BinOp::Shr => IntShape::Call("zeo_rt_int_shr"),
            BinOp::Lt => IntShape::Compare(IntCC::SignedLessThan),
            BinOp::Le => IntShape::Compare(IntCC::SignedLessThanOrEqual),
            BinOp::Gt => IntShape::Compare(IntCC::SignedGreaterThan),
            BinOp::Ge => IntShape::Compare(IntCC::SignedGreaterThanOrEqual),
            BinOp::Eq => IntShape::Compare(IntCC::Equal),
            BinOp::Ne => IntShape::Compare(IntCC::NotEqual),
            BinOp::Cmp => IntShape::Spaceship,
        }
    }

    fn float_shape(self) -> FloatShape {
        match self {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => FloatShape::Arith,
            BinOp::Mod => FloatShape::Fallible("zeo_rt_float_mod_checked"),
            BinOp::Pow => FloatShape::Fallible("zeo_rt_float_pow_checked"),
            // Real Ruby's `Float` has no bitwise or shift operators.
            BinOp::BAnd | BinOp::BOr | BinOp::BXor | BinOp::Shl | BinOp::Shr => FloatShape::None,
            BinOp::Lt => FloatShape::Compare(FloatCC::LessThan),
            BinOp::Le => FloatShape::Compare(FloatCC::LessThanOrEqual),
            BinOp::Gt => FloatShape::Compare(FloatCC::GreaterThan),
            BinOp::Ge => FloatShape::Compare(FloatCC::GreaterThanOrEqual),
            BinOp::Eq => FloatShape::Compare(FloatCC::Equal),
            BinOp::Ne => FloatShape::Compare(FloatCC::NotEqual),
            BinOp::Cmp => FloatShape::Spaceship,
        }
    }
}

/// `a op b`, the rustc emitter's exact three arms. Both operands are
/// materialized (owned ones handed to the pool -- the arms only borrow),
/// the result is a fresh owned slot -- or, when this call IS the condition
/// a branch site is lowering (`fx.branch_cond`) and the operator is a
/// comparison, the bare condition bit as `Operand::Bool`.
pub(super) fn binop(
    fx: &mut Fx,
    id: NodeId,
    name: &str,
    recv: NodeId,
    arg: NodeId,
) -> CResult<Operand> {
    let op = BinOp::of(name).expect("operator_fast_path guarded");
    // Branch mode only for the six relationals: their Int AND Float arms
    // are compares, and a dynamic result's truthiness is what the branch
    // site would compute anyway. `<=>` answers Int and stays boxed.
    let branch = fx.branch_cond == Some(id)
        && matches!(op.int_shape(), IntShape::Compare(_))
        && matches!(op.float_shape(), FloatShape::Compare(_));
    let a = super::expr::lower_expr(fx, recv)?;
    // Park an owned lhs BEFORE the rhs lowers: the rhs may raise, and the
    // raise landing never sees an operand that is owned but unpooled.
    let a = if a.owned() {
        let pa = ownership::borrow_ptr(fx, &a);
        pool_operand(fx, &a, pa);
        Operand::Ptr {
            addr: pa,
            owned: false,
            tag: a.tag(),
        }
    } else {
        super::expr::park_reassignable(fx, Some(recv), a, &[arg])
    };
    let b_op = super::expr::lower_expr(fx, arg)?;
    boxed_binop(fx, op, name, a, b_op, branch)
}

/// Whether `name`'s operator fast path may be taken. A user reopen that
/// redefines the operator on `Integer`'s or `Float`'s fast-path MRO has to
/// be honored at EVERY call site, so the whole fast path stands down and the
/// ordinary dynamic send finds the reopened row -- `analyze` recorded both
/// lanes for exactly this.
///
/// Both lanes gate the one decision because the boxed shape tests both tags:
/// a `Float#==` reopen leaves the Int arm sound, but the site cannot know
/// which arm it will take.
/// Whether NilClass's `==`/`!=` are provably untouched for the whole run:
/// nothing in the program's text defines either on NilClass, and no
/// runtime-definition machinery could. The same per-program divergence
/// model as `freeze_is_pristine` -- decided ahead of time, never per call.
fn nil_eq_pristine(fx: &Fx) -> bool {
    let c = &fx.an.compiler;
    c.method_in_chain(zeo_abi::NIL_CLASS, "==").is_none()
        && c.method_in_chain(zeo_abi::NIL_CLASS, "!=").is_none()
        && !c.may_be_patched_at_runtime("==")
        && !c.may_be_patched_at_runtime("!=")
}

pub(super) fn operator_fast_path(fx: &Fx, name: &str) -> bool {
    BinOp::of(name).is_some()
        && !fx.an.compiler.redefined_int_ops.contains(name)
        && !fx.an.compiler.redefined_float_ops.contains(name)
}

fn store_int(fx: &mut Fx, v: cranelift_codegen::ir::Value, dst: cranelift_codegen::ir::Value) {
    let fl = MemFlagsData::trusted();
    let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
    fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
    fx.b.ins().store(fl, v, dst, PAYLOAD_OFFSET as i32);
}

fn store_bool_tag(fx: &mut Fx, v: cranelift_codegen::ir::Value, dst: cranelift_codegen::ir::Value) {
    let fl = MemFlagsData::trusted();
    let tag =
        fx.b.ins()
            .iconst(types::I8, i64::from(ValueTag::Bool as u8));
    fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
    fx.b.ins().store(fl, v, dst, payload_off());
}

fn payload_off() -> i32 {
    PAYLOAD_OFFSET as i32
}

/// The Int arm: both operands carry the `Int` tag, so both payloads are
/// plain `i64`. Ends by jumping to `join`.
fn int_arm(
    fx: &mut Fx,
    op: BinOp,
    pa: cranelift_codegen::ir::Value,
    pb: cranelift_codegen::ir::Value,
    dst: cranelift_codegen::ir::Value,
    join: cranelift_codegen::ir::Block,
) {
    let fl = MemFlagsData::trusted();
    let payload = payload_off();
    match op.int_shape() {
        IntShape::Call(f) => {
            let status = fx
                .call(f, &[pa, pb, dst])
                .expect("a fallible op returns one");
            fx.fallible(status);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Compare(cc) => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            let c = fx.b.ins().icmp(cc, av, bv);
            store_bool_tag(fx, c, dst);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Spaceship => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            let gt = fx.b.ins().icmp(IntCC::SignedGreaterThan, av, bv);
            let lt = fx.b.ins().icmp(IntCC::SignedLessThan, av, bv);
            let gt = fx.b.ins().sextend(types::I64, gt);
            let lt = fx.b.ins().sextend(types::I64, lt);
            // Both are 0/1, so `gt - lt` is -1/0/1.
            let c = fx.b.ins().isub(gt, lt);
            store_int(fx, c, dst);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Bits => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            let v = match op {
                BinOp::BAnd => fx.b.ins().band(av, bv),
                BinOp::BOr => fx.b.ins().bor(av, bv),
                BinOp::BXor => fx.b.ins().bxor(av, bv),
                _ => unreachable!("only the bitwise operators take this shape"),
            };
            store_int(fx, v, dst);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Overflow(slow) => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            let (v, ovf) = match op {
                BinOp::Add => fx.b.ins().sadd_overflow(av, bv),
                BinOp::Sub => fx.b.ins().ssub_overflow(av, bv),
                BinOp::Mul => fx.b.ins().smul_overflow(av, bv),
                _ => unreachable!("only the checked arithmetic takes this shape"),
            };
            let slow_b = fx.b.create_block();
            let ok = fx.b.create_block();
            fx.b.ins().brif(ovf, slow_b, &[], ok, &[]);
            fx.b.switch_to_block(ok);
            store_int(fx, v, dst);
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(slow_b);
            fx.call(slow, &[pa, pb, dst]);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Floored { modulo, slow } => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            // A zero divisor raises; `i64::MIN op -1` is the one pair the
            // native instruction cannot answer, and the runtime promotes it.
            let zero = fx.b.ins().icmp_imm_s(IntCC::Equal, bv, 0);
            let raise = fx.b.create_block();
            let live = fx.b.create_block();
            fx.b.ins().brif(zero, raise, &[], live, &[]);

            fx.b.switch_to_block(raise);
            let cid =
                fx.b.ins()
                    .iconst(types::I32, i64::from(zeo_abi::ZERO_DIVISION_ERROR_CLASS.0));
            let (mptr, mlen) = super::expr::rodata_name(fx, "divided by 0");
            fx.call("zeo_rt_raise_error", &[cid, mptr, mlen]);
            fx.b.ins().jump(fx.land, &[]);

            fx.b.switch_to_block(live);
            let min = fx.b.ins().icmp_imm_s(IntCC::Equal, av, i64::MIN);
            let neg1 = fx.b.ins().icmp_imm_s(IntCC::Equal, bv, -1);
            let overflows = fx.b.ins().band(min, neg1);
            let promote = fx.b.create_block();
            let native = fx.b.create_block();
            fx.b.ins().brif(overflows, promote, &[], native, &[]);

            fx.b.switch_to_block(promote);
            fx.call(slow, &[pa, pb, dst]);
            fx.b.ins().jump(join, &[]);

            fx.b.switch_to_block(native);
            // Ruby floors toward negative infinity, where the machine
            // truncates: the quotient loses one and the remainder gains a
            // divisor whenever the remainder's sign differs from `b`'s.
            let r = fx.b.ins().srem(av, bv);
            let r_nz = fx.b.ins().icmp_imm_s(IntCC::NotEqual, r, 0);
            let r_neg = fx.b.ins().icmp_imm_s(IntCC::SignedLessThan, r, 0);
            let b_neg = fx.b.ins().icmp_imm_s(IntCC::SignedLessThan, bv, 0);
            let signs_differ = fx.b.ins().bxor(r_neg, b_neg);
            let adjust = fx.b.ins().band(r_nz, signs_differ);
            let v = if modulo {
                let adjusted = fx.b.ins().iadd(r, bv);
                fx.b.ins().select(adjust, adjusted, r)
            } else {
                let q = fx.b.ins().sdiv(av, bv);
                let lowered = fx.b.ins().iadd_imm_s(q, -1);
                fx.b.ins().select(adjust, lowered, q)
            };
            store_int(fx, v, dst);
            fx.b.ins().jump(join, &[]);
        }
    }
}

/// Load one operand's payload as an `f64`: a Float reads its own bits, an
/// Int promotes -- ruby's numeric tower, where `1 + 2.0` runs the Float
/// operation on both sides.
fn as_f64(
    fx: &mut Fx,
    p: cranelift_codegen::ir::Value,
    is_int: bool,
) -> cranelift_codegen::ir::Value {
    let fl = MemFlagsData::trusted();
    let payload = payload_off();
    if is_int {
        let i = fx.b.ins().load(types::I64, fl, p, payload);
        fx.b.ins().fcvt_from_sint(types::F64, i)
    } else {
        fx.b.ins().load(types::F64, fl, p, payload)
    }
}

/// The Float arm, over the two already-promoted payloads. Ends by jumping
/// to `join`.
fn float_arm(
    fx: &mut Fx,
    op: BinOp,
    av: cranelift_codegen::ir::Value,
    bv: cranelift_codegen::ir::Value,
    dst: cranelift_codegen::ir::Value,
    join: cranelift_codegen::ir::Block,
) {
    let fl = MemFlagsData::trusted();
    let payload = payload_off();
    match op.float_shape() {
        FloatShape::None => unreachable!("an armless operator never reaches here"),
        FloatShape::Compare(cc) => {
            let c = fx.b.ins().fcmp(cc, av, bv);
            store_bool_tag(fx, c, dst);
        }
        FloatShape::Arith => {
            let v = match op {
                BinOp::Add => fx.b.ins().fadd(av, bv),
                BinOp::Sub => fx.b.ins().fsub(av, bv),
                BinOp::Mul => fx.b.ins().fmul(av, bv),
                BinOp::Div => fx.b.ins().fdiv(av, bv),
                _ => unreachable!("only the total arithmetic takes this shape"),
            };
            let tag =
                fx.b.ins()
                    .iconst(types::I8, i64::from(ValueTag::Float as u8));
            fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
            fx.b.ins().store(fl, v, dst, payload);
        }
        FloatShape::Spaceship => {
            fx.call("zeo_rt_float_cmp", &[av, bv, dst]);
        }
        FloatShape::Fallible(f) => {
            let status = fx
                .call(f, &[av, bv, dst])
                .expect("a fallible op returns one");
            fx.fallible(status);
        }
    }
    fx.b.ins().jump(join, &[]);
}

/// The general three-arm shape over boxed operands. In `branch` mode the
/// arms join on a bare i8 condition bit (a `join` block param) instead of
/// writing a boxed Bool -- the dynamic arm reduces its result through
/// `zeo_rt_truthy`, exactly what the branch site would do to the boxed
/// value.
fn boxed_binop(
    fx: &mut Fx,
    op: BinOp,
    name: &str,
    a: Operand,
    b_op: Operand,
    branch: bool,
) -> CResult<Operand> {
    let fl = MemFlagsData::trusted();
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

    // Static tag knowledge trims arms at COMPILE time: a literal operand
    // (`x + 1`, `i < 1000000`, `x == nil`) has a Known tag, so its tag
    // byte is never re-loaded and every arm its tag rules out is never
    // emitted. Both-Int emits ONLY the int arm (branch mode collapses to
    // one icmp); a Known non-numeric side goes straight to the cached
    // dynamic arm. TyKind is not consulted -- this is the emitter's own
    // Operand tag, sound by construction.
    let known = |t: TagInfo| match t {
        TagInfo::Known(v) => Some(v),
        _ => None,
    };
    let ka = known(a.tag());
    let kb = known(b_op.tag());
    // BOTH sides Known and not both Int (literal-vs-literal mixed pairs,
    // rare): pretend both unknown so every arm keeps a runtime condition
    // and the block graph stays total. Both-Int returns above.
    let (ka, kb) = if ka.is_some() && kb.is_some() && !(ka == Some(ValueTag::Int as u8) && kb == Some(ValueTag::Int as u8)) {
        (None, None)
    } else {
        (ka, kb)
    };
    let int_t = ValueTag::Int as u8;
    let float_t = ValueTag::Float as u8;
    // Some(true/false) = statically decided; None = ask at run time.
    let s_int = |k: Option<u8>| k.map(|t| t == int_t);
    let (sa_int, sb_int) = (s_int(ka), s_int(kb));

    // BOTH statically Int: the int arm IS the operation.
    if sa_int == Some(true) && sb_int == Some(true) {
        if branch {
            let IntShape::Compare(cc) = op.int_shape() else {
                unreachable!("branch mode admits only comparisons");
            };
            let av = fx.b.ins().load(types::I64, fl, pa, payload_off());
            let bv = fx.b.ins().load(types::I64, fl, pb, payload_off());
            let bit = fx.b.ins().icmp(cc, av, bv);
            return Ok(Operand::Bool(bit));
        }
        let join = fx.b.create_block();
        int_arm(fx, op, pa, pb, dst, join);
        fx.b.switch_to_block(join);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        });
    }

    // The int arm is DEAD when a side is statically non-Int; its block
    // is then never created (a created-but-unfilled block fails
    // finalization).
    let int_live = sa_int != Some(false) && sb_int != Some(false);
    let b_int = int_live.then(|| fx.b.create_block());
    let not_int = fx.b.create_block();
    let b_dyn = fx.b.create_block();
    let join = fx.b.create_block();
    if branch {
        fx.b.append_block_param(join, types::I8);
    }

    // The tag bytes, loaded only for the sides not statically known.
    let ta = (ka.is_none())
        .then(|| fx.b.ins().load(types::I8, fl, pa, TAG_OFFSET as i32));
    // `x == nil` / `x != nil` against a LITERAL nil, with NilClass's own
    // rows untouched: a receiver whose tag IS Nil answers the constant --
    // that answer is NilClass#=='s, which pristineness pins. Every other
    // receiver falls through to the ordinary arms, so a user-defined `==`
    // still sees its nil argument. The guard also peels NilClass off the
    // operator's CallSite: a nil-terminated walk (`while n != nil`) was
    // NilClass|Node polymorphic, which a fill-once site can never serve.
    if matches!(op, BinOp::Eq | BinOp::Ne)
        && matches!(b_op.tag(), TagInfo::Known(t) if t == ValueTag::Nil as u8)
        && nil_eq_pristine(fx)
        && let Some(ta) = ta
    {
        let fast = fx.b.create_block();
        let rest = fx.b.create_block();
        let is_nil =
            fx.b.ins()
                .icmp_imm_u(IntCC::Equal, ta, i64::from(ValueTag::Nil as u8));
        fx.b.ins().brif(is_nil, fast, &[], rest, &[]);
        fx.b.switch_to_block(fast);
        let c =
            fx.b.ins()
                .iconst(types::I8, i64::from(matches!(op, BinOp::Eq)));
        if branch {
            fx.b.ins().jump(join, &[c.into()]);
        } else {
            store_bool_tag(fx, c, dst);
            fx.b.ins().jump(join, &[]);
        }
        fx.b.switch_to_block(rest);
    }
    let tb = (kb.is_none())
        .then(|| fx.b.ins().load(types::I8, fl, pb, TAG_OFFSET as i32));
    let int_tag = i64::from(ValueTag::Int as u8);
    // Per side: None = statically false (the compare is never emitted),
    // Some = the runtime bit. A statically-TRUE side contributes no test.
    let a_int = match sa_int {
        Some(false) => None,
        Some(true) => Some(None),
        None => Some(Some(fx.b.ins().icmp_imm_u(
            IntCC::Equal,
            ta.expect("unknown side loads its tag"),
            int_tag,
        ))),
    };
    let b_int_p = match sb_int {
        Some(false) => None,
        Some(true) => Some(None),
        None => Some(Some(fx.b.ins().icmp_imm_u(
            IntCC::Equal,
            tb.expect("unknown side loads its tag"),
            int_tag,
        ))),
    };
    match (b_int, a_int, b_int_p) {
        // A side statically non-Int: the int arm is dead and has no block.
        (None, _, _) => {
            fx.b.ins().jump(not_int, &[]);
        }
        (Some(b_int), Some(av), Some(bv)) => {
            let cond = match (av, bv) {
                (Some(x), Some(y)) => fx.b.ins().band(x, y),
                (Some(x), None) | (None, Some(x)) => x,
                // Both statically Int returned above.
                (None, None) => unreachable!("both-Int returns early"),
            };
            fx.b.ins().brif(cond, b_int, &[], not_int, &[]);
        }
        (Some(_), _, _) => unreachable!("int_live implies both sides possible"),
    }

    if let Some(b_int) = b_int {
        fx.b.switch_to_block(b_int);
        if branch {
            let IntShape::Compare(cc) = op.int_shape() else {
                unreachable!("branch mode admits only comparisons");
            };
            let av = fx.b.ins().load(types::I64, fl, pa, payload_off());
            let bv = fx.b.ins().load(types::I64, fl, pb, payload_off());
            let bit = fx.b.ins().icmp(cc, av, bv);
            fx.b.ins().jump(join, &[bit.into()]);
        } else {
            int_arm(fx, op, pa, pb, dst, join);
        }
    }

    fx.b.switch_to_block(not_int);
    // A Known NON-NUMERIC side (`x == nil` past the peel, `"a" + b`)
    // rules every numeric pair out: straight to the dynamic arm.
    let known_other =
        |k: Option<u8>| matches!(k, Some(t) if t != int_t && t != float_t);
    if matches!(op.float_shape(), FloatShape::None) || known_other(ka) || known_other(kb) {
        fx.b.ins().jump(b_dyn, &[]);
    } else {
        // The remaining numeric pairs, each its own arm: a MIXED pair
        // is ruby's numeric tower, not a coercion protocol call -- `1 + 2.0`
        // promotes the Int side and runs `Float#+`. Leaving them to the
        // dynamic send is what made `2 * zr` (an Int literal against a Float
        // local, in the middle of `bm_so_mandelbrot`'s inner loop) a full
        // dispatch per evaluation. A pair a Known side rules out is not
        // emitted; a Known side that satisfies a pair contributes no test.
        let float_tag = i64::from(ValueTag::Float as u8);
        let a_f = ka
            .is_none()
            .then(|| {
                fx.b.ins().icmp_imm_u(
                    IntCC::Equal,
                    ta.expect("unknown side loads its tag"),
                    float_tag,
                )
            });
        let b_f = kb
            .is_none()
            .then(|| {
                fx.b.ins().icmp_imm_u(
                    IntCC::Equal,
                    tb.expect("unknown side loads its tag"),
                    float_tag,
                )
            });
        let a_i_bit = a_int.and_then(|x| x);
        let b_i_bit = b_int_p.and_then(|x| x);
        // A MIXED pair's COMPARISON is exact in ruby (`rb_integer_float_cmp`),
        // and promoting the Int to a double is not: `2**53 + 1` and
        // `2.0**53` are different numbers that share one double, so an
        // inline `fcmp` answers `==` true. Past 2**53 the arm gives the pair
        // up to the runtime, which compares in integers. Arithmetic keeps
        // the promotion -- that IS what ruby's `1 + 2.0` does.
        let exact_pair = matches!(
            op.float_shape(),
            FloatShape::Compare(_) | FloatShape::Spaceship
        );
        for (a_is_int, b_is_int) in [(false, false), (false, true), (true, false)] {
            // A side the static tag rules out kills the pair.
            let side_dead = |is_int: bool, k: Option<u8>| match k {
                Some(t) if is_int => t != int_t,
                Some(t) => t != float_t,
                None => false,
            };
            if side_dead(a_is_int, ka) || side_dead(b_is_int, kb) {
                continue;
            }
            let mut cond: Option<cranelift_codegen::ir::Value> = None;
            for (is_int, k, ib, fb) in
                [(a_is_int, ka, a_i_bit, a_f), (b_is_int, kb, b_i_bit, b_f)]
            {
                if k.is_some() {
                    continue; // statically satisfied
                }
                let bit = if is_int {
                    ib.expect("unknown side has its int bit")
                } else {
                    fb.expect("unknown side has its float bit")
                };
                cond = Some(match cond {
                    Some(c) => fx.b.ins().band(c, bit),
                    None => bit,
                });
            }
            let cond = cond.expect("at most one Known side leaves a runtime test");
            let arm = fx.b.create_block();
            let next = fx.b.create_block();
            fx.b.ins().brif(cond, arm, &[], next, &[]);
            fx.b.switch_to_block(arm);
            if exact_pair && a_is_int != b_is_int {
                let p_int = if a_is_int { pa } else { pb };
                let i = fx.b.ins().load(types::I64, fl, p_int, payload_off());
                // `|i| <= 2**53`, as one unsigned compare on the biased value.
                let biased = fx.b.ins().iadd_imm_s(i, 1 << 53);
                let exact =
                    fx.b.ins()
                        .icmp_imm_u(IntCC::UnsignedLessThanOrEqual, biased, 1 << 54);
                let ok = fx.b.create_block();
                fx.b.ins().brif(exact, ok, &[], next, &[]);
                fx.b.switch_to_block(ok);
            }
            let av = as_f64(fx, pa, a_is_int);
            let bv = as_f64(fx, pb, b_is_int);
            if branch {
                let FloatShape::Compare(fcc) = op.float_shape() else {
                    unreachable!("branch mode admits only comparisons");
                };
                let bit = fx.b.ins().fcmp(fcc, av, bv);
                fx.b.ins().jump(join, &[bit.into()]);
            } else {
                float_arm(fx, op, av, bv, dst, join);
            }
            fx.b.switch_to_block(next);
        }
        fx.b.ins().jump(b_dyn, &[]);
    }

    fx.b.switch_to_block(b_dyn);
    {
        // A monomorphic inline cache, vetted against FCALL like the
        // operator's ruby form (operators run no visibility check). One
        // site per lowered operator: `"a" + b` in a loop fills once with
        // String and hits from then on. `send_value_in` here was a full
        // uncached walk per evaluation.
        let sym = fx.sym_id(name);
        let cache = fx.callsite_ptr(super::call::FCALL);
        // The site's OWN box, not 0: inside a `Ruby::Box` the operator has to
        // walk that box's ancestry, or a box's `include Comparable` left
        // `[1] < [2]` raising while `send(:<)` -- which threads the box --
        // answered.
        let bx = fx.box_v();
        let one = fx.b.ins().iconst(fx.em.ptr, 1);
        let null = fx.b.ins().iconst(fx.em.ptr, 0);
        let status = fx.call_status(
            "zeo_rt_send_value_cached",
            &[cache, bx, pa, sym, pb, one, null, dst],
        );
        fx.fallible(status);
        if branch {
            // The boxed result only feeds the branch: pool it and reduce
            // to its truthiness, `ownership::truthy`'s exact order. The
            // send created an owned value in `dst`; the pool consumes it,
            // so the ledger records both.
            fx.owned_created += 1;
            ownership::pool_owned(fx, dst, TagInfo::Unknown);
            let bit = fx.call_status("zeo_rt_truthy", &[dst]);
            fx.b.ins().jump(join, &[bit.into()]);
        } else {
            fx.b.ins().jump(join, &[]);
        }
    }

    fx.b.switch_to_block(join);
    if branch {
        let bit = fx.b.block_params(join)[0];
        return Ok(Operand::Bool(bit));
    }
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
