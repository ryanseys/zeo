//! Expression lowering for the M0 slice: literals, local reads, and the
//! numeric binary operators with their three-arm shape -- inline Int,
//! inline Float, dynamic `send_value_in` fallback (exactly the rustc
//! emitter's match).

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};
use zeo_abi::abi::{PAYLOAD_OFFSET, ValueTag};

/// `EncodingId(1)` = UTF-8, every plain source literal's encoding.
const ENC_UTF8: i64 = 1;

pub(crate) fn lower_expr(fx: &mut Fx, id: NodeId) -> Result<Operand, String> {
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
                let off = fx.em.intern_rodata(text.as_bytes());
                let ss = fx.temp_slot();
                let dst = fx.slot_addr(ss, 0);
                let ptr = fx.rod(off);
                let len_v = fx.b.ins().iconst(fx.em.ptr, text.len() as i64);
                let enc = fx.b.ins().iconst(types::I8, ENC_UTF8);
                // `# frozen_string_literal: true`: a non-interpolated
                // literal IS its interned frozen twin (equal literals share
                // one object, and mutation raises).
                let entry = if fx.an.compiler.hir.literal_frozen_at(id) {
                    "zeo_rt_str_lit"
                } else {
                    "zeo_rt_str_new"
                };
                fx.call(entry, &[ptr, len_v, enc, dst]);
                fx.owned_created += 1;
                return Ok(Operand::Slot {
                    ss,
                    owned: true,
                    tag: TagInfo::Known(ValueTag::Str as u8),
                });
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
                        let status = fx
                            .call("zeo_rt_str_append_value", &[dst, p])
                            .expect("append_value returns a status");
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
            super::stmt::lower_multi_group(fx, id, &targets, ptr)?;
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
            symbol_value(fx, &name)
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
            let cref =
                class_immediate(fx, fx.method_class.unwrap_or(crate::compiler::OBJECT_CLASS));
            let cref_ptr = ownership::borrow_ptr(fx, &cref);
            if cref.owned() {
                ownership::pool_owned(fx, cref_ptr, cref.tag());
            }
            let self_ptr = super::stmt::dyn_ivar_recv(fx);
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
            let status = fx
                .call(
                    "zeo_rt_alias_in_default_definee",
                    &[cref_ptr, self_ptr, new_ptr, old_ptr, out],
                )
                .expect("alias_in_default_definee returns a status");
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
        // A `class`/`module` written where a value is READ -- `x = class C;
        // 7; end`, or a `class << self` body ending a method. Ruby's value
        // is the body's last statement, which the site computes.
        HirNode::ClassDef { .. } if fx.eval_mode.is_some() => super::stmt::eval_class_def(fx, id),
        HirNode::ClassDef { .. } => super::stmt::class_body_value(fx, id, true),
        HirNode::ClassRef(name) => {
            let name = name.clone();
            const_read(fx, id, &name)
        }
        HirNode::QualifiedConstRead(scope, name) => {
            let (scope, name) = (scope.clone(), name.clone());
            scoped_const_read(fx, id, &scope, &name)
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
                Some(s) => match resolve_class_here(fx, s) {
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
                        .class(owner_class)
                        .const_owners
                        .get(&name)
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
            let status = fx
                .call(entry, &[ptr, nptr, nlen, out])
                .expect("scope_const_get returns a status");
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
            let status = fx
                .call("zeo_rt_scope_const_set", &[sptr, nptr, nlen, vptr, out])
                .expect("scope_const_set returns a status");
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
                let status = fx
                    .call("zeo_rt_yield_args", &[blk, arr, kw, out])
                    .expect("yield_args returns a status");
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
                fx.call("zeo_rt_eval_yield", &[argv_ptr, argc_v, out])
                    .expect("eval_yield returns a status")
            } else {
                fx.call("zeo_rt_yield", &[blk, argv_ptr, argc_v, out])
                    .expect("yield returns a status")
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
                let given = fx
                    .call("zeo_rt_eval_block_given", &[])
                    .expect("eval_block_given answers");
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
            super::stmt::ivar_read_op(fx, &name)
        }
        HirNode::Defined(v) => {
            let v = *v;
            lower_defined(fx, id, v)
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
        // The handle VALUE `box = Ruby::Box.new` binds: a Class of the
        // box's top-level surrogate, so `p box` prints its registered
        // `#<Ruby::Box:N>` name and handle equality works.
        HirNode::BoxHandle(box_id) => match fx.an.compiler.box_surrogate(*box_id) {
            Some(cid) => Ok(class_immediate(fx, cid)),
            None => fx.unsupported(id, "a box with no surrogate"),
        },
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
            // top-level defs as free functions), so the rustc backend folds
            // it to a fresh `Object` and so does this. A user-defined
            // `initialize` (top-level `def initialize`, or a `class Object`
            // reopen) still runs, through its VALUE-channel row.
            if let Some(cid) = resolve_class_here(fx, &class_name)
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
                        None,
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
                && let Some(cid) = resolve_class_here(fx, &class_name).filter(|&cid| {
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
                let blk_ptr = match block {
                    Some(b) => Some(super::blocks::literal_block_ptr(fx, id, b)?),
                    None => None,
                };
                return super::call::construct_compiled(fx, id, cid, &elems, blk_ptr);
            }
            // A statically-known class is a Class immediate; a constant
            // holding a RUNTIME class (`Struct.new`/`Data.define`) is read
            // at the call, exactly the rustc `__rtclass` shape.
            let recv = const_read(fx, id, &class_name)?;
            let elems: Vec<ArrayElem> = args.iter().map(|&a| ArrayElem::Single(a)).collect();
            match (block, kwargs.is_empty()) {
                // `Foo.new(x) { .. }`: the literal block forwards to
                // `initialize`, so `yield`/`block_given?` inside it see it.
                (Some(blk), true) => super::blocks::block_send_op(fx, id, recv, "new", &elems, blk),
                (Some(blk), false) => {
                    let bp = super::blocks::literal_block_ptr(fx, id, blk)?;
                    super::call::kw_send(
                        fx,
                        id,
                        super::call::Recv::at(recv),
                        "new",
                        &elems,
                        &kwargs,
                        Some(bp),
                    )
                }
                (None, true) => super::call::dynamic_send_value(fx, id, recv, "new", &elems, false),
                (None, false) => super::call::kw_send(
                    fx,
                    id,
                    super::call::Recv::at(recv),
                    "new",
                    &elems,
                    &kwargs,
                    None,
                ),
            }
        }
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
            let tv = fx.b.ins().load(types::I8, MemFlagsData::trusted(), ptr, 0);
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
            } else if let Some(bp) = blk {
                super::blocks::send_with_block_ptr_ops(fx, id, through, &name, &args, bp, bypass)?
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
            // A typed-receiver `arr.each` (`Compiler::inline_iter_sites`):
            // fused under a runtime guard, with the ordinary block send on
            // the other arm. The literal shapes below never nominate --
            // their receivers are not locals -- so the order is free.
            if args.is_empty()
                && let Some(r) = receiver
                && fx.an.compiler.inline_iter_sites.get(&blk)
                    == Some(&crate::compiler::InlineIterKind::ArrayEach)
            {
                return Ok(super::iter::lower_array_each(fx, id, r, blk, true)?
                    .expect("a wanted result is always built"));
            }
            if args.is_empty()
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
                let bp = super::blocks::literal_block_ptr(fx, id, blk)?;
                let bypass = bypasses_visibility(fx, receiver);
                return super::call::splat_send(
                    fx,
                    id,
                    super::call::Recv::maybe(recv, bypass),
                    &name,
                    &args,
                    &[],
                    Some(bp),
                );
            }
            super::blocks::block_send(fx, id, receiver, &name, &args, blk)
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
            if let Some(op) = module_nesting(fx, receiver, &name, &args)? {
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
                return symbol_value(fx, &origin);
            }
            if let Some(op) = method_capture_intrinsic(fx, receiver, &name, &args, &[], None, None)?
            {
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
                    None,
                );
            }
            match receiver {
                Some(recv) if operator_fast_path(fx, &name) && args.len() == 1 => {
                    let [ArrayElem::Single(arg)] = args.as_slice() else {
                        return fx.unsupported(id, "a splat operand");
                    };
                    binop(
                        fx,
                        BinOp::of(&name).expect("guarded above"),
                        &name,
                        recv,
                        *arg,
                    )
                }
                Some(recv) => super::call::dynamic_send(fx, id, recv, &name, &args),
                None if let Some(folded) = inline_accessor(fx, &name, &args, &[], None, None) => {
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
            if let Some(op) =
                method_capture_intrinsic(fx, receiver, &name, &args, &kwargs, block, block_arg)?
            {
                return Ok(op);
            }
            // A receiverless keyword call naming a compiled method whose
            // keywords are ALL required, covered exactly by literal keys,
            // fills the slots itself: no Hash, no dynamic send, no binder.
            // The rustc backend has routed this shape statically all
            // along; CLIF sent every keyword call the long way round.
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
                return super::call::direct_call_kw(
                    fx,
                    id,
                    &name,
                    &args,
                    Some((&kwargs, &order)),
                    block,
                );
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
            let park = |fx: &mut Fx, n: Option<NodeId>| -> Result<Option<Operand>, String> {
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
            let status = fx
                .call("zeo_rt_range_new", &[s_ptr, e_ptr, excl, out])
                .expect("range_new returns a status");
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
            let status = fx
                .call("zeo_rt_gvar_assign", &[bx, nptr, nlen, ptr])
                .expect("gvar_assign returns a status");
            fx.fallible(status);
            Ok(Operand::Ptr {
                addr: ptr,
                owned: false,
                tag,
            })
        }
        HirNode::ClassVarRead(name) => {
            let name = name.clone();
            let owner = cvar_owner(fx, &name);
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
                let status = fx
                    .call("zeo_rt_cvar_get_checked", &[owner_v, nptr, nlen, out])
                    .expect("cvar_get_checked returns a status");
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
            super::stmt::ivar_write_op(fx, &name, borrowed)?;
            Ok(Operand::Ptr {
                addr: ptr,
                owned: false,
                tag,
            })
        }
        HirNode::ClassVarWrite(name, value) => {
            let (name, value) = (name.clone(), *value);
            let owner = cvar_owner(fx, &name);
            let op = lower_expr(fx, value)?;
            let tag = op.tag();
            let ptr = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, ptr, tag);
            }
            let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner));
            let (nptr, nlen) = rodata_name(fx, &name);
            let status = fx
                .call("zeo_rt_cvar_set", &[owner_v, nptr, nlen, ptr])
                .expect("cvar_set returns a status");
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
                Some(s) => match resolve_class_here(fx, s) {
                    Some(cid) => cid,
                    // A scope no compile-time class backs may still be a
                    // RUNTIME constant holding one (`class SK::Enc` where
                    // `SK` came from `class SK < DelegateClass(Hash)`), so
                    // the path resolves at run time. Ruby reads the scope
                    // BEFORE the value (`Nope::X = (puts 1; 5)` prints
                    // nothing -- oracle-verified), which is the order the
                    // read below already gives.
                    None => return runtime_scope_const_write(fx, id, s, &name, value),
                },
                // A bare `NAME =` is owned by the lexically enclosing
                // class/module (the emitting context); the box's top level
                // otherwise -- rustc's `const_owner_id_opt` fallback.
                None => fx.method_class.unwrap_or_else(|| box_top(fx)),
            };
            let owner = match eval_owner {
                Some(cid) => cid,
                None => {
                    fx.an
                        .compiler
                        .class(owner_class)
                        .const_owners
                        .get(&name)
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
            const_added_send(fx, owner, &name, Some(id))?;
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

/// `if` in VALUE position: both arms move their value into one result
/// slot.
fn if_expr(
    fx: &mut Fx,
    cond: NodeId,
    then_body: &[NodeId],
    else_body: &[NodeId],
) -> Result<Operand, String> {
    let c = lower_expr(fx, cond)?;
    let t = ownership::truthy(fx, c);
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
) -> Result<Option<cranelift_codegen::ir::Value>, String> {
    match (block, block_arg) {
        (None, None) => Ok(None),
        (Some(blk), None) => Ok(Some(super::blocks::literal_block_ptr(fx, id, blk)?)),
        (None, Some(ba)) => Ok(Some(super::blocks::block_arg_ptr(fx, ba)?)),
        (Some(_), Some(_)) => fx.unsupported(id, "a literal block beside a `&` block argument"),
    }
}

/// A short human label for refusal messages.
fn node_kind(node: &HirNode) -> String {
    // One `match` would be 80 arms of labels nothing else needs; the
    // refusal text only has to orient, not classify -- so a kind without a
    // hand-written phrase names its HIR VARIANT, which is what a triage
    // histogram over the corpus reads.
    match node {
        HirNode::Call { .. } => "a method call".to_string(),
        HirNode::If { .. } => "an `if` in value position".to_string(),
        HirNode::While { .. } => "a loop in value position".to_string(),
        other => format!("the node kind `{}`", variant_name(other)),
    }
}

/// The bare variant name of a node (`Debug`'s leading identifier).
pub(crate) fn variant_name(node: &HirNode) -> String {
    let text = format!("{node:?}");
    text.split(['(', ' ', '{'])
        .next()
        .unwrap_or("Unknown")
        .to_string()
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

/// A constant read: a statically-resolved class becomes a Class immediate;
/// anything else (a value constant like `ARGV`) reads through the uncached
/// runtime lookup, `NameError` on miss.
/// The lexical cref chain enclosing the current body, outermost first --
/// `Compiler::cref_of`'s frozen answer for the emitting class; empty at
/// the top level (rustc's `Ctx::cref_chain`).
fn cref_chain<'a>(fx: &'a Fx) -> &'a [crate::compiler::ClassId] {
    lexical_class(fx)
        .map(|c| fx.an.compiler.cref_of_ref(c))
        .unwrap_or(&[])
}

/// The class a cref-less constant belongs to: `Object`, or -- inside a BOX
/// -- the box's own SURROGATE. A box is a copy of MASTER, so its top-level
/// constants must not land in (or be read from) main's `Object` table;
/// rustc's `box_top_owner` draws the same line.
pub(crate) fn box_top(fx: &Fx) -> crate::compiler::ClassId {
    if fx.box_id == 0 {
        return crate::compiler::OBJECT_CLASS;
    }
    fx.an
        .compiler
        .box_surrogate(fx.box_id)
        .expect("analyze registers a surrogate for every allocated box")
}

/// The class a LEXICAL question resolves against: the singleton surrogate
/// when the body was written in a constant-bearing `class << self`, else the
/// class the body was WRITTEN in. `Scope::lexical_home`'s rule over rustc's
/// `cref_chain`, which reads `defining_class`.
///
/// The owner is the last resort, not the first: a method materialized onto
/// a subclass or an includer keeps the cref it was written in, so `rescue
/// Boom` inside `M::Base#go` still names `M::Boom` when `Sub` runs it.
pub(crate) fn lexical_class(fx: &Fx) -> Option<crate::compiler::ClassId> {
    fx.lexical_home.or(fx.defining_class).or(fx.method_class)
}

/// Resolve a class name against the current cref and BOX (rustc's
/// `Ctx::resolve_class`).
/// A receiverless (or literal-`self`) call naming an accessor of THIS
/// body's own class, replaced by the ivar access itself: no dispatch, no
/// trampoline, no frame. The rustc emitter's `emit_inline_accessor` at a
/// Path-1 site, with `self` as the statically-typed receiver.
///
/// `Compiler::accessor_shape` carries the gate that matters -- a
/// HAND-written accessor keeps its body wherever instrumentation can
/// observe the call (TracePoint, line coverage), while an `attr_*`
/// GENERATED one is iseq-less either way, exactly as CRuby compiles it --
/// and `ivar_read_op`/`ivar_write_op` carry the rest: a dynamic or class
/// `self`, a native-backed owner, and a name the layout has no slot for
/// all take the name-keyed path on their own.
///
/// Runtime redefinition is no more a hazard here than at any Path-1 site:
/// zeo binds these statically in both backends, and a later
/// `define_method` does not displace them (`tests/gaps/
/// issue_runtime_redefine_accessor.rb`). This preserves that; it does not
/// widen it.
fn inline_accessor(
    fx: &mut Fx,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[crate::hir::KwArg],
    block: Option<crate::hir::NodeId>,
    block_arg: Option<crate::hir::NodeId>,
) -> Option<Result<Operand, String>> {
    use crate::compiler::AccessorKind;
    if !kwargs.is_empty() || block.is_some() || block_arg.is_some() {
        return None;
    }
    if fx.self_is_dynamic || fx.self_is_class || fx.dyn_ivars {
        return None;
    }
    let cid = fx.method_class?;
    let (_owner, scope_id) = fx.an.compiler.method_in_chain(cid, name)?;
    let scope = fx.an.compiler.scope(scope_id);
    let shape = fx.an.compiler.accessor_shape(cid, scope)?;
    let ivar = shape.ivar.clone();
    match (shape.kind, args) {
        (AccessorKind::Reader, []) => Some(super::stmt::ivar_read_op(fx, &ivar)),
        (AccessorKind::Writer, [ArrayElem::Single(arg)]) => {
            let arg = *arg;
            Some((|| {
                let op = lower_expr(fx, arg)?;
                // `obj.x = v` answers `v`, so the write takes a copy and
                // the value is handed back.
                let p = ownership::borrow_ptr(fx, &op);
                let tag = op.tag();
                if op.owned() {
                    ownership::pool_owned(fx, p, tag);
                }
                let borrowed = || Operand::Ptr {
                    addr: p,
                    owned: false,
                    tag,
                };
                super::stmt::ivar_write_op(fx, &ivar, borrowed())?;
                Ok(borrowed())
            })())
        }
        // An argument count the accessor does not take must still raise
        // `ArgumentError`, which is the trampoline's job.
        _ => None,
    }
}

pub(crate) fn resolve_class_here(fx: &Fx, name: &str) -> Option<crate::compiler::ClassId> {
    // A snippet under a run-time cref may see a constant that shadows the
    // one this (fresh) compiler would fold to, and the compiler cannot
    // know: the class was minted by a compile that is already over. So the
    // fold stands down and every name takes the run-time walk.
    if fx.eval_cref.is_some() {
        return None;
    }
    fx.an
        .compiler
        .resolve_class(name, cref_chain(fx), fx.box_id)
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
) -> Result<Option<Operand>, String> {
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
    let Some(target) = resolve_class_here(fx, &path) else {
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
    let status = fx
        .call("zeo_rt_method_capture_inherited", &[recv_ptr, sym_ptr, out])
        .expect("method_capture_inherited returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

pub(crate) fn const_read(fx: &mut Fx, id: NodeId, name: &str) -> Result<Operand, String> {
    if let Some(cid) = resolve_class_here(fx, name) {
        return class_value_of(fx, id, name, cid);
    }
    // A PATH whose leaf is not a class -- `M::ALIAS` where the constant only
    // HOLDS one -- is the scope operator's question, not the bare-name cref
    // walk's: asking the walk for the whole string looks up a constant
    // literally called "M::ALIAS" and misses.
    if let Some((scope, leaf)) = name.rsplit_once("::")
        && !scope.is_empty()
    {
        return scoped_const_read(fx, id, scope, leaf);
    }
    // `::X` names the TOP LEVEL explicitly: no cref is consulted, and
    // ruby's message does not echo the `::` (`uninitialized constant
    // Nope`). Only the single-segment form reaches here -- a longer path
    // already split above, and its scope re-asks this walk.
    let top_level = name.starts_with("::");
    let name = name.strip_prefix("::").unwrap_or(name);
    // The rustc `emit_const_read` bare-name shape: owner from the
    // compile-time claim map, then every enclosing cref scope, then the
    // top -- one runtime walk through `const_get_cref`, whose miss raises
    // the NameError with the cref-qualified message.
    // Inside a BOX the top level is the box's own SURROGATE -- both where
    // a cref-less read starts and where the chain ends. The tail past it
    // reaches the MASTER constants and stops (the flag below), never
    // main's own top-level table.
    let top = box_top(fx);
    let defining = if top_level {
        top
    } else {
        lexical_class(fx).unwrap_or(top)
    };
    let compiler = &fx.an.compiler;
    // A run-time cref answers the whole question: its own table, then the
    // top. There is no claim map to consult and no lexical parent to walk
    // -- CRuby's string `*_eval` has one cref and no nesting either.
    if let Some(cref) = fx.eval_cref.clone() {
        // The snippet's own lexical chain, then the top -- a `class` body
        // opened inside a snippet prepends its class to the chain it
        // inherited, so a constant of an ENCLOSING `class_eval` is still
        // in reach.
        let mut chain: Vec<u32> = cref.chain.iter().copied().filter(|&c| c != top.0).collect();
        chain.push(top.0);
        let qualified = format!("{}::{name}", cref.name);
        // Whether the cref's chain defines `const_missing` is a RUN-TIME
        // question in a snippet -- the class is one the running program
        // registered and this compiler has no entry for -- so the miss
        // always goes through the dispatch, whose default row raises the
        // same NameError the baked one would.
        return const_cref_call(fx, &chain, name, &qualified, true);
    }
    let owner = compiler
        .class(defining)
        .const_owners
        .get(name)
        .copied()
        .unwrap_or(defining);
    // A miss on an owner whose chain defines a USER `const_missing`
    // dispatches the hook instead of the baked raise (CRuby's protocol,
    // rustc's `miss` arm); the runtime does it so the walk and the hook
    // stay one call.
    let hook = compiler
        .class_method_in_chain(owner, "const_missing")
        .is_some();
    let mut chain: Vec<u32> = vec![owner.0];
    let mut at = compiler.class(owner).cref_parent;
    while let Some(cid) = at {
        if cid != owner && cid != top {
            chain.push(cid.0);
        }
        at = compiler.class(cid).cref_parent;
    }
    if owner != top {
        chain.push(top.0);
    }
    let qualified = if defining == top {
        name.to_string()
    } else {
        format!("{}::{name}", compiler.fq_name(defining))
    };
    const_cref_call(fx, &chain, name, &qualified, hook)
}

/// The cref walk itself: ids in `.rodata`, the name, and the qualified
/// spelling the NameError carries on a miss.
fn const_cref_call(
    fx: &mut Fx,
    chain: &[u32],
    name: &str,
    qualified: &str,
    hook: bool,
) -> Result<Operand, String> {
    let bytes: Vec<u8> = chain.iter().flat_map(|c| c.to_le_bytes()).collect();
    let ids_off = fx.em.intern_rodata_aligned(&bytes, 4);
    let ids_ptr = fx.rod(ids_off);
    let n_ids = fx.b.ins().iconst(fx.em.ptr, chain.len() as i64);
    let (nptr, nlen) = rodata_name(fx, name);
    let (qptr, qlen) = rodata_name(fx, qualified);
    let flags = u8::from(hook) | if fx.box_id == 0 { 0 } else { 2 };
    let hook_v = fx.b.ins().iconst(types::I8, i64::from(flags));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_const_get_cref",
            &[ids_ptr, n_ids, nptr, nlen, qptr, qlen, hook_v, out],
        )
        .expect("const_get_cref returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A constant read by PATH: `Foo::Bar` splits and takes the scoped read,
/// a bare name the cref walk. What a rescue clause's unresolved class
/// name needs -- ruby evaluates a clause's class expression only while
/// MATCHING, so a name that resolves to nothing here may still hold one
/// then (`ALIAS = Base`, `Foo = Class.new`).
pub(crate) fn const_path_read(fx: &mut Fx, id: NodeId, path: &str) -> Result<Operand, String> {
    match crate::hir::split_const_path(path) {
        (Some(scope), leaf) if !scope.is_empty() => scoped_const_read(fx, id, scope, leaf),
        (_, leaf) => const_read(fx, id, leaf),
    }
}

/// An explicit `Scope::NAME` read whose scope resolves at compile time:
/// the scope operator's own search on the scope class, ruby's
/// as-written miss message (`Object::` prints bare -- it is where a
/// lookup ENDS, not a qualifier).
/// `private_constant` is a runtime FLAG, not a compile-time fact -- a later
/// `M.public_constant :S` restores the name -- so the guard is emitted where
/// the compiler saw the directive, and asks. The flag lives on the
/// constant's OWNER, which the claim map may redirect to (rustc's
/// `const_owner_id_opt`).
fn emit_private_constant_guard(fx: &mut Fx, scope_cid: crate::compiler::ClassId, name: &str) {
    let compiler = &fx.an.compiler;
    let owner_cid = compiler
        .class(scope_cid)
        .const_owners
        .get(name)
        .copied()
        .unwrap_or(scope_cid);
    // A directive named the constant somewhere (privacy is positional, so
    // WHICH one last ran is the run time's answer), or nothing static can
    // see one and the run time is the only place the answer lives.
    let info = compiler.class(owner_cid);
    if !info.const_visibility_names.contains(name)
        && !info.private_constants.contains(name)
        && !compiler.hir.constant_privacy_is_runtime()
    {
        return;
    }
    let path = format!("{}::{name}", compiler.fq_name(owner_cid));
    let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner_cid.0));
    let (nptr, nlen) = rodata_name(fx, name);
    let private = fx
        .call("zeo_rt_const_private", &[owner_v, nptr, nlen])
        .expect("const_private answers");
    let hidden = fx.b.create_block();
    let go = fx.b.create_block();
    fx.b.ins().brif(private, hidden, &[], go, &[]);
    fx.b.switch_to_block(hidden);
    let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner_cid.0));
    let (nptr, nlen) = rodata_name(fx, name);
    let (pptr, plen) = rodata_name(fx, &path);
    let st = fx
        .call(
            "zeo_rt_raise_private_constant",
            &[owner_v, nptr, nlen, pptr, plen],
        )
        .expect("raise_private_constant returns a status");
    fx.fallible(st);
    fx.b.ins().jump(go, &[]);
    fx.b.switch_to_block(go);
}

fn scoped_const_read(fx: &mut Fx, id: NodeId, scope: &str, name: &str) -> Result<Operand, String> {
    // The private guard comes FIRST: a private constant naming a nested
    // class would otherwise fold to a Class immediate below and never ask
    // (`M::Hidden` answered the class where ruby raises).
    if let Some(scope_cid) = resolve_class_here(fx, scope) {
        emit_private_constant_guard(fx, scope_cid, name);
    }
    // `Scope::NAME` naming a nested class/module is a Class immediate.
    let path = format!("{scope}::{name}");
    if let Some(cid) = resolve_class_here(fx, &path) {
        return class_value_of(fx, id, &path, cid);
    }
    // A top-level anchor `::Name` lowers with scope "Object", where the
    // name may be an ordinary top-level class.
    if scope == "Object"
        && let Some(cid) = fx.an.compiler.resolve_class(name, &[], 0)
    {
        return class_value_of(fx, id, name, cid);
    }
    let Some(scope_cid) = resolve_class_here(fx, scope) else {
        return runtime_scope_const_read(fx, id, scope, name);
    };
    let compiler = &fx.an.compiler;
    // In a snippet the scope's class methods are the running program's,
    // which this compiler cannot see -- so the run time decides.
    let hook = fx.eval_mode.is_some()
        || compiler
            .class_method_in_chain(scope_cid, "const_missing")
            .is_some();
    let qualified = if scope == "Object" {
        name.to_string()
    } else {
        format!("{scope}::{name}")
    };
    let owner_v = fx.b.ins().iconst(types::I32, i64::from(scope_cid.0));
    let (nptr, nlen) = rodata_name(fx, name);
    let (qptr, qlen) = rodata_name(fx, &qualified);
    let hook_v = fx.b.ins().iconst(types::I8, i64::from(hook));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_const_get_scoped",
            &[owner_v, nptr, nlen, qptr, qlen, hook_v, out],
        )
        .expect("const_get_scoped returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A scope that is no compile-time class may still be a RUNTIME constant
/// holding one (`Line = Struct.new(..)`, a class under a computed
/// superclass), so the path resolves at run time: read the scope by its
/// own rules -- recursively, so `K::C::P` reports a missing HEAD exactly
/// as a bare miss does -- then the leaf on the class it names (rustc's
/// `emit_const_read` runtime-scope arm).
fn runtime_scope_const_read(
    fx: &mut Fx,
    id: NodeId,
    scope: &str,
    name: &str,
) -> Result<Operand, String> {
    // A TOP-ANCHORED scope (`::Tilt::Template`) splits with an empty head;
    // that is the anchor, not a namespace to look `Tilt` up in.
    let (head, leaf) = crate::hir::split_const_path(scope);
    let leaf = leaf.to_string();
    let scope_op = match head.filter(|h| !h.is_empty()) {
        Some(h) => {
            let h = h.to_string();
            scoped_const_read(fx, id, &h, &leaf)?
        }
        None => const_read(fx, id, &leaf)?,
    };
    // `Object` is never NAMED as the scope: ruby reports `Object::X` as a
    // bare miss, since a top-level constant lives on Object anyway.
    let qualified = if scope == "Object" {
        name.to_string()
    } else {
        format!("{scope}::{name}")
    };
    let sptr = ownership::borrow_ptr(fx, &scope_op);
    if scope_op.owned() {
        ownership::pool_owned(fx, sptr, scope_op.tag());
    }
    let (nptr, nlen) = rodata_name(fx, name);
    let (qptr, qlen) = rodata_name(fx, &qualified);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_const_get_on_value",
            &[sptr, nptr, nlen, qptr, qlen, out],
        )
        .expect("const_get_on_value returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
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
) -> Result<Operand, String> {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let on_blk = fx.b.create_block();
    let test_left = fx.b.create_block();
    let turned_on = fx.b.create_block();
    let off_blk = fx.b.create_block();
    let join = fx.b.create_block();
    let state_v = fx.b.ins().iconst(types::I32, i64::from(state));
    let on = fx
        .call("zeo_rt_flip_flop_on", &[state_v])
        .expect("flip_flop_on answers");
    fx.b.ins().brif(on, on_blk, &[], test_left, &[]);

    // Already on: the right operand decides whether this is the last true.
    let turn_off = |fx: &mut Fx| -> Result<(), String> {
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
fn short_circuit(fx: &mut Fx, a: NodeId, b: NodeId, keep_truthy: bool) -> Result<Operand, String> {
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

/// The Class-immediate materialization for an already-resolved id.
fn class_value_of(
    fx: &mut Fx,
    _id: NodeId,
    _name: &str,
    cid: crate::compiler::ClassId,
) -> Result<Operand, String> {
    // Registered but not PROMISED: whether a runtime-conditional class's
    // constant exists is settled by the guarded body having run, so the
    // reference asks -- `NameError` until `reveal_class` fires there.
    if fx.an.compiler.class(cid).runtime_conditional {
        let fq = fx.an.compiler.fq_name(cid);
        let owner = fx
            .an
            .compiler
            .class(cid)
            .lexical_parent
            .unwrap_or(crate::compiler::OBJECT_CLASS);
        let ss = fx.temp_slot();
        let dst = fx.slot_addr(ss, 0);
        let cid_v = fx.b.ins().iconst(types::I32, i64::from(cid.0));
        let (nptr, nlen) = rodata_name(fx, &fq);
        let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner.0));
        let st = fx
            .call(
                "zeo_rt_conditional_class_ref",
                &[cid_v, nptr, nlen, owner_v, dst],
            )
            .expect("conditional_class_ref returns a status");
        fx.fallible(st);
        return Ok(Operand::Slot {
            ss,
            owned: false,
            tag: TagInfo::Known(ValueTag::Class as u8),
        });
    }
    Ok(class_immediate(fx, cid))
}

/// A `RubyValue::Class(cid)` written into a fresh temp slot -- an
/// immediate, so unowned (no retain, nothing to release).
/// A Symbol value for `name`, interned by `zeo_unit_init`.
pub(crate) fn symbol_value(fx: &mut Fx, name: &str) -> Result<Operand, String> {
    let sym = fx.sym_id(name);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    fx.call("zeo_rt_sym_value", &[sym, out]);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(ValueTag::Symbol as u8),
    })
}

pub(crate) fn class_immediate(fx: &mut Fx, cid: crate::compiler::ClassId) -> Operand {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let fl = MemFlagsData::trusted();
    let z = fx.b.ins().iconst(types::I64, 0);
    for off in [0, 8, 16] {
        fx.b.ins().store(fl, z, dst, off);
    }
    let tag =
        fx.b.ins()
            .iconst(types::I8, i64::from(ValueTag::Class as u8));
    fx.b.ins().store(fl, tag, dst, 0);
    let cid_v = fx.b.ins().iconst(types::I32, i64::from(cid.0));
    fx.b.ins().store(fl, cid_v, dst, PAYLOAD_OFFSET as i32);
    Operand::Slot {
        ss,
        owned: false,
        tag: TagInfo::Class(cid.0),
    }
}

/// The operator set lowered inline -- the rustc emitter's
/// `INT_BINARY_OPS`/`FLOAT_BINARY_OPS` tables, which the boxed three-arm
/// shape below serves from ONE site each (rustc needs a statically known
/// operand pair; the tag test asks at run time instead).
#[derive(Clone, Copy, PartialEq, Eq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    BAnd,
    BOr,
    BXor,
    Shl,
    Shr,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Cmp,
}

/// How the Int arm shapes an operator over two `i64` payloads.
enum IntShape {
    /// Overflow-checked arithmetic; the overflow arm calls the named capi.
    Overflow(&'static str),
    /// Total bitwise arithmetic -- an `i64` pair's result is an `i64`.
    Bits,
    /// A comparison, answering `Bool`.
    Compare(IntCC),
    /// `<=>`, answering `Int` -1/0/1.
    Spaceship,
    /// Floored `/` or `%`: a zero divisor raises here, and the one
    /// overflowing pair (`i64::MIN op -1`) takes the named capi.
    Floored { modulo: bool, slow: &'static str },
    /// A fallible call on the boxed operands (bignum promotion lives in
    /// the runtime).
    Call(&'static str),
}

/// How the Float arm shapes an operator over two `f64` payloads.
enum FloatShape {
    /// Total arithmetic, answering `Float`.
    Arith,
    /// A comparison, answering `Bool`.
    Compare(FloatCC),
    /// `zeo_rt_float_mod_checked` / `float_pow_checked` -- fallible.
    Fallible(&'static str),
    /// `zeo_rt_float_cmp` -- total, but answers `nil` against a NaN.
    Spaceship,
    /// `Float` has no such operator; the arm is not emitted at all and a
    /// Float pair falls to the dynamic send, where the runtime raises.
    None,
}

impl BinOp {
    fn of(name: &str) -> Option<BinOp> {
        match name {
            "+" => Some(BinOp::Add),
            "-" => Some(BinOp::Sub),
            "*" => Some(BinOp::Mul),
            "/" => Some(BinOp::Div),
            "%" => Some(BinOp::Mod),
            "**" => Some(BinOp::Pow),
            "&" => Some(BinOp::BAnd),
            "|" => Some(BinOp::BOr),
            "^" => Some(BinOp::BXor),
            "<<" => Some(BinOp::Shl),
            ">>" => Some(BinOp::Shr),
            "<" => Some(BinOp::Lt),
            "<=" => Some(BinOp::Le),
            ">" => Some(BinOp::Gt),
            ">=" => Some(BinOp::Ge),
            "==" => Some(BinOp::Eq),
            "!=" => Some(BinOp::Ne),
            "<=>" => Some(BinOp::Cmp),
            _ => None,
        }
    }

    fn int_shape(self) -> IntShape {
        match self {
            BinOp::Add => IntShape::Overflow("zeo_rt_int_add_slow"),
            BinOp::Sub => IntShape::Overflow("zeo_rt_int_sub_slow"),
            BinOp::Mul => IntShape::Overflow("zeo_rt_int_mul_slow"),
            BinOp::Div => IntShape::Floored {
                modulo: false,
                slow: "zeo_rt_int_div",
            },
            BinOp::Mod => IntShape::Floored {
                modulo: true,
                slow: "zeo_rt_int_mod",
            },
            BinOp::Pow => IntShape::Call("zeo_rt_int_pow"),
            BinOp::BAnd | BinOp::BOr | BinOp::BXor => IntShape::Bits,
            BinOp::Shl => IntShape::Call("zeo_rt_int_shl"),
            BinOp::Shr => IntShape::Call("zeo_rt_int_shr"),
            BinOp::Lt => IntShape::Compare(IntCC::SignedLessThan),
            BinOp::Le => IntShape::Compare(IntCC::SignedLessThanOrEqual),
            BinOp::Gt => IntShape::Compare(IntCC::SignedGreaterThan),
            BinOp::Ge => IntShape::Compare(IntCC::SignedGreaterThanOrEqual),
            BinOp::Eq => IntShape::Compare(IntCC::Equal),
            BinOp::Ne => IntShape::Compare(IntCC::NotEqual),
            BinOp::Cmp => IntShape::Spaceship,
        }
    }

    fn float_shape(self) -> FloatShape {
        match self {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => FloatShape::Arith,
            BinOp::Mod => FloatShape::Fallible("zeo_rt_float_mod_checked"),
            BinOp::Pow => FloatShape::Fallible("zeo_rt_float_pow_checked"),
            // Real Ruby's `Float` has no bitwise or shift operators.
            BinOp::BAnd | BinOp::BOr | BinOp::BXor | BinOp::Shl | BinOp::Shr => FloatShape::None,
            BinOp::Lt => FloatShape::Compare(FloatCC::LessThan),
            BinOp::Le => FloatShape::Compare(FloatCC::LessThanOrEqual),
            BinOp::Gt => FloatShape::Compare(FloatCC::GreaterThan),
            BinOp::Ge => FloatShape::Compare(FloatCC::GreaterThanOrEqual),
            BinOp::Eq => FloatShape::Compare(FloatCC::Equal),
            BinOp::Ne => FloatShape::Compare(FloatCC::NotEqual),
            BinOp::Cmp => FloatShape::Spaceship,
        }
    }
}

/// `a op b`, the rustc emitter's exact three arms. Both operands are
/// materialized (owned ones handed to the pool -- the arms only borrow),
/// the result is a fresh owned slot.
fn binop(fx: &mut Fx, op: BinOp, name: &str, recv: NodeId, arg: NodeId) -> Result<Operand, String> {
    let a = lower_expr(fx, recv)?;
    // Park an owned lhs BEFORE the rhs lowers: the rhs may raise, and the
    // raise landing never sees an operand that is owned but unpooled.
    let a = if a.owned() {
        let pa = ownership::borrow_ptr(fx, &a);
        pool_operand(fx, &a, pa);
        Operand::Ptr {
            addr: pa,
            owned: false,
            tag: a.tag(),
        }
    } else {
        park_reassignable(fx, Some(recv), a, &[arg])
    };
    let b_op = lower_expr(fx, arg)?;
    boxed_binop(fx, op, name, a, b_op)
}

/// Whether `name`'s operator fast path may be taken. A user reopen that
/// redefines the operator on `Integer`'s or `Float`'s fast-path MRO has to
/// be honored at EVERY call site, so the whole fast path stands down and the
/// ordinary dynamic send finds the reopened row -- `analyze` recorded both
/// lanes for exactly this, and the rustc backend consults the same sets.
///
/// Both lanes gate the one decision because the boxed shape tests both tags:
/// a `Float#==` reopen leaves the Int arm sound, but the site cannot know
/// which arm it will take.
fn operator_fast_path(fx: &Fx, name: &str) -> bool {
    BinOp::of(name).is_some()
        && !fx.an.compiler.redefined_int_ops.contains(name)
        && !fx.an.compiler.redefined_float_ops.contains(name)
}

fn store_int(fx: &mut Fx, v: cranelift_codegen::ir::Value, dst: cranelift_codegen::ir::Value) {
    let fl = MemFlagsData::trusted();
    let tag = fx.b.ins().iconst(types::I8, i64::from(ValueTag::Int as u8));
    fx.b.ins().store(fl, tag, dst, 0);
    fx.b.ins().store(fl, v, dst, PAYLOAD_OFFSET as i32);
}

fn store_bool_tag(fx: &mut Fx, v: cranelift_codegen::ir::Value, dst: cranelift_codegen::ir::Value) {
    let fl = MemFlagsData::trusted();
    let tag =
        fx.b.ins()
            .iconst(types::I8, i64::from(ValueTag::Bool as u8));
    fx.b.ins().store(fl, tag, dst, 0);
    fx.b.ins().store(fl, v, dst, payload_off());
}

fn payload_off() -> i32 {
    PAYLOAD_OFFSET as i32
}

/// The Int arm: both operands carry the `Int` tag, so both payloads are
/// plain `i64`. Ends by jumping to `join`.
fn int_arm(
    fx: &mut Fx,
    op: BinOp,
    pa: cranelift_codegen::ir::Value,
    pb: cranelift_codegen::ir::Value,
    dst: cranelift_codegen::ir::Value,
    join: cranelift_codegen::ir::Block,
) {
    let fl = MemFlagsData::trusted();
    let payload = payload_off();
    match op.int_shape() {
        IntShape::Call(f) => {
            let status = fx
                .call(f, &[pa, pb, dst])
                .expect("a fallible op returns one");
            fx.fallible(status);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Compare(cc) => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            let c = fx.b.ins().icmp(cc, av, bv);
            store_bool_tag(fx, c, dst);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Spaceship => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            let gt = fx.b.ins().icmp(IntCC::SignedGreaterThan, av, bv);
            let lt = fx.b.ins().icmp(IntCC::SignedLessThan, av, bv);
            let gt = fx.b.ins().sextend(types::I64, gt);
            let lt = fx.b.ins().sextend(types::I64, lt);
            // Both are 0/1, so `gt - lt` is -1/0/1.
            let c = fx.b.ins().isub(gt, lt);
            store_int(fx, c, dst);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Bits => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            let v = match op {
                BinOp::BAnd => fx.b.ins().band(av, bv),
                BinOp::BOr => fx.b.ins().bor(av, bv),
                BinOp::BXor => fx.b.ins().bxor(av, bv),
                _ => unreachable!("only the bitwise operators take this shape"),
            };
            store_int(fx, v, dst);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Overflow(slow) => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            let (v, ovf) = match op {
                BinOp::Add => fx.b.ins().sadd_overflow(av, bv),
                BinOp::Sub => fx.b.ins().ssub_overflow(av, bv),
                BinOp::Mul => fx.b.ins().smul_overflow(av, bv),
                _ => unreachable!("only the checked arithmetic takes this shape"),
            };
            let slow_b = fx.b.create_block();
            let ok = fx.b.create_block();
            fx.b.ins().brif(ovf, slow_b, &[], ok, &[]);
            fx.b.switch_to_block(ok);
            store_int(fx, v, dst);
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(slow_b);
            fx.call(slow, &[pa, pb, dst]);
            fx.b.ins().jump(join, &[]);
        }
        IntShape::Floored { modulo, slow } => {
            let av = fx.b.ins().load(types::I64, fl, pa, payload);
            let bv = fx.b.ins().load(types::I64, fl, pb, payload);
            // A zero divisor raises; `i64::MIN op -1` is the one pair the
            // native instruction cannot answer, and the runtime promotes it.
            let zero = fx.b.ins().icmp_imm_s(IntCC::Equal, bv, 0);
            let raise = fx.b.create_block();
            let live = fx.b.create_block();
            fx.b.ins().brif(zero, raise, &[], live, &[]);

            fx.b.switch_to_block(raise);
            let cid =
                fx.b.ins()
                    .iconst(types::I32, i64::from(zeo_abi::ZERO_DIVISION_ERROR_CLASS.0));
            let (mptr, mlen) = rodata_name(fx, "divided by 0");
            fx.call("zeo_rt_raise_error", &[cid, mptr, mlen]);
            fx.b.ins().jump(fx.land, &[]);

            fx.b.switch_to_block(live);
            let min = fx.b.ins().icmp_imm_s(IntCC::Equal, av, i64::MIN);
            let neg1 = fx.b.ins().icmp_imm_s(IntCC::Equal, bv, -1);
            let overflows = fx.b.ins().band(min, neg1);
            let promote = fx.b.create_block();
            let native = fx.b.create_block();
            fx.b.ins().brif(overflows, promote, &[], native, &[]);

            fx.b.switch_to_block(promote);
            fx.call(slow, &[pa, pb, dst]);
            fx.b.ins().jump(join, &[]);

            fx.b.switch_to_block(native);
            // Ruby floors toward negative infinity, where the machine
            // truncates: the quotient loses one and the remainder gains a
            // divisor whenever the remainder's sign differs from `b`'s.
            let r = fx.b.ins().srem(av, bv);
            let r_nz = fx.b.ins().icmp_imm_s(IntCC::NotEqual, r, 0);
            let r_neg = fx.b.ins().icmp_imm_s(IntCC::SignedLessThan, r, 0);
            let b_neg = fx.b.ins().icmp_imm_s(IntCC::SignedLessThan, bv, 0);
            let signs_differ = fx.b.ins().bxor(r_neg, b_neg);
            let adjust = fx.b.ins().band(r_nz, signs_differ);
            let v = if modulo {
                let adjusted = fx.b.ins().iadd(r, bv);
                fx.b.ins().select(adjust, adjusted, r)
            } else {
                let q = fx.b.ins().sdiv(av, bv);
                let lowered = fx.b.ins().iadd_imm_s(q, -1);
                fx.b.ins().select(adjust, lowered, q)
            };
            store_int(fx, v, dst);
            fx.b.ins().jump(join, &[]);
        }
    }
}

/// Load one operand's payload as an `f64`: a Float reads its own bits, an
/// Int promotes -- ruby's numeric tower, where `1 + 2.0` runs the Float
/// operation on both sides.
fn as_f64(
    fx: &mut Fx,
    p: cranelift_codegen::ir::Value,
    is_int: bool,
) -> cranelift_codegen::ir::Value {
    let fl = MemFlagsData::trusted();
    let payload = payload_off();
    if is_int {
        let i = fx.b.ins().load(types::I64, fl, p, payload);
        fx.b.ins().fcvt_from_sint(types::F64, i)
    } else {
        fx.b.ins().load(types::F64, fl, p, payload)
    }
}

/// The Float arm, over the two already-promoted payloads. Ends by jumping
/// to `join`.
fn float_arm(
    fx: &mut Fx,
    op: BinOp,
    av: cranelift_codegen::ir::Value,
    bv: cranelift_codegen::ir::Value,
    dst: cranelift_codegen::ir::Value,
    join: cranelift_codegen::ir::Block,
) {
    let fl = MemFlagsData::trusted();
    let payload = payload_off();
    match op.float_shape() {
        FloatShape::None => unreachable!("an armless operator never reaches here"),
        FloatShape::Compare(cc) => {
            let c = fx.b.ins().fcmp(cc, av, bv);
            store_bool_tag(fx, c, dst);
        }
        FloatShape::Arith => {
            let v = match op {
                BinOp::Add => fx.b.ins().fadd(av, bv),
                BinOp::Sub => fx.b.ins().fsub(av, bv),
                BinOp::Mul => fx.b.ins().fmul(av, bv),
                BinOp::Div => fx.b.ins().fdiv(av, bv),
                _ => unreachable!("only the total arithmetic takes this shape"),
            };
            let tag =
                fx.b.ins()
                    .iconst(types::I8, i64::from(ValueTag::Float as u8));
            fx.b.ins().store(fl, tag, dst, 0);
            fx.b.ins().store(fl, v, dst, payload);
        }
        FloatShape::Spaceship => {
            fx.call("zeo_rt_float_cmp", &[av, bv, dst]);
        }
        FloatShape::Fallible(f) => {
            let status = fx
                .call(f, &[av, bv, dst])
                .expect("a fallible op returns one");
            fx.fallible(status);
        }
    }
    fx.b.ins().jump(join, &[]);
}

/// The general three-arm shape over boxed operands.
fn boxed_binop(
    fx: &mut Fx,
    op: BinOp,
    name: &str,
    a: Operand,
    b_op: Operand,
) -> Result<Operand, String> {
    let fl = MemFlagsData::trusted();
    // Owned operands hand ownership to the pool; every arm below only
    // borrows the bytes.
    let pa = ownership::borrow_ptr(fx, &a);
    if a.owned() {
        pool_operand(fx, &a, pa);
    }
    let pb = ownership::borrow_ptr(fx, &b_op);
    if b_op.owned() {
        pool_operand(fx, &b_op, pb);
    }
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);

    let b_int = fx.b.create_block();
    let not_int = fx.b.create_block();
    let b_dyn = fx.b.create_block();
    let join = fx.b.create_block();

    let ta = fx.b.ins().load(types::I8, fl, pa, 0);
    let tb = fx.b.ins().load(types::I8, fl, pb, 0);
    let int_tag = i64::from(ValueTag::Int as u8);
    let a_int = fx.b.ins().icmp_imm_u(IntCC::Equal, ta, int_tag);
    let b_int_p = fx.b.ins().icmp_imm_u(IntCC::Equal, tb, int_tag);
    let both_int = fx.b.ins().band(a_int, b_int_p);
    fx.b.ins().brif(both_int, b_int, &[], not_int, &[]);

    fx.b.switch_to_block(b_int);
    int_arm(fx, op, pa, pb, dst, join);

    fx.b.switch_to_block(not_int);
    if matches!(op.float_shape(), FloatShape::None) {
        fx.b.ins().jump(b_dyn, &[]);
    } else {
        // The three remaining numeric pairs, each its own arm: a MIXED pair
        // is ruby's numeric tower, not a coercion protocol call -- `1 + 2.0`
        // promotes the Int side and runs `Float#+`. Leaving them to the
        // dynamic send is what made `2 * zr` (an Int literal against a Float
        // local, in the middle of `bm_so_mandelbrot`'s inner loop) a full
        // dispatch per evaluation.
        let float_tag = i64::from(ValueTag::Float as u8);
        let a_f = fx.b.ins().icmp_imm_u(IntCC::Equal, ta, float_tag);
        let b_f = fx.b.ins().icmp_imm_u(IntCC::Equal, tb, float_tag);
        for (a_is_int, b_is_int) in [(false, false), (false, true), (true, false)] {
            let arm = fx.b.create_block();
            let next = fx.b.create_block();
            let a_ok = if a_is_int { a_int } else { a_f };
            let b_ok = if b_is_int { b_int_p } else { b_f };
            let both = fx.b.ins().band(a_ok, b_ok);
            fx.b.ins().brif(both, arm, &[], next, &[]);
            fx.b.switch_to_block(arm);
            let av = as_f64(fx, pa, a_is_int);
            let bv = as_f64(fx, pb, b_is_int);
            float_arm(fx, op, av, bv, dst, join);
            fx.b.switch_to_block(next);
        }
        fx.b.ins().jump(b_dyn, &[]);
    }

    fx.b.switch_to_block(b_dyn);
    {
        let sym = fx.sym_id(name);
        let zero = fx.b.ins().iconst(types::I32, 0);
        let one = fx.b.ins().iconst(fx.em.ptr, 1);
        let null = fx.b.ins().iconst(fx.em.ptr, 0);
        let status = fx
            .call("zeo_rt_send_value_in", &[zero, pa, sym, pb, one, null, dst])
            .expect("send returns a status");
        fx.fallible(status);
        fx.b.ins().jump(join, &[]);
    }

    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// Hand an owned operand's value to the pool, keeping `addr` borrowable.
fn pool_operand(fx: &mut Fx, op: &Operand, addr: cranelift_codegen::ir::Value) {
    ownership::pool_owned(fx, addr, op.tag());
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
) -> Result<Operand, String> {
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
        let site = fx.em.regexp_sites;
        fx.em.regexp_sites += 1;
        let site_v = fx.b.ins().iconst(types::I32, i64::from(site));
        let off = fx.em.intern_rodata(source.as_bytes());
        let ptr = fx.rod(off);
        let len_v = fx.b.ins().iconst(fx.em.ptr, source.len() as i64);
        let (ic, ext, ml, enc) = flag_vals(fx);
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let status = fx
            .call(
                "zeo_rt_regexp_lit",
                &[site_v, ptr, len_v, ic, ext, ml, enc, out],
            )
            .expect("regexp_lit returns a status");
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
                let status = fx
                    .call("zeo_rt_str_append_value", &[pat, p])
                    .expect("append_value returns a status");
                fx.fallible(status);
            }
        }
    }
    let (ic, ext, ml, enc) = flag_vals(fx);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call("zeo_rt_regexp_interp", &[pat, ic, ext, ml, enc, out])
        .expect("regexp_interp returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(ValueTag::Regexp as u8),
    })
}

/// `defined?(s)` -- a FRESH string answer (rustc's `string_new`) or nil.
fn defined_str(fx: &mut Fx, dst: cranelift_codegen::ir::Value, s: &str) {
    let off = fx.em.intern_rodata(s.as_bytes());
    let ptr = fx.rod(off);
    let len_v = fx.b.ins().iconst(fx.em.ptr, s.len() as i64);
    let enc = fx.b.ins().iconst(types::I8, ENC_UTF8);
    fx.call("zeo_rt_str_new", &[ptr, len_v, enc, dst]);
}

/// `defined?`'s runtime-conditional answer: `s` when `hit` (an i8) is
/// non-zero, else nil, in one owned temp.
fn defined_cond(fx: &mut Fx, hit: cranelift_codegen::ir::Value, s: &str) -> Operand {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let yes = fx.b.create_block();
    let no = fx.b.create_block();
    let merge = fx.b.create_block();
    fx.b.ins().brif(hit, yes, &[], no, &[]);
    fx.b.switch_to_block(yes);
    defined_str(fx, dst, s);
    fx.b.ins().jump(merge, &[]);
    fx.b.switch_to_block(no);
    ownership::write_move_into(fx, &Operand::Nil, dst);
    fx.b.ins().jump(merge, &[]);
    fx.b.switch_to_block(merge);
    fx.owned_created += 1;
    Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }
}

/// `"expression"` when every one of `nodes` is itself defined, else nil --
/// what a collection literal answers, CRuby recursing into its elements.
/// Each element's own `defined?` is lowered and the answers are ANDed; an
/// empty literal is defined outright.
fn defined_all_or_nil(fx: &mut Fx, site: NodeId, nodes: &[NodeId]) -> Result<Operand, String> {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let yes = fx.b.create_block();
    let no = fx.b.create_block();
    let merge = fx.b.create_block();
    for &n in nodes {
        let op = lower_defined(fx, site, n)?;
        let p = ownership::borrow_ptr(fx, &op);
        if op.owned() {
            ownership::pool_owned(fx, p, op.tag());
        }
        let fl = cranelift_codegen::ir::MemFlagsData::trusted();
        let tag = fx.b.ins().load(types::I8, fl, p, 0);
        let defined = fx.b.ins().icmp_imm_u(
            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
            tag,
            i64::from(ValueTag::Nil as u8),
        );
        let next = fx.b.create_block();
        fx.b.ins().brif(defined, next, &[], no, &[]);
        fx.b.switch_to_block(next);
    }
    fx.b.ins().jump(yes, &[]);
    fx.b.switch_to_block(yes);
    defined_str(fx, dst, "expression");
    fx.b.ins().jump(merge, &[]);
    fx.b.switch_to_block(no);
    ownership::write_move_into(fx, &Operand::Nil, dst);
    fx.b.ins().jump(merge, &[]);
    fx.b.switch_to_block(merge);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The static half of `defined_cond`.
fn defined_static(fx: &mut Fx, s: Option<&str>) -> Operand {
    match s {
        None => Operand::Nil,
        Some(s) => {
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            defined_str(fx, dst, s);
            fx.owned_created += 1;
            Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Known(ValueTag::Str as u8),
            }
        }
    }
}

/// `is_predefined_global`'s list, verbatim from the rustc emitter.
fn is_predefined_global(name: &str) -> bool {
    matches!(
        name,
        "$!" | "$@"
            | "$;"
            | "$,"
            | "$/"
            | "$\\"
            | "$."
            | "$<"
            | "$>"
            | "$_"
            | "$0"
            | "$*"
            | "$:"
            | "$\""
            | "$$"
            | "$?"
            | "$DEBUG"
            | "$VERBOSE"
            | "$FILENAME"
            | "$PROGRAM_NAME"
            | "$stdin"
            | "$stdout"
            | "$stderr"
            | "$LOAD_PATH"
            | "$LOADED_FEATURES"
    )
}

/// `defined?(expr)` -- rustc's `emit_defined`, branch for branch. The
/// runtime-probing forms call one capi each; everything else classifies
/// statically. The collection-literal recursion and dynamic-scope const
/// forms still refuse.
fn lower_defined(fx: &mut Fx, site: NodeId, inner: NodeId) -> Result<Operand, String> {
    // `defined?(yield)`: runtime -- the block channel is or isn't there.
    if matches!(&fx.an.compiler.hir[inner], HirNode::Yield(_)) {
        // A snippet's own level has no channel; the one it means is the
        // enclosing method's, published for the call.
        if fx.blk_ptr.is_none() && fx.eval_mode.is_some() {
            let hit = fx
                .call("zeo_rt_eval_block_given", &[])
                .expect("eval_block_given answers");
            return Ok(defined_cond(fx, hit, "yield"));
        }
        return Ok(match fx.blk_ptr {
            None => Operand::Nil,
            Some(blk) => {
                let hit = fx.b.ins().icmp_imm_u(
                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                    blk,
                    0,
                );
                defined_cond(fx, hit, "yield")
            }
        });
    }
    // `defined?(super)`: probe the same walk `super` runs.
    if matches!(&fx.an.compiler.hir[inner], HirNode::SuperCall { .. })
        && !fx.self_is_class
        && let (Some(dc), Some(m)) = (fx.defining_class, fx.method_name.clone())
    {
        let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
        let dc_v = fx.b.ins().iconst(types::I32, i64::from(dc.0));
        let sym = fx.sym_id(&m);
        let hit = fx
            .call("zeo_rt_super_defined", &[self_ptr, dc_v, sym])
            .expect("super_defined answers");
        return Ok(defined_cond(fx, hit, "super"));
    }
    // The same probe at a SNIPPET's own level, where the target is the
    // enclosing method's rather than this scope's.
    if matches!(&fx.an.compiler.hir[inner], HirNode::SuperCall { .. }) && fx.eval_mode.is_some() {
        let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
        let hit = fx
            .call("zeo_rt_eval_super_defined", &[self_ptr])
            .expect("eval_super_defined answers");
        return Ok(defined_cond(fx, hit, "super"));
    }
    // Inside a RUN-TIME eval a bare name that IS one of the caller's
    // locals could only arrive as a vcall -- prism parsed the source
    // alone. `defined?` has to call it what ruby calls it, or a name the
    // caller holds reports as an undefined method.
    //
    // The compile-time SPLICE is deliberately not included: it shares the
    // enclosing scope's storage outright, so a name an EARLIER splice
    // introduced is still hoisted there, and ruby -- whose eval locals
    // die with the call -- answers nil for it.
    if fx.eval_mode.is_some()
        && fx
            .an
            .compiler
            .hir
            .has_flag(inner, crate::hir::NodeFlag::VCALL)
        && let HirNode::Call {
            receiver: None,
            name,
            ..
        } = &fx.an.compiler.hir[inner]
        && fx.locals.contains_key(name)
    {
        return Ok(defined_static(fx, Some("local-variable")));
    }
    // `defined?(a_call)`: evaluate the receiver (its raise SWALLOWED to
    // nil -- CRuby's catch entry over the whole expression) and probe it.
    if let HirNode::Call { receiver, name, .. } = &fx.an.compiler.hir[inner] {
        let (receiver, name) = (*receiver, name.clone());
        let ss = fx.temp_slot();
        let dst = fx.slot_addr(ss, 0);
        let hit_ss = fx.temp_slot();
        let hit_ptr = fx.slot_addr(hit_ss, 0);
        let sym = fx.sym_id(&name);
        let merge = fx.b.create_block();
        let check = fx.b.create_block();
        match receiver {
            None => {
                let self_ptr = super::stmt::dyn_ivar_recv(fx);
                let one = fx.b.ins().iconst(types::I8, 1);
                fx.call("zeo_rt_defined_method", &[self_ptr, sym, one, hit_ptr]);
                fx.b.ins().jump(check, &[]);
            }
            Some(rid) => {
                let swallow = fx.b.create_block();
                let saved = fx.land;
                fx.land = swallow;
                let op = lower_expr(fx, rid)?;
                fx.land = saved;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let zero = fx.b.ins().iconst(types::I8, 0);
                fx.call("zeo_rt_defined_method", &[p, sym, zero, hit_ptr]);
                fx.b.ins().jump(check, &[]);
                // The swallow landing: drop the pending signal (rustc's
                // `unwrap_or(Nil)` drops the Err) and answer nil.
                fx.b.switch_to_block(swallow);
                let sig_ss = fx.temp_slot();
                let sig_dst = fx.slot_addr(sig_ss, 0);
                ownership::write_move_into(fx, &Operand::Nil, sig_dst);
                fx.call("zeo_rt_signal_take", &[sig_dst]);
                fx.owned_created += 1;
                ownership::pool_owned(fx, sig_dst, TagInfo::Unknown);
                ownership::write_move_into(fx, &Operand::Nil, dst);
                fx.b.ins().jump(merge, &[]);
            }
        }
        fx.b.switch_to_block(check);
        let fl = cranelift_codegen::ir::MemFlagsData::trusted();
        let hit = fx.b.ins().load(types::I8, fl, hit_ptr, 0);
        let yes = fx.b.create_block();
        let no = fx.b.create_block();
        fx.b.ins().brif(hit, yes, &[], no, &[]);
        fx.b.switch_to_block(yes);
        defined_str(fx, dst, "method");
        fx.b.ins().jump(merge, &[]);
        fx.b.switch_to_block(no);
        ownership::write_move_into(fx, &Operand::Nil, dst);
        fx.b.ins().jump(merge, &[]);
        fx.b.switch_to_block(merge);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        });
    }
    // `defined?(Scope::NAME)` with a compile-time scope but non-static
    // membership: privacy then membership, both runtime probes.
    if let HirNode::QualifiedConstRead(scope, name) = &fx.an.compiler.hir[inner] {
        let (scope, name) = (scope.clone(), name.clone());
        let env = crate::analyze::constfold::ConstEnv {
            compiler: &fx.an.compiler,
            defining_class: fx.defining_class.or(fx.method_class),
            box_id: 0,
        };
        if crate::analyze::constfold::const_form_resolves(&env, inner) != Some(true) {
            let Some(scope_id) = resolve_class_here(fx, &scope) else {
                // A scope only the run time can name (`Scoped = Module.new`)
                // -- or one nothing ever defines, where reading it raises
                // and the swallow answers nil, as ruby's does.
                return defined_const_under_runtime_scope(fx, &name, |fx| {
                    const_path_read(fx, inner, &scope)
                });
            };
            let sid = fx.b.ins().iconst(types::I32, i64::from(scope_id.0));
            let (nptr, nlen) = rodata_name(fx, &name);
            let private = fx
                .call("zeo_rt_const_private", &[sid, nptr, nlen])
                .expect("const_private answers");
            let hidden = fx.b.create_block();
            let probe = fx.b.create_block();
            let merge = fx.b.create_block();
            let ss = fx.temp_slot();
            let dst = fx.slot_addr(ss, 0);
            fx.b.ins().brif(private, hidden, &[], probe, &[]);
            fx.b.switch_to_block(hidden);
            ownership::write_move_into(fx, &Operand::Nil, dst);
            fx.b.ins().jump(merge, &[]);
            fx.b.switch_to_block(probe);
            let sid2 = fx.b.ins().iconst(types::I32, i64::from(scope_id.0));
            let (nptr2, nlen2) = rodata_name(fx, &name);
            let hit = fx
                .call("zeo_rt_defined_const_in", &[sid2, nptr2, nlen2])
                .expect("defined_const_in answers");
            let yes = fx.b.create_block();
            let no = fx.b.create_block();
            fx.b.ins().brif(hit, yes, &[], no, &[]);
            fx.b.switch_to_block(yes);
            defined_str(fx, dst, "constant");
            fx.b.ins().jump(merge, &[]);
            fx.b.switch_to_block(no);
            ownership::write_move_into(fx, &Operand::Nil, dst);
            fx.b.ins().jump(merge, &[]);
            fx.b.switch_to_block(merge);
            fx.owned_created += 1;
            return Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            });
        }
        return Ok(defined_static(fx, Some("constant")));
    }
    // `defined?(obj::NAME)`: only the run time can answer. `"constant"`
    // when the scope operator finds the name, nil otherwise -- including
    // when the scope is not a module at all, and including a raise from
    // the scope expression itself (CRuby's catch entry over the whole
    // form swallows it).
    if let HirNode::DynConstRead { scope, name, .. } = &fx.an.compiler.hir[inner] {
        let (scope, name) = (*scope, name.clone());
        return defined_const_under_runtime_scope(fx, &name, |fx| lower_expr(fx, scope));
    }
    defined_rest(fx, site, inner)
}

/// `defined?` of a constant under a scope only the run time settles
/// (`obj::NAME`, or `Scope::NAME` whose scope this compile cannot name).
/// The scope is evaluated under a SWALLOW landing -- CRuby's catch entry
/// over the whole form eats a raise from it -- and then the scope
/// operator's own search answers `"constant"` or nil, nil too when the
/// scope turns out to be no module at all.
fn defined_const_under_runtime_scope(
    fx: &mut Fx,
    name: &str,
    scope: impl FnOnce(&mut Fx) -> Result<Operand, String>,
) -> Result<Operand, String> {
    {
        let ss = fx.temp_slot();
        let dst = fx.slot_addr(ss, 0);
        let swallow = fx.b.create_block();
        let merge = fx.b.create_block();
        let saved = fx.land;
        fx.land = swallow;
        let op = scope(fx)?;
        fx.land = saved;
        let p = ownership::borrow_ptr(fx, &op);
        if op.owned() {
            ownership::pool_owned(fx, p, op.tag());
        }
        let (nptr, nlen) = rodata_name(fx, name);
        let hit = fx
            .call("zeo_rt_scope_const_defined", &[p, nptr, nlen])
            .expect("scope_const_defined answers");
        let yes = fx.b.create_block();
        let no = fx.b.create_block();
        fx.b.ins().brif(hit, yes, &[], no, &[]);
        fx.b.switch_to_block(yes);
        defined_str(fx, dst, "constant");
        fx.b.ins().jump(merge, &[]);
        fx.b.switch_to_block(no);
        ownership::write_move_into(fx, &Operand::Nil, dst);
        fx.b.ins().jump(merge, &[]);
        // The swallow landing: drop the pending signal and answer nil.
        fx.b.switch_to_block(swallow);
        let sig_ss = fx.temp_slot();
        let sig_dst = fx.slot_addr(sig_ss, 0);
        ownership::write_move_into(fx, &Operand::Nil, sig_dst);
        fx.call("zeo_rt_signal_take", &[sig_dst]);
        fx.owned_created += 1;
        ownership::pool_owned(fx, sig_dst, TagInfo::Unknown);
        ownership::write_move_into(fx, &Operand::Nil, dst);
        fx.b.ins().jump(merge, &[]);
        fx.b.switch_to_block(merge);
        fx.owned_created += 1;
        Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        })
    }
}

/// The rest of [`lower_defined`]'s classification chain, split off only so
/// that neither half runs to a thousand lines.
fn defined_rest(fx: &mut Fx, site: NodeId, inner: NodeId) -> Result<Operand, String> {
    use crate::hir::LastMatch;
    if let HirNode::GlobalRead(name) = &fx.an.compiler.hir[inner] {
        let name = name.clone();
        if is_predefined_global(&name) {
            return Ok(defined_static(fx, Some("global-variable")));
        }
        let bx = fx.box_v();
        let (nptr, nlen) = rodata_name(fx, &name);
        let hit = fx
            .call("zeo_rt_defined_gvar", &[bx, nptr, nlen])
            .expect("defined_gvar answers");
        return Ok(defined_cond(fx, hit, "global-variable"));
    }
    if let HirNode::LastMatchRef(which) = &fx.an.compiler.hir[inner] {
        let which = *which;
        if matches!(which, LastMatch::Data) {
            return Ok(defined_static(fx, Some("global-variable")));
        }
        let (kind, n) = match which {
            LastMatch::Data => unreachable!("returned above"),
            LastMatch::Group(n) => (1i64, n),
            LastMatch::Pre => (2, 0),
            LastMatch::Post => (3, 0),
            LastMatch::LastGroup => (4, 0),
        };
        let kind_v = fx.b.ins().iconst(types::I8, kind);
        let n_v = fx.b.ins().iconst(fx.em.ptr, n as i64);
        let mss = fx.temp_slot();
        let mout = fx.slot_addr(mss, 0);
        fx.call("zeo_rt_last_match_ref", &[kind_v, n_v, mout]);
        fx.owned_created += 1;
        ownership::pool_owned(fx, mout, TagInfo::Unknown);
        let fl = cranelift_codegen::ir::MemFlagsData::trusted();
        let tag = fx.b.ins().load(types::I8, fl, mout, 0);
        return Ok(defined_cond(fx, tag, "global-variable"));
    }
    // An array/hash literal answers "expression" only if EVERY element is
    // itself defined -- CRuby recurses (`defined?([Missing, Array])` is nil,
    // `defined?([1, Array])` is "expression"). An empty literal is defined.
    if let HirNode::ArrayLit(elems) = &fx.an.compiler.hir[inner] {
        let nodes: Vec<NodeId> = elems
            .iter()
            .map(|e| match e {
                crate::hir::ArrayElem::Single(n) | crate::hir::ArrayElem::Splat(n) => *n,
            })
            .collect();
        return defined_all_or_nil(fx, site, &nodes);
    }
    if let HirNode::HashLit(entries) = &fx.an.compiler.hir[inner] {
        let mut nodes = Vec::new();
        for e in entries {
            match e {
                crate::hir::KwArg::Pair(k, v) => nodes.extend([*k, *v]),
                crate::hir::KwArg::DoubleSplat(n) => nodes.push(*n),
            }
        }
        return defined_all_or_nil(fx, site, &nodes);
    }
    if let HirNode::IvarRead(name) = &fx.an.compiler.hir[inner] {
        let name = name.clone();
        let self_ptr = super::stmt::dyn_ivar_recv(fx);
        let (nptr, nlen) = rodata_name(fx, &name);
        let hit_ss = fx.temp_slot();
        let hit_ptr = fx.slot_addr(hit_ss, 0);
        let status = fx
            .call("zeo_rt_defined_ivar", &[self_ptr, nptr, nlen, hit_ptr])
            .expect("defined_ivar returns a status");
        fx.fallible(status);
        let fl = cranelift_codegen::ir::MemFlagsData::trusted();
        let hit = fx.b.ins().load(types::I8, fl, hit_ptr, 0);
        return Ok(defined_cond(fx, hit, "instance-variable"));
    }
    if let HirNode::ClassVarRead(name) = &fx.an.compiler.hir[inner] {
        let name = name.clone();
        let owner = cvar_owner(fx, &name);
        let ov = fx.b.ins().iconst(types::I32, i64::from(owner));
        let (nptr, nlen) = rodata_name(fx, &name);
        let hit = fx
            .call("zeo_rt_defined_cvar", &[ov, nptr, nlen])
            .expect("defined_cvar answers");
        return Ok(defined_cond(fx, hit, "class variable"));
    }
    // A bare constant naming a RUNTIME-CONDITIONAL class: registered but
    // not PROMISED, so whether it exists is settled by the guarded body
    // having run. Foldable in neither direction -- probe its owner.
    if let HirNode::ClassRef(name) = &fx.an.compiler.hir[inner] {
        let name = name.clone();
        let env = crate::analyze::constfold::ConstEnv {
            compiler: &fx.an.compiler,
            defining_class: fx.defining_class.or(fx.method_class),
            box_id: 0,
        };
        if crate::analyze::constfold::const_form_resolves(&env, inner).is_none()
            && let Some(cid) = resolve_class_here(fx, &name)
            && fx.an.compiler.class(cid).runtime_conditional
        {
            let owner = fx
                .an
                .compiler
                .class(cid)
                .lexical_parent
                .unwrap_or(crate::compiler::OBJECT_CLASS);
            let leaf = fx.an.compiler.leaf_name(cid).to_string();
            let sid = fx.b.ins().iconst(types::I32, i64::from(owner.0));
            let (nptr, nlen) = rodata_name(fx, &leaf);
            let hit = fx
                .call("zeo_rt_defined_const_in", &[sid, nptr, nlen])
                .expect("defined_const_in answers");
            return Ok(defined_cond(fx, hit, "constant"));
        }
    }
    // The static classification tail -- rustc's, in its order.
    let classification: Option<&str> = match &fx.an.compiler.hir[inner] {
        HirNode::LocalRead(name) => fx.locals.contains_key(name).then_some("local-variable"),
        HirNode::ClassRef(_) => {
            let env = crate::analyze::constfold::ConstEnv {
                compiler: &fx.an.compiler,
                defining_class: fx.defining_class.or(fx.method_class),
                box_id: 0,
            };
            match crate::analyze::constfold::const_form_resolves(&env, inner) {
                Some(true) => Some("constant"),
                _ => None,
            }
        }
        HirNode::SelfRef => Some("self"),
        HirNode::New { .. }
        | HirNode::SuperCall { .. }
        | HirNode::BlockGiven
        | HirNode::Raise(..) => Some("method"),
        HirNode::NilLit => Some("nil"),
        HirNode::BoolLit(true) => Some("true"),
        HirNode::BoolLit(false) => Some("false"),
        HirNode::IntegerLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::StringLit(_)
        | HirNode::RegexpLit(..)
        | HirNode::RangeLit { .. }
        | HirNode::And(..)
        | HirNode::Or(..)
        | HirNode::Defined(_)
        | HirNode::If { .. }
        | HirNode::CaseWhen { .. }
        | HirNode::CaseIn { .. }
        | HirNode::MatchPredicate { .. }
        | HirNode::MatchRequired { .. }
        | HirNode::Begin { .. }
        | HirNode::Seq(_)
        | HirNode::While { .. }
        | HirNode::Loop { .. }
        | HirNode::Lambda { .. } => Some("expression"),
        HirNode::LocalWrite(..)
        | HirNode::IvarWrite(..)
        | HirNode::ClassVarWrite(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ConstWrite { .. }
        | HirNode::MultiWrite { .. } => Some("assignment"),
        HirNode::Break(_) | HirNode::Next(_) | HirNode::Redo | HirNode::Return(_) => None,
        _ => {
            return fx.unsupported(site, "this `defined?` form");
        }
    };
    Ok(defined_static(fx, classification))
}

/// The class that OWNS `@@name` at this lowering site -- the rustc
/// emitter's `cvar_owner_id` rule: the lexically enclosing class (`Object`
/// at the toplevel), looked through a `class << self` surrogate, then
/// resolved through the analyzer's `cvar_owners` claim map (a subclass
/// writing a parent-declared cvar stores on the parent).
/// `owner.const_added(:name)` -- ruby announces a constant the moment it
/// becomes readable. Emits nothing unless the owner's chain answers the
/// hook by this point in the file (`Module`'s own default is a no-op), so
/// a program without one is unchanged.
pub(crate) fn const_added_send(
    fx: &mut Fx,
    owner: u32,
    name: &str,
    at: Option<NodeId>,
) -> Result<(), String> {
    // A snippet's owner may be a RUN-TIME class the fresh compiler has no
    // entry for at all -- the announcement is unconditional there, and the
    // runtime's own dispatch decides whether a hook answers it.
    if fx.eval_cref.is_some() && owner as usize >= fx.an.compiler.classes.len() {
        return const_added_announce(fx, owner, name);
    }
    // A `class Module; def const_added` reopen answers for every module,
    // and no per-class scan can see it -- `Compiler::global_def_hooks`.
    if !fx.an.compiler.global_def_hooks.contains("const_added") {
        let Some((_, hook)) = fx
            .an
            .compiler
            .class_method_in_chain(zeo_abi::ClassId(owner), "const_added")
        else {
            return Ok(());
        };
        if !crate::analyze::def_hooks::hook_installed_before(&fx.an.compiler, hook, at) {
            return Ok(());
        }
    }
    const_added_announce(fx, owner, name)
}

/// [`const_added_send`] with the hook check already made by the caller.
pub(crate) fn const_added_announce(fx: &mut Fx, owner: u32, name: &str) -> Result<(), String> {
    let recv = class_immediate(fx, crate::compiler::ClassId(owner));
    let recv_ptr = ownership::borrow_ptr(fx, &recv);
    let arg = symbol_value(fx, name)?;
    let argv = ownership::borrow_ptr(fx, &arg);
    if arg.owned() {
        ownership::pool_owned(fx, argv, arg.tag());
    }
    let sym = fx.sym_id("const_added");
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let zero_box = fx.box_v();
    let argc = fx.b.ins().iconst(fx.em.ptr, 1);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let status = fx
        .call(
            "zeo_rt_send_value_in",
            &[zero_box, recv_ptr, sym, argv, argc, null, out],
        )
        .expect("send returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    ownership::discard(
        fx,
        Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        },
    );
    Ok(())
}

pub(crate) fn cvar_owner(fx: &Fx, name: &str) -> u32 {
    // A snippet's cvar belongs to its cref, which is a RUN-TIME class the
    // fresh compiler has no entry for: no owner walk to do, and the
    // runtime's own `cvar_get`/`set` climb the live ancestry from there.
    if let Some(cid) = fx.eval_cref.as_ref().and_then(|c| c.chain.first().copied()) {
        return cid;
    }
    // Where the code was WRITTEN, never the receiver that reaches it: a
    // class method inherited by a subclass still reads its own class's
    // storage (`Sub.note` writes `Base`'s `@@subs`), so a materialized
    // copy's `defining_class` -- not `method_class` -- is the question.
    let defining = fx
        .defining_class
        .or(fx.method_class)
        .unwrap_or(crate::compiler::OBJECT_CLASS);
    let defining = if fx.an.compiler.is_singleton_surrogate(defining) {
        fx.an
            .compiler
            .class(defining)
            .lexical_parent
            .unwrap_or(defining)
    } else {
        defining
    };
    fx.an
        .compiler
        .class(defining)
        .cvar_owners
        .get(name)
        .copied()
        .unwrap_or(defining)
        .0
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
) -> Result<Operand, String> {
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
                    let status = fx
                        .call("zeo_rt_case_eq", &[p, s, hit_ptr])
                        .expect("case_eq returns a status");
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
                    let status = fx
                        .call("zeo_rt_case_eq_any", &[p, s, hit_ptr])
                        .expect("case_eq_any returns a status");
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
pub(crate) fn binding_value(fx: &mut Fx, site: NodeId) -> Result<Option<Operand>, String> {
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

/// `Module.nesting` -- the lexical class/module chain at THIS call site,
/// innermost first. It is compile-time knowledge and nothing else: a builtin
/// row runs with no view of its caller's lexical scope, so folding here is
/// the only way to answer anything but `[]` (rustc folds it the same way).
/// `cref_chain` is outermost-first.
fn module_nesting(
    fx: &mut Fx,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
) -> Result<Option<Operand>, String> {
    if name != "nesting" || !args.is_empty() {
        return Ok(None);
    }
    let Some(r) = receiver else { return Ok(None) };
    if !matches!(&fx.an.compiler.hir[r], HirNode::ClassRef(n) if n == "Module") {
        return Ok(None);
    }
    let chain: Vec<crate::compiler::ClassId> = cref_chain(fx).iter().rev().copied().collect();
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let cap = fx.b.ins().iconst(fx.em.ptr, chain.len() as i64);
    fx.call("zeo_rt_array_new", &[cap, out]);
    fx.owned_created += 1;
    for cid in chain {
        let op = class_immediate(fx, cid);
        let p = ownership::move_ptr(fx, &op);
        fx.call("zeo_rt_array_push", &[out, p]);
    }
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(zeo_abi::abi::ValueTag::Array as u8),
    }))
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
) -> Result<Operand, String> {
    use cranelift_codegen::ir::types;
    // A `def`'s frame is labeled after the METHOD it creates; a
    // `define_method` body is genuinely the block ruby labels it as.
    let frame = if is_def {
        super::blocks::FrameName::Method(match fx.method_class {
            Some(c) if c != crate::compiler::OBJECT_CLASS => {
                format!("{}#{name}", fx.an.compiler.fq_name(c))
            }
            _ => format!("Object#{name}"),
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
        let status = fx
            .call(
                "zeo_rt_eval_define",
                &[mode_v, self_ptr, sym, proc_addr, vis, out],
            )
            .expect("eval_define returns a status");
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
        fx.b.ins().store(fl, tag, cref, 0);
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
    let status = fx
        .call(
            "zeo_rt_define_in_default_definee",
            &[cref, self_ptr, sym, proc_addr, private, body_vis, out],
        )
        .expect("define_in_default_definee returns a status");
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
fn ractor_new(fx: &mut Fx, id: NodeId) -> Result<Option<Operand>, String> {
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
    if resolve_class_here(fx, target) != Some(zeo_abi::RACTOR_CLASS) {
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
    let status = fx
        .call(
            "zeo_rt_ractor_new",
            &[blk, argv, argc, name_ptr, loc_ptr, loc_len, out],
        )
        .expect("ractor_new returns a status");
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
            resolve_class_here(fx, target) == Some(zeo_abi::RACTOR_CLASS)
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
/// The string-LITERAL form never reaches here: it lowered to
/// `HirNode::Eval`, an inline splice, at lower time.
fn runtime_eval(
    fx: &mut Fx,
    id: NodeId,
    receiver: Option<NodeId>,
    name: &str,
    args: &[crate::hir::ArrayElem],
) -> Result<Option<Operand>, String> {
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
        let status = fx
            .call(
                "zeo_rt_eval_value_in_scope_argv",
                &[args_ptr, scope_ptr, out],
            )
            .expect("eval_value_in_scope_argv returns a status");
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
    let status = fx
        .call(
            "zeo_rt_eval_value_in_scope",
            &[ptrs[0], scope_ptr, ptrs[1], ptrs[2], ptrs[3], out],
        )
        .expect("eval_value_in_scope returns a status");
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
/// its block is the corpus shape; the rustc backend gets the same fold
/// wherever it can type a receiver as a Proc.
fn block_param_call(fx: &mut Fx, id: NodeId) -> Result<Option<Operand>, String> {
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
    let status = fx
        .call(
            "zeo_rt_proc_call_or_send",
            &[recv_ptr, sym, argv, argc, null, caller, out],
        )
        .expect("proc_call_or_send returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// `Scope::NAME = v` where `Scope` is no compile-time class -- see
/// [`runtime_scope_const_read`], whose scope half this shares. The write
/// goes through the scope VALUE, so a runtime-minted namespace binds its
/// nested name exactly where ruby does.
fn runtime_scope_const_write(
    fx: &mut Fx,
    id: NodeId,
    scope: &str,
    name: &str,
    value: NodeId,
) -> Result<Operand, String> {
    let (head, leaf) = crate::hir::split_const_path(scope);
    let leaf = leaf.to_string();
    let scope_op = match head.filter(|h| !h.is_empty()) {
        Some(h) => {
            let h = h.to_string();
            scoped_const_read(fx, id, &h, &leaf)?
        }
        None => const_read(fx, id, &leaf)?,
    };
    let sptr = ownership::borrow_ptr(fx, &scope_op);
    if scope_op.owned() {
        ownership::pool_owned(fx, sptr, scope_op.tag());
    }
    let op = lower_expr(fx, value)?;
    let vptr = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        ownership::pool_owned(fx, vptr, op.tag());
    }
    let (nptr, nlen) = rodata_name(fx, name);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call("zeo_rt_scope_const_set", &[sptr, nptr, nlen, vptr, out])
        .expect("scope_const_set returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}
