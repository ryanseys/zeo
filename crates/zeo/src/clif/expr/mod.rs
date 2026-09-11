//! Expression lowering to Cranelift IR: literals, local reads, calls, and
//! the numeric binary operators with their three-arm shape -- inline Int,
//! inline Float, dynamic `send_value_in` fallback.

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::diagnostics::clif::CResult;
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};
use zeo_abi::abi::{PAYLOAD_OFFSET, TAG_OFFSET, ValueTag};

mod branching;
mod calls;
mod literals;
mod runtime;
pub(in crate::clif) use branching::lower_condition;
use branching::*;
use calls::*;
pub(crate) use calls::{
    bypasses_visibility, later_nodes, method_class_shadows, park_reassignable, site_decided_call,
};
use literals::*;
pub(crate) use literals::{pure_literal, rodata_name};
use runtime::*;
pub(crate) use runtime::{binding_value, binding_value_at};

/// `EncodingId(1)` = UTF-8, every plain source literal's encoding.
pub(super) const ENC_UTF8: i64 = 1;

/// Whether `String#freeze` is still the builtin row this fold stands in for.
///
/// TWO questions, because they are recorded in different places and asking
/// only one is a wrong answer rather than a missed fold:
///
/// * a RUNTIME (re)definition -- `define_method(:freeze)`, a `send` to a
///   definition verb -- joins `runtime_patches`;
/// * a COMPILE-TIME reopen (`class String; def freeze`) does NOT. It
///   registers on the value channel, and `a_later_def_on_a_builtin_reaches_back`
///   records that `runtime_patches` never sees one.
///
/// Gating on the first alone let `class String; def freeze = "x"; end` keep
/// answering the literal, which is a silently wrong program rather than a
/// slower one.
///
/// The whole chain is asked, not just `String`: a `Kernel#freeze` or an
/// `Object#freeze` reopen intercepts the same call.
///
/// One narrow divergence stays, and it is the pre-existing one rather than a
/// new one. CRuby checks its redefinition flag when the instruction RUNS, so
/// a literal frozen BEFORE a later `define_method(:freeze)` still interns
/// there. zeo decides per program, so a program that redefines `freeze`
/// anywhere gets the ordinary dispatch everywhere.
fn freeze_is_pristine(fx: &Fx) -> bool {
    let c = &fx.an.compiler;
    !c.may_be_patched_at_runtime("freeze")
        && c.method_in_chain(crate::compiler::STRING_CLASS, "freeze")
            .is_none()
}

/// The text of a string literal `.freeze` may fold, or `None`.
///
/// Only the shape the interned path can serve: no interpolation, no raw-byte
/// segment, and no `# encoding:` magic comment (which tags every literal in
/// the file byte-built and skips the frozen pool, as the ordinary literal
/// path already records).
fn frozen_foldable_literal(fx: &Fx, recv: crate::hir::NodeId) -> Option<String> {
    let HirNode::StringLit(parts) = &fx.an.compiler.hir[recv] else {
        return None;
    };
    if script_encoding_id(fx).is_some() || parts.iter().any(|p| matches!(p, StrPart::Bytes(_))) {
        return None;
    }
    pure_literal(parts)
}

/// A non-interpolated string literal, straight out of `.rodata`.
///
/// `frozen` picks the entry: the interned immortal twin, so two equal
/// literals are ONE object, or a fresh allocation per evaluation. Shared by
/// the ordinary literal path and by the `.freeze` fold, which is the same
/// question asked at one site rather than per file.
fn emit_pure_str_literal(fx: &mut Fx, text: &str, frozen: bool) -> Operand {
    use cranelift_codegen::ir::InstBuilder;
    let off = fx.em.intern_rodata(text.as_bytes());
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let ptr = fx.rod(off);
    let len_v = fx.b.ins().iconst(fx.em.ptr, text.len() as i64);
    let enc = fx.b.ins().iconst(types::I8, ENC_UTF8);
    let entry = match frozen {
        true => "zeo_rt_str_lit",
        // The rodata bytes are process-lived, so the fresh string BORROWS
        // them (COW) instead of copying per evaluation.
        false => "zeo_rt_str_lit_ro",
    };
    fx.call(entry, &[ptr, len_v, enc, dst]);
    fx.owned_created += 1;
    Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(ValueTag::Str as u8),
    }
}

/// Stamps the node's span on any error propagating out
/// (`with_span_if_missing` -- the innermost frame wins), so rejection
/// sites keep raising bare messages and still end up located.
pub(crate) fn lower_expr(fx: &mut Fx, id: NodeId) -> CResult<Operand> {
    lower_expr_inner(fx, id).map_err(|e| e.with_span_if_missing(fx.an.compiler.hir.span(id)))
}

fn lower_expr_inner(fx: &mut Fx, id: NodeId) -> CResult<Operand> {
    // `blk.call(..)` on the scope's own `&block` parameter -- see
    // `block_param_call`.
    if let Some(op) = block_param_call(fx, id)? {
        return Ok(op);
    }
    // `Ractor.new(*args, name: ...) { }` -- the one concurrency constructor
    // with no usable runtime row: the block must be built as an isolated
    // Proc where the compiler can see the capture set, so the site is
    // recognized here rather than dispatched.
    if let Some(op) = ractor_new(fx, id)? {
        return Ok(op);
    }
    // A refined name at a site some `using` covers, ahead of every fold and
    // fast path below -- a refinement may well override one of them
    // (`String#size` is both a refinable name and an inlined length read),
    // and the fold would devirtualize straight past the runtime match.
    if let Some(op) = super::refine::refined_call(fx, id)? {
        return Ok(op);
    }
    match &fx.an.compiler.hir[id] {
        HirNode::IntegerLit(v) => {
            let v = *v;
            Ok(Operand::Int(fx.b.ins().iconst(types::I64, v)))
        }
        HirNode::FloatLit(v) => {
            let v = *v;
            Ok(Operand::Float(fx.b.ins().f64const(v)))
        }
        HirNode::BoolLit(v) => {
            let v = i64::from(*v);
            Ok(Operand::Bool(fx.b.ins().iconst(types::I8, v)))
        }
        HirNode::NilLit => Ok(Operand::Nil),
        // The bignum / rational / imaginary literals: digits baked into
        // rodata, assembled by the runtime at the use site (no
        // compile-time bigint dependency, no string parsing).
        // `int_from_u32_digits` demotes to `Int` when it fits, so
        // one Ruby Integer class covers both payloads.
        HirNode::BigIntegerLit { negative, digits } => {
            let (negative, digits) = (*negative, digits.clone());
            let neg = fx.b.ins().iconst(types::I8, i64::from(negative));
            let (ptr, n) = u32_array(fx, &digits);
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            fx.call("zeo_rt_int_digits", &[neg, ptr, n, dst]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::RationalLit {
            negative,
            num_digits,
            den_digits,
        } => {
            let (negative, num_digits, den_digits) =
                (*negative, num_digits.clone(), den_digits.clone());
            let neg = fx.b.ins().iconst(types::I8, i64::from(negative));
            let (nptr, nn) = u32_array(fx, &num_digits);
            let (dptr, dn) = u32_array(fx, &den_digits);
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            fx.call("zeo_rt_rational_digits", &[neg, nptr, nn, dptr, dn, dst]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Known(ValueTag::Rational as u8),
            })
        }
        HirNode::ImaginaryLit(inner) => {
            let inner = *inner;
            let op = lower_expr(fx, inner)?;
            let tag = op.tag();
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, tag);
            }
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            fx.call("zeo_rt_complex_lit", &[p, dst]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Known(ValueTag::Complex as u8),
            })
        }
        HirNode::StringLit(parts) => {
            let parts = parts.clone();
            // A `# encoding:` magic comment tags EVERY literal in the file
            // with that encoding, byte-built -- and skips the frozen pool.
            // Otherwise a raw-byte segment forces the byte
            // builder at the SOURCE encoding (an invalid `\xNN` literal
            // stays UTF-8-and-invalid, matching CRuby), and a purely-UTF-8
            // literal takes the readable path.
            let script_enc = script_encoding_id(fx);
            let enc_id = script_enc.unwrap_or(ENC_UTF8);
            let bytes_built =
                script_enc.is_some() || parts.iter().any(|p| matches!(p, StrPart::Bytes(_)));
            if !bytes_built && let Some(text) = pure_literal(&parts) {
                // `# frozen_string_literal: true`: a non-interpolated
                // literal IS its interned frozen twin (equal literals share
                // one object, and mutation raises).
                let frozen = fx.an.compiler.hir.literal_frozen_at(id);
                return Ok(emit_pure_str_literal(fx, &text, frozen));
            }
            // The builder: a fresh mutable string, literal pieces appended
            // raw, interpolated values through the runtime's to_s dispatch
            // (whose raise propagates). Pooled at creation -- a piece can
            // raise mid-build.
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            let null = fx.b.ins().iconst(fx.em.ptr, 0);
            let zero = fx.b.ins().iconst(fx.em.ptr, 0);
            let enc = fx.b.ins().iconst(types::I8, enc_id);
            fx.call("zeo_rt_str_new", &[null, zero, enc, dst]);
            fx.owned_created += 1;
            ownership::pool_owned(fx, dst, TagInfo::Known(ValueTag::Str as u8));
            for part in &parts {
                match part {
                    StrPart::Lit(text) => {
                        if text.is_empty() {
                            continue;
                        }
                        let off = fx.em.intern_rodata(text.as_bytes());
                        let ptr = fx.rod(off);
                        let len_v = fx.b.ins().iconst(fx.em.ptr, text.len() as i64);
                        fx.call("zeo_rt_str_append_lit", &[dst, ptr, len_v]);
                    }
                    StrPart::Bytes(raw) => {
                        if raw.is_empty() {
                            continue;
                        }
                        let off = fx.em.intern_rodata(raw);
                        let ptr = fx.rod(off);
                        let len_v = fx.b.ins().iconst(fx.em.ptr, raw.len() as i64);
                        fx.call("zeo_rt_str_append_bytes", &[dst, ptr, len_v]);
                    }
                    StrPart::Interp(n) => {
                        let op = lower_expr(fx, *n)?;
                        let p = ownership::borrow_ptr(fx, &op);
                        if op.owned() {
                            ownership::pool_owned(fx, p, op.tag());
                        }
                        let status = fx.call_status("zeo_rt_str_append_value", &[dst, p]);
                        fx.fallible(status);
                    }
                }
            }
            Ok(Operand::Ptr {
                addr: dst,
                owned: false,
                tag: TagInfo::Known(ValueTag::Str as u8),
            })
        }
        HirNode::LocalRead(name) => {
            let name = name.clone();
            match ownership::read_local(fx, &name) {
                Some(op) => Ok(op),
                // A read before any write is nil in Ruby only via `defined?`
                // shapes the slice does not lower; a plain read of an
                // unhoisted name cannot reach here.
                None => fx.unsupported(id, "a read of an unknown local"),
            }
        }
        // `a..b` used as a CONDITION -- a latch, not a Range. It answers
        // true from the evaluation whose left operand is truthy through the
        // one whose right operand is; two dots retest the right operand in
        // the very evaluation that turned the latch on, three dots wait for
        // the next.
        HirNode::FlipFlop {
            state,
            left,
            right,
            exclusive,
        } => {
            let (state, left, right, exclusive) = (*state, *left, *right, *exclusive);
            flip_flop(fx, state, left, right, exclusive)
        }
        // A loop in VALUE position: its own value is nil (`for` answers the
        // collection it walked), and a `break v` supplies its own.
        HirNode::While { .. } | HirNode::Loop { .. } | HirNode::For { .. } => {
            super::stmt::loop_value(fx, id)
        }
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
            // As the statement arm: a decided guard emits one branch only.
            if let Some(taken) = super::stmt::static_cond(fx, cond) {
                let taken = if taken { &then_body } else { &else_body };
                let ss = fx.temp_slot();
                let dst = fx.slot_addr(ss, 0);
                super::stmt::lower_value_body_into(fx, taken, dst)?;
                fx.owned_created += 1;
                return Ok(Operand::Slot {
                    ss,
                    owned: true,
                    tag: TagInfo::Unknown,
                });
            }
            if_expr(fx, cond, &then_body, &else_body)
        }
        // `raise` in VALUE position (`a = raise "x"`). Kernel#raise never
        // returns, so its value is unreachable -- but a program that defines
        // its OWN `raise` gets an ordinary method, and the expression has to
        // carry what that method answered. The send below finds whichever of
        // the two is in scope.
        HirNode::Raise(args, crate::hir::RaiseCause::Absent) => {
            let args = args.clone();
            let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
            // The statement path stamps the line before it lowers; a `raise`
            // that ends a body reaches this arm instead and its backtrace
            // frame would otherwise name the line above it.
            super::stmt::stamp_line(fx, id);
            super::call::implicit_send(fx, id, "raise", &elems)
        }
        // A diverging expression in value position: the signal leaves the
        // block unreachable; the nil is never read.
        HirNode::Break(..)
        | HirNode::Next(..)
        | HirNode::Redo
        | HirNode::Raise(..)
        | HirNode::Return(..) => {
            super::stmt::lower_stmt(fx, id)?;
            Ok(Operand::Nil)
        }
        // A parenthesized sequence in VALUE position (`(a; b)`, a default
        // expression with side effects): statements, then the last as the
        // value.
        HirNode::Seq(stmts) => {
            let stmts = stmts.clone();
            match stmts.split_last() {
                Some((last, init)) => {
                    super::stmt::lower_stmts(fx, init)?;
                    lower_expr(fx, *last)
                }
                None => Ok(Operand::Nil),
            }
        }
        // A multiple assignment in VALUE position yields the RAW right-hand
        // side (CRuby: `(a, b = arr)` is `arr` itself, not a fresh copy).
        HirNode::MultiWrite { targets, value } => {
            let (targets, value) = (targets.clone(), *value);
            let op = lower_expr(fx, value)?;
            let tag = op.tag();
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, tag);
            }
            super::multi::lower_multi_group(fx, id, &targets, ptr)?;
            Ok(Operand::Ptr {
                addr: ptr,
                owned: false,
                tag,
            })
        }
        // Assignment in EXPRESSION position (`f(x = 1)`, the desugared
        // `[]=` value hand-back): run the statement, answer the local.
        HirNode::LocalWrite(name, _) => {
            let name = name.clone();
            super::stmt::lower_stmt(fx, id)?;
            Ok(ownership::read_local(fx, &name).expect("just assigned"))
        }
        HirNode::SymbolLit(name) => {
            let name = name.clone();
            super::consts::symbol_value(fx, &name)
        }
        HirNode::ArrayLit(elems) => {
            let elems = elems.clone();
            let addr = super::call::build_array(fx, &elems)?;
            Ok(Operand::Ptr {
                addr,
                owned: false,
                tag: TagInfo::Known(ValueTag::Array as u8),
            })
        }
        HirNode::HashLit(pairs) => {
            // A KWARGS_HASH-flagged literal is a folded keyword set (a
            // `yield`'s kwargs), whose empty-drop/mark semantics the yield
            // lowering does not carry yet.
            if fx
                .an
                .compiler
                .hir
                .has_flag(id, crate::hir::NodeFlag::KWARGS_HASH)
            {
                return fx.unsupported(id, "keyword arguments to `yield`");
            }
            let pairs = pairs.clone();
            let addr = super::call::build_hash(fx, &pairs)?;
            Ok(Operand::Ptr {
                addr,
                owned: false,
                tag: TagInfo::Known(ValueTag::Hash as u8),
            })
        }
        // A `def` in EXPRESSION position is a RUNTIME method install: its
        // body becomes a method-body proc and the install answers the
        // method-name Symbol.
        HirNode::DefMethod {
            name,
            params,
            body,
            is_class_method,
            visibility,
            is_def,
        } => {
            let (name, params, body, is_class_method, visibility, is_def) = (
                name.clone(),
                params.as_ref().clone(),
                body.clone(),
                *is_class_method,
                *visibility,
                *is_def,
            );
            runtime_def(
                fx,
                id,
                &name,
                &params,
                &body,
                is_class_method,
                visibility,
                is_def,
            )
        }
        HirNode::Lambda {
            params,
            body,
            method_body,
        } => {
            let (params, body, method_body) = (params.as_ref().clone(), body.clone(), *method_body);
            if method_body {
                // The two lowering sites that build one are the `def obj.m`
                // and `class << obj` desugars, so this IS a real `def`'s
                // body: its bare `yield` reaches the installed method's
                // call-site block, and a bare `super` forwards the
                // parameter list a `def` actually wrote.
                // A per-object singleton `def` labels its frame after the
                // METHOD, bare (`m`), where a `def self.x` in a class body is
                // qualified (`C.x`). Anything else reaching here really is a
                // block. See `Hir::singleton_def_names`.
                let frame = match fx.an.compiler.hir.singleton_def_names.get(&id) {
                    Some(name) => super::blocks::FrameName::Method(name.clone()),
                    None => super::blocks::FrameName::Block,
                };
                let ss = super::blocks::build_method_body(
                    fx,
                    id,
                    &params,
                    &body,
                    frame,
                    super::blocks::MethodBody::Def,
                )?;
                return Ok(Operand::Slot {
                    ss,
                    owned: true,
                    tag: TagInfo::Known(ValueTag::Proc as u8),
                });
            }
            let (ss, _) = super::blocks::build_lambda(fx, id, &params, &body)?;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Known(ValueTag::Proc as u8),
            })
        }
        // The `alias new old` KEYWORD (lowering's marker call): it installs
        // on the frame's DEFAULT DEFINEE, which is the cref's class unless
        // an `*_eval`/`instance_exec` re-homed the block -- so the runtime
        // decides, from the two candidates the emitter hands it.
        HirNode::Call {
            receiver: None,
            name,
            args,
            ..
        } if name == "__zeo_alias_keyword" && args.len() == 2 => {
            let [ArrayElem::Single(new_id), ArrayElem::Single(old_id)] = args.as_slice() else {
                return fx.unsupported(id, "a splatted `alias`");
            };
            let (new_id, old_id) = (*new_id, *old_id);
            let cref = super::consts::class_immediate(
                fx,
                fx.method_class.unwrap_or(crate::compiler::OBJECT_CLASS),
            );
            let cref_ptr = ownership::borrow_ptr(fx, &cref);
            if cref.owned() {
                ownership::pool_owned(fx, cref_ptr, cref.tag());
            }
            let self_ptr = super::ivars::dyn_ivar_recv(fx);
            let new_op = lower_expr(fx, new_id)?;
            let new_ptr = ownership::borrow_ptr(fx, &new_op);
            if new_op.owned() {
                ownership::pool_owned(fx, new_ptr, new_op.tag());
            }
            let old_op = lower_expr(fx, old_id)?;
            let old_ptr = ownership::borrow_ptr(fx, &old_op);
            if old_op.owned() {
                ownership::pool_owned(fx, old_ptr, old_op.tag());
            }
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            let status = fx.call_status(
                "zeo_rt_alias_in_default_definee",
                &[cref_ptr, self_ptr, new_ptr, old_ptr, out],
            );
            fx.fallible(status);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        // `alias $new $old` -- a bidirectional alias of the STORAGE, so a
        // write through either name is visible through both. Answers nil.
        HirNode::AliasGlobal(new_name, old_name) => {
            let (new_name, old_name) = (new_name.clone(), old_name.clone());
            let bx = fx.box_v();
            let (nptr, nlen) = rodata_name(fx, &new_name);
            let (optr, olen) = rodata_name(fx, &old_name);
            fx.call("zeo_rt_gvar_alias", &[bx, nptr, nlen, optr, olen]);
            Ok(Operand::Nil)
        }
        // A `require` that succeeded, at its own document position: record
        // the feature the way CRuby's `rb_provide_feature` does, before the
        // file it names runs. Answers nil -- the require's own `true`/`false`
        // was folded at its call site.
        HirNode::FeatureLoaded { entry, feature } => {
            let (entry, feature) = (entry.clone(), feature.clone());
            let bx = fx.box_v();
            let (eptr, elen) = rodata_name(fx, &entry);
            let (fptr, flen) = rodata_name(fx, feature.as_deref().unwrap_or(""));
            fx.call("zeo_rt_feature_loaded", &[bx, eptr, elen, fptr, flen]);
            Ok(Operand::Nil)
        }
        // A gem's compiled C extension, loaded at its own document position.
        // `Init_` runs here, so everything the extension defines becomes
        // answerable from this line and not from line 1.
        HirNode::CExtLoaded { library, init } => {
            let (library, init) = (library.clone(), init.clone());
            let (lptr, llen) = rodata_name(fx, &library);
            let (iptr, ilen) = rodata_name(fx, &init);
            let status = fx.call_status("zeo_rt_cext_load", &[lptr, llen, iptr, ilen]);
            // A dlopen failure and a raise from inside `Init_` both travel,
            // which is why this is fallible where `FeatureLoaded` is not.
            fx.fallible(status);
            Ok(Operand::Nil)
        }
        // A `class`/`module` written where a value is READ -- `x = class C;
        // 7; end`, or a `class << self` body ending a method. Ruby's value
        // is the body's last statement, which the site computes.
        HirNode::ClassDef { .. } if fx.eval_mode.is_some() => super::eval::eval_class_def(fx, id),
        HirNode::ClassDef { .. } => super::stmt::class_body_value(fx, id, true),
        HirNode::ClassRef(name) => {
            let name = name.clone();
            super::consts::const_read(fx, id, &name)
        }
        HirNode::QualifiedConstRead(scope, name) => {
            let (scope, name) = (scope.clone(), name.clone());
            super::consts::scoped_const_read(fx, id, &scope, &name)
        }
        // `X ||= v`'s read half: an absent constant -- or a scope the
        // compiler never registered -- is nil, so the write half can
        // define it.
        HirNode::ConstReadOrNil(scope, name) => {
            let (scope, name) = (scope.clone(), name.clone());
            // A run-time cref owns the constant a snippet writes bare, and
            // it is a class the fresh compiler has no entry for -- so the
            // id goes straight through, with no claim map to consult.
            let eval_owner = fx
                .eval_cref
                .as_ref()
                .and_then(|c| c.chain.first().copied())
                .filter(|_| scope.is_none());
            let owner_class = match scope.as_deref() {
                Some(s) => match super::boxes::resolve_class_here(fx, s) {
                    Some(cid) => cid,
                    None => return Ok(Operand::Nil),
                },
                None => fx.method_class.unwrap_or(crate::compiler::OBJECT_CLASS),
            };
            let owner = match eval_owner {
                Some(cid) => cid,
                None => {
                    fx.an
                        .compiler
                        .class_opt(owner_class)
                        .and_then(|c| c.const_owners.get(&name))
                        .copied()
                        .unwrap_or(owner_class)
                        .0
                }
            };
            let owner_v = fx.cid_value(owner);
            let (nptr, nlen) = rodata_name(fx, &name);
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            fx.call("zeo_rt_const_get_or_nil", &[owner_v, nptr, nlen, out]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        // `expr::NAME` / `expr::NAME = v`: the scope is a VALUE, so the
        // whole search happens at run time -- the scope operator's own,
        // which rejects a non-module scope with ruby's TypeError.
        HirNode::DynConstRead {
            scope,
            name,
            lenient,
        } => {
            let (scope, name, lenient) = (*scope, name.clone(), *lenient);
            let op = lower_expr(fx, scope)?;
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, op.tag());
            }
            let (nptr, nlen) = rodata_name(fx, &name);
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            let entry = if lenient {
                "zeo_rt_scope_const_get_or_nil"
            } else {
                "zeo_rt_scope_const_get"
            };
            let status = fx.call_status(entry, &[ptr, nptr, nlen, out]);
            fx.fallible(status);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::DynConstWrite { scope, name, value } => {
            let (scope, name, value) = (*scope, name.clone(), *value);
            // Ruby evaluates the scope FIRST and the value second, and
            // demands a module of the scope only once both are in hand
            // (`1::X = (puts 2; 3)` prints before it raises) -- unlike the
            // static form, which raises on an unresolvable scope untouched.
            let sop = lower_expr(fx, scope)?;
            let sptr = ownership::borrow_ptr(fx, &sop);
            if sop.owned() {
                ownership::pool_owned(fx, sptr, sop.tag());
            }
            let vop = lower_expr(fx, value)?;
            let vptr = ownership::borrow_ptr(fx, &vop);
            if vop.owned() {
                ownership::pool_owned(fx, vptr, vop.tag());
            }
            let (nptr, nlen) = rodata_name(fx, &name);
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            let status = fx.call_status("zeo_rt_scope_const_set", &[sptr, nptr, nlen, vptr, out]);
            fx.fallible(status);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
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
                id,
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
        HirNode::Yield(args) => {
            let args = args.clone();
            // A trailing hash the yield spelled as KEYWORDS (`yield(v, **h)`)
            // is dropped when `h` turns out empty, so its length is as much
            // a runtime question as a splat's.
            let kw_tail = args.last().and_then(|a| match a {
                ArrayElem::Single(n)
                    if fx
                        .an
                        .compiler
                        .hir
                        .has_flag(*n, crate::hir::NodeFlag::KWARGS_HASH) =>
                {
                    Some(*n)
                }
                ArrayElem::Single(_) | ArrayElem::Splat(_) => None,
            });
            // `yield(*a)`: the length is a runtime question, so the list is
            // built as an Array and the block binds from its contents.
            if kw_tail.is_some() || args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
                let fixed = &args[..args.len() - usize::from(kw_tail.is_some())];
                let arr = super::call::build_array(fx, fixed)?;
                let kw = match kw_tail {
                    Some(n) => {
                        let HirNode::HashLit(pairs) = &fx.an.compiler.hir[n] else {
                            unreachable!("a KWARGS_HASH node is always a HashLit")
                        };
                        let pairs = pairs.clone();
                        super::call::build_hash(fx, &pairs)?
                    }
                    None => fx.b.ins().iconst(fx.em.ptr, 0),
                };
                let blk = match fx.blk_ptr {
                    Some(b) => b,
                    None if fx.eval_mode.is_some() => {
                        // The enclosing method's channel, published for the
                        // snippet -- materialized as a value the shared
                        // entry can take.
                        let bss = fx.temp_slot();
                        let bp = fx.slot_addr(bss, 0);
                        fx.call("zeo_rt_eval_home_block", &[bp]);
                        fx.owned_created += 1;
                        ownership::pool_owned(fx, bp, TagInfo::Unknown);
                        bp
                    }
                    None => fx.b.ins().iconst(fx.em.ptr, 0),
                };
                let ss = fx.temp_slot();
                let out = fx.slot_addr(ss, 0);
                let status = fx.call_status("zeo_rt_yield_args", &[blk, arr, kw, out]);
                fx.fallible(status);
                fx.owned_created += 1;
                return Ok(Operand::Slot {
                    ss,
                    owned: true,
                    tag: TagInfo::Unknown,
                });
            }
            let argv_ptr = super::call::build_argv(fx, id, &args)?;
            let blk = match fx.blk_ptr {
                Some(b) => b,
                None => fx.b.ins().iconst(fx.em.ptr, 0),
            };
            let argc_v = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            // A snippet's OWN level has no block channel; the block it
            // means is the enclosing method's, which that method published.
            let status = if fx.blk_ptr.is_none() && fx.eval_mode.is_some() {
                fx.call_status("zeo_rt_eval_yield", &[argv_ptr, argc_v, out])
            } else {
                fx.call_status("zeo_rt_yield", &[blk, argv_ptr, argc_v, out])
            };
            fx.fallible(status);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::BlockGiven => match fx.blk_ptr {
            Some(b) => {
                let given =
                    fx.b.ins()
                        .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::NotEqual, b, 0);
                Ok(Operand::Bool(given))
            }
            // A snippet carries no block channel of its own; the one it
            // means is the enclosing method's, published for the call.
            None if fx.eval_mode.is_some() => {
                let given = fx.call_status("zeo_rt_eval_block_given", &[]);
                Ok(Operand::Bool(given))
            }
            None => Ok(Operand::Bool(fx.b.ins().iconst(types::I8, 0))),
        },
        HirNode::SelfRef => {
            let addr = fx.self_ptr.expect("self_ptr is set in the prologue");
            Ok(Operand::Ptr {
                addr,
                owned: false,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::IvarRead(name) => {
            let name = name.clone();
            super::ivars::ivar_read_op(fx, &name)
        }
        HirNode::Defined(v) => {
            let v = *v;
            super::defined::lower_defined(fx, id, v)
        }
        HirNode::NotNil(v) => {
            let v = *v;
            let op = lower_expr(fx, v)?;
            let tag = op.tag();
            if let TagInfo::Known(t) = tag {
                ownership::discard(fx, op);
                let known = fx.b.ins().iconst(types::I8, i64::from(t != 0));
                return Ok(Operand::Bool(known));
            }
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, tag);
            }
            let tv =
                fx.b.ins()
                    .load(types::I8, MemFlagsData::trusted(), ptr, TAG_OFFSET as i32);
            Ok(Operand::Bool(fx.b.ins().icmp_imm_u(IntCC::NotEqual, tv, 0)))
        }
        HirNode::LastMatchRef(which) => {
            use crate::hir::LastMatch;
            let (kind, n) = match which {
                LastMatch::Data => (0i64, 0usize),
                LastMatch::Group(n) => (1, *n),
                LastMatch::Pre => (2, 0),
                LastMatch::Post => (3, 0),
                LastMatch::LastGroup => (4, 0),
            };
            let kind_v = fx.b.ins().iconst(types::I8, kind);
            let n_v = fx.b.ins().iconst(fx.em.ptr, n as i64);
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            fx.call("zeo_rt_last_match_ref", &[kind_v, n_v, out]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::SuperCall {
            args,
            kwargs,
            zsuper,
            block,
            block_arg,
        } => {
            let (args, kwargs, zsuper, block, block_arg) =
                (args.clone(), kwargs.clone(), *zsuper, *block, *block_arg);
            super::call::lower_super(fx, id, &args, &kwargs, zsuper, block, block_arg)
        }
        // The four `__zeo_ffi_*` markers a deferred `ffi_lib`/`enum`
        // desugars to (see `lower::ffi`): they run where they stand, in
        // class-body order.
        // `"lit".freeze` on a LITERAL receiver IS the interned frozen twin --
        // CRuby compiles it to `opt_str_freeze`, so two of them are one
        // object. The rule is about the literal receiver and nothing else:
        // `s = +"x"; s.freeze` allocates and dedups nothing on either engine.
        //
        // Guarded on `freeze` not being reopened, exactly as CRuby guards
        // `opt_str_freeze` on its own redefinition flag. A program that
        // redefines `String#freeze` takes the ordinary dispatch.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: false,
        } if name == "freeze"
            && args.is_empty()
            && kwargs.is_empty()
            && block.is_none()
            && block_arg.is_none()
            && freeze_is_pristine(fx)
            && frozen_foldable_literal(fx, *recv).is_some() =>
        {
            let text = frozen_foldable_literal(fx, *recv).expect("checked in the guard");
            Ok(emit_pure_str_literal(fx, &text, true))
        }
        HirNode::Call {
            receiver: None,
            name,
            args,
            ..
        } if super::ffi::is_marker(name) => {
            let (name, args) = (name.clone(), args.clone());
            super::ffi::marker_call(fx, id, &name, &args)
        }
        // A box-scoped splice (`box.eval("..")`, a `box.require`d file's
        // statements, `box::X`): the statements run here, the last one is
        // the value, with the box overridden for the body -- the AOT
        // translation of CRuby's loading-box context.
        HirNode::BoxScope { box_id, body } => {
            let (box_id, body) = (*box_id, body.clone());
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            let was = std::mem::replace(&mut fx.box_id, box_id);
            let r = if fx.an.compiler.hir.has_flag(id, crate::hir::NodeFlag::LITERAL_BOX_EVAL) {
                super::stmt::literal_box_eval(fx, id, &body, dst)
            } else {
                super::stmt::lower_value_body_into(fx, &body, dst)
            };
            fx.box_id = was;
            r?;
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        // The handle VALUE `box = Ruby::Box.new` binds. It IS the box's
        // top-level surrogate class, which the emitter knows -- but the
        // ENV GATE is a run-time question (`RUBY_BOX=1` is read by the
        // program, not by the compile that built it), so the handle comes
        // from the run time, which raises CRuby's refusal when boxes are
        // off.
        HirNode::BoxHandle(box_id) => super::boxes::box_handle(fx, *box_id),
        HirNode::Ffi(call) => {
            let call = call.clone();
            super::ffi::lower_ffi_call(fx, id, &call)
        }
        HirNode::RegexpLit(parts, flags) => {
            let (parts, flags) = (parts.clone(), *flags);
            regexp_lit(fx, &parts, flags)
        }
        HirNode::Or(a, b) => {
            let (a, b) = (*a, *b);
            short_circuit(fx, a, b, true)
        }
        HirNode::And(a, b) => {
            let (a, b) = (*a, *b);
            short_circuit(fx, a, b, false)
        }
        HirNode::New {
            class_name,
            args,
            kwargs,
            block,
        } => {
            let (class_name, args, kwargs, block) =
                (class_name.clone(), args.clone(), kwargs.clone(), *block);
            // `Object.new`: a bare sentinel instance of the runtime root --
            // no registered constructor exists (Object's container holds
            // top-level defs as free functions), so the emitter folds
            // it to a fresh `Object`. A user-defined
            // `initialize` (top-level `def initialize`, or a `class Object`
            // reopen) still runs, through its VALUE-channel row.
            if let Some(cid) = super::boxes::resolve_class_here(fx, &class_name)
                && cid == crate::compiler::OBJECT_CLASS
                && block.is_none()
            {
                let ss = fx.temp_slot();
                let dst = fx.slot_addr(ss, 0);
                fx.call("zeo_rt_object_new_sentinel", &[dst]);
                fx.owned_created += 1;
                let recv = Operand::Slot {
                    ss,
                    owned: true,
                    tag: TagInfo::Known(zeo_abi::abi::ValueTag::Object as u8),
                };
                if fx.an.compiler.method_in_chain(cid, "initialize").is_none() {
                    if !args.is_empty() || !kwargs.is_empty() {
                        return fx.unsupported(id, "`Object.new` with arguments");
                    }
                    return Ok(recv);
                }
                // The sentinel is the RECEIVER of its own `initialize`; the
                // call's value is discarded and the object handed back.
                let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
                let p = ownership::borrow_ptr(fx, &recv);
                ownership::pool_owned(fx, p, recv.tag());
                let tag = recv.tag();
                let borrowed = || Operand::Ptr {
                    addr: p,
                    owned: false,
                    tag,
                };
                let init = if kwargs.is_empty() {
                    super::call::dynamic_send_value(
                        fx,
                        id,
                        borrowed(),
                        "initialize",
                        &elems,
                        false,
                    )?
                } else {
                    super::call::kw_send(
                        fx,
                        id,
                        super::call::Recv::at(borrowed()),
                        "initialize",
                        &elems,
                        &kwargs,
                        super::blocks::BlockChannel::None,
                    )?
                };
                ownership::discard(fx, init);
                return Ok(borrowed());
            }
            // A plain compiled class allocates DIRECTLY: a `Foo.new` that
            // walks the singleton chain looks for a `new` that is served by
            // the constructor rather than a class-method row, so the site's
            // cache could only ever remember the miss. The gate is
            // "statically constructed", term for term --
            // a generated-struct class (which is what `CompiledObject`'s
            // layout table holds), not runtime-conditional, with neither
            // `initialize` nor `new` patchable at run time, and no own
            // `new` on the singleton chain to override the constructor.
            // A `private_class_method :new` keeps the dynamic route, which
            // is what raises its NoMethodError, and so does a keyword
            // construction: the runtime's `new` entry takes the keyword
            // Hash on its own channel and marks it there, which this
            // positional-only entry has nowhere to put.
            if kwargs.is_empty()
                && let Some(cid) =
                    super::boxes::resolve_class_here(fx, &class_name).filter(|&cid| {
                        let c = &fx.an.compiler;
                        c.has_generated_struct(cid)
                            && !c.class(cid).runtime_conditional
                            && !c.may_be_patched_at_runtime("initialize")
                            && !c.may_be_patched_at_runtime("new")
                            && c.class_method_in_chain(cid, "new").is_none()
                            && !c.class_method_is_private(cid, "new")
                    })
            {
                // The fold below skips `const_read`, so the class's own
                // `autoload` would never run: `Foo.new` READS `Foo`, and a
                // read is what runs the target. The touch `class_value_of`
                // would have emitted goes here instead.
                super::consts::autoload_touch(fx, cid);
                let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
                let blk = match block {
                    Some(b) => super::blocks::BlockChannel::Literal(b),
                    None => super::blocks::BlockChannel::None,
                };
                return super::call::construct_compiled(fx, id, cid, &elems, blk);
            }
            // A statically-known class is a Class immediate; a constant
            // holding a RUNTIME class (`Struct.new`/`Data.define`) is read
            // at the call.
            let recv = super::consts::const_read(fx, id, &class_name)?;
            let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
            match (block, kwargs.is_empty()) {
                // `Foo.new(x) { .. }`: the literal block forwards to
                // `initialize`, so `yield`/`block_given?` inside it see it.
                (Some(blk), true) => super::blocks::block_send_op(fx, id, recv, "new", &elems, blk),
                (Some(blk), false) => super::call::kw_send(
                    fx,
                    id,
                    super::call::Recv::at(recv),
                    "new",
                    &elems,
                    &kwargs,
                    super::blocks::BlockChannel::Literal(blk),
                ),
                (None, true) => super::call::dynamic_send_value(fx, id, recv, "new", &elems, false),
                (None, false) => super::call::kw_send(
                    fx,
                    id,
                    super::call::Recv::at(recv),
                    "new",
                    &elems,
                    &kwargs,
                    super::blocks::BlockChannel::None,
                ),
            }
        }
        // The send shapes, in match order. The marker arms above
        // (`__zeo_alias_keyword`, the pristine `"lit".freeze` fold, the
        // FFI markers) must win first; these four are then disjoint by
        // (safe, block, block_arg, kwargs), and each body is the named
        // fn the arm forwards to.
        //
        // `recv&.m(args) { blk }`: the receiver is evaluated ONCE and a nil
        // one answers nil without evaluating the arguments or building the
        // block -- oracle-verified (`nil&.push(*a, f())` never calls `f`),
        // which is why the whole argument build sits in the call branch.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: true,
        } => {
            let (recv, name, args, kwargs, block, block_arg) = (
                *recv,
                name.clone(),
                args.clone(),
                kwargs.clone(),
                *block,
                *block_arg,
            );
            safe_nav_call(fx, id, recv, name, args, kwargs, block, block_arg)
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
            let (receiver, name, args, blk) = (
                self_receiver(fx, *receiver),
                name.clone(),
                args.clone(),
                *blk,
            );
            literal_block_call(fx, id, receiver, name, args, blk)
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block: None,
            block_arg: None,
            safe: false,
        } if kwargs.is_empty() => {
            let (receiver, name, args) = (self_receiver(fx, *receiver), name.clone(), args.clone());
            plain_call(fx, id, receiver, name, args)
        }
        // A `&expr` block argument (no literal block, no keywords).
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block: None,
            block_arg: Some(ba),
            safe: false,
        } if kwargs.is_empty() => {
            let (receiver, name, args, ba) = (
                self_receiver(fx, *receiver),
                name.clone(),
                args.clone(),
                *ba,
            );
            super::blocks::block_arg_send(fx, id, receiver, &name, &args, ba)
        }
        // Call-site keywords: the kw/splat send entries append the marked
        // hash per the trailing-kwargs convention. A block rides the same
        // channel it does on a keyword-free call -- the receiver still
        // evaluates first, then the arguments, then the block.
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: false,
        } => {
            let (receiver, name, args, kwargs, block, block_arg) = (
                self_receiver(fx, *receiver),
                name.clone(),
                args.clone(),
                kwargs.clone(),
                *block,
                *block_arg,
            );
            keyword_call(fx, id, receiver, name, args, kwargs, block, block_arg)
        }
        HirNode::RangeLit {
            start,
            end,
            exclusive,
        } => {
            let (start, end, exclusive) = (*start, *end, *exclusive);
            // Endpoints are evaluated in order, parked BORROWED (owned
            // temps pooled -- the second endpoint's evaluation can raise),
            // then handed to `range_new` as MOVED copies (`move_ptr`
            // retains a borrowed source; nothing fallible runs between the
            // copies and the call). A nil endpoint folds into the absent
            // one inside the runtime (`nil..5` IS `..5`), and the
            // construction runs CRuby's `begin <=> end` comparability
            // check, so the call is fallible.
            let park = |fx: &mut Fx, n: Option<NodeId>| -> CResult<Option<Operand>> {
                let Some(n) = n else { return Ok(None) };
                let op = lower_expr(fx, n)?;
                let tag = op.tag();
                let ptr = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, ptr, tag);
                }
                Ok(Some(Operand::Ptr {
                    addr: ptr,
                    owned: false,
                    tag,
                }))
            };
            let s_op = park(fx, start)?;
            let e_op = park(fx, end)?;
            let null = fx.b.ins().iconst(fx.em.ptr, 0);
            let s_ptr = match &s_op {
                Some(op) => ownership::move_ptr(fx, op),
                None => null,
            };
            let e_ptr = match &e_op {
                Some(op) => ownership::move_ptr(fx, op),
                None => null,
            };
            let excl = fx.b.ins().iconst(types::I8, i64::from(exclusive));
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            let status = fx.call_status("zeo_rt_range_new", &[s_ptr, e_ptr, excl, out]);
            fx.fallible(status);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Known(ValueTag::Range as u8),
            })
        }
        // `$!` -- the exception being handled (the bare-`raise` slot), nil
        // outside any rescue. NOT the `$foo` table.
        HirNode::GlobalRead(name) if name == "$!" => {
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            fx.call("zeo_rt_gvar_err_info", &[out]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        // `$?` -- the last child's wait status slot, nil until a child ran.
        HirNode::GlobalRead(name) if name == "$?" => {
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            fx.call("zeo_rt_gvar_child_status", &[out]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::GlobalRead(name) => {
            let name = name.clone();
            let (nptr, nlen) = rodata_name(fx, &name);
            // Globals are per-box tables; everything the CLIF backend
            // compiles today is the main program (box scopes refuse).
            let bx = fx.box_v();
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            fx.call("zeo_rt_gvar_get", &[bx, nptr, nlen, out]);
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        // Assignment answers the assigned value (the table takes a clone,
        // so the operand keeps its own ownership). Fallible: read-only
        // globals (`$0 = 1` is fine, `$FILENAME = ..` is not) raise.
        HirNode::GlobalWrite(name, value) => {
            let (name, value) = (name.clone(), *value);
            let op = lower_expr(fx, value)?;
            let tag = op.tag();
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, tag);
            }
            let (nptr, nlen) = rodata_name(fx, &name);
            let bx = fx.box_v();
            let status = fx.call_status("zeo_rt_gvar_assign", &[bx, nptr, nlen, ptr]);
            fx.fallible(status);
            Ok(Operand::Ptr {
                addr: ptr,
                owned: false,
                tag,
            })
        }
        HirNode::ClassVarRead(name) => {
            let name = name.clone();
            let owner = super::ivars::cvar_owner(fx, &name);
            let owner_v = fx.cid_value(owner);
            let (nptr, nlen) = rodata_name(fx, &name);
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            // The `@@x ||= v` read half tolerates an unassigned cvar (nil);
            // every other read is ruby's NameError.
            if fx
                .an
                .compiler
                .hir
                .has_flag(id, crate::hir::NodeFlag::LENIENT_CVAR_READ)
            {
                fx.call("zeo_rt_cvar_get", &[owner_v, nptr, nlen, out]);
            } else {
                let status = fx.call_status("zeo_rt_cvar_get_checked", &[owner_v, nptr, nlen, out]);
                fx.fallible(status);
            }
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        // `@name = v` in VALUE position answers the value ASSIGNED, not a
        // read-back. The park-then-write shape is the cvar/const twin --
        // the write's frozen check can raise, so an owned rhs is pooled
        // first and the store takes a moved COPY.
        HirNode::IvarWrite(name, value) => {
            let (name, value) = (name.clone(), *value);
            let op = lower_expr(fx, value)?;
            let tag = op.tag();
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, tag);
            }
            let borrowed = Operand::Ptr {
                addr: ptr,
                owned: false,
                tag,
            };
            super::ivars::ivar_write_op(fx, &name, borrowed)?;
            Ok(Operand::Ptr {
                addr: ptr,
                owned: false,
                tag,
            })
        }
        HirNode::ClassVarWrite(name, value) => {
            let (name, value) = (name.clone(), *value);
            let owner = super::ivars::cvar_owner(fx, &name);
            let op = lower_expr(fx, value)?;
            let tag = op.tag();
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, tag);
            }
            let owner_v = fx.cid_value(owner);
            let (nptr, nlen) = rodata_name(fx, &name);
            let status = fx.call_status("zeo_rt_cvar_set", &[owner_v, nptr, nlen, ptr]);
            fx.fallible(status);
            Ok(Operand::Ptr {
                addr: ptr,
                owned: false,
                tag,
            })
        }
        HirNode::ConstWrite { scope, name, value } => {
            let (scope, name, value) = (scope.clone(), name.clone(), *value);
            // As in the `||=` read half above: a run-time cref owns what a
            // snippet writes bare, and the fresh compiler has no entry for
            // it, so the id goes straight through.
            let eval_owner = fx
                .eval_cref
                .as_ref()
                .and_then(|c| c.chain.first().copied())
                .filter(|_| scope.is_none());
            // An explicit `Scope::NAME = ..` whose scope isn't a registered
            // class needs a runtime-scope path -- not lowered yet.
            let owner_class = match scope.as_deref() {
                Some(s) => match super::boxes::resolve_class_here(fx, s) {
                    Some(cid) => cid,
                    // A scope no compile-time class backs may still be a
                    // RUNTIME constant holding one (`class SK::Enc` where
                    // `SK` came from `class SK < DelegateClass(Hash)`), so
                    // the path resolves at run time. Ruby reads the scope
                    // BEFORE the value (`Nope::X = (puts 1; 5)` prints
                    // nothing -- oracle-verified), which is the order the
                    // read below already gives.
                    None => {
                        return super::consts::runtime_scope_const_write(fx, id, s, &name, value);
                    }
                },
                // A bare `NAME =` is owned by the lexically enclosing
                // class/module (the emitting context); the box's top level
                // otherwise.
                None => fx.method_class.unwrap_or_else(|| super::boxes::box_top(fx)),
            };
            let owner = match eval_owner {
                Some(cid) => cid,
                None => {
                    fx.an
                        .compiler
                        .class_opt(owner_class)
                        .and_then(|c| c.const_owners.get(&name))
                        .copied()
                        .unwrap_or(owner_class)
                        .0
                }
            };
            let Some((file, line)) = crate::analyze::source::source_location(&fx.an.compiler, id)
            else {
                return fx.unsupported(id, "a span-less constant write");
            };
            let op = lower_expr(fx, value)?;
            let tag = op.tag();
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, tag);
            }
            let owner_v = fx.cid_value(owner);
            let (nptr, nlen) = rodata_name(fx, &name);
            let (fptr, flen) = rodata_name(fx, file);
            let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
            // The box the assignment is WRITTEN in: a box's constant lands in
            // its own record, so main keeps the pristine one.
            let fxbox = fx.box_v();
            fx.call(
                "zeo_rt_const_set_at",
                &[owner_v, nptr, nlen, ptr, fptr, flen, line_v, fxbox],
            );
            // Ruby announces the constant AFTER the write, so the hook body
            // can already read it -- and on every assignment, re-assignment
            // included.
            super::consts::const_added_send(fx, owner, &name, Some(id))?;
            Ok(Operand::Ptr {
                addr: ptr,
                owned: false,
                tag,
            })
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            let (subject, arms, else_body) = (*subject, arms.clone(), else_body.clone());
            case_when(fx, subject, &arms, &else_body)
        }
        HirNode::CaseIn {
            subject,
            arms,
            else_body,
        } => {
            let (subject, arms, else_body) = (*subject, arms.clone(), else_body.clone());
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            super::patterns::lower_case_in(fx, id, subject, &arms, &else_body, dst)?;
            fx.owned_created += 1;
            Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            })
        }
        HirNode::MatchPredicate { subject, pattern } => {
            let (subject, pattern) = (*subject, pattern.clone());
            super::patterns::lower_match_predicate(fx, id, subject, &pattern)
        }
        HirNode::MatchRequired { subject, pattern } => {
            let (subject, pattern) = (*subject, pattern.clone());
            super::patterns::lower_match_required(fx, id, subject, &pattern)
        }
        other => {
            let what = format!("this expression ({})", node_kind(other));
            fx.unsupported(id, &what)
        }
    }
}

/// A short human label for refusal messages.
pub(crate) fn node_kind(node: &HirNode) -> String {
    // One `match` would be 80 arms of labels nothing else needs; the
    // refusal text only has to orient, not classify -- so a kind without a
    // hand-written phrase names its HIR VARIANT, which is what a triage
    // histogram over the corpus reads. The phrase arms only fire where the
    // node actually gets refused (an `if` never reaches a statement
    // refusal, so its value-position phrase cannot mislead there).
    match node {
        HirNode::Call { .. } => "a method call".to_string(),
        HirNode::If { .. } => "an `if` in value position".to_string(),
        HirNode::While { .. } => "a loop in value position".to_string(),
        HirNode::ClassDef { .. } => "a class definition".to_string(),
        HirNode::DefMethod { .. } => "a method definition".to_string(),
        HirNode::Begin { .. } => "a begin/rescue/ensure".to_string(),
        other => format!("the node kind `{}`", variant_name(other)),
    }
}

/// The bare variant name of a node -- `Debug`'s leading identifier,
/// captured WITHOUT formatting the node's whole subtree: the writer
/// stops the walk at the first delimiter, so a deep node costs nothing.
pub(crate) fn variant_name(node: &HirNode) -> String {
    use std::fmt::Write as _;
    struct Prefix(String);
    impl std::fmt::Write for Prefix {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            match s.find(['(', ' ', '{']) {
                Some(i) => {
                    self.0.push_str(&s[..i]);
                    Err(std::fmt::Error) // the name is complete; stop the walk
                }
                None => {
                    self.0.push_str(s);
                    Ok(())
                }
            }
        }
    }
    let mut w = Prefix(String::new());
    let _ = write!(w, "{node:?}");
    if w.0.is_empty() {
        "Unknown".to_string()
    } else {
        w.0
    }
}
