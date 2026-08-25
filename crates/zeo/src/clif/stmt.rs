//! Statement lowering to Cranelift IR: assignments, control flow, loop
//! signals (`break`/`next`/`redo`), definitions, and expression
//! statements. An unsupported shape refuses loudly with its source location.

use super::ctx::{Fx, LoopCtl, VALUE_SIZE};
use super::expr::lower_expr;
use super::ownership;
use crate::hir::{ArrayElem, HirNode, NodeId};
use cranelift_codegen::ir::{InstBuilder, StackSlotData, StackSlotKind, types};
use cranelift_module::Module;

pub(crate) fn lower_stmts(fx: &mut Fx, stmts: &[NodeId]) -> Result<(), String> {
    let mut i = 0;
    while i < stmts.len() {
        // A `class << self` body's statements are SPLICED into the
        // enclosing class body, tagged with the marker they came from
        // (`Hir::singleton_frame_stmts`). Each contiguous run gets its own
        // `singleton class` backtrace frame, rustc's `emit_singleton_frame`.
        if let Some(&origin) = fx.an.compiler.hir.singleton_frame_stmts.get(&stmts[i]) {
            let mut j = i + 1;
            while j < stmts.len()
                && fx.an.compiler.hir.singleton_frame_stmts.get(&stmts[j]) == Some(&origin)
            {
                j += 1;
            }
            singleton_frame(fx, origin, &stmts[i..j], None)?;
            i = j;
            continue;
        }
        let mark = fx.stmt_mark();
        let pool = fx
            .drain_temps
            .then(|| fx.call_status("zeo_rt_pool_mark", &[]));
        lower_stmt(fx, stmts[i])?;
        if let Some(pool) = pool {
            fx.call("zeo_rt_pool_reset", &[pool]);
        }
        fx.end_stmt(mark);
        i += 1;
    }
    Ok(())
}

/// One `class << self` group inside its own frame. Two positions have to
/// be right: the frame itself is labelled `singleton class` and tracks the
/// group's own lines, and the ENCLOSING frame is left reading the `class
/// << self` KEYWORD's line -- so the head stamps that line BEFORE the
/// push rather than letting the first grouped statement stamp it
/// underneath. `frame_label` carries down so a block written here is
/// `block in singleton class`.
fn singleton_frame(
    fx: &mut Fx,
    origin: NodeId,
    group: &[NodeId],
    dst: Option<cranelift_codegen::ir::Value>,
) -> Result<(), String> {
    let Some((file, line)) = fx.location(origin) else {
        return lower_group(fx, group, dst);
    };
    let (file, line) = (file.to_string(), line);
    stamp_line(fx, origin);
    let end_line = crate::analyze::source::source_end_line(&fx.an.compiler, origin);
    let label = "singleton class";
    let foff = fx.em.intern_rodata(file.as_bytes());
    let loff = fx.em.intern_rodata(label.as_bytes());
    let fptr = fx.rod(foff);
    let flen = fx.b.ins().iconst(fx.em.ptr, file.len() as i64);
    let lptr = fx.rod(loff);
    let llen = fx.b.ins().iconst(fx.em.ptr, label.len() as i64);
    let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
    let end_v = fx.b.ins().iconst(types::I32, i64::from(end_line));
    fx.call(
        "zeo_rt_frame_push",
        &[fptr, flen, lptr, llen, line_v, end_v],
    );
    // The error path pops this frame before the enclosing landing runs.
    let outer_land = fx.land;
    let pop_land = fx.b.create_block();
    fx.land = pop_land;
    let saved_label = std::mem::replace(&mut fx.frame_label, label.to_string());
    let saved_line = fx.prev_line.take();
    let r = lower_group(fx, group, dst);
    fx.frame_label = saved_label;
    fx.land = outer_land;
    fx.prev_line = saved_line;
    r?;
    fx.call("zeo_rt_frame_pop", &[]);
    let after = fx.b.create_block();
    fx.b.ins().jump(after, &[]);
    fx.b.switch_to_block(pop_land);
    fx.call("zeo_rt_frame_pop", &[]);
    fx.b.ins().jump(outer_land, &[]);
    fx.b.switch_to_block(after);
    Ok(())
}

/// [`lower_stmts`]' loop without the singleton grouping -- so a group can
/// reuse it without re-detecting itself. With a `dst` the group's last
/// statement is its VALUE.
fn lower_group(
    fx: &mut Fx,
    stmts: &[NodeId],
    dst: Option<cranelift_codegen::ir::Value>,
) -> Result<(), String> {
    let Some(dst) = dst else {
        for &stmt in stmts {
            let mark = fx.stmt_mark();
            lower_stmt(fx, stmt)?;
            fx.end_stmt(mark);
        }
        return Ok(());
    };
    let Some((&tail, init)) = stmts.split_last() else {
        ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
        return Ok(());
    };
    for &stmt in init {
        let mark = fx.stmt_mark();
        lower_stmt(fx, stmt)?;
        fx.end_stmt(mark);
    }
    stamp_line(fx, tail);
    let op = lower_tail_expr(fx, tail)?;
    ownership::write_move_into(fx, &op, dst);
    Ok(())
}

/// A body in VALUE position (a method body, an `if`-expression arm): the
/// leading statements run as statements, the tail's value MOVES into the
/// caller's `dst` slot. An empty body is nil.
pub(crate) fn lower_value_body_into(
    fx: &mut Fx,
    stmts: &[NodeId],
    dst: cranelift_codegen::ir::Value,
) -> Result<(), String> {
    let Some((&tail, init)) = stmts.split_last() else {
        ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
        return Ok(());
    };
    // A tail inside a `class << self` group supplies the body's value, so
    // its frame block is an expression rather than a statement.
    if let Some(&origin) = fx.an.compiler.hir.singleton_frame_stmts.get(&tail) {
        let mut start = stmts.len() - 1;
        while start > 0
            && fx
                .an
                .compiler
                .hir
                .singleton_frame_stmts
                .get(&stmts[start - 1])
                == Some(&origin)
        {
            start -= 1;
        }
        lower_stmts(fx, &stmts[..start])?;
        return singleton_frame(fx, origin, &stmts[start..], Some(dst));
    }
    lower_stmts(fx, init)?;
    stamp_line(fx, tail);
    let op = lower_tail_expr(fx, tail)?;
    ownership::write_move_into(fx, &op, dst);
    Ok(())
}

/// A tail position accepts a few statement-shaped nodes whose value Ruby
/// defines: an assignment answers the assigned value, a loop answers nil.
fn lower_tail_expr(fx: &mut Fx, tail: NodeId) -> Result<super::operand::Operand, String> {
    use super::operand::{Operand, TagInfo};
    match &fx.an.compiler.hir[tail] {
        // A `def` answers its method-name Symbol; the install itself is the
        // ordinary expression lowering.
        HirNode::DefMethod { .. } => super::expr::lower_expr(fx, tail),
        HirNode::LocalWrite(name, _) => {
            let name = name.clone();
            lower_stmt(fx, tail)?;
            Ok(ownership::read_local(fx, &name).expect("just assigned"))
        }
        HirNode::While { .. } | HirNode::For { .. } | HirNode::Loop { .. } => loop_value(fx, tail),
        HirNode::Break(..)
        | HirNode::Next(..)
        | HirNode::Redo
        | HirNode::Retry
        | HirNode::Raise(..)
        | HirNode::Return(..) => {
            // The jump/signal leaves this block unreachable; the nil is
            // never read.
            lower_stmt(fx, tail)?;
            Ok(Operand::Nil)
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            let (body, rescues, else_body, ensure_body) = (
                body.clone(),
                rescues.clone(),
                else_body.clone(),
                ensure_body.clone(),
            );
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            super::control::lower_begin(
                fx,
                tail,
                &body,
                &rescues,
                else_body.as_deref(),
                ensure_body.as_deref(),
                Some(dst),
            )?;
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::If { .. }
        | HirNode::IntegerLit(..)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
        | HirNode::ImaginaryLit(..)
        | HirNode::FloatLit(..)
        | HirNode::BoolLit(..)
        | HirNode::NilLit
        | HirNode::StringLit(..)
        | HirNode::LocalRead(..)
        | HirNode::IvarRead(..)
        | HirNode::IvarWrite(..)
        | HirNode::Or(..)
        | HirNode::And(..)
        | HirNode::ClassRef(..)
        | HirNode::New { .. }
        | HirNode::SelfRef
        | HirNode::Yield(..)
        | HirNode::BlockGiven
        | HirNode::MultiWrite { .. }
        | HirNode::Lambda { .. }
        | HirNode::ArrayLit(..)
        | HirNode::HashLit(..)
        | HirNode::SymbolLit(..)
        | HirNode::Seq(..)
        | HirNode::CaseWhen { .. }
        | HirNode::CaseIn { .. }
        | HirNode::MatchPredicate { .. }
        | HirNode::MatchRequired { .. }
        | HirNode::RangeLit { .. }
        | HirNode::GlobalRead(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ClassVarRead(..)
        | HirNode::ClassVarWrite(..)
        | HirNode::ConstWrite { .. }
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::DynConstRead { .. }
        | HirNode::DynConstWrite { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::FeatureLoaded { .. }
        | HirNode::CExtLoaded { .. }
        | HirNode::RegexpLit(..)
        | HirNode::Ffi(..)
        | HirNode::BoxScope { .. }
        | HirNode::BoxHandle(_)
        | HirNode::LastMatchRef(..)
        | HirNode::SuperCall { .. }
        | HirNode::Defined(..)
        | HirNode::Call { .. } => lower_expr(fx, tail),
        // A `class`/`module` in TAIL position: the site runs and answers
        // its body's last value; a body ending in a definition-level
        // construct answers nil, since nothing necessarily reads it.
        HirNode::ClassDef { .. } if fx.eval_mode.is_some() => super::eval::eval_class_def(fx, tail),
        HirNode::ClassDef { .. } => class_body_value(fx, tail, false),
        // A snippet's definition-level statements run and answer what ruby
        // answers: the KEYWORD forms (`alias`, `undef`, `private_constant`)
        // are nil, and the CALL forms answer what the send does.
        HirNode::AliasMethod { .. }
        | HirNode::Undef(..)
        | HirNode::ClassMethodUndef(..)
        | HirNode::ConstantVisibility { .. }
            if fx.eval_mode.is_some() =>
        {
            lower_stmt(fx, tail)?;
            Ok(Operand::Nil)
        }
        // `using M` answers the receiver it activated on -- `main` at a
        // snippet's own level, the class under `class_eval`.
        HirNode::Using(..) if fx.eval_mode.is_some() => {
            lower_stmt(fx, tail)?;
            let addr = fx.self_ptr.expect("self_ptr is set in the prologue");
            Ok(Operand::Ptr {
                addr,
                owned: false,
                tag: super::operand::TagInfo::Unknown,
            })
        }
        HirNode::MethodVisibility { name, visibility } if fx.eval_mode.is_some() => {
            let (name, visibility) = (name.clone(), *visibility);
            let verb = match visibility {
                crate::hir::Visibility::Private => "private",
                crate::hir::Visibility::Protected => "protected",
                crate::hir::Visibility::Public => "public",
            };
            super::eval::eval_definee_send_value(fx, verb, std::slice::from_ref(&name), false)
        }
        HirNode::ClassMethodVisibility { name, visibility } if fx.eval_mode.is_some() => {
            let (name, visibility) = (name.clone(), *visibility);
            let verb = match visibility {
                crate::hir::Visibility::Private => "private_class_method",
                _ => "public_class_method",
            };
            super::eval::eval_definee_send_value(fx, verb, std::slice::from_ref(&name), false)
        }
        HirNode::ModuleFunction(name) if fx.eval_mode.is_some() => {
            let name = name.clone();
            super::eval::eval_definee_send_value(
                fx,
                "module_function",
                std::slice::from_ref(&name),
                false,
            )
        }
        // `include M`/`extend`/`prepend` answer the receiver, which the
        // send already does -- in a snippet they ARE a send (nothing was
        // edited into an ancestry at compile time).
        HirNode::Include(module) | HirNode::Extend(module) | HirNode::Prepend(module)
            if fx.eval_mode.is_some() =>
        {
            let module = module.clone();
            let verb = mixin_verb(&fx.an.compiler.hir[tail]);
            super::eval::eval_mixin_send_value(fx, tail, &module, verb)
        }
        other => {
            let what = format!("this tail expression ({})", super::expr::node_kind(other));
            fx.unsupported(tail, &what)
        }
    }
}

pub(crate) fn lower_stmt(fx: &mut Fx, stmt: NodeId) -> Result<(), String> {
    stamp_line(fx, stmt);
    // A call the SITE decides (a refinement-covered name, `Ractor.new`)
    // lowers as an expression whose value is discarded: the arms below
    // include shapes -- a literal block, an inline iterator -- that would
    // otherwise take a dispatch path the site is entitled to override.
    if super::expr::site_decided_call(fx, stmt) {
        let op = lower_expr(fx, stmt)?;
        ownership::discard(fx, op);
        return Ok(());
    }
    match &fx.an.compiler.hir[stmt] {
        HirNode::LocalWrite(name, value) => {
            let name = name.clone();
            let value = *value;
            let op = lower_expr(fx, value)?;
            ownership::write_local(fx, &name, &op);
            Ok(())
        }
        HirNode::MultiWrite { targets, value } => {
            let (targets, value) = (targets.clone(), *value);
            let op = lower_expr(fx, value)?;
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, op.tag());
            }
            super::multi::lower_multi_group(fx, stmt, &targets, ptr)
        }
        HirNode::Call {
            receiver: None,
            name,
            args,
            kwargs,
            block: None,
            block_arg: None,
            safe: false,
        } if name == "puts"
            && kwargs.is_empty()
            && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) =>
        {
            let args = args.clone();
            lower_puts(fx, stmt, &args)
        }
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
            // A build-time guard (`if defined?(Gone)`, a version gate) decides
            // WHICH BRANCH IS EMITTED, so the other one is never lowered. That
            // matters beyond code size: the unreached branch is written for a
            // ruby zeo is not, and may name constructs this emitter would
            // refuse.
            if let Some(taken) = static_cond(fx, cond) {
                // The fold decides the BRANCH; it must not eat the
                // condition's own effect. A build gate written `=~` still
                // writes `$~`, and the line after it may read one. Only the
                // match is re-run: the fold succeeded, so both of its
                // operands are static and nothing else in it can raise.
                if let Some(m) = folded_match(fx, cond) {
                    let v = lower_expr(fx, m)?;
                    ownership::discard(fx, v);
                }
                return lower_stmts(fx, if taken { &then_body } else { &else_body });
            }
            let c = lower_expr(fx, cond)?;
            let t = ownership::truthy(fx, c);
            let b_then = fx.b.create_block();
            let b_else = fx.b.create_block();
            let join = fx.b.create_block();
            fx.b.ins().brif(t, b_then, &[], b_else, &[]);
            fx.b.switch_to_block(b_then);
            lower_stmts(fx, &then_body)?;
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(b_else);
            lower_stmts(fx, &else_body)?;
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(join);
            Ok(())
        }
        HirNode::While {
            cond,
            body,
            negate,
            post,
        } => {
            let (cond, body, negate, post) = (*cond, body.clone(), *negate, *post);
            lower_loop(fx, Some((cond, negate)), &body, post, None)
        }
        HirNode::Loop { body } => {
            let body = body.clone();
            lower_loop(fx, None, &body, false, None)
        }
        HirNode::For {
            target,
            iterable,
            body,
        } => {
            let (target, iterable, body) = (target.clone(), *iterable, body.clone());
            lower_for(fx, stmt, &target, iterable, &body, None)
        }
        HirNode::Break(value) => {
            let value = *value;
            if fx.loops.is_empty() {
                // A `break` at the top of a `define_method` body RETURNS
                // from the method: the block became the method, so there is
                // no yielding call left to break out of, and CRuby's rule
                // for a block-turned-method is the lambda one.
                if fx.define_method_body
                    && let Some((out, ret_ok)) = fx.ret.or(fx.block_next)
                {
                    if fx.ensure_depth != 0 {
                        return signal_jump(fx, value, zeo_abi::abi::SignalKind::Next);
                    }
                    let op = match value {
                        Some(v) => lower_expr(fx, v)?,
                        None => super::operand::Operand::Nil,
                    };
                    ownership::write_move_into(fx, &op, out);
                    fx.pop_handling_to(0);
                    fx.b.ins().jump(ret_ok, &[]);
                    fx.continue_unreachable();
                    return Ok(());
                }
                // In an escaping block, `break` arms the Break signal the
                // send-site's catch_break receives.
                if fx.block_next.is_some() {
                    let op = match value {
                        Some(v) => lower_expr(fx, v)?,
                        None => super::operand::Operand::Nil,
                    };
                    let ptr = ownership::move_ptr(fx, &op);
                    let kind =
                        fx.b.ins()
                            .iconst(types::I8, i64::from(zeo_abi::abi::SignalKind::Break as u8));
                    // The landing chain pops what it pushed -- popping `$!`
                    // here as well would drop an ENCLOSING clause's entry.
                    fx.call("zeo_rt_signal_set", &[kind, ptr]);
                    let land = fx.land;
                    fx.b.ins().jump(land, &[]);
                    fx.continue_unreachable();
                    return Ok(());
                }
                return fx.unsupported(stmt, "`break` outside a loop");
            }
            if fx.loops.last().expect("checked above").depth != fx.ensure_depth {
                return signal_jump(fx, value, zeo_abi::abi::SignalKind::Break);
            }
            let result = fx.loops.last().expect("checked above").result;
            match (value, result) {
                // `break v` in a value-position loop: v IS the loop's value.
                (Some(v), Some(dst)) => {
                    let op = lower_expr(fx, v)?;
                    ownership::write_move_into(fx, &op, dst);
                }
                (Some(v), None) => {
                    let op = lower_expr(fx, v)?;
                    ownership::discard(fx, op);
                }
                (None, Some(dst)) => {
                    ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
                }
                (None, None) => {}
            }
            let ctl = fx.loops.last().expect("checked above");
            let (exit, handling) = (ctl.exit, ctl.handling);
            fx.pop_handling_to(handling);
            fx.b.ins().jump(exit, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Next(value) => {
            let value = *value;
            if fx.loops.is_empty()
                && let Some((out, ret_ok)) = fx.block_next
            {
                if fx.ensure_depth != 0 {
                    return signal_jump(fx, value, zeo_abi::abi::SignalKind::Next);
                }
                // In an escaping block, `next v` IS the block's return.
                match value {
                    Some(v) => {
                        let op = lower_expr(fx, v)?;
                        ownership::write_move_into(fx, &op, out);
                    }
                    None => {
                        ownership::write_move_into(fx, &super::operand::Operand::Nil, out);
                    }
                }
                fx.pop_handling_to(0);
                fx.b.ins().jump(ret_ok, &[]);
                fx.continue_unreachable();
                return Ok(());
            }
            let Some(ctl) = fx.loops.last() else {
                return fx.unsupported(stmt, "`next` outside a loop");
            };
            if ctl.depth != fx.ensure_depth {
                return signal_jump(fx, value, zeo_abi::abi::SignalKind::Next);
            }
            if let Some(v) = value {
                let op = lower_expr(fx, v)?;
                ownership::discard(fx, op);
            }
            let ctl = fx.loops.last().expect("checked above");
            let (latch, handling) = (ctl.latch, ctl.handling);
            fx.pop_handling_to(handling);
            fx.b.ins().jump(latch, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Redo => {
            let Some(ctl) = fx.loops.last() else {
                // `redo` in a block re-runs the block from its binding head
                // (the bindings re-run too -- they sit inside the loop).
                let Some(head) = fx.block_redo else {
                    return fx.unsupported(stmt, "`redo` outside a loop");
                };
                if fx.ensure_depth != 0 {
                    return signal_jump(fx, None, zeo_abi::abi::SignalKind::Redo);
                }
                fx.pop_handling_to(0);
                fx.b.ins().jump(head, &[]);
                fx.continue_unreachable();
                return Ok(());
            };
            if ctl.depth != fx.ensure_depth {
                return signal_jump(fx, None, zeo_abi::abi::SignalKind::Redo);
            }
            let ctl = fx.loops.last().expect("checked above");
            let (body, handling) = (ctl.body, ctl.handling);
            fx.pop_handling_to(handling);
            fx.b.ins().jump(body, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::IvarWrite(name, value) => {
            let (name, value) = (name.clone(), *value);
            super::ivars::lower_ivar_write(fx, &name, value)
        }
        HirNode::Return(value) => {
            let value = *value;
            let Some((out, ret_ok)) = fx.ret else {
                // In an escaping block, `return` arms the Return signal;
                // the runtime resolves it against the proc's captured home
                // (dead home -> LocalJumpError) and the defining method's
                // boundary folds a targeted one.
                // ...and at the TOP LEVEL, where a `return` ends the program
                // silently and successfully: the same signal, settled by
                // `zeo_rt_main` rather than by any method boundary. (Ruby
                // rejects a `return` in a class body at parse time, so this
                // is the only other body without one.)
                let op = match value {
                    Some(v) => lower_expr(fx, v)?,
                    None => super::operand::Operand::Nil,
                };
                let ptr = ownership::move_ptr(fx, &op);
                let kind =
                    fx.b.ins()
                        .iconst(types::I8, i64::from(zeo_abi::abi::SignalKind::Return as u8));
                fx.call("zeo_rt_signal_set", &[kind, ptr]);
                let land = fx.land;
                fx.b.ins().jump(land, &[]);
                fx.continue_unreachable();
                return Ok(());
            };
            // The returned expression runs BEFORE `$!` unwinds -- inside a
            // rescue clause `return $!.message` reads the rescued one.
            let op = match value {
                Some(v) => lower_expr(fx, v)?,
                None => super::operand::Operand::Nil,
            };
            if fx.ensure_depth != 0 {
                // An `ensure` sits between here and the method's exit, so
                // the return travels as a signal: the bracket saves and
                // restores it around the ensure body, and this method's own
                // boundary folds it (an unmarked `Return` belongs to the
                // nearest catcher). The landing chain pops `$!` on the way.
                let ptr = ownership::move_ptr(fx, &op);
                let kind =
                    fx.b.ins()
                        .iconst(types::I8, i64::from(zeo_abi::abi::SignalKind::Return as u8));
                fx.call("zeo_rt_signal_set", &[kind, ptr]);
                let land = fx.land;
                fx.b.ins().jump(land, &[]);
                fx.continue_unreachable();
                return Ok(());
            }
            fx.pop_handling_to(0);
            ownership::write_move_into(fx, &op, out);
            fx.b.ins().jump(ret_ok, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            let (body, rescues, else_body, ensure_body) = (
                body.clone(),
                rescues.clone(),
                else_body.clone(),
                ensure_body.clone(),
            );
            super::control::lower_begin(
                fx,
                stmt,
                &body,
                &rescues,
                else_body.as_deref(),
                ensure_body.as_deref(),
                None,
            )
        }
        HirNode::Raise(args, cause) => {
            // `raise ..., cause: c`: the cause is explicit, so the automatic
            // `$!` chaining the row does is skipped and the runtime builds,
            // causes and stamps the exception in one entry.
            if let crate::hir::RaiseCause::Explicit(node) = cause {
                let (args, node) = (args.clone(), *node);
                let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
                let argv = super::call::build_argv(fx, stmt, &elems)?;
                let cause_op = super::expr::lower_expr(fx, node)?;
                let cause_ptr = ownership::borrow_ptr(fx, &cause_op);
                if cause_op.owned() {
                    ownership::pool_owned(fx, cause_ptr, cause_op.tag());
                }
                let argc = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
                let status =
                    fx.call_status("zeo_rt_raise_with_explicit_cause", &[argv, argc, cause_ptr]);
                fx.fallible(status);
                return Ok(());
            }
            let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
            // `raise` IS `Kernel#raise` -- the builtin row constructs,
            // stamps, and signals; the Ok arm is unreachable.
            let op = super::call::implicit_send(fx, stmt, "raise", &elems)?;
            ownership::discard(fx, op);
            Ok(())
        }
        HirNode::Retry => {
            let Some(&(target, depth, handling)) = fx.retries.last() else {
                return fx.unsupported(stmt, "`retry` outside a rescue clause");
            };
            if depth != fx.ensure_depth {
                return fx.unsupported(stmt, "a `retry` across an `ensure` boundary");
            }
            fx.pop_handling_to(handling);
            fx.b.ins().jump(target, &[]);
            fx.continue_unreachable();
            Ok(())
        }
        HirNode::ClassDef { .. } if fx.eval_mode.is_some() => {
            let op = super::eval::eval_class_def(fx, stmt)?;
            ownership::discard(fx, op);
            Ok(())
        }
        HirNode::ClassDef { .. } => {
            // Registration happened at startup; the marker runs the body
            // site (statements + the declaration's const-location record).
            // A miss is a hoisted or statement-free site: nothing to run.
            let call = fx.em.class_bodies.get(&stmt).cloned();
            match call {
                Some(call) => emit_class_body_call(fx, &call),
                None => Ok(()),
            }
        }
        // A mixin's ANCESTRY edit happened at compile time (analyze); what
        // remains where it was written is the module's hook send --
        // `M.included(C)` and siblings -- which is Module's own no-op
        // unless the module defines one.
        // `alias new old` is pure REGISTRATION: analyze resolved it into a
        // copy scope (user source) or an alias row (builtin source), and a
        // builtin row's source is validated at this body's END
        // (`validate_class_aliases`) -- the statement itself runs nothing.
        // In a SNIPPET nothing was registered, so the keyword is the send
        // ruby writes on the run-time definee.
        HirNode::AliasMethod {
            new_name,
            old_name,
            is_class_method,
        } if fx.eval_mode.is_some() => {
            let names = vec![new_name.clone(), old_name.clone()];
            let singleton = *is_class_method;
            super::eval::eval_definee_send(fx, "alias_method", &names, singleton)
        }
        HirNode::AliasMethod { .. } => Ok(()),
        // A `def` reached HERE is one analyze did not register statically
        // (written inside a method body or a block): a RUNTIME install,
        // whose Symbol value the statement position drops. The toplevel
        // and class-body positions filter their defs out before lowering.
        HirNode::DefMethod { .. } => {
            let op = super::expr::lower_expr(fx, stmt)?;
            ownership::discard(fx, op);
            Ok(())
        }
        // Visibility is POSITIONAL: a reopen's re-mark of a name the class
        // already owns, and a unit body's re-mark, are the ones analyze
        // leaves in the site's statements rather than folding into the
        // start-of-program override rows -- so they apply HERE.
        HirNode::MethodVisibility { name, visibility } => {
            let (name, visibility) = (name.clone(), *visibility);
            // In a snippet the class is only a run-time fact, and the verb
            // is the one ruby writes: `definee.private(:name)`.
            if fx.eval_mode.is_some() {
                let verb = match visibility {
                    crate::hir::Visibility::Private => "private",
                    crate::hir::Visibility::Protected => "protected",
                    crate::hir::Visibility::Public => "public",
                };
                return super::eval::eval_definee_send(
                    fx,
                    verb,
                    std::slice::from_ref(&name),
                    false,
                );
            }
            let verb = match visibility {
                crate::hir::Visibility::Private => 0,
                crate::hir::Visibility::Protected => 1,
                crate::hir::Visibility::Public => 2,
            };
            apply_visibility(fx, stmt, "zeo_rt_runtime_set_visibility", &name, Some(verb))
        }
        HirNode::ClassMethodVisibility { name, visibility } => {
            let (name, visibility) = (name.clone(), *visibility);
            if fx.eval_mode.is_some() {
                let verb = match visibility {
                    crate::hir::Visibility::Private => "private_class_method",
                    _ => "public_class_method",
                };
                return super::eval::eval_definee_send(
                    fx,
                    verb,
                    std::slice::from_ref(&name),
                    false,
                );
            }
            let private = u8::from(visibility == crate::hir::Visibility::Private);
            apply_visibility(
                fx,
                stmt,
                "zeo_rt_runtime_class_method_visibility",
                &name,
                Some(private),
            )
        }
        // An `undef` in a REOPENED `class << self`: a call written between
        // the two bodies still answers, so the retirement has a position.
        HirNode::ClassMethodUndef(names) => {
            let names = names.clone();
            if fx.eval_mode.is_some() {
                return super::eval::eval_definee_send(fx, "undef_method", &names, true);
            }
            for name in names {
                apply_visibility(fx, stmt, "zeo_rt_runtime_undef_class_method", &name, None)?;
            }
            Ok(())
        }
        // `undef` and `module_function` stay pure REGISTRATION: analyze
        // stamped the tables (undefined marks, module-function copies) and
        // the statements run nothing. In a SNIPPET analyze registered
        // nothing, so each is the send ruby writes on the run-time definee.
        HirNode::Undef(names) if fx.eval_mode.is_some() => {
            let names = names.clone();
            super::eval::eval_definee_send(fx, "undef_method", &names, false)
        }
        HirNode::ModuleFunction(name) if fx.eval_mode.is_some() => {
            let name = name.clone();
            super::eval::eval_definee_send(
                fx,
                "module_function",
                std::slice::from_ref(&name),
                false,
            )
        }
        HirNode::ConstantVisibility { names, private } if fx.eval_mode.is_some() => {
            let (names, private) = (names.clone(), *private);
            let verb = if private {
                "private_constant"
            } else {
                "public_constant"
            };
            super::eval::eval_definee_send(fx, verb, &names, false)
        }
        // `private_constant :A` / `public_constant :A` in a PROGRAM: a
        // registration for reflection, and a run-time flag a later
        // directive restores -- so it runs where it is WRITTEN rather than
        // at startup, or a read between the two sees the wrong answer.
        HirNode::ConstantVisibility { names, private } => {
            let (names, private) = (names.clone(), *private);
            let owner_v = fx.self_ptr.expect("self_ptr is set in the prologue");
            let private_v = fx.b.ins().iconst(types::I8, i64::from(u8::from(private)));
            for name in &names {
                let (nptr, nlen) = super::expr::rodata_name(fx, name);
                fx.call("zeo_rt_const_visibility", &[owner_v, nptr, nlen, private_v]);
            }
            Ok(())
        }
        HirNode::Undef(..) | HirNode::ModuleFunction(..) => Ok(()),
        // A `refine` marker analyze already CONSUMED runs nothing where it
        // was written: the holder module owns the methods and the
        // `Refinement` row says what they refine. It reaches statement
        // position only when the class-body walk kept the enclosing
        // statement whole -- power_assert (in the rspec stack) writes its
        // refinements under a runtime `if`, which hit this refusal. An
        // UNregistered marker is a different thing and still refuses,
        // rather than losing the refinement silently.
        HirNode::Refine { .. } if fx.an.compiler.refinement_marker_registered(stmt) => Ok(()),
        // `using M` in a SNIPPET: the module is a run-time constant, so
        // the site resolves it and fills its activation slot. Every call
        // site the `using` covers reads that slot -- which outlives the
        // call, so a `def` written after it here still sees the refinement.
        HirNode::Using(module) if fx.eval_mode.is_some() => {
            let module = module.clone();
            let Some(slot) = fx.an.compiler.eval_activation_slot(stmt) else {
                return Err("a `using` inside an `eval` that analyze did not place".to_string());
            };
            let op = super::consts::const_read(fx, stmt, &module)?;
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, op.tag());
            }
            let slot_v =
                fx.b.ins()
                    .iconst(types::I32, i64::from(fx.using_base + slot));
            let status = fx.call_status("zeo_rt_eval_using", &[ptr, slot_v]);
            fx.fallible(status);
            Ok(())
        }
        HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_) => {
            let Some((module, hook, primitive)) = mixin_parts(&fx.an.compiler.hir[stmt]) else {
                unreachable!("guarded by the four mixin arms")
            };
            let (module, hook, primitive) = (module.clone(), hook, primitive);
            // In an eval snippet NOTHING edited an ancestry at compile
            // time -- there is no class table to edit into -- so the mixin
            // is the ordinary send ruby writes: a receiverless call on
            // whatever `self` turns out to be (`main` at a snippet's own
            // level, the receiver under a `class_eval`). The hooks fire
            // from the runtime's own implementation of it.
            if fx.eval_mode.is_some() {
                return super::eval::eval_mixin_send(
                    fx,
                    stmt,
                    &module,
                    mixin_verb(&fx.an.compiler.hir[stmt]),
                );
            }
            // The PRIMITIVE first when the module overrides it -- it is what
            // performs the mixin, and analyze suppressed the static edit on
            // the strength of it. The notification follows either way,
            // exactly as ruby fires `included` even when an override skipped
            // the splice.
            let overrides = super::boxes::resolve_class_here(fx, &module)
                .is_some_and(|mid| fx.an.compiler.overrides_mixin_primitive(mid, primitive));
            if overrides {
                mixin_hook_send(fx, &module, primitive)?;
            }
            mixin_hook_send(fx, &module, hook)
        }
        // A definition report -- `Klass.method_added(:name)` and its five
        // siblings -- spliced back in at the position of a `def` the analyze
        // walk consumed. An FCALL, not a visibility-checked call: ruby
        // reaches its hooks that way, so a `private def self.method_added`
        // still runs.
        // A redefinition applied at its document position: the install
        // replaces the overlay body, and the definition's own
        // `method_added` report is a separate `DefHook` right after it.
        HirNode::MethodRedefine {
            class,
            name,
            scope,
            singleton,
        } => {
            let (class, name, scope, singleton) = (*class, name.clone(), *scope, *singleton);
            let tramp = fx.em.redef_tramps[&(class, scope)];
            let f_ref = fx.em.module.declare_func_in_func(tramp, fx.b.func);
            let f_addr = fx.b.ins().func_addr(fx.em.ptr, f_ref);
            let cid = fx.b.ins().iconst(types::I32, i64::from(class));
            let (nptr, nlen) = super::expr::rodata_name(fx, &name);
            // The two channels are different overlay maps, and a class method
            // must not land on the instance one -- `C.t` would keep answering
            // the frozen row while `C.new.t` gained a method ruby never
            // defined.
            let entry = match singleton {
                true => "zeo_rt_runtime_replace_class_method",
                false => "zeo_rt_runtime_replace_method",
            };
            fx.call(entry, &[cid, nptr, nlen, f_addr]);
            // The body is only half of a `def`. Its reflection row -- arity,
            // parameters, source location -- installs at the same position,
            // or the window keeps answering for the last body written.
            let meta = fx.em.redef_metas[&(class, scope)];
            let idx = fx.b.ins().iconst(types::I32, i64::from(meta));
            fx.call("zeo_rt_install_meta_row", &[idx]);
            // A `def` carries a VISIBILITY as well as a body, and both belong
            // at the `def`'s own position. This mark is also what CLEARS an
            // earlier one: ruby's re-`def` resets a name to the class body's
            // running default, so a `private :v` written between two bodies
            // stops applying here.
            let vis = match fx
                .an
                .compiler
                .scope(crate::compiler::ScopeId(scope))
                .visibility
            {
                crate::hir::Visibility::Private => 0i64,
                crate::hir::Visibility::Protected => 1,
                crate::hir::Visibility::Public => 2,
            };
            let sym = fx.sym_id(&name);
            let verb = fx.b.ins().iconst(types::I8, vis);
            let chan = fx.b.ins().iconst(types::I8, i64::from(singleton));
            fx.call(
                "zeo_rt_install_positional_visibility",
                &[cid, sym, verb, chan],
            );
            Ok(())
        }
        HirNode::DefHook {
            class,
            hook,
            name,
            pending,
        } => {
            let (class, hook, name, pending) =
                (*class, hook.clone(), name.clone(), pending.clone());
            let recv = super::consts::class_immediate(fx, crate::compiler::ClassId(class));
            let recv_ptr = ownership::borrow_ptr(fx, &recv);
            let arg_ss = fx.temp_slot();
            let argv = fx.slot_addr(arg_ss, 0);
            let arg_sym = fx.sym_id(&name);
            fx.call("zeo_rt_sym_value", &[arg_sym, argv]);
            let sym = fx.sym_id(&hook);
            // The half-built view the hook body must see: the names still in
            // the FUTURE here are marked pending for as long as the send is
            // on the stack. Omitted when nothing is left, so the last
            // definition in a class pays nothing.
            if !pending.is_empty() {
                let (ptr, n) = sym_id_array(fx, &pending);
                let cid_v = fx.b.ins().iconst(types::I32, i64::from(class));
                fx.call("zeo_rt_pending_defs_begin", &[cid_v, ptr, n]);
            }
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            let zero_box = fx.box_v();
            let argc = fx.b.ins().iconst(fx.em.ptr, 1);
            let null = fx.b.ins().iconst(fx.em.ptr, 0);
            let status = fx.call_status(
                "zeo_rt_send_value_in",
                &[zero_box, recv_ptr, sym, argv, argc, null, out],
            );
            if pending.is_empty() {
                fx.fallible(status);
            } else {
                // The pop must run on the RAISE path too: pair it before the
                // landing jump.
                let ok = fx.b.create_block();
                let bad = fx.b.create_block();
                fx.b.ins().brif(status, bad, &[], ok, &[]);
                fx.b.switch_to_block(bad);
                fx.call("zeo_rt_pending_defs_end", &[]);
                fx.b.ins().jump(fx.land, &[]);
                fx.b.switch_to_block(ok);
                fx.call("zeo_rt_pending_defs_end", &[]);
            }
            fx.owned_created += 1;
            ownership::discard(
                fx,
                super::operand::Operand::Slot {
                    ss,
                    owned: true,
                    tag: super::operand::TagInfo::Unknown,
                },
            );
            Ok(())
        }
        HirNode::Seq(stmts) => {
            let stmts = stmts.clone();
            lower_stmts(fx, &stmts)
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block: Some(blk),
            block_arg: None,
            safe: false,
        } if kwargs.is_empty() => {
            let (receiver, name, args, blk) = (*receiver, name.clone(), args.clone(), *blk);
            // A typed-receiver `arr.each` fuses here too -- and this is
            // where most of them are written, since `each`'s value is
            // rarely wanted.
            if args.is_empty()
                && super::iter::fusable_block(fx, blk)
                && let Some(r) = receiver
                && fx.an.compiler.inline_iter_sites.get(&blk)
                    == Some(&crate::compiler::InlineIterKind::ArrayEach)
            {
                super::iter::lower_array_each(fx, stmt, r, blk, false)?;
                return Ok(());
            }
            if args.is_empty()
                && super::iter::fusable_block(fx, blk)
                && let Some(counted) = super::iter::counted_of(fx, receiver, &name, true)
            {
                return super::iter::lower_counted(fx, stmt, &counted, blk, None);
            }
            let op = if receiver.is_none()
                && let Some(decl) = fx.em.methods.get(&name)
                // `plain` as well as the count: a richer signature binds
                // through the trampoline, and its body takes one slot per
                // PARAMETER, not per argument. Without this the statement
                // arm built a call the direct entry could not accept
                // (minitest's `describe(desc, additional_desc = nil, &blk)`
                // written as a statement).
                && decl.plain
                && decl.arity == args.len()
                && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
                && !super::expr::method_class_shadows(fx, &name)
            {
                super::call::direct_call(fx, stmt, &name, &args, Some(blk))?
            } else {
                super::blocks::block_send(fx, stmt, receiver, &name, &args, blk)?
            };
            ownership::discard(fx, op);
            Ok(())
        }
        // Anything else in statement position: try the expression lowering
        // and discard the value (it refuses on its own for shapes outside
        // the slice).
        HirNode::IntegerLit(..)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
        | HirNode::ImaginaryLit(..)
        | HirNode::FloatLit(..)
        | HirNode::BoolLit(..)
        | HirNode::NilLit
        | HirNode::StringLit(..)
        | HirNode::LocalRead(..)
        | HirNode::IvarRead(..)
        | HirNode::Or(..)
        | HirNode::And(..)
        | HirNode::ClassRef(..)
        | HirNode::New { .. }
        | HirNode::SelfRef
        | HirNode::Yield(..)
        | HirNode::BlockGiven
        | HirNode::SymbolLit(..)
        | HirNode::ArrayLit(..)
        | HirNode::HashLit(..)
        | HirNode::Lambda { .. }
        | HirNode::CaseWhen { .. }
        | HirNode::CaseIn { .. }
        | HirNode::MatchPredicate { .. }
        | HirNode::MatchRequired { .. }
        | HirNode::RangeLit { .. }
        | HirNode::GlobalRead(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ClassVarRead(..)
        | HirNode::ClassVarWrite(..)
        | HirNode::ConstWrite { .. }
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::DynConstRead { .. }
        | HirNode::DynConstWrite { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::FeatureLoaded { .. }
        | HirNode::CExtLoaded { .. }
        | HirNode::RegexpLit(..)
        | HirNode::Ffi(..)
        | HirNode::BoxScope { .. }
        | HirNode::BoxHandle(_)
        | HirNode::LastMatchRef(..)
        | HirNode::SuperCall { .. }
        | HirNode::Defined(..)
        | HirNode::Call { .. } => {
            let op = lower_expr(fx, stmt)?;
            ownership::discard(fx, op);
            Ok(())
        }
        other => {
            let what = format!("this statement ({})", super::expr::node_kind(other));
            fx.unsupported(stmt, &what)
        }
    }
}

/// A stack array of runtime symbol ids -- the ids come from `zeo_syms`,
/// which `zeo_unit_init` fills, so they are loads rather than constants
/// and the array cannot live in rodata.
fn sym_id_array(
    fx: &mut Fx,
    names: &[String],
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    let ss =
        fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (names.len() * 4) as u32,
            2,
        ));
    let fl = cranelift_codegen::ir::MemFlagsData::trusted();
    for (i, name) in names.iter().enumerate() {
        let id = fx.sym_id(name);
        let at = fx.slot_addr(ss, (i * 4) as i32);
        fx.b.ins().store(fl, id, at, 0);
    }
    let base = fx.slot_addr(ss, 0);
    let n = fx.b.ins().iconst(fx.em.ptr, names.len() as i64);
    (base, n)
}

/// `(module name, notification hook, mix-in primitive)` for the four mixin
/// nodes -- `include` is `append_features` then `included`, and its
/// siblings follow the same shape (`eval.c`'s `rb_mod_include`). The
/// singleton form fires the same hooks with the SINGLETON class as their
/// argument, which zeo has no compile-time class for; analyze rejects a
/// module that defines either, so it is named only to stay true.
fn mixin_parts(node: &HirNode) -> Option<(&String, &'static str, &'static str)> {
    match node {
        HirNode::Include(m) => Some((m, "included", "append_features")),
        HirNode::Prepend(m) | HirNode::ClassMethodPrepend(m) => {
            Some((m, "prepended", "prepend_features"))
        }
        HirNode::Extend(m) => Some((m, "extended", "extend_object")),
        _ => None,
    }
}

/// `M.hook(Target)` -- the send a mixin leaves behind where it was
/// written, its ancestry edit having happened at compile time. Nothing is
/// emitted when nobody defines the hook (Module's own default is a no-op);
/// a PRIMITIVE reaches here only when the module overrides it, and it is
/// what performs the mixin at all.
/// The verb a mixin marker spells, for the run-time send an eval needs.
fn mixin_verb(node: &HirNode) -> &'static str {
    match node {
        HirNode::Include(_) => "include",
        HirNode::Prepend(_) | HirNode::ClassMethodPrepend(_) => "prepend",
        HirNode::Extend(_) => "extend",
        other => unreachable!("guarded by the four mixin arms; got {other:?}"),
    }
}

fn mixin_hook_send(fx: &mut Fx, module: &str, hook: &str) -> Result<(), String> {
    let (Some(mid), true) = (
        super::boxes::resolve_class_here(fx, module),
        fx.self_is_class,
    ) else {
        return Ok(());
    };
    let Some(target) = fx.method_class else {
        return Ok(());
    };
    if fx.an.compiler.class_method_in_chain(mid, hook).is_none()
        && !fx.an.compiler.overrides_mixin_primitive(mid, hook)
    {
        return Ok(());
    }
    let recv = super::consts::class_immediate(fx, mid);
    let recv_ptr = ownership::borrow_ptr(fx, &recv);
    let arg = super::consts::class_immediate(fx, target);
    let argv = ownership::borrow_ptr(fx, &arg);
    let sym = fx.sym_id(hook);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let zero_box = fx.box_v();
    let argc = fx.b.ins().iconst(fx.em.ptr, 1);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let status = fx.call_status(
        "zeo_rt_send_value_in",
        &[zero_box, recv_ptr, sym, argv, argc, null, out],
    );
    fx.fallible(status);
    fx.owned_created += 1;
    ownership::discard(
        fx,
        super::operand::Operand::Slot {
            ss,
            owned: true,
            tag: super::operand::TagInfo::Unknown,
        },
    );
    Ok(())
}

/// A short label for the refusal message.
/// `puts` with arbitrary slice-lowerable arguments: a contiguous argv
/// array of borrowed copies (owned temps hand their value to the pool
/// first), then the status-protocol call.
fn lower_puts(fx: &mut Fx, stmt: NodeId, args: &[ArrayElem]) -> Result<(), String> {
    let argc = args.len();
    let argv = (argc > 0).then(|| {
        fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            argc as u32 * VALUE_SIZE,
            3,
        ))
    });
    for (i, arg) in args.iter().enumerate() {
        let ArrayElem::Single(id) = arg else {
            return fx.unsupported(stmt, "a splat argument");
        };
        let op = lower_expr(fx, *id)?;
        if op.owned() {
            let tag = op.tag();
            let addr = ownership::addr_of(fx, &op);
            ownership::pool_owned(fx, addr, tag);
        }
        let argv = argv.expect("argc > 0 here");
        let dst = fx.slot_addr(argv, (i as u32 * VALUE_SIZE) as i32);
        ownership::write_borrow(fx, &op, dst);
    }
    let argv_ptr = match argv {
        Some(ss) => fx.slot_addr(ss, 0),
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    let argc_v = fx.b.ins().iconst(fx.em.ptr, argc as i64);
    let out_ss = fx.temp_slot();
    let out = fx.slot_addr(out_ss, 0);
    let status = fx.call_status("zeo_rt_kernel_puts", &[argv_ptr, argc_v, out]);
    fx.fallible(status);
    // `puts` answers nil -- an immediate, nothing to release.
    Ok(())
}

/// A native loop: `while`/`until` (pre- or post-test) and bare `loop`.
/// The release pool is bracketed -- marked at entry, reset at the latch
/// and at the exit -- so a long loop never accumulates temps.
fn lower_loop(
    fx: &mut Fx,
    cond: Option<(NodeId, bool)>,
    body: &[NodeId],
    post: bool,
    result: Option<cranelift_codegen::ir::Value>,
) -> Result<(), String> {
    let mark = fx.call_status("zeo_rt_pool_mark", &[]);
    let head = fx.b.create_block();
    let body_blk = fx.b.create_block();
    let latch = fx.b.create_block();
    let exit_normal = fx.b.create_block();
    let exit = fx.b.create_block();
    let first = if post { body_blk } else { head };
    fx.b.ins().jump(first, &[]);

    fx.b.switch_to_block(head);
    match cond {
        Some((cond_id, negate)) => {
            let c = lower_expr(fx, cond_id)?;
            let mut t = ownership::truthy(fx, c);
            if negate {
                // `until`: flip the low bit of the 0/1 truthiness.
                t =
                    fx.b.ins()
                        .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, t, 0);
            }
            fx.b.ins().brif(t, body_blk, &[], exit_normal, &[]);
        }
        None => {
            fx.b.ins().jump(body_blk, &[]);
        }
    }

    fx.b.switch_to_block(body_blk);
    let status = fx.call_status("zeo_rt_check_ints", &[]);
    fx.fallible(status);
    fx.loops.push(LoopCtl {
        exit,
        latch,
        body: body_blk,
        result,
        depth: fx.ensure_depth,
        handling: fx.handling_depth,
    });
    lower_stmts(fx, body)?;
    fx.loops.pop();
    fx.b.ins().jump(latch, &[]);

    fx.b.switch_to_block(latch);
    fx.call("zeo_rt_pool_reset", &[mark]);
    fx.b.ins().jump(head, &[]);

    // Ran-to-completion (condition went false): a loop's own value is nil;
    // `break v` bypasses this write.
    fx.b.switch_to_block(exit_normal);
    if let Some(dst) = result {
        ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
    }
    fx.b.ins().jump(exit, &[]);

    fx.b.switch_to_block(exit);
    fx.call("zeo_rt_pool_reset", &[mark]);
    Ok(())
}

/// `for target in iterable` -- ruby performs NO type dispatch here (it
/// compiles to `iterable.each { |target| .. }`), so the runtime driver
/// decides per receiver: a live Array indexes the receiver itself (CRuby
/// re-reads its length every step), an Int-bounded Range counts, and
/// everything else walks what `each` yielded. The loop's own value is the
/// collection; a `break v` supplies its own.
fn lower_for(
    fx: &mut Fx,
    site: NodeId,
    target: &crate::hir::MultiTarget,
    iterable: NodeId,
    body: &[NodeId],
    result: Option<cranelift_codegen::ir::Value>,
) -> Result<(), String> {
    use crate::hir::MultiTarget;
    let coll = lower_expr(fx, iterable)?;
    let coll_ptr = ownership::borrow_ptr(fx, &coll);
    if coll.owned() {
        ownership::pool_owned(fx, coll_ptr, coll.tag());
    }
    // `for x in obj` is one parameter, so a multi-value yield binds its
    // FIRST value; `for k, v in obj` packs.
    let packed = u8::from(matches!(target, MultiTarget::Nested(_)));
    let state_ss = fx.b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        u32::try_from(zeo_abi::abi::FOR_STATE_SIZE).expect("the state slot fits a u32"),
        3,
    ));
    let state = fx.slot_addr(state_ss, 0);
    let packed_v = fx.b.ins().iconst(types::I8, i64::from(packed));
    let status = fx.call_status("zeo_rt_for_begin", &[coll_ptr, packed_v, state]);
    fx.fallible(status);

    let mark = fx.call_status("zeo_rt_pool_mark", &[]);
    let head = fx.b.create_block();
    let body_blk = fx.b.create_block();
    let latch = fx.b.create_block();
    let exit_normal = fx.b.create_block();
    let exit = fx.b.create_block();
    // The error path releases the state before the enclosing landing runs.
    let outer_land = fx.land;
    let end_land = fx.b.create_block();
    fx.b.ins().jump(head, &[]);

    fx.b.switch_to_block(head);
    let elem_ss = fx.temp_slot();
    let elem = fx.slot_addr(elem_ss, 0);
    let more_ss = fx.temp_slot();
    let more_ptr = fx.slot_addr(more_ss, 0);
    fx.call("zeo_rt_for_next", &[state, elem, more_ptr]);
    let fl = cranelift_codegen::ir::MemFlagsData::trusted();
    let more = fx.b.ins().load(types::I8, fl, more_ptr, 0);
    fx.b.ins().brif(more, body_blk, &[], exit_normal, &[]);

    fx.b.switch_to_block(body_blk);
    fx.owned_created += 1;
    super::multi::write_multi_target(fx, site, target, elem)?;
    let status = fx.call_status("zeo_rt_check_ints", &[]);
    fx.land = end_land;
    fx.fallible(status);
    fx.loops.push(LoopCtl {
        exit,
        latch,
        body: body_blk,
        result,
        depth: fx.ensure_depth,
        handling: fx.handling_depth,
    });
    let r = lower_stmts(fx, body);
    fx.loops.pop();
    fx.land = outer_land;
    r?;
    fx.b.ins().jump(latch, &[]);

    fx.b.switch_to_block(latch);
    fx.call("zeo_rt_pool_reset", &[mark]);
    fx.b.ins().jump(head, &[]);

    // Ran to completion: the loop answers the collection it walked.
    fx.b.switch_to_block(exit_normal);
    if let Some(dst) = result {
        fx.call("zeo_rt_for_result", &[state, dst]);
        fx.owned_created += 1;
        fx.owned_consumed += 1;
    }
    fx.b.ins().jump(exit, &[]);

    fx.b.switch_to_block(end_land);
    fx.call("zeo_rt_for_end", &[state]);
    fx.b.ins().jump(outer_land, &[]);

    fx.b.switch_to_block(exit);
    fx.call("zeo_rt_for_end", &[state]);
    fx.call("zeo_rt_pool_reset", &[mark]);
    Ok(())
}

/// A loop-targeted `break`/`next`/`redo` with an `ensure` between it and
/// its loop. Ruby runs that ensure first, so the jump travels as a signal
/// down the landing chain and the ensure-carrying `begin` settles it back
/// onto the loop's own targets ([`super::control::lower_begin`]). `$!` is
/// left to that chain, which pops exactly what it pushed.
fn signal_jump(
    fx: &mut Fx,
    value: Option<NodeId>,
    kind: zeo_abi::abi::SignalKind,
) -> Result<(), String> {
    let ptr = match value {
        Some(v) => {
            let op = lower_expr(fx, v)?;
            ownership::move_ptr(fx, &op)
        }
        // `Redo` carries nothing; the other two answer nil without one.
        None if matches!(kind, zeo_abi::abi::SignalKind::Redo) => fx.b.ins().iconst(fx.em.ptr, 0),
        None => ownership::move_ptr(fx, &super::operand::Operand::Nil),
    };
    let k = fx.b.ins().iconst(types::I8, i64::from(kind as u8));
    fx.call("zeo_rt_signal_set", &[k, ptr]);
    fx.ensure_jumps += 1;
    let land = fx.land;
    fx.b.ins().jump(land, &[]);
    fx.continue_unreachable();
    Ok(())
}

/// A `while`/`until`/`loop`/`for` in VALUE position: its own value is nil
/// (the collection, for `for`), and a `break v` supplies its own.
pub(crate) fn loop_value(fx: &mut Fx, node: NodeId) -> Result<super::operand::Operand, String> {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    match &fx.an.compiler.hir[node] {
        HirNode::While {
            cond,
            body,
            negate,
            post,
        } => {
            let (cond, body, negate, post) = (*cond, body.clone(), *negate, *post);
            lower_loop(fx, Some((cond, negate)), &body, post, Some(dst))?;
        }
        HirNode::Loop { body } => {
            let body = body.clone();
            lower_loop(fx, None, &body, false, Some(dst))?;
        }
        HirNode::For {
            target,
            iterable,
            body,
        } => {
            let (target, iterable, body) = (target.clone(), *iterable, body.clone());
            lower_for(fx, node, &target, iterable, &body, Some(dst))?;
        }
        other => {
            let what = format!("the node kind `{}`", super::expr::variant_name(other));
            return fx.unsupported(node, &what);
        }
    }
    fx.owned_created += 1;
    Ok(super::operand::Operand::Slot {
        ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    })
}

/// Stamp the frame's line for a CALL whose method NAME sits on a later
/// line than the expression it belongs to (`recv\n  .m`, `end.m(..)`).
///
/// Emitted after the receiver and the arguments, because those belong to
/// their own lines: a raise inside one must still report where it was
/// written, which is what ruby does.
///
/// The line only, never a coverage hit. Ruby counts a multi-line call's
/// FIRST line in `Coverage` while reporting the name's line in a
/// backtrace, so the two answers cannot come from one stamp.
pub(crate) fn stamp_call_line(fx: &mut Fx, site: NodeId) {
    let Some(line) = fx.an.compiler.hir.call_line(site) else {
        return;
    };
    if fx.prev_line == Some(line) {
        return;
    }
    fx.prev_line = Some(line);
    let v = fx.b.ins().iconst(types::I32, i64::from(line));
    fx.call("zeo_rt_set_line", &[v]);
}

/// Stamp a `set_line` only when the statement's line differs from the
/// previous stamp.
fn stamp_line(fx: &mut Fx, stmt: NodeId) {
    let Some((file, line)) = fx.location(stmt) else {
        return;
    };
    let file = file.to_string();
    // The DWARF row is stamped BEFORE the dedup: `set_srcloc` emits no
    // code, and skipping it whenever the previous statement shared a line
    // would leave whole blocks attributed to whatever the last STAMPED
    // statement was -- the dedup exists to save a runtime CALL, which is
    // a different question.
    if let Some(debug) = fx.em.debug.as_mut() {
        let loc = debug.srcloc(&file, line);
        fx.b.set_srcloc(loc);
    }
    if fx.prev_line == Some(line) && fx.prev_file.as_deref() == Some(file.as_str()) {
        return;
    }
    // The main script never qualifies for coverage: its top level began
    // before `Coverage.start` ran. `entry_file_name`, not `files[0]` -- the
    // vendored corelib registers its own sources ahead of the main file, so
    // the corelib -> main transition looked like entering a spliced file and
    // reported the main script.
    let entering_spliced_file = fx.prev_file.as_deref().is_some_and(|f| f != file)
        && fx
            .an
            .compiler
            .hir
            .entry_file_name()
            .is_none_or(|name| name != file);
    fx.prev_line = Some(line);
    fx.prev_file = Some(file.clone());
    let v = fx.b.ins().iconst(types::I32, i64::from(line));
    fx.call("zeo_rt_set_line", &[v]);
    // Line coverage's one hit per stamped statement -- a program without
    // the `require` emits nothing here at all.
    if fx.em.cov_active {
        let (fptr, flen) = super::expr::rodata_name(fx, &file);
        if entering_spliced_file {
            fx.call("zeo_rt_cov_file_loaded", &[fptr, flen]);
        }
        fx.call("zeo_rt_cov_line", &[fptr, flen, v]);
        fx.em.cov_lines.entry(file).or_default().insert(line);
    }
}

/// One class-body site's marker-time emission: record the declaration's
/// `const_source_location`, then call the compiled body with the class as
/// `self` (its value -- ruby's class-body tail -- is discarded here; a
/// `class` expression in value position still refuses).
/// A class-body marker in STATEMENT position: run the site, drop its
/// value.
pub(crate) fn emit_class_body_call(
    fx: &mut Fx,
    call: &super::emit::ClassBodyCall,
) -> Result<(), String> {
    let op = class_body_site(fx, call)?;
    ownership::discard(fx, op);
    Ok(())
}

/// The site's Ruby VALUE -- what a `class`/`module` written where a value
/// is read evaluates to. `expression` distinguishes the two positions: a
/// tail whose value zeo cannot name is nil (nothing necessarily reads
/// it), an expression's is a refusal.
pub(crate) fn class_body_value(
    fx: &mut Fx,
    site: NodeId,
    expression: bool,
) -> Result<super::operand::Operand, String> {
    use super::emit::BodyTail;
    let Some(call) = fx.em.class_bodies.get(&site).cloned() else {
        // A hoisted or statement-free site: its body already ran (or has
        // nothing to run), and ruby's value is the body's own tail.
        return Ok(super::operand::Operand::Nil);
    };
    let op = class_body_site(fx, &call)?;
    match &call.tail {
        BodyTail::Own => Ok(op),
        BodyTail::Sym(name) => {
            ownership::discard(fx, op);
            let name = name.clone();
            super::consts::symbol_value(fx, &name)
        }
        BodyTail::OwnClass => {
            ownership::discard(fx, op);
            Ok(super::consts::class_immediate(
                fx,
                crate::compiler::ClassId(call.class),
            ))
        }
        BodyTail::Unknown(kind) if expression => {
            let kind = *kind;
            fx.unsupported(
                site,
                &format!("a class body read for its VALUE ending in {kind}"),
            )
        }
        BodyTail::Unknown(_) => {
            ownership::discard(fx, op);
            Ok(super::operand::Operand::Nil)
        }
    }
}

/// One class-body site, under its trailing `if`/`unless` when it has one.
///
/// The condition is lowered HERE, in the enclosing scope, because that is
/// where ruby runs it and where the locals it reads live -- `class Set ...
/// end if set_pp` in pp.rb reads a top-level local. Everything the site does
/// rides inside the branch: a class whose guard failed was never declared,
/// so it announces nothing and reveals nothing, and the site's value is nil.
fn class_body_site(
    fx: &mut Fx,
    call: &super::emit::ClassBodyCall,
) -> Result<super::operand::Operand, String> {
    use cranelift_codegen::ir::InstBuilder;
    let Some((cond, run_when)) = call.guard else {
        return class_body_site_run(fx, call);
    };
    let c = super::expr::lower_expr(fx, cond)?;
    let truthy = ownership::truthy(fx, c);
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let b_run = fx.b.create_block();
    let b_skip = fx.b.create_block();
    let join = fx.b.create_block();
    let (t, f) = match run_when {
        true => (b_run, b_skip),
        false => (b_skip, b_run),
    };
    fx.b.ins().brif(truthy, t, &[], f, &[]);
    fx.b.switch_to_block(b_run);
    let op = class_body_site_run(fx, call)?;
    ownership::write_move_into(fx, &op, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(b_skip);
    ownership::write_move_into(fx, &super::operand::Operand::Nil, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(super::operand::Operand::Slot {
        ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    })
}

fn class_body_site_run(
    fx: &mut Fx,
    call: &super::emit::ClassBodyCall,
) -> Result<super::operand::Operand, String> {
    use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};
    use cranelift_module::Module;
    // The frozen-reopen guard runs FIRST: a frozen class raises before the
    // body's declaration bookkeeping, let alone its statements.
    if !call.freeze_guard.is_empty() {
        let names: Vec<&str> = call.freeze_guard.iter().map(String::as_str).collect();
        let (ptr, n) = super::statics::str_array(fx, &names);
        let cid = fx.b.ins().iconst(types::I32, i64::from(call.class));
        let st = fx.call_status("zeo_rt_guard_class_reopen", &[cid, ptr, n]);
        fx.fallible(st);
    }
    // The reopen is POSITIONAL: from here on the reopened bodies answer for
    // themselves, and above this line they defer to the rows they replaced.
    // Set before the body's own statements, which may call one.
    if !call.reopen_flags.is_empty() {
        let gv = fx
            .em
            .module
            .declare_data_in_func(fx.em.reopen_flags_id, fx.b.func);
        let base = fx.b.ins().symbol_value(fx.em.ptr, gv);
        let one = fx.b.ins().iconst(types::I8, 1);
        for &idx in &call.reopen_flags {
            fx.b.ins()
                .store(MemFlagsData::trusted(), one, base, idx as i32);
        }
    }
    // A runtime-conditional class's guarded definition just RAN: the
    // constant exists from here on, before any declaration bookkeeping and
    // at EVERY site (whichever branch runs must reveal).
    if call.reveal {
        let cid = fx.b.ins().iconst(types::I32, i64::from(call.class));
        fx.call("zeo_rt_reveal_class", &[cid]);
    }
    // The constant is set, so ruby announces it -- before `inherited` and
    // before the body, the order `vm_declare_class` hard-codes.
    if let Some((owner, name)) = &call.const_added {
        super::consts::const_added_announce(fx, *owner, name)?;
    }
    if let Some((owner, name, file, line)) = &call.const_loc {
        let owner_v = fx.b.ins().iconst(types::I32, i64::from(*owner));
        let (nptr, nlen) = name_pair(fx, name);
        let (fptr, flen) = name_pair(fx, file);
        let line_v = fx.b.ins().iconst(types::I32, i64::from(*line));
        fx.call(
            "zeo_rt_record_const_location",
            &[owner_v, nptr, nlen, fptr, flen, line_v],
        );
    }
    // ...then `Super.inherited(C)`, the order `vm_declare_class` hard-codes
    // (the constant is set, the hook fires, then the body runs).
    if let Some(parent) = call.inherited {
        let recv = super::consts::class_immediate(fx, crate::compiler::ClassId(parent));
        let recv_ptr = ownership::borrow_ptr(fx, &recv);
        let arg = super::consts::class_immediate(fx, crate::compiler::ClassId(call.class));
        let argv = ownership::borrow_ptr(fx, &arg);
        let sym = fx.sym_id("inherited");
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let zero_box = fx.box_v();
        let argc = fx.b.ins().iconst(fx.em.ptr, 1);
        let null = fx.b.ins().iconst(fx.em.ptr, 0);
        let status = fx.call_status(
            "zeo_rt_send_value_in",
            &[zero_box, recv_ptr, sym, argv, argc, null, out],
        );
        fx.fallible(status);
        fx.owned_created += 1;
        ownership::discard(
            fx,
            super::operand::Operand::Slot {
                ss,
                owned: true,
                tag: super::operand::TagInfo::Unknown,
            },
        );
    }
    // `alias`'s builtin source validates as this body finishes -- CRuby's
    // timing, run at the CALL site so an alias-only (empty-statement) body
    // still checks (rustc emits the check even for an otherwise empty
    // body).
    let validate = |fx: &mut Fx| {
        let has = !fx.an.compiler.classes[call.class as usize]
            .builtin_aliases
            .is_empty();
        if has {
            let cid = fx.b.ins().iconst(types::I32, i64::from(call.class));
            let st = fx.call_status("zeo_rt_validate_class_aliases", &[cid]);
            fx.fallible(st);
        }
    };
    let Some(func) = call.func else {
        validate(fx);
        return Ok(super::operand::Operand::Nil);
    };
    // `self` = the class, materialized as a Class immediate.
    let self_ss = fx.temp_slot();
    let self_addr = fx.slot_addr(self_ss, 0);
    let fl = MemFlagsData::trusted();
    let z = fx.b.ins().iconst(types::I64, 0);
    for off in [0, 8, 16] {
        fx.b.ins().store(fl, z, self_addr, off);
    }
    let tag =
        fx.b.ins()
            .iconst(types::I8, i64::from(zeo_abi::abi::ValueTag::Class as u8));
    fx.b.ins()
        .store(fl, tag, self_addr, zeo_abi::abi::TAG_OFFSET as i32);
    let cid = fx.b.ins().iconst(types::I32, i64::from(call.class));
    fx.b.ins()
        .store(fl, cid, self_addr, zeo_abi::abi::PAYLOAD_OFFSET as i32);
    let out_ss = fx.temp_slot();
    let out = fx.slot_addr(out_ss, 0);
    let fref = fx.em.module.declare_func_in_func(func, fx.b.func);
    let inst = fx.b.ins().call(fref, &[self_addr, out]);
    let status = fx.b.func.dfg.inst_results(inst)[0];
    fx.fallible(status);
    validate(fx);
    fx.owned_created += 1;
    Ok(super::operand::Operand::Slot {
        ss: out_ss,
        owned: true,
        tag: super::operand::TagInfo::Unknown,
    })
}

/// A `&str`'s `.rodata` `(ptr, len)` pair.
pub(super) fn name_pair(
    fx: &mut Fx,
    s: &str,
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    use cranelift_codegen::ir::InstBuilder;
    let off = fx.em.intern_rodata(s.as_bytes());
    let ptr = fx.rod(off);
    let len = fx.b.ins().iconst(fx.em.ptr, s.len() as i64);
    (ptr, len)
}

/// One positional definition-surface retag: the enclosing class body's own
/// class, the name as a Symbol, and the entry's verb byte when it takes one.
fn apply_visibility(
    fx: &mut Fx,
    stmt: NodeId,
    entry: &'static str,
    name: &str,
    verb: Option<u8>,
) -> Result<(), String> {
    use cranelift_codegen::ir::InstBuilder;
    let Some(cid) = fx.defining_class.or(fx.method_class) else {
        return fx.unsupported(stmt, "a visibility retag outside a class body");
    };
    let cid_v =
        fx.b.ins()
            .iconst(cranelift_codegen::ir::types::I32, i64::from(cid.0));
    let sym = fx.sym_id(name);
    let mut args = vec![cid_v, sym];
    if let Some(v) = verb {
        args.push(
            fx.b.ins()
                .iconst(cranelift_codegen::ir::types::I8, i64::from(v)),
        );
    }
    let status = fx.call_status(entry, &args);
    fx.fallible(status);
    Ok(())
}

/// The compile-time verdict on an `if` condition, or `None` when it has to be
/// asked at run time. See `analyze::constfold::static_cond`.
/// The `=~` a folded condition performed, if the condition IS one. Its
/// operands are static by construction (that is why the fold succeeded), so
/// re-running it for `$~` cannot raise or repeat work.
fn folded_match(fx: &Fx<'_, '_>, cond: crate::hir::NodeId) -> Option<crate::hir::NodeId> {
    match &fx.an.compiler.hir[cond] {
        crate::hir::HirNode::Call { name, .. } if name == "=~" => Some(cond),
        _ => None,
    }
}

pub(crate) fn static_cond(fx: &Fx<'_, '_>, cond: crate::hir::NodeId) -> Option<bool> {
    let env = crate::analyze::constfold::ConstEnv {
        compiler: &fx.an.compiler,
        defining_class: fx.defining_class.or(fx.method_class),
        box_id: fx.box_id,
    };
    crate::analyze::constfold::static_cond(&env, cond)
}
