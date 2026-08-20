//! Call lowering: direct compiled->compiled calls (a receiverless call
//! naming a known compiled method with matching arity) and the uncached
//! dynamic sends. Inline caches (`CallSite` slots) are a later milestone;
//! every dynamic send here is the uncached entry -- correct, then fast.

use super::ctx::{Fx, VALUE_SIZE};
use super::expr::lower_expr;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::hir::{ArrayElem, NodeId};
use cranelift_codegen::ir::{InstBuilder, StackSlotData, StackSlotKind, types};
use cranelift_module::Module;

/// Evaluate plain positional args to borrowable pointers (owned temps hand
/// their value to the pool -- alive until frame pop / loop latch).
fn arg_ptrs(
    fx: &mut Fx,
    site: NodeId,
    args: &[ArrayElem],
) -> Result<Vec<cranelift_codegen::ir::Value>, String> {
    let mut ptrs = Vec::with_capacity(args.len());
    for arg in args {
        let ArrayElem::Single(id) = arg else {
            return fx.unsupported(site, "a splat argument");
        };
        let op = lower_expr(fx, *id)?;
        let p = ownership::borrow_ptr(fx, &op);
        if op.owned() {
            ownership::pool_owned(fx, p, op.tag());
        }
        ptrs.push(p);
    }
    Ok(ptrs)
}

/// A direct call to compiled method `name`: `(self, p1..pn, out)` through
/// the status protocol. `self_ptr` is the caller's own borrowed self.
pub(crate) fn direct_call(
    fx: &mut Fx,
    site: NodeId,
    name: &str,
    args: &[ArrayElem],
    block: Option<NodeId>,
) -> Result<Operand, String> {
    let decl_has_blk = fx.em.methods[name].has_blk;
    // A literal block on a method that never uses one is never invoked --
    // nothing to build (Ruby's own rule).
    let blk_ptr = match (block, decl_has_blk) {
        (Some(blk_node), true) => {
            let (proc_ss, _) = super::blocks::build_proc(fx, site, blk_node)?;
            // The callee consumes the moved-in proc.
            fx.owned_consumed += 1;
            Some(fx.slot_addr(proc_ss, 0))
        }
        (Some(_), false) => None,
        (None, true) => Some(fx.b.ins().iconst(fx.em.ptr, 0)),
        (None, false) => None,
    };
    let ptrs = arg_ptrs(fx, site, args)?;
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let func_id = fx.em.methods[name].body;
    let fref = fx.em.module.declare_func_in_func(func_id, fx.b.func);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let mut call_args = Vec::with_capacity(ptrs.len() + 3);
    call_args.push(self_ptr);
    call_args.extend(ptrs);
    if let Some(b) = blk_ptr {
        call_args.push(b);
    }
    call_args.push(out);
    let inst = fx.b.ins().call(fref, &call_args);
    let status = fx.b.func.dfg.inst_results(inst)[0];
    // A `break` inside the literal block ENDS THIS CALL with its value
    // (CRuby's TAG_BREAK), however deep the `yield` that reached it sits --
    // the same landing every dynamic block-passing send opens. Without it
    // the signal ran past the top level.
    if blk_ptr.is_some() && block.is_some() {
        super::blocks::catch_break(fx, status, out);
    } else {
        fx.fallible(status);
    }
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A receiverless dynamic send on the current `self` -- the implicit-call
/// mode (no visibility barrier: private methods answer).
pub(crate) fn implicit_send(
    fx: &mut Fx,
    site: NodeId,
    name: &str,
    args: &[ArrayElem],
) -> Result<Operand, String> {
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    // A VCALL -- a bare identifier ruby could have read as a local. The
    // lookup is the same; the MISS is not. Ruby says "undefined local
    // variable or method" and raises NameError, because from the source it
    // cannot tell which the writer meant.
    if args.is_empty()
        && fx
            .an
            .compiler
            .hir
            .has_flag(site, crate::hir::NodeFlag::VCALL)
    {
        // Inside an eval splice the name may be the enclosing scope's own
        // local, whose storage the hoisting prelude already created.
        // Restricted to the splice: outside one, a name prism called a
        // vcall genuinely is not a local -- it would have parsed as a read.
        if fx.in_eval_splice
            && let Some(op) = ownership::read_local(fx, name)
        {
            return Ok(op);
        }
        let sym = fx.sym_id(name);
        let zero_box = fx.box_v();
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let status = fx
            .call(
                "zeo_rt_send_value_vcall_in",
                &[zero_box, self_ptr, sym, out],
            )
            .expect("send returns a status");
        fx.fallible(status);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        });
    }
    let argv_ptr = build_argv(fx, site, args)?;
    let sym = fx.sym_id(name);
    let zero_box = fx.box_v();
    let argc_v = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_send_value_in",
            &[zero_box, self_ptr, sym, argv_ptr, argc_v, null, out],
        )
        .expect("send returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A contiguous argv array of borrowed copies (owned temps hand their
/// value to the pool first). Null when empty.
pub(crate) fn build_argv(
    fx: &mut Fx,
    site: NodeId,
    args: &[ArrayElem],
) -> Result<cranelift_codegen::ir::Value, String> {
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
            return fx.unsupported(site, "a splat argument");
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
    Ok(match argv {
        Some(ss) => fx.slot_addr(ss, 0),
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    })
}

/// A Hash from `KwArg` rows (a hash literal, or a call site's keyword
/// set), evaluated in written order -- key then value per pair, `**`
/// splats merged in place, later keys overwrite. The hash is pooled AT
/// CREATION (a `**` coercion can raise mid-build, and the error edge must
/// not strand an unpooled value), so the returned address is a BORROW --
/// alive until frame pop -- and later pairs mutate through the shared
/// handle.
pub(crate) fn build_hash(
    fx: &mut Fx,
    pairs: &[crate::hir::KwArg],
) -> Result<cranelift_codegen::ir::Value, String> {
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    fx.call("zeo_rt_hash_new", &[out]);
    fx.owned_created += 1;
    ownership::pool_owned(fx, out, TagInfo::Known(zeo_abi::abi::ValueTag::Hash as u8));
    for kw in pairs {
        match kw {
            crate::hir::KwArg::Pair(k, v) => {
                let kop = lower_expr(fx, *k)?;
                let kptr = ownership::move_ptr(fx, &kop);
                let vop = lower_expr(fx, *v)?;
                let vptr = ownership::move_ptr(fx, &vop);
                fx.call("zeo_rt_hash_set", &[out, kptr, vptr]);
            }
            crate::hir::KwArg::DoubleSplat(e) => {
                let op = lower_expr(fx, *e)?;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let status = fx
                    .call("zeo_rt_kw_splat_into", &[out, p])
                    .expect("kw_splat_into returns a status");
                fx.fallible(status);
            }
        }
    }
    Ok(out)
}

/// An Array from `ArrayElem` rows (an array literal, or a splat-bearing
/// call site's arguments): singles pushed, splats expanded through the
/// runtime's `to_a` coercion (which can raise). Pooled at creation, same
/// reasoning as `build_hash` -- the returned address is a borrow.
pub(crate) fn build_array(
    fx: &mut Fx,
    args: &[ArrayElem],
) -> Result<cranelift_codegen::ir::Value, String> {
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let cap = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    fx.call("zeo_rt_array_new", &[cap, out]);
    fx.owned_created += 1;
    ownership::pool_owned(fx, out, TagInfo::Known(zeo_abi::abi::ValueTag::Array as u8));
    for arg in args {
        match arg {
            ArrayElem::Single(id) => {
                let op = lower_expr(fx, *id)?;
                let p = ownership::move_ptr(fx, &op);
                fx.call("zeo_rt_array_push", &[out, p]);
            }
            ArrayElem::Splat(id) => {
                let op = lower_expr(fx, *id)?;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let status = fx
                    .call("zeo_rt_array_push_splat", &[out, p])
                    .expect("push_splat returns a status");
                fx.fallible(status);
            }
        }
    }
    Ok(out)
}

/// The receiver half of a dynamic send: what to dispatch against, and
/// whether ruby's visibility barrier applies to the site at all.
pub(crate) struct Recv {
    /// The evaluated receiver, or `None` for the current `self` -- which is
    /// the implicit entry, the one a receiverless call takes.
    pub op: Option<Operand>,
    /// Ruby's `VM_CALL_FCALL`: run no check. True for the
    /// `self.singleton_class` a `class << self` body's statement is rebound
    /// onto, whose ruby form is RECEIVERLESS -- the surrogate must be the
    /// receiver AND the barrier must stay down. A receiver the SOURCE wrote
    /// is never marked, so `Foo.singleton_class.some_private` still raises.
    pub bypass: bool,
}

impl Recv {
    /// An evaluated receiver behind the barrier.
    pub(crate) fn at(op: Operand) -> Recv {
        Recv {
            op: Some(op),
            bypass: false,
        }
    }

    /// An evaluated receiver, or `self`, with the site's own barrier verdict.
    pub(crate) fn maybe(op: Option<Operand>, bypass: bool) -> Recv {
        Recv { op, bypass }
    }
}

/// A splat-bearing dynamic send: args built as a runtime Array, keywords
/// (when present) as the kw Hash; the runtime entry unmarks the splat
/// tail (a splat-expanded hash is positional again -- `ruby2_keywords`
/// will pass 0 here when it lands) and appends the keywords.
pub(crate) fn splat_send(
    fx: &mut Fx,
    site: NodeId,
    recv: Recv,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[crate::hir::KwArg],
    blk: Option<cranelift_codegen::ir::Value>,
) -> Result<Operand, String> {
    let _ = site;
    let bypass = recv.bypass;
    let recv_ptr = match &recv.op {
        Some(op) => {
            let p = ownership::borrow_ptr(fx, op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            p
        }
        None => fx.self_ptr.expect("self_ptr is set in the prologue"),
    };
    let args_ptr = build_array(fx, args)?;
    let kw_ptr = if kwargs.is_empty() {
        fx.b.ins().iconst(fx.em.ptr, 0)
    } else {
        build_hash(fx, kwargs)?
    };
    let sym = fx.sym_id(name);
    let zero_box = fx.box_v();
    // `ruby2_keywords`' whole purpose: a marked forwarder's splat keeps a
    // trailing hash's keyword mark.
    let unmark = fx.b.ins().iconst(types::I8, i64::from(!fx.ruby2_keywords));
    let blk_ptr = blk.unwrap_or_else(|| fx.b.ins().iconst(fx.em.ptr, 0));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = match recv.op {
        Some(_) => {
            let caller = caller_class(fx, bypass);
            fx.call(
                "zeo_rt_send_value_explicit_args_in",
                &[
                    zero_box, recv_ptr, sym, args_ptr, unmark, kw_ptr, blk_ptr, caller, out,
                ],
            )
        }
        None => fx.call(
            "zeo_rt_send_value_args_in",
            &[
                zero_box, recv_ptr, sym, args_ptr, unmark, kw_ptr, blk_ptr, out,
            ],
        ),
    }
    .expect("splat sends return a status");
    if blk.is_some() {
        super::blocks::catch_break(fx, status, out);
    } else {
        fx.fallible(status);
    }
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A dynamic send WITH call-site keywords: positionals into argv, the
/// keyword Hash built here, the runtime's kw entry does the append-if-
/// non-empty. `recv` `None` = the implicit-self mode (private methods
/// answer); `Some` = the explicit entry behind the visibility barrier.
pub(crate) fn kw_send(
    fx: &mut Fx,
    site: NodeId,
    recv: Recv,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[crate::hir::KwArg],
    blk: Option<cranelift_codegen::ir::Value>,
) -> Result<Operand, String> {
    let bypass = recv.bypass;
    let recv_ptr = match &recv.op {
        Some(op) => {
            let p = ownership::borrow_ptr(fx, op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            p
        }
        None => fx.self_ptr.expect("self_ptr is set in the prologue"),
    };
    let argv_ptr = build_argv(fx, site, args)?;
    let kw_ptr = build_hash(fx, kwargs)?;
    let sym = fx.sym_id(name);
    let zero_box = fx.box_v();
    let argc_v = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let blk_ptr = blk.unwrap_or_else(|| fx.b.ins().iconst(fx.em.ptr, 0));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = match recv.op {
        Some(_) => {
            let caller = caller_class(fx, bypass);
            fx.call(
                "zeo_rt_send_value_explicit_kw_in",
                &[
                    zero_box, recv_ptr, sym, argv_ptr, argc_v, kw_ptr, blk_ptr, caller, out,
                ],
            )
        }
        None => fx.call(
            "zeo_rt_send_value_kw_in",
            &[
                zero_box, recv_ptr, sym, argv_ptr, argc_v, kw_ptr, blk_ptr, out,
            ],
        ),
    }
    .expect("kw sends return a status");
    if blk.is_some() {
        super::blocks::catch_break(fx, status, out);
    } else {
        fx.fallible(status);
    }
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The class a visibility barrier compares against. Ruby's `protected` asks
/// whether the CALLING method's own class is ancestor-related to the target's
/// owner, so an explicit-receiver site has to name itself; `private` ignores
/// it. Outside any class body `self` is `main`, an ordinary `Object`, and
/// `Object` is what the check compares against there -- the same rule
/// `codegen::call::visibility::caller_class` spells for the rustc backend.
///
/// `bypass` is ruby's `VM_CALL_FCALL`: the site runs no check at all. A
/// literal `self` receiver never reaches here (it takes the implicit entry --
/// see `expr::self_receiver`), but the `self.singleton_class` a `class <<
/// self` body's statement is rebound onto does: its ruby form is
/// RECEIVERLESS, so the surrogate must be the receiver AND the barrier must
/// stay down.
pub(crate) fn caller_class(fx: &mut Fx, bypass: bool) -> cranelift_codegen::ir::Value {
    if bypass {
        return fx.b.ins().iconst(types::I32, i64::from(u32::MAX));
    }
    // A scope whose `self` only the run time knows (a `Class.new` body's
    // method, an `instance_eval` block) has no lexical class to compare a
    // `protected` target against -- the enclosing one is `Object`, which is
    // no kind of the class the body will belong to, and the call was
    // refused. Ask `self` instead, per call (rustc's `Caller::Runtime`).
    if fx.self_is_dynamic {
        let slf = fx.self_ptr.expect("self_ptr is set in the prologue");
        return fx
            .call("zeo_rt_class_of", &[slf])
            .expect("class_of answers a class id");
    }
    let cid = fx.method_class.map_or(0, |c| i64::from(c.0));
    fx.b.ins().iconst(types::I32, cid)
}

/// An explicit-receiver dynamic send through the uncached entry.
pub(crate) fn dynamic_send(
    fx: &mut Fx,
    site: NodeId,
    recv: NodeId,
    name: &str,
    args: &[ArrayElem],
) -> Result<Operand, String> {
    let bypass = super::expr::bypasses_visibility(fx, Some(recv));
    let later = super::expr::later_nodes(args, &[], None);
    let recv_op = lower_expr(fx, recv)?;
    let recv_op = super::expr::park_reassignable(fx, Some(recv), recv_op, &later);
    dynamic_send_value(fx, site, recv_op, name, args, bypass)
}

/// `dynamic_send` on an already-lowered receiver (the `New` lowering hands
/// a Class value here).
pub(crate) fn dynamic_send_value(
    fx: &mut Fx,
    site: NodeId,
    recv_op: Operand,
    name: &str,
    args: &[ArrayElem],
    bypass: bool,
) -> Result<Operand, String> {
    let recv_ptr = ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    let argv_ptr = build_argv(fx, site, args)?;
    dynamic_send_argv(fx, recv_ptr, name, argv_ptr, args.len(), bypass)
}

/// [`dynamic_send_value`] over an ALREADY-BUILT argv -- what a runtime
/// `def` install needs, whose two arguments are a Symbol and a proc the
/// caller moved into place.
pub(crate) fn dynamic_send_ptr(
    fx: &mut Fx,
    recv_op: Operand,
    name: &str,
    argv_ptr: cranelift_codegen::ir::Value,
    argc: usize,
) -> Result<Operand, String> {
    let recv_ptr = ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    dynamic_send_argv(fx, recv_ptr, name, argv_ptr, argc, false)
}

fn dynamic_send_argv(
    fx: &mut Fx,
    recv_ptr: cranelift_codegen::ir::Value,
    name: &str,
    argv_ptr: cranelift_codegen::ir::Value,
    argc: usize,
    bypass: bool,
) -> Result<Operand, String> {
    let sym = fx.sym_id(name);
    let zero_box = fx.box_v();
    let argc_v = fx.b.ins().iconst(fx.em.ptr, argc as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let caller = caller_class(fx, bypass);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_send_value_explicit_in",
            &[zero_box, recv_ptr, sym, argv_ptr, argc_v, null, caller, out],
        )
        .expect("send returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// `super` -- rustc's `emit_super`, the instance-method channels only
/// (class-method/singleton-chain `super` and `super` inside a block still
/// refuse). One runtime mechanism: `send_super_from`'s per-position MRO
/// walk resumes AFTER the class the `def` was WRITTEN in
/// (`fx.defining_class` -- a module method's copy keeps the module, which
/// sits in the receiver's ancestry); a value-builtin subclass whose walk
/// finds no user definition above bridges into the native root instead
/// (`value_super`). Bare `super` forwards the current method's own params
/// by NAME (splat rest, keywords as one marked hash -- the G2
/// convention); the current block forwards unless the site writes one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_super(
    fx: &mut Fx,
    site: NodeId,
    args: &[ArrayElem],
    kwargs: &[crate::hir::KwArg],
    zsuper: bool,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> Result<Operand, String> {
    // A BARE `super` from a `define_method` body is an error in ruby: a
    // zsuper forwards the CURRENT values of the method's parameters, and a
    // block-shaped body has no parameter list to forward from, so ruby
    // refuses rather than guessing. Raised at DISPATCH time (the method may
    // never be called), like rustc's.
    if zsuper && fx.define_method_body {
        let msg = "implicit argument passing of super from method defined by \
                   define_method() is not supported. Specify all arguments explicitly.";
        let cid = fx.b.ins().iconst(
            cranelift_codegen::ir::types::I32,
            i64::from(zeo_abi::RUNTIME_ERROR_CLASS.0),
        );
        let (mptr, mlen) = super::expr::rodata_name(fx, msg);
        let status = fx
            .call("zeo_rt_raise_error", &[cid, mptr, mlen])
            .expect("raise_error returns a status");
        fx.fallible(status);
        return Ok(Operand::Nil);
    }
    let Some(params) = fx.method_params.clone() else {
        return fx.unsupported(site, "a `super` outside a compiled method");
    };

    // The argument Array (rustc's `__super_args` Vec) + kw hash + unmark.
    let (args_ptr, kw_ptr, unmark) = if zsuper {
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let cap = fx.b.ins().iconst(
            fx.em.ptr,
            (params.required.len() + params.optional.len() + params.post.len()) as i64,
        );
        fx.call("zeo_rt_array_new", &[cap, out]);
        fx.owned_created += 1;
        ownership::pool_owned(fx, out, TagInfo::Known(zeo_abi::abi::ValueTag::Array as u8));
        let push_named = |fx: &mut Fx, name: &str| {
            let op = ownership::read_local(fx, name).expect("a param is always bound");
            let p = ownership::move_ptr(fx, &op);
            fx.call("zeo_rt_array_push", &[out, p]);
        };
        for name in &params.required {
            push_named(fx, name);
        }
        for (name, _) in &params.optional {
            push_named(fx, name);
        }
        if let Some(Some(rest)) = &params.rest {
            let op = ownership::read_local(fx, rest).expect("a param is always bound");
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            let status = fx
                .call("zeo_rt_array_push_splat", &[out, p])
                .expect("push_splat returns a status");
            fx.fallible(status);
        }
        for name in &params.post {
            push_named(fx, name);
        }
        let has_kw = !params.keywords.is_empty() || matches!(params.keyword_rest, Some(Some(_)));
        let kw_ptr = if has_kw {
            let kss = fx.temp_slot();
            let kw = fx.slot_addr(kss, 0);
            fx.call("zeo_rt_hash_new", &[kw]);
            fx.owned_created += 1;
            ownership::pool_owned(fx, kw, TagInfo::Known(zeo_abi::abi::ValueTag::Hash as u8));
            for k in &params.keywords {
                let key = match k {
                    crate::hir::KeywordParam::Required(n)
                    | crate::hir::KeywordParam::Optional(n, _) => n.clone(),
                };
                let sym = fx.sym_id(&key);
                let sss = fx.temp_slot();
                let sptr = fx.slot_addr(sss, 0);
                fx.call("zeo_rt_sym_value", &[sym, sptr]);
                fx.owned_created += 1;
                fx.owned_consumed += 1; // hash_set moves the key temp
                let vop = ownership::read_local(fx, &key).expect("a param is always bound");
                let vp = ownership::move_ptr(fx, &vop);
                fx.call("zeo_rt_hash_set", &[kw, sptr, vp]);
            }
            if let Some(Some(krest)) = &params.keyword_rest {
                let op = ownership::read_local(fx, krest).expect("a param is always bound");
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let status = fx
                    .call("zeo_rt_kw_splat_into", &[kw, p])
                    .expect("kw_splat_into returns a status");
                fx.fallible(status);
            }
            kw
        } else {
            fx.b.ins().iconst(fx.em.ptr, 0)
        };
        let unmark = matches!(params.rest, Some(Some(_))) && !fx.ruby2_keywords;
        (out, kw_ptr, unmark)
    } else {
        let args_ptr = build_array(fx, args)?;
        let kw_ptr = if kwargs.is_empty() {
            fx.b.ins().iconst(fx.em.ptr, 0)
        } else {
            build_hash(fx, kwargs)?
        };
        let unmark = args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) && !fx.ruby2_keywords;
        (args_ptr, kw_ptr, unmark)
    };

    // The block: a literal proc moves in; `&expr` coerces; neither written
    // = forward the CURRENT method's block (a RETAINED extra reference --
    // the callee consumes one, the epilogue still releases ours).
    let blk_ptr = match (block, block_arg) {
        (Some(b), _) => {
            let (proc_ss, _names) = super::blocks::build_proc(fx, site, b)?;
            fx.owned_consumed += 1;
            fx.slot_addr(proc_ss, 0)
        }
        (None, Some(e)) => {
            let op = lower_expr(fx, e)?;
            let vp = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, vp, op.tag());
            }
            let conv_ss = fx.temp_slot();
            let conv = fx.slot_addr(conv_ss, 0);
            let status = fx
                .call("zeo_rt_block_arg_to_proc", &[vp, conv])
                .expect("block_arg_to_proc returns a status");
            fx.fallible(status);
            let fl = cranelift_codegen::ir::MemFlagsData::trusted();
            let tag =
                fx.b.ins()
                    .load(cranelift_codegen::ir::types::I8, fl, conv, 0);
            let is_nil =
                fx.b.ins()
                    .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, tag, 0);
            let null = fx.b.ins().iconst(fx.em.ptr, 0);
            fx.owned_created += 1;
            fx.owned_consumed += 1;
            fx.b.ins().select(is_nil, null, conv)
        }
        (None, None) => match fx.blk_ptr {
            Some(blk) => {
                let got = fx.b.ins().icmp_imm_u(
                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                    blk,
                    0,
                );
                let do_retain = fx.b.create_block();
                let cont = fx.b.create_block();
                fx.b.ins().brif(got, do_retain, &[], cont, &[]);
                fx.b.switch_to_block(do_retain);
                fx.call("zeo_rt_retain", &[blk]);
                fx.b.ins().jump(cont, &[]);
                fx.b.switch_to_block(cont);
                blk
            }
            None => fx.b.ins().iconst(fx.em.ptr, 0),
        },
    };

    // A RUNTIME-installed body's defining class is minted at run time, so
    // the walk resumes from the (class, name) pair the method-frame stack
    // recorded when the body was entered -- rustc's `send_super_dynamic`.
    if fx.runtime_method_body {
        let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let unmark_v =
            fx.b.ins()
                .iconst(cranelift_codegen::ir::types::I8, i64::from(unmark));
        let status = fx
            .call(
                "zeo_rt_send_super_dynamic_args",
                &[self_ptr, args_ptr, unmark_v, kw_ptr, blk_ptr, out],
            )
            .expect("send_super_dynamic_args returns a status");
        fx.fallible(status);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        });
    }

    let (Some(def_class), Some(mname), Some(owner)) =
        (fx.defining_class, fx.method_name.clone(), fx.method_class)
    else {
        return fx.unsupported(site, "a `super` outside a compiled method");
    };

    // A CLASS-method `super` resolves against the SINGLETON-class chain,
    // which this compiler reconstructs exactly for compile-time extends
    // (sibling-extend order is compile-time knowledge the runtime registry
    // does not record). A resolved target dispatches its registered row
    // directly; a miss -- a builtin default, or a runtime-defined method --
    // defers to the runtime walk, resumed from the ancestor whose singleton
    // slot HOLDS the defining class (an extended module is not itself in
    // the ancestry, and naming it restarts the walk at the top).
    if fx.self_is_class {
        let compiler = &fx.an.compiler;
        let target = crate::analyze::class_query::extended_singleton_super(
            compiler, owner, def_class, &mname,
        );
        let resume =
            crate::analyze::class_query::singleton_chain_host(compiler, owner, Some(def_class))
                .unwrap_or(def_class);
        let sym = fx.sym_id(&mname);
        let recv_v =
            fx.b.ins()
                .iconst(cranelift_codegen::ir::types::I32, i64::from(owner.0));
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let unmark_v =
            fx.b.ins()
                .iconst(cranelift_codegen::ir::types::I8, i64::from(unmark));
        let status = match target {
            Some((t, _sid, module_instance)) => {
                let t_v =
                    fx.b.ins()
                        .iconst(cranelift_codegen::ir::types::I32, i64::from(t.0));
                let mi =
                    fx.b.ins()
                        .iconst(cranelift_codegen::ir::types::I8, i64::from(module_instance));
                fx.call(
                    "zeo_rt_call_singleton_super_target_args",
                    &[
                        t_v, mi, recv_v, sym, args_ptr, unmark_v, kw_ptr, blk_ptr, out,
                    ],
                )
                .expect("call_singleton_super_target_args returns a status")
            }
            None => {
                let def_v =
                    fx.b.ins()
                        .iconst(cranelift_codegen::ir::types::I32, i64::from(resume.0));
                fx.call(
                    "zeo_rt_send_super_class_from_args",
                    &[recv_v, def_v, sym, args_ptr, unmark_v, kw_ptr, blk_ptr, out],
                )
                .expect("send_super_class_from_args returns a status")
            }
        };
        fx.fallible(status);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        });
    }

    // Channel: a value-builtin subclass whose walk above `def_class` finds
    // no user definition targets the native root (rustc's `value_channel`
    // predicate, verbatim); everything else is the per-position MRO walk.
    let compiler = &fx.an.compiler;
    let ancestors = &compiler.class(owner).ancestors;
    let pos = ancestors.iter().position(|&a| a == def_class);
    let found = pos.is_some_and(|pos| {
        ancestors[pos + 1..].iter().any(|&anc| {
            compiler
                .class(anc)
                .own_methods
                .iter()
                .any(|&s| compiler.scope(s).name == mname)
        })
    });
    let value_channel = !found && pos.is_some() && compiler.is_value_subclass(owner);

    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let unmark_v =
        fx.b.ins()
            .iconst(cranelift_codegen::ir::types::I8, i64::from(unmark));
    let status = if value_channel {
        let (nptr, nlen) = super::expr::rodata_name(fx, &mname);
        fx.call(
            "zeo_rt_value_super_args",
            &[
                self_ptr, nptr, nlen, args_ptr, unmark_v, kw_ptr, blk_ptr, out,
            ],
        )
        .expect("value_super_args returns a status")
    } else {
        let def_v =
            fx.b.ins()
                .iconst(cranelift_codegen::ir::types::I32, i64::from(def_class.0));
        let sym = fx.sym_id(&mname);
        fx.call(
            "zeo_rt_send_super_from_args",
            &[
                self_ptr, def_v, sym, args_ptr, unmark_v, kw_ptr, blk_ptr, out,
            ],
        )
        .expect("send_super_from_args returns a status")
    };
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}
