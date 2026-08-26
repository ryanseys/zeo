//! Inline call-frame sequences -- the emitted twin of `zeo-rt`'s frame
//! hot path. A prologue fetches the thread's `FrameHot` header once
//! (`zeo_rt_frame_hot`), then pushes, pops and stamps lines through plain
//! loads and stores at the `zeo_abi::abi::FRAMEHOT_*`/`FRAME_*` offsets.
//!
//! Every sequence tests `GATE_FRAMES_INDIRECT` first and falls back to
//! the capi call when it is set: the call path is what fires `TracePoint`
//! events and consumes a pending frame label, and the bit is a monotone
//! latch, so an inline PUSH can never be paired with a stale pop -- a
//! frame pushed inline and popped after the latch flips goes through the
//! traced pop, which is CRuby's own "returns fire for frames entered
//! before the trace" rule.
//!
//! `top` is never cached across a call -- a callee can grow (move) the
//! buffer -- but the HEADER address is thread-stable (fibers swap its
//! contents, never the header), so one fetch per function is sound.
//!
//! The release-pool ops stay capi calls: `zeo_rt_pool_push` carries
//! leakcheck accounting an inline store would skip. The frame push only
//! READS the pool trio (the watermark stamp), which is accounting-free.

use super::ctx::Fx;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, types};
use cranelift_module::Module;
use zeo_abi::abi as a;

/// The thread's `FrameHot` header address, fetched at most once per
/// function. Callers in the ENTRY REGION (the prologue) may fetch;
/// everyone else must use [`hot_of`] and fall back to the call form when
/// no prologue fetched it -- a fetch inside a branch arm would not
/// dominate later users.
pub(crate) fn fetch_frame_hot(fx: &mut Fx) -> ir::Value {
    if let Some(v) = fx.frame_hot {
        return v;
    }
    let v = fx.call_status("zeo_rt_frame_hot", &[]);
    fx.frame_hot = Some(v);
    v
}

/// The already-fetched header, if the prologue fetched one.
fn hot_of(fx: &Fx) -> Option<ir::Value> {
    fx.frame_hot
}

/// `gates & GATE_FRAMES_INDIRECT`, as a branchable value.
fn indirect_bit(fx: &mut Fx) -> ir::Value {
    let gv = fx
        .em
        .module
        .declare_data_in_func(fx.em.gates_id, fx.b.func);
    let base = fx.b.ins().symbol_value(fx.em.ptr, gv);
    let g = fx
        .b
        .ins()
        .load(types::I16, MemFlagsData::trusted(), base, 0);
    fx.b.ins()
        .band_imm_u(g, i64::from(a::GATE_FRAMES_INDIRECT_BIT))
}

/// The inline store sequence: bounds test (full -> the capi push, which
/// grows), the five field stores plus the pool watermark, the bump.
fn push_stores(fx: &mut Fx, hot: ir::Value, args: &[ir::Value; 6]) {
    let fl = MemFlagsData::trusted();
    let top = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_TOP as i32);
    let end = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_END as i32);
    let full = fx.b.ins().icmp(IntCC::Equal, top, end);
    let grow = fx.b.create_block();
    let store = fx.b.create_block();
    let done = fx.b.create_block();
    fx.b.set_cold_block(grow);
    fx.b.ins().brif(full, grow, &[], store, &[]);

    fx.b.switch_to_block(grow);
    fx.call("zeo_rt_frame_push", args);
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(store);
    let [file_ptr, file_len, label_ptr, label_len, line_v, end_v] = *args;
    fx.b.ins().store(fl, file_ptr, top, a::FRAME_FILE_PTR as i32);
    fx.b.ins().store(fl, file_len, top, a::FRAME_FILE_LEN as i32);
    fx.b
        .ins()
        .store(fl, label_ptr, top, a::FRAME_METHOD_PTR as i32);
    fx.b
        .ins()
        .store(fl, label_len, top, a::FRAME_METHOD_LEN as i32);
    fx.b.ins().store(fl, line_v, top, a::FRAME_LINE as i32);
    fx.b.ins().store(fl, end_v, top, a::FRAME_END_LINE as i32);
    // The pool watermark: the value COUNT between the pool trio's base
    // and top (both null before the first pooled value = count 0).
    let pt = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_POOL_TOP as i32);
    let pb = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_POOL_BASE as i32);
    let bytes = fx.b.ins().isub(pt, pb);
    let count = fx.b.ins().udiv_imm_u(bytes, a::VALUE_SIZE as i64);
    let mark = fx.b.ins().ireduce(types::I32, count);
    fx.b.ins().store(fl, mark, top, a::FRAME_POOL_MARK as i32);
    let bumped = fx.b.ins().iadd_imm_s(top, a::FRAME_SIZE as i64);
    fx.b.ins().store(fl, bumped, hot, a::FRAMEHOT_TOP as i32);
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(done);
}

/// The method/block prologue: frame push + interrupt checkpoint. The
/// gated arm is `zeo_rt_frame_enter` (push and checkpoint fused, label
/// handover and trace events included); the fast arm stores the frame
/// inline and takes the inline checkpoint. Also fetches the header for
/// the function's later pops and line stamps.
pub(crate) fn emit_frame_enter(fx: &mut Fx, args: &[ir::Value; 6]) {
    let hot = fetch_frame_hot(fx);
    let ind = indirect_bit(fx);
    let slow = fx.b.create_block();
    let fast = fx.b.create_block();
    let done = fx.b.create_block();
    fx.b.set_cold_block(slow);
    fx.b.ins().brif(ind, slow, &[], fast, &[]);

    fx.b.switch_to_block(slow);
    let status = fx.call_status("zeo_rt_frame_enter", args);
    fx.fallible(status);
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(fast);
    push_stores(fx, hot, args);
    fx.check_ints();
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(done);
}

/// A bare frame push (the svar prologue keeps its push separate from the
/// checkpoint -- the svar scope push sits between them; a `class << self`
/// group pushes mid-body). Falls back to the plain call when no PROLOGUE
/// fetched the header -- only a prologue may fetch (domination).
pub(crate) fn emit_frame_push(fx: &mut Fx, args: &[ir::Value; 6]) {
    let Some(hot) = hot_of(fx) else {
        fx.call("zeo_rt_frame_push", args);
        return;
    };
    let ind = indirect_bit(fx);
    let slow = fx.b.create_block();
    let fast = fx.b.create_block();
    let done = fx.b.create_block();
    fx.b.set_cold_block(slow);
    fx.b.ins().brif(ind, slow, &[], fast, &[]);

    fx.b.switch_to_block(slow);
    fx.call("zeo_rt_frame_push", args);
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(fast);
    push_stores(fx, hot, args);
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(done);
}

/// The epilogue pop: lower `top`, then drain the release pool to the
/// popped frame's watermark IF anything sits above it (the drain itself
/// stays the capi call -- it releases values). Falls back to the plain
/// call when the gate is set (traced pop) or no prologue fetched the
/// header (an epilogue block cannot fetch -- it would not dominate its
/// siblings).
pub(crate) fn emit_frame_pop(fx: &mut Fx) {
    let Some(hot) = hot_of(fx) else {
        fx.call("zeo_rt_frame_pop", &[]);
        return;
    };
    let ind = indirect_bit(fx);
    let slow = fx.b.create_block();
    let fast = fx.b.create_block();
    let done = fx.b.create_block();
    fx.b.set_cold_block(slow);
    fx.b.ins().brif(ind, slow, &[], fast, &[]);

    fx.b.switch_to_block(slow);
    fx.call("zeo_rt_frame_pop", &[]);
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(fast);
    let fl = MemFlagsData::trusted();
    let top = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_TOP as i32);
    let base = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_BASE as i32);
    let empty = fx.b.ins().icmp(IntCC::Equal, top, base);
    let pop = fx.b.create_block();
    fx.b.ins().brif(empty, done, &[], pop, &[]);

    fx.b.switch_to_block(pop);
    let lowered = fx.b.ins().iadd_imm_s(top, -(a::FRAME_SIZE as i64));
    fx.b.ins().store(fl, lowered, hot, a::FRAMEHOT_TOP as i32);
    let mark = fx
        .b
        .ins()
        .load(types::I32, fl, lowered, a::FRAME_POOL_MARK as i32);
    // NO_MARK (a frame that brackets no pool scope) drains nothing.
    let no_mark = fx.b.ins().icmp_imm_s(IntCC::Equal, mark, -1);
    let check = fx.b.create_block();
    fx.b.ins().brif(no_mark, done, &[], check, &[]);

    fx.b.switch_to_block(check);
    let pt = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_POOL_TOP as i32);
    let pb = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_POOL_BASE as i32);
    let bytes = fx.b.ins().isub(pt, pb);
    let count = fx.b.ins().udiv_imm_u(bytes, a::VALUE_SIZE as i64);
    let mark_w = fx.b.ins().uextend(types::I64, mark);
    let above = fx.b.ins().icmp(IntCC::UnsignedGreaterThan, count, mark_w);
    let drain = fx.b.create_block();
    fx.b.set_cold_block(drain);
    fx.b.ins().brif(above, drain, &[], done, &[]);

    fx.b.switch_to_block(drain);
    fx.call("zeo_rt_pool_reset", &[mark_w]);
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(done);
}

/// A line stamp: store to the innermost frame's `line` field. Falls back
/// to the call under the gate (the traced stamp fires `:line` events) or
/// without a fetched header.
pub(crate) fn emit_set_line(fx: &mut Fx, line_v: ir::Value) {
    let Some(hot) = hot_of(fx) else {
        fx.call("zeo_rt_set_line", &[line_v]);
        return;
    };
    let ind = indirect_bit(fx);
    let slow = fx.b.create_block();
    let fast = fx.b.create_block();
    let done = fx.b.create_block();
    fx.b.set_cold_block(slow);
    fx.b.ins().brif(ind, slow, &[], fast, &[]);

    fx.b.switch_to_block(slow);
    fx.call("zeo_rt_set_line", &[line_v]);
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(fast);
    let fl = MemFlagsData::trusted();
    let top = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_TOP as i32);
    let base = fx
        .b
        .ins()
        .load(fx.em.ptr, fl, hot, a::FRAMEHOT_BASE as i32);
    let empty = fx.b.ins().icmp(IntCC::Equal, top, base);
    let stamp = fx.b.create_block();
    fx.b.ins().brif(empty, done, &[], stamp, &[]);

    fx.b.switch_to_block(stamp);
    fx.b.ins().store(
        fl,
        line_v,
        top,
        (a::FRAME_LINE as i64 - a::FRAME_SIZE as i64) as i32,
    );
    fx.b.ins().jump(done, &[]);

    fx.b.switch_to_block(done);
}
