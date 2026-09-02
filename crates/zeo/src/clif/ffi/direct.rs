//! The direct tier: a fixed-signature `attach_function` over plain C
//! scalars, called by the emitted code on the declared C signature.
//!
//! Each Ruby argument is converted through one runtime row (ruby-ffi's own
//! range checks and error texts live there) into the C value the signature
//! wants, the C function is `call_indirect`ed, and the result is wrapped
//! inline where a register can hold it. What the emitted code cannot do
//! itself -- a NUL-terminated string copy, a Bignum, an `FFI::Pointer` --
//! is one more row.

use cranelift_codegen::ir::{self, AbiParam, InstBuilder, MemFlagsData, condcodes::IntCC, types};
use cranelift_module::Module;
use zeo_abi::ffi::CScalar;

use crate::codegen_error::CResult;
use crate::hir::FfiCall;

use super::super::ctx::Fx;
use super::super::expr::lower_expr;
use super::super::operand::{Operand, TagInfo};
use super::super::ownership;
use super::{CallSpec, TySpec};

/// A signature the emitted code can call itself: plain C scalars only.
pub(super) struct DirectSig {
    args: Vec<CScalar>,
    ret: CScalar,
}

impl DirectSig {
    /// The direct tier's qualification. A `:void` ARGUMENT is left to the
    /// libffi tier, which raises the gem's TypeError for it.
    pub(super) fn of(spec: &CallSpec) -> Option<DirectSig> {
        if spec.variadic || spec.blocking {
            return None;
        }
        let mut args = Vec::with_capacity(spec.args.len());
        for a in &spec.args {
            match a {
                TySpec::Scalar(s) if *s != CScalar::Void => args.push(*s),
                _ => return None,
            }
        }
        let TySpec::Scalar(ret) = spec.ret else {
            return None;
        };
        Some(DirectSig { args, ret })
    }
}

/// One direct-tier call: every argument expression is lowered first (so a
/// coercion that raises strands no half-built list, the libffi tier's
/// rule), then converted left to right -- the order the gem raises in --
/// and the C function is called on its own signature.
pub(super) fn lower(
    fx: &mut Fx,
    call: &FfiCall,
    sig: &DirectSig,
    addr: ir::Value,
) -> CResult<Operand> {
    let mut ops = Vec::with_capacity(call.args.len());
    for (id, _) in &call.args {
        ops.push(lower_expr(fx, *id)?);
    }
    let mut cargs = Vec::with_capacity(ops.len());
    for (op, kind) in ops.iter().zip(&sig.args) {
        let ptr = ownership::borrow_ptr(fx, op);
        if op.owned() {
            ownership::pool_owned(fx, ptr, op.tag());
        }
        cargs.push(marshal_arg(fx, *kind, ptr));
    }
    let mut csig = fx.em.module.make_signature();
    csig.params
        .extend(sig.args.iter().map(|k| c_param(*k, fx.em.ptr)));
    if sig.ret != CScalar::Void {
        csig.returns.push(AbiParam::new(c_type(sig.ret, fx.em.ptr)));
    }
    let sigref = fx.b.import_signature(csig);
    let inst = fx.b.ins().call_indirect(sigref, addr, &cargs);
    let ret = fx.b.func.dfg.inst_results(inst).first().copied();
    // A callback may have raised while C had the stack: the exception was
    // stashed, and this is where it surfaces -- after EVERY call, since
    // the C side keeps the function pointers it was handed.
    let status = fx.call_status("zeo_rt_ffi_after_call", &[]);
    fx.fallible(status);
    Ok(wrap_return(fx, sig.ret, ret))
}

/// The Cranelift type a C scalar travels in.
fn c_type(k: CScalar, ptr: types::Type) -> types::Type {
    use CScalar::*;
    match k {
        I8 | U8 | Bool => types::I8,
        I16 | U16 => types::I16,
        I32 | U32 => types::I32,
        I64 | U64 | Long | ULong => types::I64,
        F32 => types::F32,
        F64 => types::F64,
        Str | Pointer => ptr,
        Void => unreachable!("a void scalar has no register type"),
    }
}

/// A C ARGUMENT's ABI slot: a sub-32-bit integer is the caller's to
/// extend, signed or zero per the C type (the Apple arm64 rule, a no-op
/// on the System V targets).
fn c_param(k: CScalar, ptr: types::Type) -> AbiParam {
    use CScalar::*;
    let p = AbiParam::new(c_type(k, ptr));
    match k {
        I8 | I16 => p.sext(),
        U8 | U16 | Bool => p.uext(),
        _ => p,
    }
}

/// An 8-byte, 8-aligned stack word for a runtime row's scalar answer.
fn word_slot(fx: &mut Fx) -> ir::Value {
    use ir::{StackSlotData, StackSlotKind};
    let ss =
        fx.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    fx.slot_addr(ss, 0)
}

/// One Ruby argument (borrowed at `ptr`) as the C value of `kind`.
fn marshal_arg(fx: &mut Fx, kind: CScalar, ptr: ir::Value) -> ir::Value {
    use CScalar::*;
    let fl = MemFlagsData::trusted();
    let kind_v = fx.b.ins().iconst(types::I8, i64::from(kind.code()));
    match kind {
        F32 | F64 => {
            let out = word_slot(fx);
            let status = fx.call_status("zeo_rt_ffi_to_f64", &[ptr, out]);
            fx.fallible(status);
            let f = fx.b.ins().load(types::F64, fl, out, 0);
            if kind == F32 {
                fx.b.ins().fdemote(types::F32, f)
            } else {
                f
            }
        }
        Str | Pointer => {
            // A String's NUL-terminated copy is owned by `tmp`, which the
            // frame pool releases after the statement -- so it outlives
            // the call. Every other pointer leaves `tmp` nil.
            let tmp = fx.temp_slot();
            let tmp_addr = fx.slot_addr(tmp, 0);
            let out = word_slot(fx);
            let status = fx.call_status("zeo_rt_ffi_to_ptr", &[kind_v, ptr, tmp_addr, out]);
            fx.fallible(status);
            fx.owned_created += 1;
            ownership::pool_owned(fx, tmp_addr, TagInfo::Unknown);
            fx.b.ins().load(fx.em.ptr, fl, out, 0)
        }
        I8 | I16 | I32 | I64 | U8 | U16 | U32 | U64 | Long | ULong | Bool => {
            let out = word_slot(fx);
            let status = fx.call_status("zeo_rt_ffi_to_int", &[kind_v, ptr, out]);
            fx.fallible(status);
            let bits = fx.b.ins().load(types::I64, fl, out, 0);
            let ty = c_type(kind, fx.em.ptr);
            if ty == types::I64 {
                bits
            } else {
                fx.b.ins().ireduce(ty, bits)
            }
        }
        Void => unreachable!("DirectSig::of refuses a void argument"),
    }
}

/// The C result as an operand: an integer, float or bool inline; a
/// pointer, string or over-wide unsigned through its runtime wrap.
fn wrap_return(fx: &mut Fx, kind: CScalar, ret: Option<ir::Value>) -> Operand {
    use CScalar::*;
    let Some(v) = ret else {
        return Operand::Nil;
    };
    match kind {
        Void => Operand::Nil,
        I8 | I16 | I32 => Operand::Int(fx.b.ins().sextend(types::I64, v)),
        U8 | U16 | U32 => Operand::Int(fx.b.ins().uextend(types::I64, v)),
        I64 | Long => Operand::Int(v),
        U64 | ULong => wrapped(fx, "zeo_rt_ffi_from_uint", v),
        F32 => Operand::Float(fx.b.ins().fpromote(types::F64, v)),
        F64 => Operand::Float(v),
        Bool => Operand::Bool(fx.b.ins().icmp_imm_u(IntCC::NotEqual, v, 0)),
        Str => wrapped(fx, "zeo_rt_ffi_from_cstr", v),
        Pointer => wrapped(fx, "zeo_rt_ffi_from_ptr", v),
    }
}

/// A result the runtime wraps into an owned value in a fresh temp slot.
fn wrapped(fx: &mut Fx, row: &'static str, v: ir::Value) -> Operand {
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    fx.call(row, &[v, out]);
    fx.owned_created += 1;
    Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }
}
