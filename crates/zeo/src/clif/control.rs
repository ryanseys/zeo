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
use crate::codegen_error::CResult;
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
) -> CResult<()> {
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
            // The clause's own CREF, not the top level: a method copied onto
            // a subclass keeps the cref it was WRITTEN in, so `rescue Boom`
            // inside `M::Base#go` still names `M::Boom` when `Sub` runs it.
            match super::boxes::resolve_class_here(fx, name) {
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

    let settle_target = if ensure_body.is_some() {
        settle_target(fx)
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
    let kind = fx.call_status("zeo_rt_signal_kind", &[]);
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
        // `Signal::Terminate` (fiber/enumerator teardown) SKIPS the user
        // ensure body: the old force-unwind ran no ruby `ensure` either, and
        // the whole point of the teardown signal is that only releases run.
        {
            use cranelift_codegen::ir::condcodes::IntCC;
            let kind = fx.call_status("zeo_rt_signal_kind", &[]);
            let is_terminate = fx.b.ins().icmp_imm_u(
                IntCC::Equal,
                kind,
                zeo_abi::abi::SignalKind::Terminate as i64,
            );
            let run = fx.b.create_block();
            fx.b.ins().brif(is_terminate, outer_land, &[], run, &[]);
            fx.b.switch_to_block(run);
        }
        let saved = fx.call("zeo_rt_signal_save", &[]).expect("saved handle");
        let pushed = fx.call_status("zeo_rt_propagating_enter", &[saved]);
        let efail = fx.b.create_block();
        let saved_land = fx.land;
        fx.land = efail;
        fx.ensure_depth += 1;
        lower_section(fx, ensure_stmts, None)?;
        fx.ensure_depth -= 1;
        fx.land = saved_land;
        // Only now is it known whether a jump inside this begin (its body,
        // a clause, or the ensure itself) armed a signal for the enclosing
        // target. Without one, nothing here may claim a passing signal.
        let settle = (fx.ensure_jumps > jumps_before)
            .then_some(settle_target)
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

/// Where each jump that crossed an `ensure` lands once it has run -- the
/// targets the jump itself could not reach, read off the enclosing
/// [`super::ctx::LoopCtl`] or, inside an escaping block, off the block's
/// own boundary.
#[derive(Clone, Copy)]
struct Settle {
    /// `break`'s target and the slot its value fills. `None` = it keeps
    /// travelling: an escaping block's `break` belongs to the call site's
    /// `catch_break`, not to anything lexically here.
    brk: Option<(ir::Block, Option<ir::Value>)>,
    /// `next`'s target and the slot its value fills (a native loop has
    /// nowhere to send one; a block's `next` IS its return value).
    nxt: (ir::Block, Option<ir::Value>),
    /// Where `redo` re-enters.
    rdo: Option<ir::Block>,
    /// The `$!` depth of that target -- the settle pops down to it.
    handling: usize,
}

/// The target an ensure-crossing jump inside the begin about to be lowered
/// was aiming at. A native loop only counts at the SAME ensure depth: a
/// farther one has another ensure in between, and THAT begin settles it.
/// With no loop in reach, an escaping block's own boundary is the target.
fn settle_target(fx: &Fx) -> Option<Settle> {
    if let Some(ctl) = fx.loops.last() {
        return (ctl.depth == fx.ensure_depth).then_some(Settle {
            brk: Some((ctl.exit, ctl.result)),
            // A fused accumulator loop's latch consumes the iteration
            // value, so an ensure-crossing `next v` settles v into the
            // loop's value slot on its way there.
            nxt: (ctl.latch, ctl.next_value),
            rdo: Some(ctl.body),
            handling: ctl.handling,
        });
    }
    let (out, ret_ok) = fx.block_next?;
    (fx.ensure_depth == 0).then_some(Settle {
        brk: None,
        nxt: (ret_ok, Some(out)),
        rdo: fx.block_redo,
        handling: 0,
    })
}

/// Turn the signal an ensure-crossing jump armed back into the jump it
/// was, now that the ensure has run. Anything the target does not claim is
/// a real unwind and passes through.
fn lower_settle(fx: &mut Fx, settle: ir::Block, outer_land: ir::Block, target: Settle) {
    fx.b.switch_to_block(settle);
    let kind = fx.call_status("zeo_rt_signal_kind", &[]);
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
    let b_break = target.brk.map(|t| (arm(fx, SignalKind::Break), t));
    let b_next = (arm(fx, SignalKind::Next), target.nxt);
    let b_redo = target.rdo.map(|t| (arm(fx, SignalKind::Redo), t));
    fx.b.ins().jump(outer_land, &[]);

    for (blk, (dest, dst)) in [b_break, Some(b_next)].into_iter().flatten() {
        fx.b.switch_to_block(blk);
        let value = take_signal_value(fx);
        match dst {
            Some(dst) => ownership::write_move_into(fx, &value, dst),
            None => ownership::discard(fx, value),
        }
        fx.pop_handling_to(target.handling);
        fx.b.ins().jump(dest, &[]);
    }

    // `redo` carries nothing, so clearing the slot is a save-and-drop.
    if let Some((blk, dest)) = b_redo {
        fx.b.switch_to_block(blk);
        let saved = fx.call("zeo_rt_signal_save", &[]).expect("saved handle");
        fx.call("zeo_rt_signal_drop", &[saved]);
        fx.pop_handling_to(target.handling);
        fx.b.ins().jump(dest, &[]);
    }
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
) -> CResult<ir::Value> {
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let fl = ir::MemFlagsData::trusted();
    let hit = fx.b.create_block();
    let join = fx.b.create_block();
    let zero = fx.b.ins().iconst(types::I8, 0);
    fx.b.ins().store(fl, zero, out, 0);
    if !ids.is_empty() {
        let (ids_ptr, n) = fx.cid_array(ids);
        let m = fx.call_status("zeo_rt_rescue_matches", &[exc, ids_ptr, n]);
        let next = fx.b.create_block();
        fx.b.ins().brif(m, hit, &[], next, &[]);
        fx.b.switch_to_block(next);
    }
    for matcher in dynamic {
        let val = match matcher {
            Matcher::Class(cid) => {
                super::consts::class_immediate(fx, crate::compiler::ClassId(*cid))
            }
            Matcher::Const(name) => {
                let name = name.clone();
                super::consts::const_path_read(fx, site, &name)?
            }
            Matcher::Splat(node) => super::expr::lower_expr(fx, *node)?,
        };
        let ptr = ownership::borrow_ptr(fx, &val);
        if val.owned() {
            ownership::pool_owned(fx, ptr, val.tag());
        }
        let m_ss = fx.temp_slot();
        let m_ptr = fx.slot_addr(m_ss, 0);
        let status = fx.call_status("zeo_rt_rescue_matches_any", &[exc, ptr, m_ptr]);
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
fn lower_section(fx: &mut Fx, stmts: &[NodeId], result: Option<ir::Value>) -> CResult<()> {
    match result {
        Some(dst) => super::stmt::lower_value_body_into(fx, stmts, dst),
        None => super::stmt::lower_stmts(fx, stmts),
    }
}
