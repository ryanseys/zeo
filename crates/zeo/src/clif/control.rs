//! `begin/rescue/else/ensure/retry` -- the landing-chain lowering (plan
//! §1.4). The begin body lowers against its OWN landing; a `Raise` is
//! taken, pooled, and matched against the clause chain (`$!` bracketed per
//! clause); everything else propagates outward. An `ensure` body runs
//! inline on the normal path and under the signal-save bracket on the
//! landing path. A jump across an `ensure` boundary travels as a signal
//! and this construct settles it back onto its target once the ensure ran.

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::hir::{NodeId, RescueClause};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{self, InstBuilder, types};
use zeo_abi::abi::SignalKind;

/// Lower one `begin` construct. `result` = the value slot in value
/// position (`None` = statement position).
pub(crate) fn lower_begin(
    fx: &mut Fx,
    site: NodeId,
    body: &[NodeId],
    rescues: &[RescueClause],
    else_body: Option<&[NodeId]>,
    ensure_body: Option<&[NodeId]>,
    result: Option<ir::Value>,
) -> Result<(), String> {
    // Resolve every clause's matchers up front: the ids a baked ancestry
    // test settles, and the ones only the run time can (see [`Matcher`]).
    let mut clause_ids: Vec<(Vec<u32>, Vec<Matcher>)> = Vec::with_capacity(rescues.len());
    for clause in rescues {
        // A bare `rescue` (no listed classes AND no splats) matches
        // StandardError; a `rescue *errs` with no static classes does NOT
        // get that default -- only the splat list matters.
        if clause.classes.is_empty() && clause.splats.is_empty() {
            let std_err = fx
                .an
                .compiler
                .resolve_class("StandardError", &[], 0)
                .expect("StandardError is always registered");
            clause_ids.push((vec![std_err.0], Vec::new()));
            continue;
        }
        let mut ids = Vec::with_capacity(clause.classes.len());
        let mut dynamic = Vec::new();
        for name in &clause.classes {
            match fx.an.compiler.resolve_class(name, &[], 0) {
                // `rescue` matches via `===`, and the baked ancestry test
                // IS `Module#===` -- so it stands only while nothing can
                // override the matcher's own `===`.
                Some(cid)
                    if fx.an.compiler.class_method_in_chain(cid, "===").is_none()
                        && !fx.an.compiler.may_be_patched_at_runtime("===") =>
                {
                    ids.push(cid.0);
                }
                Some(cid) => dynamic.push(Matcher::Class(cid.0)),
                None => dynamic.push(Matcher::Const(name.clone())),
            }
        }
        dynamic.extend(clause.splats.iter().map(|&n| Matcher::Splat(n)));
        clause_ids.push((ids, dynamic));
    }

    let outer_land = fx.land;
    let done = fx.b.create_block();
    // With an ensure, EVERY outward path detours through the save/run/
    // restore bracket; without one, paths go straight to the outer landing.
    let (propagate, ensure_normal) = if ensure_body.is_some() {
        (fx.b.create_block(), Some(fx.b.create_block()))
    } else {
        (outer_land, None)
    };
    let begin_land = fx.b.create_block();
    let begin_head = fx.b.create_block();
    fx.b.ins().jump(begin_head, &[]);
    fx.b.switch_to_block(begin_head);

    // The loop a jump out of this begin would have gone to directly. Only
    // one at the SAME ensure depth is ours: a farther one has another
    // ensure in between, and that begin settles it.
    let loop_target = if ensure_body.is_some() {
        fx.loops
            .last()
            .filter(|ctl| ctl.depth == fx.ensure_depth)
            .map(|ctl| Settle {
                exit: ctl.exit,
                latch: ctl.latch,
                body: ctl.body,
                result: ctl.result,
                handling: ctl.handling,
            })
    } else {
        None
    };
    let jumps_before = fx.ensure_jumps;

    // The body (and `else`) lower against the begin's landing; a direct
    // jump out of an ensure-carrying begin becomes a signal (the depth
    // stamp) so the bracket below runs before it reaches its target.
    fx.land = begin_land;
    if ensure_body.is_some() {
        fx.ensure_depth += 1;
    }
    let body_result = if else_body.is_some() { None } else { result };
    lower_section(fx, body, body_result)?;
    if let Some(else_body) = else_body {
        lower_section(fx, else_body, result)?;
    }
    fx.land = outer_land;
    match ensure_normal {
        Some(en) => {
            fx.b.ins().jump(en, &[]);
        }
        None => {
            fx.b.ins().jump(done, &[]);
        }
    }

    // The begin landing: a Raise is taken and matched; everything else
    // propagates (through the ensure bracket when one exists). The whole
    // landing region -- matchers and clause bodies alike -- still sits
    // inside the ensure's reach, so it lands on the bracket, not outside.
    fx.b.switch_to_block(begin_land);
    fx.land = propagate;
    let kind = fx.call("zeo_rt_signal_kind", &[]).expect("kind answers");
    let is_raise =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, kind, i64::from(SignalKind::Raise as u8));
    let b_take = fx.b.create_block();
    fx.b.ins().brif(is_raise, b_take, &[], propagate, &[]);

    fx.b.switch_to_block(b_take);
    let exc_ss = fx.temp_slot();
    let exc = fx.slot_addr(exc_ss, 0);
    fx.call("zeo_rt_signal_take", &[exc]);
    // The pool owns the exception from here; every use below borrows.
    fx.owned_created += 1;
    ownership::pool_owned(fx, exc, TagInfo::Unknown);

    let no_match = fx.b.create_block();
    for (clause, matchers) in rescues.iter().zip(&clause_ids.clone()) {
        let m = clause_match(fx, site, exc, &matchers.0, &matchers.1)?;
        let clause_blk = fx.b.create_block();
        let next = fx.b.create_block();
        fx.b.ins().brif(m, clause_blk, &[], next, &[]);

        fx.b.switch_to_block(clause_blk);
        // `$!` holds the exception while the clause runs -- popped on BOTH
        // exits (the clause's own landing pops before propagating), and by
        // any direct jump out (`Fx::pop_handling_to`).
        fx.call("zeo_rt_handling_push", &[exc]);
        fx.handling_depth += 1;
        if let Some(binding) = &clause.binding {
            let borrowed = Operand::Ptr {
                addr: exc,
                owned: false,
                tag: TagInfo::Unknown,
            };
            ownership::write_local(fx, binding, &borrowed);
        }
        let clause_land = fx.b.create_block();
        let saved_land = fx.land;
        fx.land = clause_land;
        fx.retries
            .push((begin_head, fx.ensure_depth, fx.handling_depth - 1));
        lower_section(fx, &clause.body, result)?;
        fx.retries.pop();
        fx.land = saved_land;
        fx.call("zeo_rt_handling_pop", &[]);
        fx.handling_depth -= 1;
        match ensure_normal {
            Some(en) => {
                fx.b.ins().jump(en, &[]);
            }
            None => {
                fx.b.ins().jump(done, &[]);
            }
        }
        fx.b.switch_to_block(clause_land);
        fx.call("zeo_rt_handling_pop", &[]);
        fx.b.ins().jump(propagate, &[]);

        fx.b.switch_to_block(next);
    }
    // No clause matched: re-arm the pending Raise with a fresh reference
    // (the pool keeps its own) and propagate.
    fx.b.ins().jump(no_match, &[]);
    fx.b.switch_to_block(no_match);
    let raise_kind =
        fx.b.ins()
            .iconst(types::I8, i64::from(SignalKind::Raise as u8));
    fx.call("zeo_rt_retain", &[exc]);
    fx.call("zeo_rt_signal_set", &[raise_kind, exc]);
    fx.b.ins().jump(propagate, &[]);

    // Past the landing region: the ensure bracket itself is outside the
    // begin's own reach again.
    fx.land = outer_land;
    if ensure_body.is_some() {
        fx.ensure_depth -= 1;
    }

    // The ensure bracket, both flavors.
    if let Some(ensure_stmts) = ensure_body {
        // Normal path: the ensure body runs inline (its own signals go to
        // the OUTER landing, and its own jumps go straight to their target
        // -- nothing is left to run).
        let en = ensure_normal.expect("created with ensure_body");
        fx.b.switch_to_block(en);
        lower_section(fx, ensure_stmts, None)?;
        fx.b.ins().jump(done, &[]);

        // Signal path: save, run the ensure under the $!-propagation
        // bracket, restore, propagate. An ensure body that itself signals
        // lands in `efail`: leave the bracket, DROP the saved signal
        // (CRuby: the newer signal wins), propagate its own.
        fx.b.switch_to_block(propagate);
        let saved = fx.call("zeo_rt_signal_save", &[]).expect("saved handle");
        let pushed = fx
            .call("zeo_rt_propagating_enter", &[saved])
            .expect("enter answers");
        let efail = fx.b.create_block();
        let saved_land = fx.land;
        fx.land = efail;
        fx.ensure_depth += 1;
        lower_section(fx, ensure_stmts, None)?;
        fx.ensure_depth -= 1;
        fx.land = saved_land;
        // Only now is it known whether a jump inside this begin (its body,
        // a clause, or the ensure itself) armed a signal for the enclosing
        // loop. Without one, nothing here may claim a passing signal.
        let settle = (fx.ensure_jumps > jumps_before)
            .then_some(loop_target)
            .flatten()
            .map(|t| (fx.b.create_block(), t));
        let after = settle.map_or(outer_land, |(blk, _)| blk);
        fx.call("zeo_rt_propagating_leave", &[pushed]);
        fx.call("zeo_rt_signal_restore", &[saved]);
        fx.b.ins().jump(after, &[]);

        fx.b.switch_to_block(efail);
        fx.call("zeo_rt_propagating_leave", &[pushed]);
        fx.call("zeo_rt_signal_drop", &[saved]);
        fx.b.ins().jump(after, &[]);

        if let Some((settle_blk, target)) = settle {
            lower_settle(fx, settle_blk, outer_land, target);
        }
    }

    fx.b.switch_to_block(done);
    Ok(())
}

/// The enclosing native loop a jump out of an ensure-carrying `begin` was
/// aiming at: its own `break`/`next`/`redo` targets, value slot, and `$!`
/// depth, read off the [`super::ctx::LoopCtl`] the jump could not reach.
#[derive(Clone, Copy)]
struct Settle {
    exit: ir::Block,
    latch: ir::Block,
    body: ir::Block,
    result: Option<ir::Value>,
    handling: usize,
}

/// Turn the signal an ensure-crossing jump armed back into the jump it
/// was: the ensure has run, so `Break` supplies the loop's value and
/// leaves, `Next` discards its own and reaches the latch, and `Redo`
/// re-enters the body. Anything else is a real unwind and passes through.
fn lower_settle(fx: &mut Fx, settle: ir::Block, outer_land: ir::Block, target: Settle) {
    fx.b.switch_to_block(settle);
    let kind = fx.call("zeo_rt_signal_kind", &[]).expect("kind answers");
    let arm = |fx: &mut Fx, k: SignalKind| {
        let taken = fx.b.create_block();
        let rest = fx.b.create_block();
        let is =
            fx.b.ins()
                .icmp_imm_u(IntCC::Equal, kind, i64::from(k as u8));
        fx.b.ins().brif(is, taken, &[], rest, &[]);
        fx.b.switch_to_block(rest);
        taken
    };
    let b_break = arm(fx, SignalKind::Break);
    let b_next = arm(fx, SignalKind::Next);
    let b_redo = arm(fx, SignalKind::Redo);
    fx.b.ins().jump(outer_land, &[]);

    // `break v`: v IS the loop's value in value position.
    fx.b.switch_to_block(b_break);
    let value = take_signal_value(fx);
    match target.result {
        Some(dst) => ownership::write_move_into(fx, &value, dst),
        None => ownership::discard(fx, value),
    }
    fx.pop_handling_to(target.handling);
    fx.b.ins().jump(target.exit, &[]);

    // `next v`: a native loop has nowhere to send the value.
    fx.b.switch_to_block(b_next);
    let value = take_signal_value(fx);
    ownership::discard(fx, value);
    fx.pop_handling_to(target.handling);
    fx.b.ins().jump(target.latch, &[]);

    // `redo` carries nothing, so clearing the slot is a save-and-drop.
    fx.b.switch_to_block(b_redo);
    let saved = fx.call("zeo_rt_signal_save", &[]).expect("saved handle");
    fx.call("zeo_rt_signal_drop", &[saved]);
    fx.pop_handling_to(target.handling);
    fx.b.ins().jump(target.body, &[]);
}

/// Move the pending signal's payload into a fresh temp, owned.
fn take_signal_value(fx: &mut Fx) -> Operand {
    let ss = fx.temp_slot();
    let addr = fx.slot_addr(ss, 0);
    fx.call("zeo_rt_signal_take", &[addr]);
    fx.owned_created += 1;
    Operand::Ptr {
        addr,
        owned: true,
        tag: TagInfo::Unknown,
    }
}

/// One rescue matcher only the RUN TIME can settle. Ruby evaluates a
/// clause's class expression while MATCHING, not while compiling, so each
/// of these asks then -- and its own error (an undefined constant, a
/// non-Module element) propagates from there, exactly as ruby's does.
#[derive(Clone)]
enum Matcher {
    /// A resolved class whose `===` may be overridden: keep the identity
    /// static, re-ask through the runtime so the override is honoured.
    Class(u32),
    /// A name that resolves to no class HERE but may hold one then
    /// (`ALIAS = Base`, `Foo = Class.new`).
    Const(String),
    /// `rescue *errs` -- an Array of classes, or a single one.
    Splat(NodeId),
}

/// One clause's `||`-joined match: the baked ancestry test over the static
/// ids first (a single call), then each runtime matcher, short-circuiting.
fn clause_match(
    fx: &mut Fx,
    site: NodeId,
    exc: ir::Value,
    ids: &[u32],
    dynamic: &[Matcher],
) -> Result<ir::Value, String> {
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let fl = ir::MemFlagsData::trusted();
    let hit = fx.b.create_block();
    let join = fx.b.create_block();
    let zero = fx.b.ins().iconst(types::I8, 0);
    fx.b.ins().store(fl, zero, out, 0);
    if !ids.is_empty() {
        let id_bytes: Vec<u8> = ids.iter().flat_map(|i| i.to_le_bytes()).collect();
        let off = fx.em.intern_rodata_aligned(&id_bytes, 4);
        let ids_ptr = fx.rod(off);
        let n = fx.b.ins().iconst(fx.em.ptr, ids.len() as i64);
        let m = fx
            .call("zeo_rt_rescue_matches", &[exc, ids_ptr, n])
            .expect("rescue_matches answers");
        let next = fx.b.create_block();
        fx.b.ins().brif(m, hit, &[], next, &[]);
        fx.b.switch_to_block(next);
    }
    for matcher in dynamic {
        let val = match matcher {
            Matcher::Class(cid) => super::expr::class_immediate(fx, crate::compiler::ClassId(*cid)),
            Matcher::Const(name) => {
                let name = name.clone();
                super::expr::const_path_read(fx, site, &name)?
            }
            Matcher::Splat(node) => super::expr::lower_expr(fx, *node)?,
        };
        let ptr = ownership::borrow_ptr(fx, &val);
        if val.owned() {
            ownership::pool_owned(fx, ptr, val.tag());
        }
        let m_ss = fx.temp_slot();
        let m_ptr = fx.slot_addr(m_ss, 0);
        let status = fx
            .call("zeo_rt_rescue_matches_any", &[exc, ptr, m_ptr])
            .expect("rescue_matches_any returns a status");
        fx.fallible(status);
        let m = fx.b.ins().load(types::I8, fl, m_ptr, 0);
        let next = fx.b.create_block();
        fx.b.ins().brif(m, hit, &[], next, &[]);
        fx.b.switch_to_block(next);
    }
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(hit);
    let one = fx.b.ins().iconst(types::I8, 1);
    fx.b.ins().store(fl, one, out, 0);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    Ok(fx.b.ins().load(types::I8, fl, out, 0))
}

/// A begin section: statements, with the tail as a value when `result`/// A begin section: statements, with the tail as a value when `result`
/// asks for one.
fn lower_section(fx: &mut Fx, stmts: &[NodeId], result: Option<ir::Value>) -> Result<(), String> {
    match result {
        Some(dst) => super::stmt::lower_value_body_into(fx, stmts, dst),
        None => super::stmt::lower_stmts(fx, stmts),
    }
}
