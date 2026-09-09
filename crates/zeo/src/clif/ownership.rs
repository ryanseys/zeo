//! The ownership lowering: heap temporaries move into the
//! frame's release pool at their last use site, values written into slots
//! move or retain, and the `verify` ledger counts every owned emission
//! site against its one consumption site.

use super::ctx::{Fx, Local};
use super::operand::{Operand, TagInfo};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, types};
use zeo_abi::abi::{FIRST_HEAP_TAG, PAYLOAD_OFFSET, TAG_OFFSET, ValueTag};

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
            fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
            let v = *v;
            fx.b.ins().store(fl, v, dst, payload);
        }
        Operand::Float(v) => {
            let tag =
                fx.b.ins()
                    .iconst(types::I8, i64::from(ValueTag::Float as u8));
            fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
            let v = *v;
            fx.b.ins().store(fl, v, dst, payload);
        }
        Operand::Bool(v) => {
            let tag =
                fx.b.ins()
                    .iconst(types::I8, i64::from(ValueTag::Bool as u8));
            fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
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
        release_if_heap(fx, dst);
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
            let t = fx.b.ins().load(types::I8, fl, addr, TAG_OFFSET as i32);
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

/// Release the value at `addr` iff its runtime tag is a heap tag -- an
/// assignment's release of the old value. `zeo_rt_release` is already a
/// no-op for an immediate, so the guard buys nothing but the CALL, which
/// is the whole point: a loop that reassigns an Int local would otherwise
/// make one per iteration. A released slot's poison byte is itself above
/// the heap boundary, so a double release still reaches the runtime's check.
pub(crate) fn release_if_heap(fx: &mut Fx, addr: ir::Value) {
    let fl = MemFlagsData::trusted();
    let t = fx.b.ins().load(types::I8, fl, addr, TAG_OFFSET as i32);
    let is_heap = fx.b.ins().icmp_imm_u(
        ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
        t,
        i64::from(FIRST_HEAP_TAG),
    );
    let do_release = fx.b.create_block();
    let cont = fx.b.create_block();
    fx.b.ins().brif(is_heap, do_release, &[], cont, &[]);
    fx.b.switch_to_block(do_release);
    fx.call("zeo_rt_release", &[addr]);
    fx.b.ins().jump(cont, &[]);
    fx.b.switch_to_block(cont);
}

/// Retain the value at `addr` iff its runtime tag is a heap tag.
pub(crate) fn retain_if_heap(fx: &mut Fx, addr: ir::Value) {
    let fl = MemFlagsData::trusted();
    let t = fx.b.ins().load(types::I8, fl, addr, TAG_OFFSET as i32);
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

/// [`retain_if_heap`] for a raw trampoline body (no `Fx`): run `retains`
/// only when the value at `addr` carries a heap tag.
pub(crate) fn retain_if_heap_raw(
    b: &mut cranelift_frontend::FunctionBuilder,
    addr: ir::Value,
    retains: impl FnOnce(&mut cranelift_frontend::FunctionBuilder),
) {
    let fl = MemFlagsData::trusted();
    let t = b.ins().load(types::I8, fl, addr, TAG_OFFSET as i32);
    let is_heap = b.ins().icmp_imm_u(
        ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
        t,
        i64::from(FIRST_HEAP_TAG),
    );
    let do_retain = b.create_block();
    let cont = b.create_block();
    b.ins().brif(is_heap, do_retain, &[], cont, &[]);
    b.switch_to_block(do_retain);
    retains(b);
    b.ins().jump(cont, &[]);
    b.switch_to_block(cont);
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

/// Read local `name`: a slot local is a borrow of its storage; a cell
/// local loads a fresh owned clone.
pub(crate) fn read_local(fx: &mut Fx, name: &str) -> Option<Operand> {
    let l = *fx.locals.get(name)?;
    Some(match l {
        Local::Slot(ss) => {
            let addr = fx.slot_addr(ss, 0);
            Operand::Ptr {
                addr,
                owned: false,
                tag: TagInfo::Unknown,
            }
        }
        Local::Cell { ss, .. } => {
            let ptr = fx.cell_ptr(ss);
            let t = fx.temp_slot();
            let out = fx.slot_addr(t, 0);
            fx.call("zeo_rt_cell_load", &[ptr, out]);
            fx.owned_created += 1;
            Operand::Slot {
                ss: t,
                owned: true,
                tag: TagInfo::Unknown,
            }
        }
    })
}

/// Assign local `name` = `op` (release-old/move-in for slots; a cell
/// stores the moved value under its lock).
pub(crate) fn write_local(fx: &mut Fx, name: &str, op: &Operand) {
    let l = *fx
        .locals
        .get(name)
        .unwrap_or_else(|| panic!("local `{name}` must be hoisted"));
    match l {
        Local::Slot(ss) => {
            let dst = fx.slot_addr(ss, 0);
            write_assign(fx, op, dst);
        }
        Local::Cell { ss, .. } => {
            let ptr = fx.cell_ptr(ss);
            let mp = move_ptr(fx, op);
            fx.call("zeo_rt_cell_store", &[ptr, mp]);
        }
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
                // A Class immediate is truthy like any other non-nil.
                TagInfo::Class(_) => fx.b.ins().iconst(types::I8, 1),
                TagInfo::Known(t) if t == ValueTag::Nil as u8 => fx.b.ins().iconst(types::I8, 0),
                TagInfo::Known(_) => {
                    // Known Bool: its payload byte IS the answer (0 or 1).
                    let fl = ir::MemFlagsData::trusted();
                    let pb =
                        fx.b.ins()
                            .load(types::I8, fl, addr, zeo_abi::abi::PAYLOAD_OFFSET as i32);
                    fx.b.ins().icmp_imm_u(IntCC::NotEqual, pb, 0)
                }
                TagInfo::Unknown => {
                    // Inline ruby's truthiness over the ABI bytes: falsy is
                    // Nil (tag 0) or Bool false (tag 1, payload byte 0) --
                    // `zeo_rt_truthy` was a call per dynamic condition.
                    let fl = ir::MemFlagsData::trusted();
                    let t =
                        fx.b.ins()
                            .load(types::I8, fl, addr, zeo_abi::abi::TAG_OFFSET as i32);
                    let pb =
                        fx.b.ins()
                            .load(types::I8, fl, addr, zeo_abi::abi::PAYLOAD_OFFSET as i32);
                    let gt1 = fx.b.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, t, 1);
                    let is_bool = fx.b.ins().icmp_imm_u(IntCC::Equal, t, 1);
                    let nz = fx.b.ins().icmp_imm_u(IntCC::NotEqual, pb, 0);
                    let true_bool = fx.b.ins().band(is_bool, nz);
                    fx.b.ins().bor(gt1, true_bool)
                }
            }
        }
    }
}
