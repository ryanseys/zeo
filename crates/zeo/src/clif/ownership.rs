//! The ownership lowering (plan §1.4): heap temporaries move into the
//! frame's release pool at their last use site, values written into slots
//! move or retain, and the `verify` ledger counts every owned emission
//! site against its one consumption site.

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, types};
use zeo_abi::abi::{FIRST_HEAP_TAG, PAYLOAD_OFFSET, ValueTag};

/// Write `op`'s tag+payload to memory at `dst` (no ownership transfer --
/// the raw store both `write_borrow` and `move_into` build on).
fn store_bits(fx: &mut Fx, op: &Operand, dst: ir::Value) {
    let fl = MemFlagsData::trusted();
    let payload = PAYLOAD_OFFSET as i32;
    match op {
        Operand::Nil => {
            // Zero the whole slot: a Nil's bits must be releasable and
            // copyable like any value.
            let z = fx.b.ins().iconst(types::I64, 0);
            for off in [0, 8, 16] {
                fx.b.ins().store(fl, z, dst, off);
            }
        }
        Operand::Int(v) => {
            let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
            fx.b.ins().store(fl, tag, dst, 0);
            let v = *v;
            fx.b.ins().store(fl, v, dst, payload);
        }
        Operand::Float(v) => {
            let tag =
                fx.b.ins()
                    .iconst(types::I8, i64::from(ValueTag::Float as u8));
            fx.b.ins().store(fl, tag, dst, 0);
            let v = *v;
            fx.b.ins().store(fl, v, dst, payload);
        }
        Operand::Bool(v) => {
            let tag =
                fx.b.ins()
                    .iconst(types::I8, i64::from(ValueTag::Bool as u8));
            fx.b.ins().store(fl, tag, dst, 0);
            let v = *v;
            fx.b.ins().store(fl, v, dst, payload);
        }
        Operand::Slot { .. } | Operand::Ptr { .. } => {
            let src = addr_of(fx, op);
            for off in [0, 8, 16] {
                let w = fx.b.ins().load(types::I64, fl, src, off);
                fx.b.ins().store(fl, w, dst, off);
            }
        }
    }
}

/// The address of a boxed operand's bytes (immediates have none).
pub(crate) fn addr_of(fx: &mut Fx, op: &Operand) -> ir::Value {
    match op {
        Operand::Slot { ss, .. } => fx.slot_addr(*ss, 0),
        Operand::Ptr { addr, .. } => *addr,
        Operand::Nil | Operand::Int(_) | Operand::Float(_) | Operand::Bool(_) => {
            unreachable!("addr_of on an unboxed operand -- materialize first")
        }
    }
}

/// `op` as a `*const Value` the callee BORROWS: boxed operands hand over
/// their address; unboxed ones are written to a temp slot first. Ownership
/// is untouched.
pub(crate) fn borrow_ptr(fx: &mut Fx, op: &Operand) -> ir::Value {
    match op {
        Operand::Slot { .. } | Operand::Ptr { .. } => addr_of(fx, op),
        Operand::Nil | Operand::Int(_) | Operand::Float(_) | Operand::Bool(_) => {
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            store_bits(fx, op, dst);
            dst
        }
    }
}

/// Copy `op` into `dst` WITHOUT taking its ownership -- the argv-array
/// write (the callee borrows argv; the value stays owned wherever it was).
pub(crate) fn write_borrow(fx: &mut Fx, op: &Operand, dst: ir::Value) {
    store_bits(fx, op, dst);
}

/// An assignment's write: the destination's old value is released and the
/// new value MOVES in (a borrowed source is retained first -- before the
/// release, so `x = x` never touches a dead value).
pub(crate) fn write_assign(fx: &mut Fx, op: &Operand, dst: ir::Value) {
    take_into(fx, op, dst, true);
}

/// Move `op` into FRESH storage at `dst` (no old value to release) -- an
/// `out` slot, an if-expression's result, a param copy.
pub(crate) fn write_move_into(fx: &mut Fx, op: &Operand, dst: ir::Value) {
    take_into(fx, op, dst, false);
}

fn take_into(fx: &mut Fx, op: &Operand, dst: ir::Value, release_dst: bool) {
    if let Operand::Slot { owned: false, .. } | Operand::Ptr { owned: false, .. } = op {
        match op.tag().heap() {
            Some(true) => {
                let src = addr_of(fx, op);
                fx.call("zeo_rt_retain", &[src]);
            }
            Some(false) => {}
            None => {
                let src = addr_of(fx, op);
                retain_if_heap(fx, src);
            }
        }
    }
    if op.owned() {
        fx.owned_consumed += 1;
    }
    if release_dst {
        fx.call("zeo_rt_release", &[dst]);
    }
    store_bits(fx, op, dst);
}

/// Transfer an OWNED boxed value at `addr` into the frame's release pool
/// (immediates are skipped -- statically when the tag is known, by a
/// runtime tag test otherwise). The bytes at `addr` remain a valid borrow
/// until the pool drains at frame pop / loop latch.
pub(crate) fn pool_owned(fx: &mut Fx, addr: ir::Value, tag: TagInfo) {
    fx.owned_consumed += 1;
    match tag.heap() {
        Some(true) => {
            fx.call("zeo_rt_pool_push", &[addr]);
        }
        Some(false) => {}
        None => {
            let fl = MemFlagsData::trusted();
            let t = fx.b.ins().load(types::I8, fl, addr, 0);
            let is_heap = fx.b.ins().icmp_imm_u(
                ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
                t,
                i64::from(FIRST_HEAP_TAG),
            );
            let do_pool = fx.b.create_block();
            let cont = fx.b.create_block();
            fx.b.ins().brif(is_heap, do_pool, &[], cont, &[]);
            fx.b.switch_to_block(do_pool);
            fx.call("zeo_rt_pool_push", &[addr]);
            fx.b.ins().jump(cont, &[]);
            fx.b.switch_to_block(cont);
        }
    }
}

/// Retain the value at `addr` iff its runtime tag is a heap tag.
pub(crate) fn retain_if_heap(fx: &mut Fx, addr: ir::Value) {
    let fl = MemFlagsData::trusted();
    let t = fx.b.ins().load(types::I8, fl, addr, 0);
    let is_heap = fx.b.ins().icmp_imm_u(
        ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
        t,
        i64::from(FIRST_HEAP_TAG),
    );
    let do_retain = fx.b.create_block();
    let cont = fx.b.create_block();
    fx.b.ins().brif(is_heap, do_retain, &[], cont, &[]);
    fx.b.switch_to_block(do_retain);
    fx.call("zeo_rt_retain", &[addr]);
    fx.b.ins().jump(cont, &[]);
    fx.b.switch_to_block(cont);
}

/// `op` as a pointer whose pointee the callee will MOVE from
/// (`ivar_set_slot`'s convention). An owned operand hands over its own
/// storage; a borrowed one is copied to a temp and retained (moving from
/// the original would kill the local it borrows); unboxed values
/// materialize plainly.
pub(crate) fn move_ptr(fx: &mut Fx, op: &Operand) -> ir::Value {
    match op {
        Operand::Slot { owned: true, .. } | Operand::Ptr { owned: true, .. } => {
            fx.owned_consumed += 1;
            addr_of(fx, op)
        }
        Operand::Slot { owned: false, .. } | Operand::Ptr { owned: false, .. } => {
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            store_bits(fx, op, dst);
            match op.tag().heap() {
                Some(true) => {
                    fx.call("zeo_rt_retain", &[dst]);
                }
                Some(false) => {}
                None => retain_if_heap(fx, dst),
            }
            dst
        }
        Operand::Nil | Operand::Int(_) | Operand::Float(_) | Operand::Bool(_) => borrow_ptr(fx, op),
    }
}

/// Evaluate-and-ignore: an owned result is pooled (heap) or simply
/// forgotten (immediate); borrowed and unboxed results need nothing.
pub(crate) fn discard(fx: &mut Fx, op: Operand) {
    if op.owned() {
        let tag = op.tag();
        let addr = addr_of(fx, &op);
        pool_owned(fx, addr, tag);
    }
}

/// `op` as an i8 truthiness value (0 = falsy). Ownership of a boxed
/// operand is handed to the pool first -- the test only needs the tag.
pub(crate) fn truthy(fx: &mut Fx, op: Operand) -> ir::Value {
    match op {
        Operand::Nil => fx.b.ins().iconst(types::I8, 0),
        Operand::Int(_) | Operand::Float(_) => fx.b.ins().iconst(types::I8, 1),
        Operand::Bool(v) => v,
        Operand::Slot { .. } | Operand::Ptr { .. } => {
            let tag = op.tag();
            let addr = addr_of(fx, &op);
            if op.owned() {
                pool_owned(fx, addr, tag);
            }
            match tag {
                TagInfo::Known(t) if t != ValueTag::Nil as u8 && t != ValueTag::Bool as u8 => {
                    fx.b.ins().iconst(types::I8, 1)
                }
                TagInfo::Known(_) | TagInfo::Unknown => fx
                    .call("zeo_rt_truthy", &[addr])
                    .expect("truthy returns i8"),
            }
        }
    }
}
