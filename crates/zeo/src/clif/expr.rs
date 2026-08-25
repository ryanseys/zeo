//! Expression lowering to Cranelift IR: literals, local reads, calls, and
//! the numeric binary operators with their three-arm shape -- inline Int,
//! inline Float, dynamic `send_value_in` fallback.

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::codegen_error::CResult;
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};
use zeo_abi::abi::{PAYLOAD_OFFSET, TAG_OFFSET, ValueTag};

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
        // compile-time bigint dependency, no string parsing) -- rustc's
        // shape. `int_from_u32_digits` demotes to `Int` when it fits, so
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
            // with that encoding, byte-built -- and, as rustc's does, skips
            // the frozen pool. Otherwise a raw-byte segment forces the byte
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
            let state = state + fx.flip_flop_base;
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
        // A diverging expression in value position (`a = (raise "x")`):
        // the signal leaves the block unreachable; the nil is never read.
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
        // method-name Symbol (rustc's `emit_expr` DefMethod arm).
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
                // parameter list a `def` actually wrote (rustc marks
                // `runtime_super_params` here and leaves
                // `defined_by_define_method` alone).
                let ss = super::blocks::build_method_body(
                    fx,
                    id,
                    &params,
                    &body,
                    super::blocks::FrameName::Block,
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
        // decides, from the same two candidates rustc hands it.
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
        // define it (rustc's `const_owner_id_opt` miss arm).
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
            let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner));
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
            // built as an Array and the block binds from its contents
            // (rustc's `__args` vector twin).
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
            let r = super::stmt::lower_value_body_into(fx, &body, dst);
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
            // A plain compiled class allocates DIRECTLY: `Foo.new` walked
            // the singleton chain looking for a `new` that is served by the
            // constructor rather than a class-method row, so the site's
            // cache could only ever remember the miss. The gate is the
            // rustc emitter's `statically_constructed`, term for term --
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
                let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
                let blk = match block {
                    Some(b) => super::blocks::BlockChannel::Literal(b),
                    None => super::blocks::BlockChannel::None,
                };
                return super::call::construct_compiled(fx, id, cid, &elems, blk);
            }
            // A statically-known class is a Class immediate; a constant
            // holding a RUNTIME class (`Struct.new`/`Data.define`) is read
            // at the call, exactly the rustc `__rtclass` shape.
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
            let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner));
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
        // read-back: rustc binds the rhs once (`let __v = ..`) and hands
        // the write a clone. The park-then-write shape is the cvar/const
        // twin -- the write's frozen check can raise, so an owned rhs is
        // pooled first and the store takes a moved COPY.
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
            let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner));
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
            // class takes rustc's runtime-scope path -- not lowered yet.
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
                // otherwise -- rustc's `const_owner_id_opt` fallback.
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
            let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner));
            let (nptr, nlen) = rodata_name(fx, &name);
            let (fptr, flen) = rodata_name(fx, file);
            let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
            fx.call(
                "zeo_rt_const_set_at",
                &[owner_v, nptr, nlen, ptr, fptr, flen, line_v],
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
            case_when(fx, id, subject, &arms, &else_body)
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

/// `recv&.name(...)`: the nil-test diamond. A nil receiver answers nil
/// without evaluating the arguments or building the block (ruby's rule,
/// oracle-verified) -- so the whole argument build sits in the call arm.
#[allow(clippy::too_many_arguments)]
fn safe_nav_call(
    fx: &mut Fx,
    id: NodeId,
    recv: NodeId,
    name: String,
    args: Vec<ArrayElem>,
    kwargs: Vec<crate::hir::KwArg>,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> CResult<Operand> {
    let op = lower_expr(fx, recv)?;
    let tag = op.tag();
    let ptr = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        ownership::pool_owned(fx, ptr, tag);
    }
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let b_nil = fx.b.create_block();
    let b_call = fx.b.create_block();
    let join = fx.b.create_block();
    let tv =
        fx.b.ins()
            .load(types::I8, MemFlagsData::trusted(), ptr, TAG_OFFSET as i32);
    let is_nil = fx.b.ins().icmp_imm_u(IntCC::Equal, tv, 0);
    fx.b.ins().brif(is_nil, b_nil, &[], b_call, &[]);
    fx.b.switch_to_block(b_nil);
    ownership::write_move_into(fx, &Operand::Nil, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(b_call);
    let recv_op = Operand::Ptr {
        addr: ptr,
        owned: false,
        tag,
    };
    let blk = block_channel(fx, id, block, block_arg)?;
    // `self&.x` reaches a private `x` exactly as `self.x` does: the
    // safe part is the nil test, and it changes no visibility rule.
    let through = self_receiver(fx, Some(recv)).map(|_| recv_op);
    let bypass = bypasses_visibility(fx, Some(recv));
    let res = if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
        super::call::splat_send(
            fx,
            id,
            super::call::Recv::maybe(through, bypass),
            &name,
            &args,
            &kwargs,
            blk,
        )?
    } else if !kwargs.is_empty() {
        super::call::kw_send(
            fx,
            id,
            super::call::Recv::maybe(through, bypass),
            &name,
            &args,
            &kwargs,
            blk,
        )?
    } else if blk.is_open() {
        super::blocks::send_with_block_ptr_ops(fx, id, through, &name, &args, blk, bypass)?
    } else if let Some(op) = through {
        super::call::dynamic_send_value(fx, id, op, &name, &args, bypass)?
    } else {
        super::call::implicit_send(fx, id, &name, &args)?
    };
    ownership::write_move_into(fx, &res, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A literal-block send (`x.each { .. }`): the fused-iterator shapes
/// first, then the direct call for a compiled receiverless name, then
/// the block-passing dynamic sends.
fn literal_block_call(
    fx: &mut Fx,
    id: NodeId,
    receiver: Option<NodeId>,
    name: String,
    args: Vec<ArrayElem>,
    blk: NodeId,
) -> CResult<Operand> {
    // A typed-receiver `arr.each` (`Compiler::inline_iter_sites`):
    // fused under a runtime guard, with the ordinary block send on
    // the other arm. The literal shapes below never nominate --
    // their receivers are not locals -- so the order is free.
    if args.is_empty()
        && super::iter::fusable_block(fx, blk)
        && let Some(r) = receiver
        && fx.an.compiler.inline_iter_sites.get(&blk)
            == Some(&crate::compiler::InlineIterKind::ArrayEach)
    {
        return Ok(super::iter::lower_array_each(fx, id, r, blk, true)?
            .expect("a wanted result is always built"));
    }
    if args.is_empty()
        && super::iter::fusable_block(fx, blk)
        && let Some(r) = receiver
        && fx.an.compiler.inline_iter_sites.get(&blk)
            == Some(&crate::compiler::InlineIterKind::TimesInt)
    {
        return Ok(super::iter::lower_counted_int(fx, id, r, blk, true)?
            .expect("a wanted result is always built"));
    }
    if args.is_empty()
        && super::iter::fusable_block(fx, blk)
        && let Some(counted) = super::iter::counted_of(fx, receiver, &name, true)
    {
        let ss = fx.temp_slot();
        let dst = fx.slot_addr(ss, 0);
        super::iter::lower_counted(fx, id, &counted, blk, Some(dst))?;
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        });
    }
    // A receiverless block call naming a compiled method goes
    // direct; everything else is a block-passing dynamic send.
    if receiver.is_none()
        && let Some(decl) = fx.em.methods.get(&name)
        && decl.plain
        && decl.arity == args.len()
        && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
        && !method_class_shadows(fx, &name)
    {
        return super::call::direct_call(fx, id, &name, &args, Some(blk));
    }
    // A splatted argument list builds its Array in the runtime, so
    // it takes the args entry with the block on the same channel.
    if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
        let recv = match receiver {
            Some(r) => Some(lower_expr(fx, r)?),
            None => None,
        };
        let bypass = bypasses_visibility(fx, receiver);
        return super::call::splat_send(
            fx,
            id,
            super::call::Recv::maybe(recv, bypass),
            &name,
            &args,
            &[],
            super::blocks::BlockChannel::Literal(blk),
        );
    }
    super::blocks::block_send(fx, id, receiver, &name, &args, blk)
}

/// A plain send (no block, no keywords): the compile-time folds
/// (`Module.nesting`, `Ruby::Box.current`, `__method__`, method
/// captures, `binding`, `eval`), then the splat entry, the operator
/// fast path, the direct call, and the dynamic sends.
fn plain_call(
    fx: &mut Fx,
    id: NodeId,
    receiver: Option<NodeId>,
    name: String,
    args: Vec<ArrayElem>,
) -> CResult<Operand> {
    if let Some(op) = super::boxes::module_nesting(fx, receiver, &name, &args)? {
        return Ok(op);
    }
    if let Some(op) = super::boxes::box_current(fx, receiver, &name, &args)? {
        return Ok(op);
    }
    // `__method__`/`__callee__` under an ALIAS: `__method__` is the
    // name the body was DEFINED under, `__callee__` the name it was
    // reached through, and the runtime row -- which reads the frame
    // -- can only ever see the latter. Folded only where the
    // emitter knows the enclosing method; everywhere else (a
    // top-level scope, a body installed at run time) the row's
    // frame read is the better answer and this falls through.
    if receiver.is_none()
        && args.is_empty()
        && name == "__method__"
        && let Some(origin) = fx.method_origin.clone()
    {
        return super::consts::symbol_value(fx, &origin);
    }
    if let Some(op) = method_capture_intrinsic(fx, receiver, &name, &args, &[], None, None)? {
        return Ok(op);
    }
    if receiver.is_none()
        && name == "binding"
        && args.is_empty()
        && let Some(op) = binding_value(fx, id)?
    {
        return Ok(op);
    }
    if let Some(op) = runtime_eval(fx, id, receiver, &name, &args)? {
        return Ok(op);
    }
    if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
        let later = later_nodes(&args, &[], None);
        let recv = match receiver {
            Some(r) => {
                let op = lower_expr(fx, r)?;
                Some(park_reassignable(fx, Some(r), op, &later))
            }
            None => None,
        };
        let bypass = bypasses_visibility(fx, receiver);
        return super::call::splat_send(
            fx,
            id,
            super::call::Recv::maybe(recv, bypass),
            &name,
            &args,
            &[],
            super::blocks::BlockChannel::None,
        );
    }
    match receiver {
        Some(recv) if super::binop::operator_fast_path(fx, &name) && args.len() == 1 => {
            let [ArrayElem::Single(arg)] = args.as_slice() else {
                return fx.unsupported(id, "a splat operand");
            };
            super::binop::binop(fx, id, &name, recv, *arg)
        }
        Some(recv) if name == "nil?" && args.is_empty() => {
            match nil_p_call(fx, id, recv)? {
                Some(fold) => Ok(fold),
                None => super::call::dynamic_send(fx, id, recv, &name, &args),
            }
        }
        Some(recv)
            if let Some(site) = fx.an.compiler.accessor_sites.get(&id).copied() =>
        {
            super::boxes::explicit_accessor(fx, id, recv, &name, &args, site)
        }
        Some(recv) => match super::call::indexed_send(fx, id, recv, &name, &args)? {
            Some(fast) => Ok(fast),
            None => super::call::dynamic_send(fx, id, recv, &name, &args),
        },
        None if let Some(folded) =
            super::boxes::inline_accessor(fx, id, &name, &args, &[], None, None) =>
        {
            folded
        }
        None => match fx.em.methods.get(&name) {
            Some(decl)
                if decl.plain
                    && decl.arity == args.len()
                    && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
                    && !method_class_shadows(fx, &name) =>
            {
                super::call::direct_call(fx, id, &name, &args, None)
            }
            // Unknown names and arity mismatches go through the
            // implicit-self dynamic send (the runtime raises the
            // NoMethodError/ArgumentError, exactly where rustc's
            // fallback does).
            Some(_) | None => super::call::implicit_send(fx, id, &name, &args),
        },
    }
}

/// `x.nil?` folded to a receiver tag test, when the answer is provably
/// the builtin's: NilClass's `nil?` is untouched by the program's text
/// and no runtime-definition machinery could touch it. Two tiers. When
/// NO scope anywhere defines a `nil?` (the `defines_bang` shape), the
/// tag test IS the answer -- `NilClass#nil?` is true and `Kernel#nil?`
/// is false, the only reachable bodies. When some class defines its own
/// (the null-object pattern), only the nil receiver folds and every
/// other receiver dispatches -- which also keeps NilClass off that
/// site's cache. `None` = no fold, ordinary send.
fn nil_p_call(fx: &mut Fx, id: NodeId, recv: NodeId) -> CResult<Option<Operand>> {
    let compiler = &fx.an.compiler;
    if compiler.may_be_patched_at_runtime("nil?")
        || compiler
            .method_in_chain(zeo_abi::NIL_CLASS, "nil?")
            .is_some()
    {
        return Ok(None);
    }
    // A blank-slate (BasicObject-rooted) receiver must raise
    // NoMethodError -- `nil?` is Kernel's -- and a Ractor-moved husk must
    // raise its moved error from dispatch, so a static false is only
    // sound while neither receiver can exist.
    let total = !compiler.blank_slate_possible()
        && !compiler.moved_receiver_possible()
        && !compiler.scopes.iter().any(|s| s.name == "nil?");
    let bypass = bypasses_visibility(fx, Some(recv));
    let op = lower_expr(fx, recv)?;
    if let TagInfo::Known(t) = op.tag() {
        let is_nil = t == ValueTag::Nil as u8;
        // A known nil answers under the NilClass gate alone; a known
        // OTHER tag answers false only when no scope anywhere defines a
        // `nil?` (a reopened `Integer#nil?` must still dispatch).
        if is_nil || total {
            // The compile-time answer; the receiver was still evaluated.
            ownership::discard(fx, op);
            let bit = fx.b.ins().iconst(types::I8, i64::from(is_nil));
            return Ok(Some(Operand::Bool(bit)));
        }
    }
    let pa = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        ownership::pool_owned(fx, pa, op.tag());
    }
    let fl = MemFlagsData::trusted();
    let tag = fx.b.ins().load(types::I8, fl, pa, TAG_OFFSET as i32);
    let is_nil =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, tag, i64::from(ValueTag::Nil as u8));
    if total {
        return Ok(Some(Operand::Bool(is_nil)));
    }
    let fast = fx.b.create_block();
    let slow = fx.b.create_block();
    let join = fx.b.create_block();
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    fx.b.ins().brif(is_nil, fast, &[], slow, &[]);

    fx.b.switch_to_block(fast);
    let btag =
        fx.b.ins()
            .iconst(types::I8, i64::from(ValueTag::Bool as u8));
    fx.b.ins().store(fl, btag, dst, TAG_OFFSET as i32);
    let one = fx.b.ins().iconst(types::I8, 1);
    fx.b.ins().store(fl, one, dst, PAYLOAD_OFFSET as i32);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(slow);
    let borrowed = Operand::Ptr {
        addr: pa,
        owned: false,
        tag: TagInfo::Unknown,
    };
    let r = super::call::dynamic_send_value(fx, id, borrowed, "nil?", &[], bypass)?;
    ownership::write_move_into(fx, &r, dst);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// A keyword-carrying send: the all-required direct-fill shape first,
/// then the kw/splat entries with the block on its usual channel.
#[allow(clippy::too_many_arguments)]
fn keyword_call(
    fx: &mut Fx,
    id: NodeId,
    receiver: Option<NodeId>,
    name: String,
    args: Vec<ArrayElem>,
    kwargs: Vec<crate::hir::KwArg>,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> CResult<Operand> {
    if let Some(op) =
        method_capture_intrinsic(fx, receiver, &name, &args, &kwargs, block, block_arg)?
    {
        return Ok(op);
    }
    // A receiverless keyword call naming a compiled method whose
    // keywords are ALL required, covered exactly by literal keys,
    // fills the slots itself: no Hash, no dynamic send, no binder.
    // The deleted rustc backend routed this shape statically all
    // along; CLIF once sent every keyword call the long way round.
    if receiver.is_none()
        && block_arg.is_none()
        && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
        && !method_class_shadows(fx, &name)
        && fx
            .em
            .methods
            .get(&name)
            .is_some_and(|d| d.arity == args.len())
        && let Some(order) = super::call::kw_direct_order(fx, &name, &kwargs)
    {
        return super::call::direct_call_kw(fx, id, &name, &args, Some((&kwargs, &order)), block);
    }
    let later = later_nodes(&args, &kwargs, block_arg);
    let recv = match receiver {
        Some(r) => {
            let op = lower_expr(fx, r)?;
            Some(park_reassignable(fx, Some(r), op, &later))
        }
        None => None,
    };
    let blk = block_channel(fx, id, block, block_arg)?;
    let bypass = bypasses_visibility(fx, receiver);
    if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
        super::call::splat_send(
            fx,
            id,
            super::call::Recv::maybe(recv, bypass),
            &name,
            &args,
            &kwargs,
            blk,
        )
    } else {
        super::call::kw_send(
            fx,
            id,
            super::call::Recv::maybe(recv, bypass),
            &name,
            &args,
            &kwargs,
            blk,
        )
    }
}

/// Lower `cond` for a BRANCH: the result is an i8 truthiness value. A
/// comparison the operator fast path owns answers its condition bit
/// directly (no boxed Bool, no `zeo_rt_truthy` call); everything else
/// lowers normally and reduces through `ownership::truthy`. The flag is
/// keyed by node id and consumed only by the binop arm, so routing stays
/// in `lower_expr` and cannot drift.
pub(super) fn lower_condition(
    fx: &mut Fx,
    cond: NodeId,
) -> CResult<cranelift_codegen::ir::Value> {
    let saved = fx.branch_cond.replace(cond);
    let r = lower_expr(fx, cond);
    fx.branch_cond = saved;
    Ok(ownership::truthy(fx, r?))
}

/// `if` in VALUE position: both arms move their value into one result
/// slot.
fn if_expr(
    fx: &mut Fx,
    cond: NodeId,
    then_body: &[NodeId],
    else_body: &[NodeId],
) -> CResult<Operand> {
    let t = lower_condition(fx, cond)?;
    let ss = fx.temp_slot();
    // The result address is computed BEFORE the branch, so it dominates
    // both arms.
    let dst = fx.slot_addr(ss, 0);
    let b_then = fx.b.create_block();
    let b_else = fx.b.create_block();
    let join = fx.b.create_block();
    fx.b.ins().brif(t, b_then, &[], b_else, &[]);
    fx.b.switch_to_block(b_then);
    super::stmt::lower_value_body_into(fx, then_body, dst)?;
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(b_else);
    super::stmt::lower_value_body_into(fx, else_body, dst)?;
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The block channel a call opens, if any: a literal block builds its
/// proc, a `&expr` argument converts through `to_proc` (nil = no block).
/// Ruby's grammar admits only one of the two.
fn block_channel(
    fx: &mut Fx,
    id: NodeId,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> CResult<super::blocks::BlockChannel> {
    use super::blocks::BlockChannel;
    match (block, block_arg) {
        (None, None) => Ok(BlockChannel::None),
        // The literal stays UNBUILT; the `&expr` conversion cannot, since
        // it runs ruby code of its own and belongs in written order.
        (Some(blk), None) => Ok(BlockChannel::Literal(blk)),
        (None, Some(ba)) => Ok(BlockChannel::Ready(super::blocks::block_arg_ptr(fx, ba)?)),
        (Some(_), Some(_)) => fx.unsupported(id, "a literal block beside a `&` block argument"),
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

/// A baked `u32` array in rodata, 4-aligned (the runtime reads it as
/// `&[u32]`) -- a numeric literal's digits.
fn u32_array(
    fx: &mut Fx,
    digits: &[u32],
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    let bytes: Vec<u8> = digits.iter().flat_map(|d| d.to_ne_bytes()).collect();
    let off = fx.em.intern_rodata_aligned(&bytes, 4);
    let ptr = fx.rod(off);
    let n = fx.b.ins().iconst(fx.em.ptr, digits.len() as i64);
    (ptr, n)
}

/// The `EncodingId` a `# encoding:` magic comment puts on every literal in
/// the program, if one is set. Lowering already normalized the comment to
/// one of the four names zeo supports (UTF-8 is `None` -- the default).
fn script_encoding_id(fx: &Fx) -> Option<i64> {
    let name = fx.an.compiler.hir.script_encoding.as_deref()?;
    let id = match name {
        "US_ASCII" => zeo_rt::encoding::US_ASCII,
        "ASCII_8BIT" => zeo_rt::encoding::ASCII_8BIT,
        "ISO_8859_1" => zeo_rt::encoding::ISO_8859_1,
        other => unreachable!("lower::literals normalizes the magic comment; got `{other}`"),
    };
    Some(i64::from(id.0))
}

/// The pure (single non-interpolated UTF-8 part) text of a string literal.
pub(crate) fn pure_literal(parts: &[StrPart]) -> Option<String> {
    match parts {
        [] => Some(String::new()),
        [StrPart::Lit(s)] => Some(s.clone()),
        [StrPart::Bytes(_) | StrPart::Interp(_)] | [_, _, ..] => None,
    }
}

/// `K.method(:name)` captured BEFORE every own `def self.name` in the
/// document: CRuby resolves at capture time, so the Method binds the
/// INHERITED entry and a by-name capture would recurse forever through
/// the later override (rspec-support's `NEW_MUTEX_METHOD =
/// Mutex.method(:new)` / `def self.new = NEW_MUTEX_METHOD.call` pair).
#[allow(clippy::too_many_arguments)]
fn method_capture_intrinsic(
    fx: &mut Fx,
    receiver: Option<NodeId>,
    name: &str,
    args: &[crate::hir::ArrayElem],
    kwargs: &[crate::hir::KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> CResult<Option<Operand>> {
    if name != "method"
        || args.len() != 1
        || !kwargs.is_empty()
        || block.is_some()
        || block_arg.is_some()
    {
        return Ok(None);
    }
    let (Some(recv_id), crate::hir::ArrayElem::Single(sym_id)) = (receiver, &args[0]) else {
        return Ok(None);
    };
    let HirNode::ClassRef(path) = &fx.an.compiler.hir[recv_id] else {
        return Ok(None);
    };
    let path = path.clone();
    let sym_id = *sym_id;
    let HirNode::SymbolLit(sym) = &fx.an.compiler.hir[sym_id] else {
        return Ok(None);
    };
    let sym = sym.clone();
    let Some(target) = super::boxes::resolve_class_here(fx, &path) else {
        return Ok(None);
    };
    if !crate::analyze::class_query::class_method_defined_only_later(
        &fx.an.compiler,
        target,
        &sym,
        recv_id,
    ) {
        return Ok(None);
    }
    let recv_op = lower_expr(fx, recv_id)?;
    let recv_ptr = super::ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        super::ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    let sym_op = lower_expr(fx, sym_id)?;
    let sym_ptr = super::ownership::borrow_ptr(fx, &sym_op);
    if sym_op.owned() {
        super::ownership::pool_owned(fx, sym_ptr, sym_op.tag());
    }
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status("zeo_rt_method_capture_inherited", &[recv_ptr, sym_ptr, out]);
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// A `true`/`false` immediate into `dst`.
fn write_bool(fx: &mut Fx, dst: cranelift_codegen::ir::Value, v: bool) {
    let b = fx.b.ins().iconst(types::I8, i64::from(v));
    ownership::write_move_into(fx, &Operand::Bool(b), dst);
}

/// One flip-flop evaluation: the latch decides, and either operand may
/// raise, so both are ordinary lowered expressions guarded by the latch's
/// own branches (rustc's `emit_flip_flop`, branch for branch).
fn flip_flop(
    fx: &mut Fx,
    state: u32,
    left: NodeId,
    right: NodeId,
    exclusive: bool,
) -> CResult<Operand> {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let on_blk = fx.b.create_block();
    let test_left = fx.b.create_block();
    let turned_on = fx.b.create_block();
    let off_blk = fx.b.create_block();
    let join = fx.b.create_block();
    let state_v = fx.b.ins().iconst(types::I32, i64::from(state));
    let on = fx.call_status("zeo_rt_flip_flop_on", &[state_v]);
    fx.b.ins().brif(on, on_blk, &[], test_left, &[]);

    // Already on: the right operand decides whether this is the last true.
    let turn_off = |fx: &mut Fx| -> CResult<()> {
        let op = lower_expr(fx, right)?;
        let t = ownership::truthy(fx, op);
        let clear = fx.b.create_block();
        let done = fx.b.create_block();
        fx.b.ins().brif(t, clear, &[], done, &[]);
        fx.b.switch_to_block(clear);
        let state_v = fx.b.ins().iconst(types::I32, i64::from(state));
        let zero = fx.b.ins().iconst(types::I8, 0);
        fx.call("zeo_rt_flip_flop_set", &[state_v, zero]);
        fx.b.ins().jump(done, &[]);
        fx.b.switch_to_block(done);
        Ok(())
    };
    fx.b.switch_to_block(on_blk);
    turn_off(fx)?;
    write_bool(fx, dst, true);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(test_left);
    let op = lower_expr(fx, left)?;
    let t = ownership::truthy(fx, op);
    fx.b.ins().brif(t, turned_on, &[], off_blk, &[]);

    fx.b.switch_to_block(turned_on);
    let state_v = fx.b.ins().iconst(types::I32, i64::from(state));
    let one = fx.b.ins().iconst(types::I8, 1);
    fx.call("zeo_rt_flip_flop_set", &[state_v, one]);
    if !exclusive {
        turn_off(fx)?;
    }
    write_bool(fx, dst, true);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(off_blk);
    write_bool(fx, dst, false);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(join);
    Ok(Operand::Ptr {
        addr: dst,
        owned: false,
        tag: TagInfo::Known(ValueTag::Bool as u8),
    })
}

/// `a || b` / `a && b`: keep `a` when its truthiness matches
/// `keep_truthy`, else evaluate and keep `b` -- the OPERAND is the value,
/// Ruby's rule.
fn short_circuit(fx: &mut Fx, a: NodeId, b: NodeId, keep_truthy: bool) -> CResult<Operand> {
    let a_op = lower_expr(fx, a)?;
    let ptr = ownership::borrow_ptr(fx, &a_op);
    let a_tag = a_op.tag();
    if a_op.owned() {
        ownership::pool_owned(fx, ptr, a_tag);
    }
    let borrowed = Operand::Ptr {
        addr: ptr,
        owned: false,
        tag: a_tag,
    };
    let t = ownership::truthy(fx, borrowed);
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let keep_a = fx.b.create_block();
    let eval_b = fx.b.create_block();
    let join = fx.b.create_block();
    if keep_truthy {
        fx.b.ins().brif(t, keep_a, &[], eval_b, &[]);
    } else {
        fx.b.ins().brif(t, eval_b, &[], keep_a, &[]);
    }
    fx.b.switch_to_block(keep_a);
    let a_again = Operand::Ptr {
        addr: ptr,
        owned: false,
        tag: a_tag,
    };
    ownership::write_move_into(fx, &a_again, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(eval_b);
    let b_op = lower_expr(fx, b)?;
    ownership::write_move_into(fx, &b_op, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// `name`'s bytes interned in `.rodata`, as a `(ptr, len)` argument pair.
pub(crate) fn rodata_name(
    fx: &mut Fx,
    name: &str,
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    let off = fx.em.intern_rodata(name.as_bytes());
    let ptr = fx.rod(off);
    let len = fx.b.ins().iconst(fx.em.ptr, name.len() as i64);
    (ptr, len)
}

/// A regexp literal. Static parts (raw-byte segments rendered lossily,
/// rustc's rule) fold into one source string served by a per-site cache
/// (`zeo_rt_regexp_lit` -- one frozen object per site); an interpolated
/// pattern builds a fresh string through the to_s dispatch, then
/// `zeo_rt_regexp_interp` compiles it, frozen at birth. Both raise
/// `RegexpError` on a bad pattern.
fn regexp_lit(
    fx: &mut Fx,
    parts: &[crate::hir::StrPart],
    flags: crate::hir::RegexpFlags,
) -> CResult<Operand> {
    use crate::hir::StrPart;
    let enc_byte = match flags.encoding {
        zeo_abi::RegexpEncoding::Source => 0i64,
        zeo_abi::RegexpEncoding::None => 1,
        zeo_abi::RegexpEncoding::EucJp => 2,
        zeo_abi::RegexpEncoding::Windows31j => 3,
        zeo_abi::RegexpEncoding::Utf8 => 4,
    };
    let flag_vals = |fx: &mut Fx| {
        let ic = fx.b.ins().iconst(types::I8, i64::from(flags.ignore_case));
        let ext = fx.b.ins().iconst(types::I8, i64::from(flags.extended));
        let ml = fx.b.ins().iconst(types::I8, i64::from(flags.multiline));
        let enc = fx.b.ins().iconst(types::I8, enc_byte);
        (ic, ext, ml, enc)
    };
    let is_static = parts
        .iter()
        .all(|p| matches!(p, StrPart::Lit(_) | StrPart::Bytes(_)));
    if is_static {
        let mut source = String::new();
        for p in parts {
            match p {
                StrPart::Lit(s) => source.push_str(s),
                StrPart::Bytes(b) => source.push_str(&String::from_utf8_lossy(b)),
                StrPart::Interp(_) => unreachable!("static parts only"),
            }
        }
        let site = fx.em.mint_regexp_site();
        let site_v = fx.b.ins().iconst(types::I32, i64::from(site));
        let off = fx.em.intern_rodata(source.as_bytes());
        let ptr = fx.rod(off);
        let len_v = fx.b.ins().iconst(fx.em.ptr, source.len() as i64);
        let (ic, ext, ml, enc) = flag_vals(fx);
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let status = fx.call_status(
            "zeo_rt_regexp_lit",
            &[site_v, ptr, len_v, ic, ext, ml, enc, out],
        );
        fx.fallible(status);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Known(ValueTag::Regexp as u8),
        });
    }
    // Interpolated: assemble the pattern exactly as string interpolation
    // does (pooled at creation; an interp piece can raise mid-build).
    let ss_pat = fx.temp_slot();
    let pat = fx.slot_addr(ss_pat, 0);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let zero = fx.b.ins().iconst(fx.em.ptr, 0);
    let enc_utf8 = fx.b.ins().iconst(types::I8, ENC_UTF8);
    fx.call("zeo_rt_str_new", &[null, zero, enc_utf8, pat]);
    fx.owned_created += 1;
    ownership::pool_owned(fx, pat, TagInfo::Known(ValueTag::Str as u8));
    for part in parts {
        match part {
            StrPart::Lit(text) => {
                if text.is_empty() {
                    continue;
                }
                let off = fx.em.intern_rodata(text.as_bytes());
                let ptr = fx.rod(off);
                let len_v = fx.b.ins().iconst(fx.em.ptr, text.len() as i64);
                fx.call("zeo_rt_str_append_lit", &[pat, ptr, len_v]);
            }
            StrPart::Bytes(b) => {
                let text = String::from_utf8_lossy(b).into_owned();
                let off = fx.em.intern_rodata(text.as_bytes());
                let ptr = fx.rod(off);
                let len_v = fx.b.ins().iconst(fx.em.ptr, text.len() as i64);
                fx.call("zeo_rt_str_append_lit", &[pat, ptr, len_v]);
            }
            StrPart::Interp(n) => {
                let op = lower_expr(fx, *n)?;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let status = fx.call_status("zeo_rt_str_append_value", &[pat, p]);
                fx.fallible(status);
            }
        }
    }
    let (ic, ext, ml, enc) = flag_vals(fx);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status("zeo_rt_regexp_interp", &[pat, ic, ext, ml, enc, out]);
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(ValueTag::Regexp as u8),
    })
}

/// `case`/`when` in VALUE position: the subject is evaluated once and
/// parked borrowed; each `when` value tests through the runtime's `===`
/// dispatch (`case_eq`; a splatted list through `case_eq_any`, which
/// short-circuits exactly as the listed form's `||` chain does); the first
/// hit's body moves its value into the one result slot; no hit runs the
/// else body (an absent one answers nil). Subjectless `case` tests each
/// value's truthiness, ruby's if-chain sugar.
fn case_when(
    fx: &mut Fx,
    id: NodeId,
    subject: Option<NodeId>,
    arms: &[(Vec<ArrayElem>, Vec<NodeId>)],
    else_body: &[NodeId],
) -> CResult<Operand> {
    let subj = match subject {
        Some(n) => {
            let op = lower_expr(fx, n)?;
            let tag = op.tag();
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, tag);
            }
            Some(ptr)
        }
        None => None,
    };
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    // The `===` hit flag's scratch byte (a whole value slot; only byte 0
    // is used).
    let hit_ss = fx.temp_slot();
    let hit_ptr = fx.slot_addr(hit_ss, 0);
    let join = fx.b.create_block();
    for (values, body) in arms {
        let body_block = fx.b.create_block();
        for elem in values {
            let hit = match (elem, subj) {
                (ArrayElem::Single(v), Some(s)) => {
                    let op = lower_expr(fx, *v)?;
                    let tag = op.tag();
                    let p = ownership::borrow_ptr(fx, &op);
                    if op.owned() {
                        ownership::pool_owned(fx, p, tag);
                    }
                    let status = fx.call_status("zeo_rt_case_eq", &[p, s, hit_ptr]);
                    fx.fallible(status);
                    fx.b.ins()
                        .load(types::I8, MemFlagsData::trusted(), hit_ptr, 0)
                }
                (ArrayElem::Splat(v), Some(s)) => {
                    let op = lower_expr(fx, *v)?;
                    let tag = op.tag();
                    let p = ownership::borrow_ptr(fx, &op);
                    if op.owned() {
                        ownership::pool_owned(fx, p, tag);
                    }
                    let status = fx.call_status("zeo_rt_case_eq_any", &[p, s, hit_ptr]);
                    fx.fallible(status);
                    fx.b.ins()
                        .load(types::I8, MemFlagsData::trusted(), hit_ptr, 0)
                }
                (ArrayElem::Single(v), None) => {
                    let op = lower_expr(fx, *v)?;
                    ownership::truthy(fx, op)
                }
                (ArrayElem::Splat(_), None) => {
                    return fx.unsupported(id, "a subjectless `when *splat`");
                }
            };
            let cont = fx.b.create_block();
            fx.b.ins().brif(hit, body_block, &[], cont, &[]);
            fx.b.switch_to_block(cont);
        }
        // The fall-through block (no value hit) is where the NEXT arm's
        // tests continue; remember it, fill this arm's body, come back.
        let fall = fx.b.current_block().expect("a block is under construction");
        fx.b.switch_to_block(body_block);
        super::stmt::lower_value_body_into(fx, body, dst)?;
        fx.b.ins().jump(join, &[]);
        fx.b.switch_to_block(fall);
    }
    super::stmt::lower_value_body_into(fx, else_body, dst)?;
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The receiver a send should DISPATCH against, or `None` when it IS the
/// current `self` and the implicit entry is the right one.
///
/// Only a literal `self` qualifies. `self_ptr` already holds that object, so
/// answering `None` changes nothing but the entry -- and the implicit entry is
/// the one ruby means: private has been reachable through a literal `self`
/// receiver since 2.7.
/// A plain hoisted local reads as a BORROW of its slot, and a reassignment
/// overwrites that slot in place -- so `a << (a = [9]; 2)` would push onto
/// the array the argument just bound, and release the one the receiver
/// named. Copy the value out (retained, pooled) when anything still to be
/// evaluated can write that name; a call whose arguments assign nothing
/// keeps the borrow.
pub(crate) fn park_reassignable(
    fx: &mut Fx,
    recv: Option<NodeId>,
    op: Operand,
    later: &[NodeId],
) -> Operand {
    let Some(r) = recv else { return op };
    let HirNode::LocalRead(name) = &fx.an.compiler.hir[r] else {
        return op;
    };
    let name = name.clone();
    if !matches!(fx.locals.get(&name), Some(super::ctx::Local::Slot(_))) {
        return op;
    }
    if !later
        .iter()
        .any(|&n| crate::analyze::class_query::assigns_local(&fx.an.compiler, n, &name))
    {
        return op;
    }
    let tag = op.tag();
    let addr = ownership::move_ptr(fx, &op);
    fx.owned_created += 1;
    ownership::pool_owned(fx, addr, tag);
    Operand::Ptr {
        addr,
        owned: false,
        tag,
    }
}

/// The node ids a call still has to evaluate after its receiver: every
/// argument, every keyword value, and a `&expr` block argument.
pub(crate) fn later_nodes(
    args: &[crate::hir::ArrayElem],
    kwargs: &[crate::hir::KwArg],
    block_arg: Option<NodeId>,
) -> Vec<NodeId> {
    let mut out: Vec<NodeId> = args
        .iter()
        .map(|a| match a {
            crate::hir::ArrayElem::Single(n) | crate::hir::ArrayElem::Splat(n) => *n,
        })
        .collect();
    out.extend(kwargs.iter().flat_map(crate::hir::KwArg::node_ids));
    out.extend(block_arg);
    out
}

fn self_receiver(fx: &Fx, recv: Option<NodeId>) -> Option<NodeId> {
    let r = recv?;
    match matches!(fx.an.compiler.hir[r], HirNode::SelfRef) {
        true => None,
        false => Some(r),
    }
}

/// Whether the site runs NO visibility check -- ruby's `VM_CALL_FCALL`.
///
/// True for the receiver zeo SYNTHESIZES for a call ruby writes with none:
/// the `self.singleton_class` a `class << self` body's statement is rebound
/// onto. That receiver must still be EVALUATED -- the surrogate is where
/// those methods live -- so it cannot take the `self_receiver` route above;
/// only its barrier comes down. A receiver the SOURCE wrote is never marked,
/// so a hand-written `Foo.singleton_class.some_private_method` still raises.
pub(crate) fn bypasses_visibility(fx: &Fx, recv: Option<NodeId>) -> bool {
    recv.is_some_and(|r| fx.an.compiler.hir.is_implicit_self_receiver(r))
}

/// Whether an implicit send of `name` from the current body resolves to a
/// method of the ENCLOSING class before reaching the toplevel `Object`
/// def the direct-call table holds -- ruby's MRO puts the receiver's own
/// chain first, so a shadowed name must go through the dynamic send.
pub(crate) fn method_class_shadows(fx: &Fx, name: &str) -> bool {
    let Some(cid) = fx.method_class else {
        return false;
    };
    if cid.0 == 0 {
        return false;
    }
    if fx.self_is_class {
        fx.an.compiler.lookup_class_method(cid, name).is_some()
    } else {
        fx.an.compiler.lookup_method(cid, name).is_some()
    }
}

/// `Kernel#binding` -- this frame, captured. A builtin row cannot answer it:
/// it would have to see its CALLER's locals. The emitter can, so it builds
/// the value here, from the names `binding_scope_names` promoted to cells
/// (rustc's `emit_binding_value`).
///
/// `None` when the scope reports no names -- a `binding` reached through a
/// runtime-computed send, say -- and the ordinary dynamic send raises the
/// runtime's own refusal.
pub(crate) fn binding_value(fx: &mut Fx, site: NodeId) -> CResult<Option<Operand>> {
    if fx.binding_names.is_none() {
        return Ok(None);
    }
    let (file, line) = fx
        .location(site)
        .map_or((String::new(), 0), |(f, l)| (f.to_string(), l));
    Ok(Some(binding_value_at(fx, &file, line)))
}

/// [`binding_value`] with the location supplied rather than read from a site
/// -- `TOPLEVEL_BINDING`, whose location CRuby reports as `["<main>", 0]`.
/// A scope that reports no names yields the DEGRADED form (self, no locals),
/// which is what a program that never asks for one gets.
pub(crate) fn binding_value_at(fx: &mut Fx, file: &str, line: u32) -> Operand {
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    binding_value_with_self(fx, self_ptr, file, line)
}

/// [`binding_value_at`] with `self` supplied rather than taken from the
/// context -- `obj.send(:eval, src)`, where CRuby reads the LOCALS from
/// the caller's frame but binds `self` to the receiver.
pub(crate) fn binding_value_with_self(
    fx: &mut Fx,
    self_ptr: cranelift_codegen::ir::Value,
    file: &str,
    line: u32,
) -> Operand {
    let names = fx.binding_names.clone();
    // Only a CELL local can be shared with a binding; a name the scope
    // reports but keeps in a slot cannot be reached from one.
    let shared: Vec<&str> = names
        .iter()
        .flat_map(|n| n.iter())
        .filter(|n| matches!(fx.locals.get(*n), Some(super::ctx::Local::Cell { .. })))
        .map(String::as_str)
        .collect();
    let (names_ptr, n) = super::statics::str_array(fx, &shared);
    let cells_ptr = super::statics::cell_array(fx, &shared);
    let (fptr, flen) = rodata_name(fx, file);
    let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
    // The binding carries THIS scope's box: a `box.eval(source)` with a
    // non-literal source lowers to a receiverless `eval` inside a
    // `BoxScope`, and the box is what makes its globals and constants the
    // BOX's rather than main's.
    let box_v = fx.box_v();
    // `u32::MAX`, not 0: `ClassId(0)` is `Object`, a cref a top-level
    // binding must not claim. A CLASS BODY has no `defining_class` -- there
    // is no `def` around it -- but its own cref is its class, which is what
    // a constant read through the binding resolves against.
    let owner = fx
        .defining_class
        .or_else(|| fx.self_is_class.then_some(fx.method_class).flatten());
    let cref =
        fx.b.ins()
            .iconst(types::I32, i64::from(owner.map_or(u32::MAX, |c| c.0)));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    fx.call(
        "zeo_rt_binding_new",
        &[
            self_ptr, names_ptr, cells_ptr, n, fptr, flen, line_v, box_v, cref, out,
        ],
    );
    fx.owned_created += 1;
    Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }
}

/// A `def` in EXPRESSION position -- a RUNTIME method install whose value
/// is the method-name Symbol.
///
/// Three shapes, exactly the rustc emitter's split: `def self.x` is always
/// `define_singleton_method` on `self`; a real `def` installs on the
/// DEFAULT DEFINEE (the cref's, unless an `*_eval`/`Class.new` on the
/// stack replaced it -- only the runtime can say, so both candidates go);
/// a literal `define_method(:m){}` is an ordinary `Module#define_method`
/// send, which raises `NoMethodError` when `self` is no Module.
#[allow(
    clippy::too_many_arguments,
    reason = "the DefMethod node's own fields, each deciding a different part of the install"
)]
fn runtime_def(
    fx: &mut Fx,
    id: NodeId,
    name: &str,
    params: &crate::hir::Params,
    body: &[NodeId],
    is_class_method: bool,
    visibility: crate::hir::Visibility,
    is_def: bool,
) -> CResult<Operand> {
    use cranelift_codegen::ir::types;
    // A `def`'s frame is labeled after the METHOD it creates; a
    // `define_method` body is genuinely the block ruby labels it as.
    let frame = if is_def {
        // A `class` body written in a snippet compiles as one more
        // `class_eval` whose CREF is that class, and its `def`s install
        // there -- so the frame names the class, not the `Object` the
        // snippet's own top level would give.
        let owner = match (&fx.eval_cref, fx.method_class) {
            (Some(cref), _) => Some(cref.name.clone()),
            (None, Some(c)) if c != crate::compiler::OBJECT_CLASS => {
                Some(fx.an.compiler.fq_name(c))
            }
            _ => None,
        };
        // A CLASS method's frame separator is `.`, an instance method's `#`.
        let sep = if is_class_method { '.' } else { '#' };
        super::blocks::FrameName::Method(match owner {
            Some(owner) => format!("{owner}{sep}{name}"),
            None => format!("Object{sep}{name}"),
        })
    } else {
        super::blocks::FrameName::Block
    };
    let kind = if is_def {
        super::blocks::MethodBody::Def
    } else {
        super::blocks::MethodBody::DefineMethod
    };
    let proc_ss = super::blocks::build_method_body(fx, id, params, body, frame, kind)?;
    let proc_addr = fx.slot_addr(proc_ss, 0);
    let sym = fx.sym_id(name);
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");

    // A `def` inside a run-time `eval` installs where CRuby installs it,
    // and only the run time knows: the receiver decides, and one compiled
    // snippet may be evaluated against many. `def self.x` keeps the
    // singleton send below -- it names its own receiver.
    if let Some(mode) = fx.eval_mode
        && is_def
        && !is_class_method
    {
        let mode_v = fx.b.ins().iconst(types::I8, i64::from(mode));
        // A snippet's own body IS the class body here, so its running
        // visibility default rides along whatever the definee turns out
        // to be (`class_eval("private; def x; end")`).
        let vis = match visibility {
            crate::hir::Visibility::Private => 1,
            crate::hir::Visibility::Protected => 2,
            crate::hir::Visibility::Public => 0,
        };
        let vis = fx.b.ins().iconst(types::I8, i64::from(vis));
        let out_ss = fx.temp_slot();
        let out = fx.slot_addr(out_ss, 0);
        let status = fx.call_status(
            "zeo_rt_eval_define",
            &[mode_v, self_ptr, sym, proc_addr, vis, out],
        );
        fx.owned_consumed += 1;
        fx.fallible(status);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss: out_ss,
            owned: true,
            tag: TagInfo::Known(ValueTag::Symbol as u8),
        });
    }
    if is_class_method || !is_def {
        // Both are ordinary sends to `self`; the proc MOVES into the args.
        let verb = if is_class_method {
            "define_singleton_method"
        } else {
            "define_method"
        };
        let argv =
            fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                2 * zeo_abi::abi::VALUE_SIZE as u32,
                3,
            ));
        let a0 = fx.slot_addr(argv, 0);
        fx.call("zeo_rt_sym_value", &[sym, a0]);
        let a1 = fx.slot_addr(argv, zeo_abi::abi::VALUE_SIZE as i32);
        let moved = Operand::Slot {
            ss: proc_ss,
            owned: true,
            tag: TagInfo::Known(ValueTag::Proc as u8),
        };
        // The move retires the proc slot; the argv slot becomes the owner,
        // and since the send only BORROWS its arguments the frame pool is
        // what releases it (a raw argv slot has no other releaser).
        ownership::write_move_into(fx, &moved, a1);
        fx.owned_created += 1;
        ownership::pool_owned(fx, a1, TagInfo::Known(ValueTag::Proc as u8));
        let recv = Operand::Ptr {
            addr: self_ptr,
            owned: false,
            tag: TagInfo::Unknown,
        };
        return super::call::dynamic_send_ptr(fx, recv, verb, a0, 2);
    }

    // The cref's definee -- where the `def` was WRITTEN, a compile-time
    // fact; the runtime compares it with the live self to see whether an
    // `*_eval` replaced the definee.
    let cref_cid = fx
        .defining_class
        .or(fx.method_class)
        .unwrap_or(crate::compiler::OBJECT_CLASS);
    let cref_ss = fx.temp_slot();
    let cref = fx.slot_addr(cref_ss, 0);
    {
        use cranelift_codegen::ir::{InstBuilder, MemFlagsData};
        let fl = MemFlagsData::trusted();
        let z = fx.b.ins().iconst(types::I64, 0);
        for off in [0, 8, 16] {
            fx.b.ins().store(fl, z, cref, off);
        }
        let tag =
            fx.b.ins()
                .iconst(types::I8, i64::from(ValueTag::Class as u8));
        fx.b.ins().store(fl, tag, cref, TAG_OFFSET as i32);
        let cid = fx.b.ins().iconst(types::I32, i64::from(cref_cid.0));
        fx.b.ins()
            .store(fl, cid, cref, zeo_abi::abi::PAYLOAD_OFFSET as i32);
    }
    // Written at the TOP LEVEL a `def` is a PRIVATE instance method of
    // Object, exactly as a bare top-level `def` is.
    let top_level = fx.method_class.is_none();
    let private = fx.b.ins().iconst(types::I8, i64::from(u8::from(top_level)));
    // The class body's RUNNING visibility default rides along, applied by the
    // runtime to the definee it resolves -- which is not always the cref (a
    // `def` in a `Class.new` body installs on the new class).
    let body_vis = match visibility {
        crate::hir::Visibility::Private if fx.method_class.is_some() => 1,
        crate::hir::Visibility::Protected if fx.method_class.is_some() => 2,
        _ => 0,
    };
    let body_vis = fx.b.ins().iconst(types::I8, i64::from(body_vis));
    let out_ss = fx.temp_slot();
    let out = fx.slot_addr(out_ss, 0);
    let status = fx.call_status(
        "zeo_rt_define_in_default_definee",
        &[cref, self_ptr, sym, proc_addr, private, body_vis, out],
    );
    // The proc's reference moved into the runtime.
    fx.owned_consumed += 1;
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss: out_ss,
        owned: true,
        tag: TagInfo::Known(ValueTag::Symbol as u8),
    })
}

/// `Ractor.new(*args, name: ...) { |*p| }` / `Ractor.new(&proc)`. The
/// block is an ordinary escaping Proc carrying its own isolation verdict
/// (`RProc::with_outer_capture`), which `zeo_rt_ractor_new` reads and
/// refuses on at exactly the moment CRuby's own Proc-isolation check does.
fn ractor_new(fx: &mut Fx, id: NodeId) -> CResult<Option<Operand>> {
    let HirNode::Call {
        receiver: Some(recv),
        name,
        args,
        kwargs,
        block,
        block_arg,
        safe: false,
    } = &fx.an.compiler.hir[id]
    else {
        return Ok(None);
    };
    if name != "new" {
        return Ok(None);
    }
    let target = match &fx.an.compiler.hir[*recv] {
        HirNode::ClassRef(n) => n.as_str(),
        HirNode::QualifiedConstRead(scope, n) if scope == "Object" => n.as_str(),
        _ => return Ok(None),
    };
    if super::boxes::resolve_class_here(fx, target) != Some(zeo_abi::RACTOR_CLASS) {
        return Ok(None);
    }
    let (args, kwargs, block, block_arg) = (args.clone(), kwargs.clone(), *block, *block_arg);
    let mut name_node = None;
    for kw in &kwargs {
        match kw {
            crate::hir::KwArg::Pair(k, v) if matches!(&fx.an.compiler.hir[*k], HirNode::SymbolLit(s) if s == "name") =>
            {
                name_node = Some(*v);
            }
            _ => {
                return fx
                    .unsupported(id, "a `Ractor.new` keyword other than `name:`")
                    .map(Some);
            }
        }
    }
    let argv = super::call::build_argv(fx, id, &args)?;
    let name_ptr = match name_node {
        Some(v) => {
            let op = lower_expr(fx, v)?;
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            p
        }
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    let blk = match (block, block_arg) {
        (Some(b), _) => super::blocks::literal_block_ptr_rehomed(fx, id, b, true)?,
        (None, Some(ba)) => super::blocks::block_arg_ptr(fx, ba)?,
        (None, None) => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    // The literal block's own site, for `#inspect`'s `#<Ractor:#2 file:4>`
    // slot. A dynamic proc passes nothing: `ractor_new` reads the current
    // frame, which IS the `Ractor.new` call site -- CRuby's slot for that
    // shape.
    let (file, line) = match block.and_then(|b| fx.location(b)) {
        Some((f, l)) => (format!("{f}:{l}"), 1),
        None => (String::new(), 0),
    };
    let off = fx.em.intern_rodata(file.as_bytes());
    let loc_ptr = fx.rod(off);
    let loc_len =
        fx.b.ins()
            .iconst(fx.em.ptr, if line == 0 { 0 } else { file.len() as i64 });
    let argc = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status(
        "zeo_rt_ractor_new",
        &[blk, argv, argc, name_ptr, loc_ptr, loc_len, out],
    );
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// Whether `id` is a call whose lowering is decided by the SITE rather
/// than by dispatch -- a refinement-covered call, or `Ractor.new`. Both
/// are recognized at the head of [`lower_expr`]; a statement-position call
/// carrying a literal block does not route through it, so `lower_stmt`
/// asks this first.
pub(crate) fn site_decided_call(fx: &Fx, id: NodeId) -> bool {
    let HirNode::Call { receiver, name, .. } = &fx.an.compiler.hir[id] else {
        return false;
    };
    if !fx.an.compiler.refinements_active_at(id).is_empty() {
        return true;
    }
    name == "new"
        && receiver.is_some_and(|r| {
            let target = match &fx.an.compiler.hir[r] {
                HirNode::ClassRef(n) => n.as_str(),
                HirNode::QualifiedConstRead(scope, n) if scope == "Object" => n.as_str(),
                _ => return false,
            };
            super::boxes::resolve_class_here(fx, target) == Some(zeo_abi::RACTOR_CLASS)
        })
}

/// A RUNTIME `eval` -- the source is not a literal, so it goes to the eval
/// VM carrying THIS scope. CRuby evaluates a bare `eval` (or one given a
/// `nil` binding) in the caller's own frame, so the site materializes a
/// Binding of itself and hands it over; an explicit binding argument wins.
/// `send(:eval, ..)` is the reflective spelling of the same private
/// `Kernel#eval`, which reads its LOCALS from the caller's frame either
/// way and takes `self` from the receiver.
///
/// Every form reaches here, a literal string included: an `eval` is a
/// run-time compile, never a lower-time splice.
fn runtime_eval(
    fx: &mut Fx,
    id: NodeId,
    receiver: Option<NodeId>,
    name: &str,
    args: &[crate::hir::ArrayElem],
) -> CResult<Option<Operand>> {
    let ids: Option<Vec<NodeId>> = args
        .iter()
        .map(|a| match a {
            crate::hir::ArrayElem::Single(n) => Some(*n),
            crate::hir::ArrayElem::Splat(_) => None,
        })
        .collect();
    // A splat means the argument LIST is a run-time value: the same site,
    // with the arity check moved into the runtime entry. Only the bare
    // spelling takes it -- `send(*args)` does not say it is an eval at all.
    let splatted = ids.is_none() && receiver.is_none() && name == "eval";
    let ids = ids.unwrap_or_default();
    let sent = !splatted && crate::analyze::captures::is_sent_eval(&fx.an.compiler, name, &ids);
    let bare = receiver.is_none() && name == "eval" && (1..=4).contains(&ids.len());
    if !(bare || sent || splatted) {
        return Ok(None);
    }
    // A class that defines its OWN `eval` shadows `Kernel#eval` for a
    // receiverless call in its instance methods -- ruby's ordinary method
    // resolution, so resolve the sibling instead of the VM.
    if bare
        && let Some(owner) = fx.method_class
        && fx.an.compiler.method_in_chain(owner, "eval").is_some()
    {
        return Ok(None);
    }
    // The Binding of THIS scope. Without cell storage there is nothing to
    // share, so the eval could not read a caller local anyway; the scope
    // that can reach one was deoptimized by `binding_scope_names`.
    let self_op = match receiver {
        Some(r) => Some(lower_expr(fx, r)?),
        None => None,
    };
    let scope = match self_op {
        Some(op) => {
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            binding_value_with_self(fx, p, "(eval)", 0)
        }
        None => {
            let _ = id;
            binding_value_at(fx, "(eval)", 0)
        }
    };
    let scope_ptr = ownership::borrow_ptr(fx, &scope);
    ownership::pool_owned(fx, scope_ptr, scope.tag());
    if splatted {
        let args_ptr = super::call::build_array(fx, args)?;
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let status = fx.call_status(
            "zeo_rt_eval_value_in_scope_argv",
            &[args_ptr, scope_ptr, out],
        );
        fx.fallible(status);
        fx.owned_created += 1;
        return Ok(Some(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        }));
    }
    let rest = &ids[usize::from(sent)..];
    let mut ptrs = Vec::with_capacity(4);
    for &a in rest.iter().take(4) {
        let op = lower_expr(fx, a)?;
        let p = ownership::borrow_ptr(fx, &op);
        if op.owned() {
            ownership::pool_owned(fx, p, op.tag());
        }
        ptrs.push(p);
    }
    while ptrs.len() < 4 {
        ptrs.push(fx.b.ins().iconst(fx.em.ptr, 0));
    }
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status(
        "zeo_rt_eval_value_in_scope",
        &[ptrs[0], scope_ptr, ptrs[1], ptrs[2], ptrs[3], out],
    );
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// `blk.call(a, b)` where `blk` is the scope's own `&block` parameter --
/// a value that is a Proc or nil, and nothing else. A Proc reaches
/// `RProc::call` DIRECTLY rather than Proc's dispatch row, which is what
/// keeps a `break` inside an iterator's block a `Signal::Break` for the
/// iterator to catch instead of the `LocalJumpError` a proc-closure's
/// break raises. `Enumerable#first` driving a user `each` that forwards
/// its block is the corpus shape.
fn block_param_call(fx: &mut Fx, id: NodeId) -> CResult<Option<Operand>> {
    let HirNode::Call {
        receiver: Some(recv),
        name,
        args,
        kwargs,
        block,
        block_arg,
        safe: false,
    } = &fx.an.compiler.hir[id]
    else {
        return Ok(None);
    };
    if !matches!(name.as_str(), "call" | "()" | "[]" | "yield" | "===")
        || !kwargs.is_empty()
        || block.is_some()
        || block_arg.is_some()
        || args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
    {
        return Ok(None);
    }
    let HirNode::LocalRead(local) = &fx.an.compiler.hir[*recv] else {
        return Ok(None);
    };
    // An ANONYMOUS `&` declares no name, so no `LocalRead` can name it.
    if fx
        .method_params
        .as_ref()
        .and_then(|p| p.block.as_ref())
        .and_then(Option::as_ref)
        != Some(local)
    {
        return Ok(None);
    }
    let (name, args, recv) = (name.clone(), args.clone(), *recv);
    let op = lower_expr(fx, recv)?;
    let recv_ptr = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        ownership::pool_owned(fx, recv_ptr, op.tag());
    }
    let argv = super::call::build_argv(fx, id, &args)?;
    let sym = fx.sym_id(&name);
    let argc = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let caller = super::call::caller_class(fx, false);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status(
        "zeo_rt_proc_call_or_send",
        &[recv_ptr, sym, argv, argc, null, caller, out],
    );
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}
