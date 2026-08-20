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

#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: the refusal arm IS the default -- an unlisted node kind must refuse loudly, which is exactly what a new HirNode should do here until its lowering lands"
)]
pub(crate) fn lower_expr(fx: &mut Fx, id: NodeId) -> Result<Operand, String> {
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
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
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
            let sym = fx.sym_id(&name);
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
                // `define_method(:m) { .. }`'s body proc, built as a method
                // body (its bare `yield` reaches the installed method's
                // call-site block).
                let label = format!("block in {}", fx.frame_label);
                let ss = super::blocks::build_method_body(
                    fx,
                    id,
                    &params,
                    &body,
                    &label,
                    super::blocks::MethodBody::DefineMethod,
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
            let owner_class = match scope.as_deref() {
                Some(s) => match resolve_class_here(fx, s) {
                    Some(cid) => cid,
                    None => return Ok(Operand::Nil),
                },
                None => fx.method_class.unwrap_or(crate::compiler::OBJECT_CLASS),
            };
            let owner = fx
                .an
                .compiler
                .class(owner_class)
                .const_owners
                .get(&name)
                .copied()
                .unwrap_or(owner_class)
                .0;
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
            // `yield(*a)`: the length is a runtime question, so the list is
            // built as an Array and the block binds from its contents
            // (rustc's `__args` vector twin).
            if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
                let arr = super::call::build_array(fx, &args)?;
                let blk = match fx.blk_ptr {
                    Some(b) => b,
                    None => fx.b.ins().iconst(fx.em.ptr, 0),
                };
                let ss = fx.temp_slot();
                let out = fx.slot_addr(ss, 0);
                let status = fx
                    .call("zeo_rt_yield_args", &[blk, arr, out])
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
            let status = fx
                .call("zeo_rt_yield", &[blk, argv_ptr, argc_v, out])
                .expect("yield returns a status");
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
                    super::call::dynamic_send_value(fx, id, borrowed(), "initialize", &elems)?
                } else {
                    super::call::kw_send(
                        fx,
                        id,
                        Some(borrowed()),
                        "initialize",
                        &elems,
                        &kwargs,
                        None,
                    )?
                };
                ownership::discard(fx, init);
                return Ok(borrowed());
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
                    super::call::kw_send(fx, id, Some(recv), "new", &elems, &kwargs, Some(bp))
                }
                (None, true) => super::call::dynamic_send_value(fx, id, recv, "new", &elems),
                (None, false) => {
                    super::call::kw_send(fx, id, Some(recv), "new", &elems, &kwargs, None)
                }
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
            let res = if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
                super::call::splat_send(fx, id, Some(recv_op), &name, &args, &kwargs, blk)?
            } else if !kwargs.is_empty() {
                super::call::kw_send(fx, id, Some(recv_op), &name, &args, &kwargs, blk)?
            } else if let Some(bp) = blk {
                super::blocks::send_with_block_ptr_ops(fx, id, Some(recv_op), &name, &args, bp)?
            } else {
                super::call::dynamic_send_value(fx, id, recv_op, &name, &args)?
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
            let (receiver, name, args, blk) = (*receiver, name.clone(), args.clone(), *blk);
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
                return super::call::splat_send(fx, id, recv, &name, &args, &[], Some(bp));
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
            let (receiver, name, args) = (*receiver, name.clone(), args.clone());
            if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
                let recv = match receiver {
                    Some(r) => Some(lower_expr(fx, r)?),
                    None => None,
                };
                return super::call::splat_send(fx, id, recv, &name, &args, &[], None);
            }
            match receiver {
                Some(recv) if BinOp::of(&name).is_some() && args.len() == 1 => {
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
            let (receiver, name, args, ba) = (*receiver, name.clone(), args.clone(), *ba);
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
                *receiver,
                name.clone(),
                args.clone(),
                kwargs.clone(),
                *block,
                *block_arg,
            );
            let recv = match receiver {
                Some(r) => Some(lower_expr(fx, r)?),
                None => None,
            };
            let blk = block_channel(fx, id, block, block_arg)?;
            if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
                super::call::splat_send(fx, id, recv, &name, &args, &kwargs, blk)
            } else {
                super::call::kw_send(fx, id, recv, &name, &args, &kwargs, blk)
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
            let bx = fx.b.ins().iconst(types::I32, 0);
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
            let bx = fx.b.ins().iconst(types::I32, 0);
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
            // An explicit `Scope::NAME = ..` whose scope isn't a registered
            // class takes rustc's runtime-scope path -- not lowered yet.
            let owner_class = match scope.as_deref() {
                Some(s) => match resolve_class_here(fx, s) {
                    Some(cid) => cid,
                    // An unresolvable scope is a `NameError` on the SCOPE,
                    // raised BEFORE the value runs (`Nope::X = (puts 1; 5)`
                    // prints nothing -- oracle-verified), so the write
                    // lowers as the raise alone. Deferred to run time so a
                    // dead or guarded branch still compiles.
                    None => return raise_uninitialized_constant(fx, s),
                },
                // A bare `NAME =` is owned by the lexically enclosing
                // class/module (the emitting context); `Object` at the top
                // level -- rustc's `const_owner_id_opt` fallback.
                None => fx.method_class.unwrap_or(crate::compiler::OBJECT_CLASS),
            };
            let owner = fx
                .an
                .compiler
                .class(owner_class)
                .const_owners
                .get(&name)
                .copied()
                .unwrap_or(owner_class)
                .0;
            // A `const_added` hook would have to fire after the write
            // (rustc's `emit_const_added`) -- refuse until that lands.
            if fx.an.compiler.global_def_hooks.contains("const_added")
                || fx
                    .an
                    .compiler
                    .class_method_in_chain(zeo_abi::ClassId(owner), "const_added")
                    .is_some()
            {
                return fx.unsupported(id, "a constant write observed by `const_added`");
            }
            let Some((file, line)) = crate::codegen::source_location(&fx.an.compiler, id) else {
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
#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: a refusal-message label -- an unnamed kind falls back to its variant name, which is what triage needs"
)]
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
    fx.method_class
        .map(|c| fx.an.compiler.cref_of_ref(c))
        .unwrap_or(&[])
}

/// Resolve a class name against the current cref (rustc's
/// `Ctx::resolve_class`; box 0 -- box scopes refuse before any lowering).
pub(crate) fn resolve_class_here(fx: &Fx, name: &str) -> Option<crate::compiler::ClassId> {
    fx.an.compiler.resolve_class(name, cref_chain(fx), 0)
}

pub(crate) fn const_read(fx: &mut Fx, id: NodeId, name: &str) -> Result<Operand, String> {
    if let Some(cid) = resolve_class_here(fx, name) {
        return class_value_of(fx, id, name, cid);
    }
    // The rustc `emit_const_read` bare-name shape: owner from the
    // compile-time claim map, then every enclosing cref scope, then the
    // top -- one runtime walk through `const_get_cref`, whose miss raises
    // the NameError with the cref-qualified message.
    let compiler = &fx.an.compiler;
    let defining = fx.method_class.unwrap_or(crate::compiler::OBJECT_CLASS);
    let top = crate::compiler::OBJECT_CLASS;
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
    let qualified = if defining == crate::compiler::OBJECT_CLASS {
        name.to_string()
    } else {
        format!("{}::{name}", compiler.fq_name(defining))
    };
    let bytes: Vec<u8> = chain.iter().flat_map(|c| c.to_le_bytes()).collect();
    let ids_off = fx.em.intern_rodata_aligned(&bytes, 4);
    let ids_ptr = fx.rod(ids_off);
    let n_ids = fx.b.ins().iconst(fx.em.ptr, chain.len() as i64);
    let (nptr, nlen) = rodata_name(fx, name);
    let (qptr, qlen) = rodata_name(fx, &qualified);
    let hook_v = fx.b.ins().iconst(types::I8, i64::from(hook));
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

/// An explicit `Scope::NAME` read whose scope resolves at compile time:
/// the scope operator's own search on the scope class, ruby's
/// as-written miss message (`Object::` prints bare -- it is where a
/// lookup ENDS, not a qualifier).
fn scoped_const_read(fx: &mut Fx, id: NodeId, scope: &str, name: &str) -> Result<Operand, String> {
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
    if compiler.class(scope_cid).private_constants.contains(name) {
        return fx.unsupported(id, "a private-constant reference");
    }
    let hook = compiler
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

/// The bare `uninitialized constant <path>` NameError, in whatever
/// position the caller sits: the raise diverges, so the operand it hands
/// back is the unreachable nil every diverging arm answers.
fn raise_uninitialized_constant(fx: &mut Fx, path: &str) -> Result<Operand, String> {
    let message = format!("uninitialized constant {path}");
    let leaf = crate::constpath::ConstPath::parse(path).base().to_string();
    let (mptr, mlen) = rodata_name(fx, &message);
    let (lptr, llen) = rodata_name(fx, &leaf);
    let status = fx
        .call(
            "zeo_rt_raise_uninitialized_constant",
            &[mptr, mlen, lptr, llen],
        )
        .expect("raise_uninitialized_constant returns a status");
    fx.fallible(status);
    Ok(Operand::Nil)
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
    let (head, leaf) = crate::codegen::split_const_path(scope);
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
        tag: TagInfo::Known(ValueTag::Class as u8),
    }
}

/// The operator set the slice lowers inline.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl BinOp {
    fn of(name: &str) -> Option<BinOp> {
        match name {
            "+" => Some(BinOp::Add),
            "-" => Some(BinOp::Sub),
            "*" => Some(BinOp::Mul),
            "<" => Some(BinOp::Lt),
            "<=" => Some(BinOp::Le),
            ">" => Some(BinOp::Gt),
            ">=" => Some(BinOp::Ge),
            "==" => Some(BinOp::Eq),
            "!=" => Some(BinOp::Ne),
            _ => None,
        }
    }

    fn int_cc(self) -> Option<IntCC> {
        Some(match self {
            BinOp::Lt => IntCC::SignedLessThan,
            BinOp::Le => IntCC::SignedLessThanOrEqual,
            BinOp::Gt => IntCC::SignedGreaterThan,
            BinOp::Ge => IntCC::SignedGreaterThanOrEqual,
            BinOp::Eq => IntCC::Equal,
            BinOp::Ne => IntCC::NotEqual,
            BinOp::Add | BinOp::Sub | BinOp::Mul => return None,
        })
    }

    fn float_cc(self) -> Option<FloatCC> {
        Some(match self {
            BinOp::Lt => FloatCC::LessThan,
            BinOp::Le => FloatCC::LessThanOrEqual,
            BinOp::Gt => FloatCC::GreaterThan,
            BinOp::Ge => FloatCC::GreaterThanOrEqual,
            BinOp::Eq => FloatCC::Equal,
            BinOp::Ne => FloatCC::NotEqual,
            BinOp::Add | BinOp::Sub | BinOp::Mul => return None,
        })
    }

    /// The int-overflow slow path's capi symbol.
    fn slow(self) -> Option<&'static str> {
        Some(match self {
            BinOp::Add => "zeo_rt_int_add_slow",
            BinOp::Sub => "zeo_rt_int_sub_slow",
            BinOp::Mul => "zeo_rt_int_mul_slow",
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => return None,
        })
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
        a
    };
    let b_op = lower_expr(fx, arg)?;
    // Unboxed-both fast case: no memory, no tag tests. Everything else
    // takes the general boxed shape.
    if let (Operand::Int(av), Operand::Int(bv)) = (&a, &b_op) {
        return Ok(int_int(fx, op, *av, *bv));
    }
    boxed_binop(fx, op, name, a, b_op)
}

/// Both operands statically Int: pure SSA.
fn int_int(
    fx: &mut Fx,
    op: BinOp,
    a: cranelift_codegen::ir::Value,
    b: cranelift_codegen::ir::Value,
) -> Operand {
    if let Some(cc) = op.int_cc() {
        return Operand::Bool(fx.b.ins().icmp(cc, a, b));
    }
    // Arithmetic on two Int literals/values still needs the overflow arm;
    // route through the boxed shape's slow call only on overflow.
    let (v, ovf) = match op {
        BinOp::Add => fx.b.ins().sadd_overflow(a, b),
        BinOp::Sub => fx.b.ins().ssub_overflow(a, b),
        BinOp::Mul => fx.b.ins().smul_overflow(a, b),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
            unreachable!("comparisons returned above")
        }
    };
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let slow = fx.b.create_block();
    let ok = fx.b.create_block();
    let join = fx.b.create_block();
    fx.b.ins().brif(ovf, slow, &[], ok, &[]);
    fx.b.switch_to_block(ok);
    store_int(fx, v, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(slow);
    {
        // The slow path needs boxed operands.
        let sa = fx.temp_slot();
        let pa = fx.slot_addr(sa, 0);
        store_int(fx, a, pa);
        let sb = fx.temp_slot();
        let pb = fx.slot_addr(sb, 0);
        store_int(fx, b, pb);
        let f = op.slow().expect("arithmetic has a slow path");
        fx.call(f, &[pa, pb, dst]);
        fx.b.ins().jump(join, &[]);
    }
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }
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
    fx.b.ins().store(fl, v, dst, PAYLOAD_OFFSET as i32);
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
    let payload = PAYLOAD_OFFSET as i32;
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
    let b_float = fx.b.create_block();
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
    {
        let av = fx.b.ins().load(types::I64, fl, pa, payload);
        let bv = fx.b.ins().load(types::I64, fl, pb, payload);
        if let Some(cc) = op.int_cc() {
            let c = fx.b.ins().icmp(cc, av, bv);
            store_bool_tag(fx, c, dst);
            fx.b.ins().jump(join, &[]);
        } else {
            let (v, ovf) = match op {
                BinOp::Add => fx.b.ins().sadd_overflow(av, bv),
                BinOp::Sub => fx.b.ins().ssub_overflow(av, bv),
                BinOp::Mul => fx.b.ins().smul_overflow(av, bv),
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
                    unreachable!("comparisons handled above")
                }
            };
            let slow = fx.b.create_block();
            let ok = fx.b.create_block();
            fx.b.ins().brif(ovf, slow, &[], ok, &[]);
            fx.b.switch_to_block(ok);
            store_int(fx, v, dst);
            fx.b.ins().jump(join, &[]);
            fx.b.switch_to_block(slow);
            let f = op.slow().expect("arithmetic has a slow path");
            fx.call(f, &[pa, pb, dst]);
            fx.b.ins().jump(join, &[]);
        }
    }

    fx.b.switch_to_block(not_int);
    let float_tag = i64::from(ValueTag::Float as u8);
    let a_f = fx.b.ins().icmp_imm_u(IntCC::Equal, ta, float_tag);
    let b_f = fx.b.ins().icmp_imm_u(IntCC::Equal, tb, float_tag);
    let both_f = fx.b.ins().band(a_f, b_f);
    fx.b.ins().brif(both_f, b_float, &[], b_dyn, &[]);

    fx.b.switch_to_block(b_float);
    {
        let av = fx.b.ins().load(types::F64, fl, pa, payload);
        let bv = fx.b.ins().load(types::F64, fl, pb, payload);
        if let Some(cc) = op.float_cc() {
            let c = fx.b.ins().fcmp(cc, av, bv);
            store_bool_tag(fx, c, dst);
        } else {
            let v = match op {
                BinOp::Add => fx.b.ins().fadd(av, bv),
                BinOp::Sub => fx.b.ins().fsub(av, bv),
                BinOp::Mul => fx.b.ins().fmul(av, bv),
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
                    unreachable!("comparisons handled above")
                }
            };
            let tag =
                fx.b.ins()
                    .iconst(types::I8, i64::from(ValueTag::Float as u8));
            fx.b.ins().store(fl, tag, dst, 0);
            fx.b.ins().store(fl, v, dst, payload);
        }
        fx.b.ins().jump(join, &[]);
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
#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: the static-classification tail mirrors rustc's exhaustive match; an unlisted node kind refuses loudly below rather than misclassifying"
)]
fn lower_defined(fx: &mut Fx, site: NodeId, inner: NodeId) -> Result<Operand, String> {
    use crate::hir::LastMatch;
    // `defined?(yield)`: runtime -- the block channel is or isn't there.
    if matches!(&fx.an.compiler.hir[inner], HirNode::Yield(_)) {
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
                return fx.unsupported(site, "a `defined?` of an unresolvable scope");
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
        let ss = fx.temp_slot();
        let dst = fx.slot_addr(ss, 0);
        let swallow = fx.b.create_block();
        let merge = fx.b.create_block();
        let saved = fx.land;
        fx.land = swallow;
        let op = lower_expr(fx, scope)?;
        fx.land = saved;
        let p = ownership::borrow_ptr(fx, &op);
        if op.owned() {
            ownership::pool_owned(fx, p, op.tag());
        }
        let (nptr, nlen) = rodata_name(fx, &name);
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
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        });
    }
    if let HirNode::GlobalRead(name) = &fx.an.compiler.hir[inner] {
        let name = name.clone();
        if is_predefined_global(&name) {
            return Ok(defined_static(fx, Some("global-variable")));
        }
        let bx = fx.b.ins().iconst(types::I32, 0);
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
    if matches!(
        &fx.an.compiler.hir[inner],
        HirNode::ArrayLit(_) | HirNode::HashLit(_)
    ) {
        return fx.unsupported(site, "a `defined?` over a collection literal");
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
fn cvar_owner(fx: &Fx, name: &str) -> u32 {
    let defining = fx.method_class.unwrap_or(crate::compiler::OBJECT_CLASS);
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
    let label = if is_def {
        match fx.method_class {
            Some(c) if c != crate::compiler::OBJECT_CLASS => {
                format!("{}#{name}", fx.an.compiler.fq_name(c))
            }
            _ => format!("Object#{name}"),
        }
    } else {
        format!("block in {}", fx.frame_label)
    };
    let kind = if is_def {
        super::blocks::MethodBody::Def
    } else {
        super::blocks::MethodBody::DefineMethod
    };
    let proc_ss = super::blocks::build_method_body(fx, id, params, body, &label, kind)?;
    let proc_addr = fx.slot_addr(proc_ss, 0);
    let sym = fx.sym_id(name);
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");

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
    let out_ss = fx.temp_slot();
    let out = fx.slot_addr(out_ss, 0);
    let status = fx
        .call(
            "zeo_rt_define_in_default_definee",
            &[cref, self_ptr, sym, proc_addr, private, out],
        )
        .expect("define_in_default_definee returns a status");
    // The proc's reference moved into the runtime.
    fx.owned_consumed += 1;
    fx.fallible(status);
    // A class body's running visibility default rides along: the dynamic
    // walk reads the overlay row this wrote AHEAD of any static mark.
    if visibility != crate::hir::Visibility::Public && fx.method_class.is_some() {
        use cranelift_codegen::ir::InstBuilder;
        let cid = fx.b.ins().iconst(types::I32, i64::from(cref_cid.0));
        let verb = fx.b.ins().iconst(
            types::I8,
            i64::from(u8::from(visibility == crate::hir::Visibility::Protected)),
        );
        let st = fx
            .call("zeo_rt_runtime_set_visibility", &[cid, sym, verb])
            .expect("runtime_set_visibility returns a status");
        fx.fallible(st);
    }
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss: out_ss,
        owned: true,
        tag: TagInfo::Known(ValueTag::Symbol as u8),
    })
}
