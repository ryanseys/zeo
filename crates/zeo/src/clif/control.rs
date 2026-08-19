//! `begin/rescue/else/ensure/retry` -- the landing-chain lowering (plan
//! §1.4). The begin body lowers against its OWN landing; a `Raise` is
//! taken, pooled, and matched against the clause chain (`$!` bracketed per
//! clause); everything else propagates outward. An `ensure` body runs
//! inline on the normal path and under the signal-save bracket on the
//! landing path. Direct jumps across an `ensure` boundary refuse loudly
//! (the jump-through-ensure machinery is a later slice).

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
    // Resolve every clause's classes up front (compile-time ids).
    let mut clause_ids: Vec<Vec<u32>> = Vec::with_capacity(rescues.len());
    for clause in rescues {
        if !clause.splats.is_empty() {
            return fx.unsupported(site, "a splatted rescue list");
        }
        let ids = if clause.classes.is_empty() {
            let std_err = fx
                .an
                .compiler
                .resolve_class("StandardError", &[], 0)
                .expect("StandardError is always registered");
            vec![std_err.0]
        } else {
            let mut ids = Vec::with_capacity(clause.classes.len());
            for name in &clause.classes {
                let Some(cid) = fx.an.compiler.resolve_class(name, &[], 0) else {
                    let what = format!("the unresolved rescue class `{name}`");
                    return fx.unsupported(site, &what);
                };
                ids.push(cid.0);
            }
            ids
        };
        clause_ids.push(ids);
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

    // The body (and `else`) lower against the begin's landing; direct
    // jumps out of an ensure-carrying begin refuse via the depth stamp.
    fx.land = begin_land;
    if ensure_body.is_some() {
        fx.ensure_depth += 1;
    }
    let body_result = if else_body.is_some() { None } else { result };
    lower_section(fx, body, body_result)?;
    if let Some(else_body) = else_body {
        lower_section(fx, else_body, result)?;
    }
    if ensure_body.is_some() {
        fx.ensure_depth -= 1;
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
    // propagates (through the ensure bracket when one exists).
    fx.b.switch_to_block(begin_land);
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
    for (clause, ids) in rescues.iter().zip(&clause_ids) {
        let id_bytes: Vec<u8> = ids.iter().flat_map(|i| i.to_le_bytes()).collect();
        let off = fx.em.intern_rodata_aligned(&id_bytes, 4);
        let ids_ptr = fx.rod(off);
        let n = fx.b.ins().iconst(fx.em.ptr, ids.len() as i64);
        let m = fx
            .call("zeo_rt_rescue_matches", &[exc, ids_ptr, n])
            .expect("rescue_matches answers");
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

    // The ensure bracket, both flavors.
    if let Some(ensure_stmts) = ensure_body {
        // Normal path: the ensure body runs inline (its own signals go to
        // the OUTER landing).
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
        fx.call("zeo_rt_propagating_leave", &[pushed]);
        fx.call("zeo_rt_signal_restore", &[saved]);
        fx.b.ins().jump(outer_land, &[]);

        fx.b.switch_to_block(efail);
        fx.call("zeo_rt_propagating_leave", &[pushed]);
        fx.call("zeo_rt_signal_drop", &[saved]);
        fx.b.ins().jump(outer_land, &[]);
    }

    fx.b.switch_to_block(done);
    Ok(())
}

/// A begin section: statements, with the tail as a value when `result`
/// asks for one.
fn lower_section(fx: &mut Fx, stmts: &[NodeId], result: Option<ir::Value>) -> Result<(), String> {
    match result {
        Some(dst) => super::stmt::lower_value_body_into(fx, stmts, dst),
        None => super::stmt::lower_stmts(fx, stmts),
    }
}
