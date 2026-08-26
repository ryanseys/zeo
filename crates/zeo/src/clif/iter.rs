//! Iterator fusion. Two shapes, and the difference between them is
//! whether the fast arm needs a guard at all.
//!
//! The LITERAL `n.times { |i| }` and `(a..b).each { |i| }` shapes take
//! `analyze::fastpath`'s predicates: a literal receiver cannot be
//! redefined at run time, so no gate and no dynamic fallback.
//!
//! `arr.each { |e| }` on a statically-`Array` local is the guarded shape:
//! the receiver's class is a compile-time BELIEF, and `Array#each` can be
//! redefined, so the site tests the tag AND asks
//! `zeo_rt_iter_inline_ok_for` before it may splice -- with a real `Proc`
//! and an ordinary block send on the other arm. The body is lowered twice
//! for that reason (spliced inline, and again as the proc's own
//! function).

use super::ctx::{Fx, LoopCtl};
use super::ownership;
use crate::codegen_error::CResult;
use crate::hir::{HirNode, NodeId};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, types};
use zeo_abi::abi::{PAYLOAD_OFFSET, TAG_OFFSET, ValueTag};

/// The two fused shapes' bounds.
pub(crate) enum Counted {
    /// `n.times`: 0 up to (exclusive) n; the loop's value is `n`.
    Times { n: i64 },
    /// `(a..b).each` / `(a...b).each`: a up to b; the loop's value is the
    /// range, which the slice only supports in DISCARDED position.
    Range {
        start: i64,
        end: i64,
        exclusive: bool,
    },
    /// `arr.each { |e| }` on a statically-`Array` receiver. The bound is
    /// re-read from the array EVERY iteration, because CRuby's `each`
    /// does: a body that pushes to the array it is walking keeps walking.
    /// `recv` is a borrowed pointer to the receiver value.
    ArrayEach { recv: ir::Value },
    /// `n.times` on a guarded typed-Int receiver: `n` is the receiver's
    /// i64 payload, loaded ONCE at entry -- an Int is immutable, and
    /// CRuby fixes the bound at entry too. A negative `n` runs zero
    /// iterations; the loop's value is the receiver.
    TimesDyn { n: ir::Value },
    /// `n.upto(m)` / `n.downto(m)` on guarded typed-Int receiver AND
    /// argument payloads, both fixed at entry (CRuby's rule). The
    /// counter runs from `n` through `limit` INCLUSIVE, stepping by the
    /// direction; a start already past the limit runs zero iterations.
    /// The loop's value is the receiver.
    UpDown {
        n: ir::Value,
        limit: ir::Value,
        down: bool,
    },
}

/// What a fused loop DOES with each iteration's block value -- the
/// accumulator seam. `None` discards it (`each`/`times`, the original
/// shapes). A consuming kind routes the body's tail and every `next v`
/// into a value slot ([`LoopCtl::next_value`]) and consumes it in the
/// LATCH -- which `redo` skips, so a redone iteration cannot be counted
/// twice, structurally.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Acc {
    /// The value is discarded.
    None,
    /// `arr.count { |e| .. }`: +1 per truthy block value; the loop's
    /// value is the count. No heap accumulator, so `break` has nothing
    /// to release.
    Count,
    /// `arr.all? { .. }`: starts true, a falsy value answers false and
    /// STOPS iterating (CRuby stops too).
    All,
    /// `arr.any? { .. }`: starts false, a truthy value answers true and
    /// stops.
    Any,
    /// `arr.none? { .. }`: starts true, a truthy value answers false and
    /// stops.
    NonePred,
    /// `arr.find { .. }` / `detect`: a truthy value answers the ORIGINAL
    /// element (re-fetched by index -- a body that reassigns its param
    /// still answers the element, CRuby's rule) and stops; exhaustion
    /// answers nil.
    Find,
    /// `arr.map { .. }` / `collect`: every block value is MOVED into a
    /// heap Array accumulator; the loop's value is the array.
    Map,
    /// `arr.select { .. }` / `filter` / `find_all`: a truthy value adds
    /// the ORIGINAL element (re-fetched by index, like `Find`).
    Select,
    /// `arr.reject { .. }`: `Select` with the test inverted.
    Reject,
    /// `arr.sum { .. }`: every value folds through `Enumerable#sum`'s
    /// own `SumAcc` ladder (`zeo_rt_sum_step`) -- one definition of
    /// the arithmetic, Kahan compensation and all. The running value
    /// rides the registered slot; the compensation and sticky-generic
    /// flag ride a raw 16-byte state slot beside it.
    Sum,
    /// `arr.inject(init) { |a, e| .. }` / `reduce`: the block's value
    /// REPLACES the accumulator each iteration; the loop's value is the
    /// final accumulator (the seed, for an empty array). The explicit
    /// seed is the nomination's own rule -- the no-argument form skips
    /// the first element, which a splice cannot.
    Inject,
}

/// How the splice binds the block's required names each iteration.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bind {
    /// At most one name: the element (`ArrayEach`) or the counter.
    Element,
    /// `each_with_index`: the element, then the plain counter.
    ElementIndex,
    /// `inject`: the running accumulator, then the element.
    AccElement,
}

impl Bind {
    fn max_params(self) -> usize {
        match self {
            Bind::Element => 1,
            Bind::ElementIndex | Bind::AccElement => 2,
        }
    }
}

impl Acc {
    fn consumes_value(self) -> bool {
        !matches!(self, Acc::None)
    }

    /// The kinds whose accumulator is (or can become) a heap value. It
    /// lives in an epilogue-registered value slot (nil at entry), so a
    /// `break` or a raise mid-body frees the half-built accumulator
    /// through the ordinary epilogue/landing release, with no explicit
    /// edge.
    fn slot_acc(self) -> bool {
        matches!(
            self,
            Acc::Map | Acc::Select | Acc::Reject | Acc::Sum | Acc::Inject
        )
    }
}

/// A consuming kind's storage, all as slot ADDRESSES.
#[derive(Clone, Copy)]
struct AccStore {
    /// The raw value slot the body's tail (and every `next v`) MOVES
    /// into; the latch consumes it.
    val: ir::Value,
    /// The accumulator: the count/answer word, or -- for the
    /// [`Acc::slot_acc`] kinds -- the registered value slot.
    acc: ir::Value,
    /// `sum` only: the compensation + sticky-generic state.
    state: Option<ir::Value>,
}

/// Whether `block` has the parameter shape a fused loop can bind: at most
/// `max_required` names (1, or 2 for the kinds that yield two values --
/// `InlineIterKind::max_fused_params`), and nothing else.
///
/// Asked by the DECISION to splice, not by the splice. Ruby binds every other
/// shape happily -- `3.times { |a, b| }` gives `b` nil -- so a block this
/// answers false for takes the ordinary block send, which binds through the
/// runtime binder and gets it right.
pub(crate) fn fusable_block(fx: &Fx<'_, '_>, block: NodeId, max_required: usize) -> bool {
    let HirNode::Block { params, .. } = &fx.an.compiler.hir[block] else {
        return false;
    };
    params.destructures.is_empty()
        && params.optional.is_empty()
        && params.rest.is_none()
        && !params.implicit_rest
        && params.post.is_empty()
        && params.keywords.is_empty()
        && params.keyword_rest.is_none()
        && params.block.is_none()
        && params.required.len() <= max_required
}

/// Lower one fused counted loop. `result` = the loop's value slot when in
/// value position (`None` = statement position, value discarded); `acc`
/// = what each iteration's block value feeds (see [`Acc`]); `bind` = how
/// the block's required names bind (see [`Bind`]); `init` = `inject`'s
/// explicit seed, lowered HERE so each execution evaluates it exactly
/// once (the slow arm lowers its own copy as the send argument).
pub(crate) fn lower_counted(
    fx: &mut Fx,
    site: NodeId,
    counted: &Counted,
    block: NodeId,
    result: Option<ir::Value>,
    acc: Acc,
    bind: Bind,
    init: Option<NodeId>,
) -> CResult<()> {
    let HirNode::Block { params, body } = &fx.an.compiler.hir[block] else {
        return fx.unsupported(site, "a non-literal block");
    };
    debug_assert!(
        fusable_block(fx, block, bind.max_params()),
        "the splice decision checks the parameter shape first"
    );
    let bound: Vec<String> = params
        .required
        .iter()
        .take(bind.max_params())
        .cloned()
        .collect();
    // A name first-assigned INSIDE the block is fresh on every invocation
    // in ruby. This splice shares the enclosing scope, where the name was
    // hoisted once, so each iteration resets it -- a conditional first
    // assignment (`x = v if cond`) must not carry into the next.
    // ...except in a run-time `eval`, where prism parsed the snippet alone
    // and marked a name block-local only because it could not see the
    // CALLER's declaration of it. A name the eval's own scope already
    // holds came from the Binding, so the caller assigned it first, and
    // ruby shares it.
    let mut implicit_locals = params.implicit_block_locals.clone();
    if fx.eval_mode.is_some() {
        implicit_locals.retain(|n| !fx.locals.contains_key(n));
    }
    // `|i; n|`: names the block DECLARES as its own. They shadow any
    // enclosing local of the same name and rebind fresh per iteration,
    // exactly like the implicit ones.
    let block_locals = params.block_locals.clone();
    // What an ESCAPING closure inside the body captures. The splice has no
    // frame of its own, so those names cannot live in a plain slot: ruby
    // binds a block parameter per invocation, and two closures built in two
    // iterations must not share one storage. They get a cell each iteration
    // instead.
    let escaping = crate::analyze::captures::collect_escaping_captures(
        &fx.an.compiler,
        body,
        params,
        fx.method_class,
    )
    .locals;
    let per_iteration_cells: Vec<String> = bound
        .iter()
        .chain(implicit_locals.iter())
        .chain(block_locals.iter())
        .filter(|n| escaping.contains(*n))
        .cloned()
        .collect();
    let body = body.clone();

    // `(start, end, end_cc)` for the counted shapes; `ArrayEach` re-reads
    // its bound per iteration instead and is tested at the head below.
    let (start, end, end_cc) = match *counted {
        Counted::Times { n } => (0, n, IntCC::SignedGreaterThanOrEqual),
        Counted::Range {
            start,
            end,
            exclusive,
        } => (
            start,
            end,
            if exclusive {
                IntCC::SignedGreaterThanOrEqual
            } else {
                IntCC::SignedGreaterThan
            },
        ),
        Counted::ArrayEach { .. } | Counted::TimesDyn { .. } | Counted::UpDown { .. } => {
            (0, 0, IntCC::SignedGreaterThanOrEqual)
        }
    };

    // The block parameter SHADOWS any enclosing local of the same
    // name; the shadow registers under a synthetic
    // key so the epilogue/landing releases it, and the visible name maps to
    // it only for the loop's extent. A name a closure escapes with is a cell
    // (replaced per iteration below); everything else is a plain slot, which
    // an escaping block may not capture.
    let mut restore: Vec<(String, Option<super::ctx::Local>)> = Vec::new();
    let bind_shadow =
        |fx: &mut Fx, restore: &mut Vec<(String, Option<super::ctx::Local>)>, name: &str| {
            let local = match per_iteration_cells.iter().any(|n| n == name) {
                true => new_cell_local(fx),
                false => super::ctx::Local::Slot(fx.new_value_slot()),
            };
            let key = format!("{name}#blk{}", fx.locals.len());
            fx.locals.insert(key, local);
            let old = fx.locals.insert(name.to_string(), local);
            if matches!(local, super::ctx::Local::Slot(_)) {
                fx.shadowed.insert(name.to_string());
            }
            restore.push((name.to_string(), old));
        };
    for name in bound.clone() {
        bind_shadow(fx, &mut restore, &name);
    }
    for name in implicit_locals.iter().chain(block_locals.iter()) {
        if per_iteration_cells.iter().any(|n| n == name) || block_locals.contains(name) {
            bind_shadow(fx, &mut restore, name);
        }
    }

    let counter =
        fx.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let mark = super::frames::emit_pool_mark(fx);
    let fl = MemFlagsData::trusted();
    let start_v = match *counted {
        // The counter begins at the RECEIVER for `upto`/`downto`.
        Counted::UpDown { n, .. } => n,
        _ => fx.b.ins().iconst(types::I64, start),
    };
    let counter_addr = fx.slot_addr(counter, 0);
    fx.b.ins().store(fl, start_v, counter_addr, 0);

    // The accumulator's storage: a raw value slot the body's tail (and
    // every `next v`) MOVES into and the latch consumes -- raw rather
    // than epilogue-registered, because the latch's release leaves it
    // dead and the epilogue must not release it again. Plus the
    // per-kind accumulator: the count word, or the boolean-answer flag
    // (`all?`/`none?` start at 1, `any?` at 0; `find` uses none -- its
    // answer is pre-written nil in the result slot, overwritten on a
    // hit), or -- for the heap-accumulator kinds -- a fresh Array in an
    // epilogue-registered slot (see [`Acc::heap_acc`]).
    let acc_slots = if acc.consumes_value() {
        let val = fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            zeo_abi::abi::VALUE_SIZE as u32,
            3,
        ));
        let val_addr = fx.slot_addr(val, 0);
        let zero = fx.b.ins().iconst(types::I64, 0);
        fx.b.ins().store(fl, zero, val_addr, 0);
        fx.b.ins().store(fl, zero, val_addr, 8);
        fx.b.ins().store(fl, zero, val_addr, 16);
        let acc_addr = if acc.slot_acc() {
            let ss = fx.new_value_slot();
            // The synthetic key registers the slot with the epilogue,
            // the way a block-param shadow does. The key survives the
            // loop, so every exit -- normal, break, raise -- releases
            // whatever the slot holds (nil once the normal exit has
            // moved the accumulator out; nil on the slow arm, which
            // never builds one).
            let key = format!("#acc{}", fx.locals.len());
            fx.locals.insert(key, super::ctx::Local::Slot(ss));
            let addr = fx.slot_addr(ss, 0);
            // A `break` abandons the accumulator IN the slot; a site
            // inside an enclosing loop re-enters here with it still
            // live, so the write is an assignment, not a fresh store.
            ownership::release_if_heap(fx, addr);
            match acc {
                // The running sum starts as Int 0 (`Enumerable#sum`'s
                // default init).
                Acc::Sum => {
                    let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
                    fx.b.ins().store(fl, tag, addr, TAG_OFFSET as i32);
                    fx.b.ins().store(fl, zero, addr, PAYLOAD_OFFSET as i32);
                }
                // `inject`'s explicit seed. A borrowed operand is
                // retained in; an owned one's consumption pairs with
                // its own creation -- the exit's move-out owns the
                // ledger +1 either way.
                Acc::Inject => {
                    let seed = init.expect("Inject carries its seed");
                    let op = super::expr::lower_expr(fx, seed)?;
                    ownership::write_move_into(fx, &op, addr);
                }
                _ => {
                    let cap = match (acc, counted) {
                        // `map` answers exactly the receiver's length.
                        (Acc::Map, Counted::ArrayEach { recv }) => {
                            fx.call_status("zeo_rt_array_len", &[*recv])
                        }
                        _ => zero,
                    };
                    // Created here, consumed by the exit's move into the
                    // result (which owns the ledger +1) or by the
                    // epilogue -- neither of which counts, so neither
                    // does this.
                    fx.call("zeo_rt_array_new", &[cap, addr]);
                }
            }
            addr
        } else {
            let count =
                fx.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
            let count_addr = fx.slot_addr(count, 0);
            let init = match acc {
                Acc::All | Acc::NonePred => fx.b.ins().iconst(types::I64, 1),
                _ => zero,
            };
            fx.b.ins().store(fl, init, count_addr, 0);
            count_addr
        };
        let state = (acc == Acc::Sum).then(|| {
            let ss = fx.b.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                16,
                3,
            ));
            let addr = fx.slot_addr(ss, 0);
            // Zero bytes = compensation 0.0 on the starting Int lane.
            fx.b.ins().store(fl, zero, addr, 0);
            fx.b.ins().store(fl, zero, addr, 8);
            addr
        });
        if matches!(acc, Acc::Find)
            && let Some(dst) = result
        {
            ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
        }
        Some(AccStore {
            val: val_addr,
            acc: acc_addr,
            state,
        })
    } else {
        None
    };

    let head = fx.b.create_block();
    let body_blk = fx.b.create_block();
    let latch = fx.b.create_block();
    let exit_normal = fx.b.create_block();
    let exit = fx.b.create_block();
    fx.b.ins().jump(head, &[]);

    fx.b.switch_to_block(head);
    let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
    let done = match *counted {
        Counted::ArrayEach { recv } => {
            let len = fx.call_status("zeo_rt_array_len", &[recv]);
            fx.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, c, len)
        }
        Counted::TimesDyn { n } => fx.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, c, n),
        Counted::UpDown { limit, down, .. } => {
            let cc = if down {
                IntCC::SignedLessThan
            } else {
                IntCC::SignedGreaterThan
            };
            fx.b.ins().icmp(cc, c, limit)
        }
        _ => fx.b.ins().icmp_imm_s(end_cc, c, end),
    };
    fx.b.ins().brif(done, exit_normal, &[], body_blk, &[]);

    fx.b.switch_to_block(body_blk);
    fx.check_ints();
    // Each iteration binds fresh storage for every escaping name, so a
    // closure built last time keeps the value it captured.
    for name in &per_iteration_cells {
        replace_cell(fx, name);
    }
    // What each required name binds this iteration. The ELEMENT binding
    // (an `array_get` clone, or the counter itself) serves the first
    // name -- except under `inject`, whose first name is the running
    // accumulator (borrowed from its slot, retained into the shadow)
    // and whose SECOND name is the element.
    let bind_element = |fx: &mut Fx, name: &String| {
        let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
        match *counted {
            Counted::ArrayEach { recv } => {
                let ss = fx.temp_slot();
                let dst = fx.slot_addr(ss, 0);
                fx.call("zeo_rt_array_get", &[recv, c, dst]);
                fx.owned_created += 1;
                let elem = super::operand::Operand::Slot {
                    ss,
                    owned: true,
                    tag: super::operand::TagInfo::Unknown,
                };
                ownership::write_local(fx, name, &elem);
            }
            _ => ownership::write_local(fx, name, &super::operand::Operand::Int(c)),
        }
    };
    match bind {
        Bind::Element | Bind::ElementIndex => {
            if let Some(name) = bound.first() {
                bind_element(fx, name);
            }
            if let Some(name) = bound.get(1) {
                // `each_with_index`'s second name: the plain counter.
                let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
                ownership::write_local(fx, name, &super::operand::Operand::Int(c));
            }
        }
        Bind::AccElement => {
            let s = acc_slots.expect("inject consumes its values");
            if let Some(name) = bound.first() {
                let cur = super::operand::Operand::Ptr {
                    addr: s.acc,
                    owned: false,
                    tag: super::operand::TagInfo::Unknown,
                };
                ownership::write_local(fx, name, &cur);
            }
            if let Some(name) = bound.get(1) {
                bind_element(fx, name);
            }
        }
    }
    for name in implicit_locals.iter().chain(block_locals.iter()) {
        // A cell was just replaced; a plain slot resets to nil.
        if matches!(fx.locals.get(name), Some(super::ctx::Local::Slot(_))) {
            ownership::write_local(fx, name, &super::operand::Operand::Nil);
        }
    }
    fx.loops.push(LoopCtl {
        exit,
        latch,
        body: body_blk,
        result,
        depth: fx.ensure_depth,
        handling: fx.handling_depth,
        next_value: acc_slots.map(|s| s.val),
    });
    match acc_slots {
        Some(s) => super::stmt::lower_value_body_into(fx, &body, s.val)?,
        None => super::stmt::lower_stmts(fx, &body)?,
    }
    fx.loops.pop();
    fx.b.ins().jump(latch, &[]);

    fx.b.switch_to_block(latch);
    // Consume the iteration's value into the accumulator FIRST -- `redo`
    // re-enters the body without passing here, so a redone iteration is
    // consumed exactly once.
    if let Some(s) = acc_slots
        && acc == Acc::Map
    {
        // Every value lands in the accumulator; the push MOVES it, which
        // is this latch's consumption of the iteration value.
        fx.call("zeo_rt_array_push", &[s.acc, s.val]);
    } else if let Some(s) = acc_slots
        && acc == Acc::Sum
    {
        // The step MOVES the value into the fold; a raise from a
        // generic `+` unwinds through the landing, which releases the
        // registered accumulator slot.
        let state = s.state.expect("Sum carries its state slot");
        let status = fx.call_status("zeo_rt_sum_step", &[s.acc, state, s.val]);
        fx.fallible(status);
    } else if let Some(s) = acc_slots
        && acc == Acc::Inject
    {
        // The block's value REPLACES the accumulator: release the old,
        // move the new bits in (the value slot is dead after).
        ownership::release_if_heap(fx, s.acc);
        for off in [0i32, 8, 16] {
            let w = fx.b.ins().load(types::I64, fl, s.val, off);
            fx.b.ins().store(fl, w, s.acc, off);
        }
    } else if let Some(AccStore {
        val: val_addr,
        acc: count_addr,
        ..
    }) = acc_slots
    {
        let t = fx.b.ins().load(types::I8, fl, val_addr, TAG_OFFSET as i32);
        let p =
            fx.b.ins()
                .load(types::I8, fl, val_addr, PAYLOAD_OFFSET as i32);
        let above_bool = fx.b.ins().icmp_imm_u(
            IntCC::UnsignedGreaterThan,
            t,
            i64::from(ValueTag::Bool as u8),
        );
        let is_bool =
            fx.b.ins()
                .icmp_imm_u(IntCC::Equal, t, i64::from(ValueTag::Bool as u8));
        let set = fx.b.ins().icmp_imm_u(IntCC::NotEqual, p, 0);
        let true_bool = fx.b.ins().band(is_bool, set);
        let truthy = fx.b.ins().bor(above_bool, true_bool);
        ownership::release_if_heap(fx, val_addr);
        match acc {
            Acc::Count => {
                let inc = fx.b.ins().uextend(types::I64, truthy);
                let n = fx.b.ins().load(types::I64, fl, count_addr, 0);
                let n1 = fx.b.ins().iadd(n, inc);
                fx.b.ins().store(fl, n1, count_addr, 0);
            }
            // The short-circuit kinds: the deciding value flips the
            // answer word and STOPS iterating -- the jump to the normal
            // exit, where the word becomes the loop's value.
            Acc::All | Acc::NonePred => {
                let cont = fx.b.create_block();
                let stop = fx.b.create_block();
                let (on_truthy, on_falsy) = match acc {
                    Acc::All => (cont, stop),
                    _ => (stop, cont),
                };
                fx.b.ins().brif(truthy, on_truthy, &[], on_falsy, &[]);
                fx.b.switch_to_block(stop);
                let zero = fx.b.ins().iconst(types::I64, 0);
                fx.b.ins().store(fl, zero, count_addr, 0);
                fx.b.ins().jump(exit_normal, &[]);
                fx.b.switch_to_block(cont);
            }
            Acc::Any => {
                let cont = fx.b.create_block();
                let stop = fx.b.create_block();
                fx.b.ins().brif(truthy, stop, &[], cont, &[]);
                fx.b.switch_to_block(stop);
                let one = fx.b.ins().iconst(types::I64, 1);
                fx.b.ins().store(fl, one, count_addr, 0);
                fx.b.ins().jump(exit_normal, &[]);
                fx.b.switch_to_block(cont);
            }
            // A hit re-fetches the ORIGINAL element at the current index
            // into the (nil-prewritten) result slot and stops.
            Acc::Find => {
                let cont = fx.b.create_block();
                let found = fx.b.create_block();
                fx.b.ins().brif(truthy, found, &[], cont, &[]);
                fx.b.switch_to_block(found);
                if let Some(dst) = result {
                    let Counted::ArrayEach { recv } = *counted else {
                        unreachable!("Find pairs only with ArrayEach");
                    };
                    let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
                    fx.call("zeo_rt_array_get", &[recv, c, dst]);
                    fx.owned_created += 1;
                }
                fx.b.ins().jump(exit_normal, &[]);
                fx.b.switch_to_block(cont);
            }
            // A kept element is re-fetched by index, like `Find`: the
            // ORIGINAL element, whatever the body did to its param.
            Acc::Select | Acc::Reject => {
                let take = fx.b.create_block();
                let cont = fx.b.create_block();
                let (on_truthy, on_falsy) = match acc {
                    Acc::Select => (take, cont),
                    _ => (cont, take),
                };
                fx.b.ins().brif(truthy, on_truthy, &[], on_falsy, &[]);
                fx.b.switch_to_block(take);
                let Counted::ArrayEach { recv } = *counted else {
                    unreachable!("Select/Reject pair only with ArrayEach");
                };
                let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
                let tmp = fx.temp_slot();
                let tmp_addr = fx.slot_addr(tmp, 0);
                fx.call("zeo_rt_array_get", &[recv, c, tmp_addr]);
                // The get creates, the push moves it straight in --
                // net zero, so neither counts in the ledger.
                fx.call("zeo_rt_array_push", &[count_addr, tmp_addr]);
                fx.b.ins().jump(cont, &[]);
                fx.b.switch_to_block(cont);
            }
            Acc::Map | Acc::Sum | Acc::Inject | Acc::None => {
                unreachable!(
                    "Map/Sum/Inject consumed above; acc_slots exist only for a consuming kind"
                )
            }
        }
    }
    let c = fx.b.ins().load(types::I64, fl, counter_addr, 0);
    let step = match *counted {
        Counted::UpDown { down: true, .. } => -1,
        _ => 1,
    };
    let c1 = fx.b.ins().iadd_imm_s(c, step);
    fx.b.ins().store(fl, c1, counter_addr, 0);
    super::frames::emit_pool_reset(fx, mark);
    fx.b.ins().jump(head, &[]);

    fx.b.switch_to_block(exit_normal);
    if let Some(dst) = result
        && let Some(AccStore {
            acc: count_addr,
            state,
            ..
        }) = acc_slots
    {
        // A consuming kind's value is its ACCUMULATOR: the count, the
        // boolean answer word, or -- for `find` -- the result slot as it
        // stands (nil, or the element a hit already wrote).
        match acc {
            Acc::Count => {
                let n = fx.b.ins().load(types::I64, fl, count_addr, 0);
                let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
                fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
                fx.b.ins().store(fl, n, dst, PAYLOAD_OFFSET as i32);
                // The site's owner of `dst` (the returned Slot) -- the
                // ledger convention `ArrayEach`'s result arm set.
                fx.owned_created += 1;
            }
            Acc::All | Acc::Any | Acc::NonePred => {
                let n = fx.b.ins().load(types::I64, fl, count_addr, 0);
                let bit = fx.b.ins().ireduce(types::I8, n);
                let tag =
                    fx.b.ins()
                        .iconst(types::I8, i64::from(ValueTag::Bool as u8));
                fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
                fx.b.ins().store(fl, bit, dst, PAYLOAD_OFFSET as i32);
                fx.owned_created += 1;
            }
            // `find`'s hit arm already counted its `array_get` write; the
            // pre-written nil costs nothing.
            Acc::Find => {}
            // The array's bits MOVE into the result and nil replaces
            // them, so the epilogue's unconditional release of the
            // registered slot frees something only on the break and
            // raise paths -- exactly-once, structurally.
            Acc::Map | Acc::Select | Acc::Reject | Acc::Inject => {
                for off in [0i32, 8, 16] {
                    let w = fx.b.ins().load(types::I64, fl, count_addr, off);
                    fx.b.ins().store(fl, w, dst, off);
                }
                let z = fx.b.ins().iconst(types::I64, 0);
                for off in [0i32, 8, 16] {
                    fx.b.ins().store(fl, z, count_addr, off);
                }
                // The site's owner of `dst` -- the same convention as
                // `Count`'s arm.
                fx.owned_created += 1;
            }
            // The finish MOVES the accumulator into the result (folding
            // the float lane's compensation in) and nils the slot for
            // the epilogue.
            Acc::Sum => {
                let state = state.expect("Sum carries its state slot");
                fx.call("zeo_rt_sum_finish", &[count_addr, state, dst]);
                fx.owned_created += 1;
            }
            Acc::None => unreachable!("acc_slots exist only for a consuming kind"),
        }
    } else if let Some(dst) = result {
        // The loop's value is its RECEIVER: the count for `times`, the range
        // itself for a range-`each`, the array itself for an array-`each`.
        // The range is rebuilt here from the same literal endpoints the
        // bounds came from -- the receiver was a literal, which is what
        // made the fusion legal in the first place.
        match *counted {
            Counted::ArrayEach { recv } => {
                let src = super::operand::Operand::Ptr {
                    addr: recv,
                    owned: false,
                    tag: super::operand::TagInfo::Known(ValueTag::Array as u8),
                };
                // A borrowed source is RETAINED into `dst`, which the
                // ledger counts as this site creating an owned value.
                ownership::write_move_into(fx, &src, dst);
                fx.owned_created += 1;
            }
            Counted::Times { n } => {
                let n_v = fx.b.ins().iconst(types::I64, n);
                let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
                fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
                fx.b.ins().store(fl, n_v, dst, PAYLOAD_OFFSET as i32);
            }
            Counted::TimesDyn { n } | Counted::UpDown { n, .. } => {
                let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
                fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
                fx.b.ins().store(fl, n, dst, PAYLOAD_OFFSET as i32);
                // `lower_counted_int`/`lower_up_down_int` count nothing
                // for their result -- this arm owns the +1, as
                // `ArrayEach`'s does. (`Times`/`Range` reach here from
                // `counted_of` callers that count the result THEMSELVES,
                // so their arms stay bare.)
                fx.owned_created += 1;
            }
            Counted::Range {
                start,
                end,
                exclusive,
            } => {
                let int_slot = |fx: &mut Fx, v: i64| {
                    let ss = fx.b.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        zeo_abi::abi::VALUE_SIZE as u32,
                        3,
                    ));
                    let addr = fx.slot_addr(ss, 0);
                    let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
                    let n = fx.b.ins().iconst(types::I64, v);
                    fx.b.ins().store(fl, tag, addr, TAG_OFFSET as i32);
                    fx.b.ins().store(fl, n, addr, PAYLOAD_OFFSET as i32);
                    addr
                };
                let a = int_slot(fx, start);
                let b = int_slot(fx, end);
                let excl = fx.b.ins().iconst(types::I8, i64::from(exclusive));
                let status = fx.call_status("zeo_rt_range_new", &[a, b, excl, dst]);
                fx.fallible(status);
            }
        }
    }
    fx.b.ins().jump(exit, &[]);

    fx.b.switch_to_block(exit);
    super::frames::emit_pool_reset(fx, mark);
    // Lexical shadowing ends with the loop.
    for (name, old) in restore {
        fx.shadowed.remove(&name);
        match old {
            Some(prev) => {
                fx.locals.insert(name, prev);
            }
            None => {
                fx.locals.remove(&name);
            }
        }
    }
    Ok(())
}

/// The fused shape's bounds when `site` is a literal `times`/range-`each`
/// call (the `analyze::fastpath` predicates), else `None`.
pub(crate) fn counted_of(
    fx: &Fx,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
) -> Option<Counted> {
    let compiler = &fx.an.compiler;
    if crate::analyze::fastpath::is_times_fast_path(compiler, receiver, name, kwargs_empty) {
        let HirNode::IntegerLit(n) = compiler.hir[receiver.expect("guarded")] else {
            unreachable!("is_times_fast_path proved a literal receiver");
        };
        return Some(Counted::Times { n });
    }
    if crate::analyze::fastpath::is_range_each_fast_path(compiler, receiver, name, kwargs_empty) {
        let HirNode::RangeLit {
            start: Some(s),
            end: Some(e),
            exclusive,
        } = compiler.hir[receiver.expect("guarded")]
        else {
            unreachable!("is_range_each_fast_path proved a literal range");
        };
        let (HirNode::IntegerLit(start), HirNode::IntegerLit(end)) =
            (&compiler.hir[s], &compiler.hir[e])
        else {
            unreachable!("is_range_each_fast_path proved literal bounds");
        };
        return Some(Counted::Range {
            start: *start,
            end: *end,
            exclusive,
        });
    }
    None
}

/// A fresh owned cell in a slot of its own, ready to hold one binding.
fn new_cell_local(fx: &mut Fx) -> super::ctx::Local {
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let cellp = fx.call_status("zeo_rt_cell_new", &[null]);
    let ss = fx.new_cell_slot();
    let dst = fx.slot_addr(ss, 0);
    fx.b.ins().store(MemFlagsData::trusted(), cellp, dst, 0);
    super::ctx::Local::Cell { ss, owned: true }
}

/// Point `name`'s cell slot at a NEW cell and drop this scope's reference to
/// the old one -- a closure that captured it holds its own. The slot always
/// holds a live cell, so the epilogue's release needs no guard.
fn replace_cell(fx: &mut Fx, name: &str) {
    let Some(super::ctx::Local::Cell { ss, .. }) = fx.locals.get(name).copied() else {
        unreachable!("an escaping name was bound as a cell");
    };
    let old = fx.cell_ptr(ss);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let fresh = fx.call_status("zeo_rt_cell_new", &[null]);
    let dst = fx.slot_addr(ss, 0);
    fx.b.ins().store(MemFlagsData::trusted(), fresh, dst, 0);
    fx.call("zeo_rt_cell_release", &[old]);
}

/// `arr.each { |e| .. }` on a statically-`Array` receiver: the fused walk
/// under a guard that PROVES the belief, with an ordinary block send on
/// the other arm.
///
/// Two questions, both asked at run time and both able to say no. The tag
/// test answers "is this really an Array" -- a static `TyKind::Array` is
/// what analyze believes, not what the value is. `iter_inline_ok_for`
/// answers "may a splice stand in for `Array#each`" -- a reopen, a
/// per-object singleton, or a box makes real dispatch the only correct
/// answer, and the fallback arm is what keeps the site working then.
///
/// `want_result` is false in statement position, where `each`'s value (the
/// receiver) is discarded -- which spares the arm a retain and a release
/// per call, and that is most of them.
pub(crate) fn lower_array_each(
    fx: &mut Fx,
    site: NodeId,
    recv_id: NodeId,
    block: NodeId,
    want_result: bool,
    acc: Acc,
    bind: Bind,
    args: &[crate::hir::ArrayElem],
    slow_name: &str,
) -> CResult<Option<super::operand::Operand>> {
    use super::operand::{Operand, TagInfo};
    // `inject`'s seed rides the argument list; each ARM lowers its own
    // copy, and only one arm runs -- one evaluation per execution.
    let init = match acc {
        Acc::Inject => match args {
            [crate::hir::ArrayElem::Single(id)] => Some(*id),
            other => unreachable!("inject fuses with exactly one plain argument, got {other:?}"),
        },
        _ => None,
    };
    // The receiver is evaluated ONCE and both arms borrow it.
    let op = super::expr::lower_expr(fx, recv_id)?;
    let recv = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        ownership::pool_owned(fx, recv, op.tag());
    }
    let result = want_result.then(|| {
        let ss = fx.temp_slot();
        (ss, fx.slot_addr(ss, 0))
    });

    let gate = fx.b.create_block();
    let fast = fx.b.create_block();
    let slow = fx.b.create_block();
    let join = fx.b.create_block();

    let fl = MemFlagsData::trusted();
    let tag = fx.b.ins().load(types::I8, fl, recv, TAG_OFFSET as i32);
    let is_array =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, tag, i64::from(ValueTag::Array as u8));
    fx.b.ins().brif(is_array, gate, &[], slow, &[]);

    fx.b.switch_to_block(gate);
    let box_v = fx.box_v();
    let array_cid =
        fx.b.ins()
            .iconst(types::I32, i64::from(crate::compiler::ARRAY_CLASS.0));
    let ok = fx.call_status("zeo_rt_iter_inline_ok_for", &[box_v, array_cid]);
    fx.b.ins().brif(ok, fast, &[], slow, &[]);

    fx.b.switch_to_block(fast);
    lower_counted(
        fx,
        site,
        &Counted::ArrayEach { recv },
        block,
        result.map(|(_, dst)| dst),
        acc,
        bind,
        init,
    )?;
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(slow);
    let borrowed = Operand::Ptr {
        addr: recv,
        owned: false,
        tag: TagInfo::Unknown,
    };
    let r = super::blocks::block_send_op(fx, site, borrowed, slow_name, args, block)?;
    match result {
        Some((_, dst)) => {
            let owned = r.owned();
            ownership::write_move_into(fx, &r, dst);
            if !owned {
                fx.owned_created += 1;
            }
        }
        None => ownership::discard(fx, r),
    }
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(join);
    Ok(result.map(|(ss, _)| Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// `n.times { |i| .. }` on a statically-`Int` receiver: the same guarded
/// two-arm shape as [`lower_array_each`] -- a tag test proves the belief,
/// `iter_inline_ok_for` proves `Integer#times` is untouched, and the
/// other arm is an ordinary block send. A Bignum receiver fails the tag
/// test and dispatches; a reopened `Integer#times` fails the gate.
pub(crate) fn lower_counted_int(
    fx: &mut Fx,
    site: NodeId,
    recv_id: NodeId,
    block: NodeId,
    want_result: bool,
) -> CResult<Option<super::operand::Operand>> {
    use super::operand::{Operand, TagInfo};
    // The receiver is evaluated ONCE and both arms borrow it.
    let op = super::expr::lower_expr(fx, recv_id)?;
    let recv = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        ownership::pool_owned(fx, recv, op.tag());
    }
    let result = want_result.then(|| {
        let ss = fx.temp_slot();
        (ss, fx.slot_addr(ss, 0))
    });

    let gate = fx.b.create_block();
    let fast = fx.b.create_block();
    let slow = fx.b.create_block();
    let join = fx.b.create_block();

    let fl = MemFlagsData::trusted();
    let tag = fx.b.ins().load(types::I8, fl, recv, TAG_OFFSET as i32);
    let is_int =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, tag, i64::from(ValueTag::Int as u8));
    fx.b.ins().brif(is_int, gate, &[], slow, &[]);

    fx.b.switch_to_block(gate);
    let box_v = fx.box_v();
    let int_cid =
        fx.b.ins()
            .iconst(types::I32, i64::from(crate::compiler::INTEGER_CLASS.0));
    let ok = fx.call_status("zeo_rt_iter_inline_ok_for", &[box_v, int_cid]);
    fx.b.ins().brif(ok, fast, &[], slow, &[]);

    fx.b.switch_to_block(fast);
    let n = fx.b.ins().load(types::I64, fl, recv, PAYLOAD_OFFSET as i32);
    lower_counted(
        fx,
        site,
        &Counted::TimesDyn { n },
        block,
        result.map(|(_, dst)| dst),
        Acc::None,
        Bind::Element,
        None,
    )?;
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(slow);
    let borrowed = Operand::Ptr {
        addr: recv,
        owned: false,
        tag: TagInfo::Unknown,
    };
    let r = super::blocks::block_send_op(fx, site, borrowed, "times", &[], block)?;
    match result {
        Some((_, dst)) => {
            let owned = r.owned();
            ownership::write_move_into(fx, &r, dst);
            if !owned {
                fx.owned_created += 1;
            }
        }
        None => ownership::discard(fx, r),
    }
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(join);
    Ok(result.map(|(ss, _)| Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// `n.upto(m)` / `n.downto(m)` on a statically-Int receiver: the guarded
/// two-arm shape of [`lower_counted_int`], with the LIMIT argument
/// tag-tested too -- a Float or Bignum bound takes the dynamic row.
///
/// The argument is lowered in the guarded arm (payload read) AND
/// re-lowered by the dynamic arm's send -- and the failing-tag path runs
/// BOTH. That is why the hook fuses only an IntegerLit/FloatLit/
/// LocalRead argument: re-lowering those is observationally identical,
/// so the double evaluation cannot be seen.
pub(crate) fn lower_up_down_int(
    fx: &mut Fx,
    site: NodeId,
    recv_id: NodeId,
    arg: &crate::hir::ArrayElem,
    block: NodeId,
    want_result: bool,
    down: bool,
    slow_name: &str,
) -> CResult<Option<super::operand::Operand>> {
    use super::operand::{Operand, TagInfo};
    let crate::hir::ArrayElem::Single(arg_id) = *arg else {
        unreachable!("the hook fuses only a plain single argument");
    };
    // The receiver is evaluated ONCE and both arms borrow it.
    let op = super::expr::lower_expr(fx, recv_id)?;
    let recv = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        ownership::pool_owned(fx, recv, op.tag());
    }
    let result = want_result.then(|| {
        let ss = fx.temp_slot();
        (ss, fx.slot_addr(ss, 0))
    });

    let gate = fx.b.create_block();
    let arg_test = fx.b.create_block();
    let fast = fx.b.create_block();
    let slow = fx.b.create_block();
    let join = fx.b.create_block();

    let fl = MemFlagsData::trusted();
    let tag = fx.b.ins().load(types::I8, fl, recv, TAG_OFFSET as i32);
    let is_int =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, tag, i64::from(ValueTag::Int as u8));
    fx.b.ins().brif(is_int, gate, &[], slow, &[]);

    fx.b.switch_to_block(gate);
    let box_v = fx.box_v();
    let int_cid =
        fx.b.ins()
            .iconst(types::I32, i64::from(crate::compiler::INTEGER_CLASS.0));
    let ok = fx.call_status("zeo_rt_iter_inline_ok_for", &[box_v, int_cid]);
    fx.b.ins().brif(ok, arg_test, &[], slow, &[]);

    // The limit: lowered in THIS arm, pooled if owned, and tag-tested.
    fx.b.switch_to_block(arg_test);
    let arg_op = super::expr::lower_expr(fx, arg_id)?;
    let arg_ptr = ownership::borrow_ptr(fx, &arg_op);
    if arg_op.owned() {
        ownership::pool_owned(fx, arg_ptr, arg_op.tag());
    }
    let atag = fx.b.ins().load(types::I8, fl, arg_ptr, TAG_OFFSET as i32);
    let arg_int =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, atag, i64::from(ValueTag::Int as u8));
    fx.b.ins().brif(arg_int, fast, &[], slow, &[]);

    fx.b.switch_to_block(fast);
    let n = fx.b.ins().load(types::I64, fl, recv, PAYLOAD_OFFSET as i32);
    let limit =
        fx.b.ins()
            .load(types::I64, fl, arg_ptr, PAYLOAD_OFFSET as i32);
    lower_counted(
        fx,
        site,
        &Counted::UpDown { n, limit, down },
        block,
        result.map(|(_, dst)| dst),
        Acc::None,
        Bind::Element,
        None,
    )?;
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(slow);
    let borrowed = Operand::Ptr {
        addr: recv,
        owned: false,
        tag: TagInfo::Unknown,
    };
    let r = super::blocks::block_send_op(
        fx,
        site,
        borrowed,
        slow_name,
        std::slice::from_ref(arg),
        block,
    )?;
    match result {
        Some((_, dst)) => {
            let owned = r.owned();
            ownership::write_move_into(fx, &r, dst);
            if !owned {
                fx.owned_created += 1;
            }
        }
        None => ownership::discard(fx, r),
    }
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(join);
    Ok(result.map(|(ss, _)| Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}
