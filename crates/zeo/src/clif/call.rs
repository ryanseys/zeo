//! Call lowering: direct compiled->compiled calls (a receiverless call
//! naming a known compiled method with matching arity) and the uncached
//! dynamic sends. Inline caches (`CallSite` slots) are a later milestone;
//! every dynamic send here is the uncached entry -- correct, then fast.

use super::ctx::{Fx, VALUE_SIZE};
use super::expr::lower_expr;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::diagnostics::clif::CResult;
use crate::hir::{ArrayElem, NodeId};
use cranelift_codegen::ir::{InstBuilder, StackSlotData, StackSlotKind, types};
use cranelift_module::Module;

/// Evaluate plain positional args to borrowable pointers (owned temps hand
/// their value to the pool -- alive until frame pop / loop latch).
fn arg_ptrs(
    fx: &mut Fx,
    site: NodeId,
    args: &[ArrayElem],
) -> CResult<Vec<cranelift_codegen::ir::Value>> {
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

/// The call site's keyword keys as literal names, in written order.
/// `None` when a key is not a literal symbol or a `**` splat is present --
/// which is exactly when the pairing cannot be decided here.
pub(crate) fn literal_kw_names(fx: &Fx, kwargs: &[crate::hir::KwArg]) -> Option<Vec<String>> {
    let mut names = Vec::with_capacity(kwargs.len());
    for kw in kwargs {
        let crate::hir::KwArg::Pair(k, _) = kw else {
            return None;
        };
        match &fx.an.compiler.hir[*k] {
            crate::hir::HirNode::SymbolLit(n) => names.push(n.clone()),
            _ => return None,
        }
    }
    Some(names)
}

/// Callee slot order -> the index of the site row that fills it, when the
/// site's keys cover the callee's required keywords EXACTLY. Anything
/// else -- a missing key, a spare one, the same key twice -- is an
/// `ArgumentError` (or a last-one-wins overwrite) that the binder owns, so
/// it keeps the dynamic route and its error text cannot drift.
///
/// Callee names are distinct (ruby forbids a repeated parameter name) and
/// the lengths match, so an injective map is a bijective one: every site
/// row is used exactly once.
pub(crate) fn kw_slot_order(decl: &[String], site: &[String]) -> Option<Vec<usize>> {
    if decl.len() != site.len() {
        return None;
    }
    let mut order = Vec::with_capacity(decl.len());
    for want in decl {
        let mut found = None;
        for (i, got) in site.iter().enumerate() {
            if got == want {
                if found.is_some() {
                    return None;
                }
                found = Some(i);
            }
        }
        order.push(found?);
    }
    Some(order)
}

/// Can this site fill `name`'s keyword slots itself? Answers the slot
/// order when it can.
pub(crate) fn kw_direct_order(
    fx: &Fx,
    name: &str,
    kwargs: &[crate::hir::KwArg],
) -> Option<Vec<usize>> {
    let decl = fx.em.methods.get(name)?;
    let want = decl.kw_direct.as_ref()?;
    kw_slot_order(want, &literal_kw_names(fx, kwargs)?)
}

/// A direct call to compiled method `name`: `(self, p1..pn, k1..km, out)`
/// through the status protocol. `self_ptr` is the caller's own borrowed
/// self. `kw` is `(the site's rows, slot order)` when the site fills the
/// callee's keyword slots itself -- see [`kw_direct_order`].
pub(crate) fn direct_call(
    fx: &mut Fx,
    site: NodeId,
    name: &str,
    args: &[ArrayElem],
    block: Option<NodeId>,
) -> CResult<Operand> {
    direct_call_kw(fx, site, name, args, None, block)
}

pub(crate) fn direct_call_kw(
    fx: &mut Fx,
    site: NodeId,
    name: &str,
    args: &[ArrayElem],
    kw: Option<(&[crate::hir::KwArg], &[usize])>,
    block: Option<NodeId>,
) -> CResult<Operand> {
    let decl_has_blk = fx.em.methods[name].has_blk;
    let ptrs = arg_ptrs(fx, site, args)?;
    // Keyword VALUES evaluate in written order (they are ordinary
    // argument expressions), and land in the callee's DECLARED order --
    // so they are lowered first and permuted after.
    let kw_ptrs = match kw {
        None => Vec::new(),
        Some((rows, order)) => {
            let mut written = Vec::with_capacity(rows.len());
            for row in rows {
                let crate::hir::KwArg::Pair(_, v) = row else {
                    unreachable!("kw_direct_order rejects a ** splat");
                };
                let op = lower_expr(fx, *v)?;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                written.push(p);
            }
            order.iter().map(|&i| written[i]).collect()
        }
    };
    // The proc is built LAST -- see `BlockChannel`. A literal block on a
    // method that never uses one is never invoked, so it is not built at
    // all (Ruby's own rule).
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
    super::stmt::stamp_call_line(fx, site);
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let func_id = fx.em.methods[name].body;
    let fref = fx.em.module.declare_func_in_func(func_id, fx.b.func);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let mut call_args = Vec::with_capacity(ptrs.len() + kw_ptrs.len() + 3);
    call_args.push(self_ptr);
    call_args.extend(ptrs);
    call_args.extend(kw_ptrs);
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
) -> CResult<Operand> {
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
        // In a SNIPPET the name may be one of the caller's own locals:
        // prism parsed the snippet alone, so a name it holds could only
        // arrive as a vcall. Restricted to a snippet: elsewhere a name
        // prism called a vcall genuinely is not a local -- it would have
        // parsed as a read.
        if fx.eval_mode.is_some()
            && let Some(op) = ownership::read_local(fx, name)
        {
            return Ok(op);
        }
        let sym = fx.sym_id(name);
        let zero_box = fx.box_v();
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        // Implicit receiver = FCALL, so the site is always cacheable; the
        // vcall entry only changes the MISS message.
        let cache = fx.callsite_ptr(FCALL);
        let status = fx.call_status(
            "zeo_rt_send_value_vcall_cached",
            &[cache, zero_box, self_ptr, sym, out],
        );
        fx.fallible(status);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Unknown,
        });
    }
    let argv_ptr = build_argv(fx, site, args)?;
    implicit_send_ptr(fx, name, argv_ptr, args.len())
}

/// [`implicit_send`] with the arguments already in a contiguous slot
/// array -- what a site that has no HIR argument nodes to lower needs
/// (an `eval`'s `include M`, whose module the marker holds by NAME).
pub(crate) fn implicit_send_ptr(
    fx: &mut Fx,
    name: &str,
    argv_ptr: cranelift_codegen::ir::Value,
    argc: usize,
) -> CResult<Operand> {
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let sym = fx.sym_id(name);
    let zero_box = fx.box_v();
    let argc_v = fx.b.ins().iconst(fx.em.ptr, argc as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    // An implicit receiver asks no visibility question, so the site is
    // vetted against FCALL -- `send_value_cached` with that caller is
    // `send_value_in` with a monomorphic cache in front.
    let cache = fx.callsite_ptr(FCALL);
    let status = fx.call_status(
        "zeo_rt_send_value_cached",
        &[cache, zero_box, self_ptr, sym, argv_ptr, argc_v, null, out],
    );
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
) -> CResult<cranelift_codegen::ir::Value> {
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

/// `Foo.new(a, b)` on a plain compiled class: allocate and run
/// `initialize`, with no dispatch for `new` at all. The gate lives at the
/// call site (see `HirNode::New`); the runtime entry is `Class#new`'s
/// walk behind a per-site `ClassNewSite` cache, so `initialize`
/// resolution -- including an overlay row and an inherited one -- is
/// unchanged, and a quiet-gates hit skips the walk.
pub(crate) fn construct_compiled(
    fx: &mut Fx,
    site: NodeId,
    cid: crate::compiler::ClassId,
    args: &[ArrayElem],
    blk: super::blocks::BlockChannel,
) -> CResult<Operand> {
    let argv_ptr = build_argv(fx, site, args)?;
    let blk_ptr = blk.open(fx, site)?;
    super::stmt::stamp_call_line(fx, site);
    let site_ptr = fx.new_site_ptr();
    let cid_v = fx.cid_value(cid.0);
    let argc_v = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status(
        "zeo_rt_class_new_instance_cached",
        &[site_ptr, cid_v, argv_ptr, argc_v, blk_ptr, out],
    );
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(zeo_abi::abi::ValueTag::Object as u8),
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
) -> CResult<cranelift_codegen::ir::Value> {
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
                let status = fx.call_status("zeo_rt_hash_set", &[out, kptr, vptr]);
                fx.fallible(status);
            }
            crate::hir::KwArg::DoubleSplat(e) => {
                let op = lower_expr(fx, *e)?;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let status = fx.call_status("zeo_rt_kw_splat_into", &[out, p]);
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
) -> CResult<cranelift_codegen::ir::Value> {
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
                let status = fx.call_status("zeo_rt_array_push_splat", &[out, p]);
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
    blk: super::blocks::BlockChannel,
) -> CResult<Operand> {
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
    let blk_ptr = blk.open(fx, site)?;
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    // Same rule as `kw_send`: a statically known caller gets the cached
    // entry, a dynamic-self body keeps the uncached pair.
    let implicit_caller = match recv.op {
        Some(_) => static_caller(fx, bypass),
        None => Some(FCALL),
    };
    let status = match (implicit_caller, recv.op) {
        (Some(caller), _) => {
            let cache = fx.callsite_ptr(caller);
            fx.call(
                "zeo_rt_send_value_args_cached",
                &[
                    cache, zero_box, recv_ptr, sym, args_ptr, unmark, kw_ptr, blk_ptr, out,
                ],
            )
        }
        (None, Some(_)) => {
            let caller = caller_class(fx, bypass);
            fx.call(
                "zeo_rt_send_value_explicit_args_in",
                &[
                    zero_box, recv_ptr, sym, args_ptr, unmark, kw_ptr, blk_ptr, caller, out,
                ],
            )
        }
        (None, None) => fx.call(
            "zeo_rt_send_value_args_in",
            &[
                zero_box, recv_ptr, sym, args_ptr, unmark, kw_ptr, blk_ptr, out,
            ],
        ),
    }
    .expect("splat sends return a status");
    if blk.is_open() {
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
    blk: super::blocks::BlockChannel,
) -> CResult<Operand> {
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
    let blk_ptr = blk.open(fx, site)?;
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    // A statically known caller gets the CACHED kw entry (one entry for
    // both shapes -- an implicit receiver rides FCALL in the site, the
    // explicit one its caller class); a dynamic-self body keeps the
    // uncached pair, `dynamic_send_argv`'s rule.
    let implicit_caller = match recv.op {
        Some(_) => static_caller(fx, bypass),
        None => Some(FCALL),
    };
    let status = match (implicit_caller, recv.op) {
        (Some(caller), _) => {
            let cache = fx.callsite_ptr(caller);
            fx.call(
                "zeo_rt_send_value_kw_cached",
                &[
                    cache, zero_box, recv_ptr, sym, argv_ptr, argc_v, kw_ptr, blk_ptr, out,
                ],
            )
        }
        (None, Some(_)) => {
            let caller = caller_class(fx, bypass);
            fx.call(
                "zeo_rt_send_value_explicit_kw_in",
                &[
                    zero_box, recv_ptr, sym, argv_ptr, argc_v, kw_ptr, blk_ptr, caller, out,
                ],
            )
        }
        (None, None) => fx.call(
            "zeo_rt_send_value_kw_in",
            &[
                zero_box, recv_ptr, sym, argv_ptr, argc_v, kw_ptr, blk_ptr, out,
            ],
        ),
    }
    .expect("kw sends return a status");
    if blk.is_open() {
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
/// `Object` is what the check compares against there.
///
/// `bypass` is ruby's `VM_CALL_FCALL`: the site runs no check at all. A
/// literal `self` receiver never reaches here (it takes the implicit entry --
/// see `expr::self_receiver`), but the `self.singleton_class` a `class <<
/// self` body's statement is rebound onto does: its ruby form is
/// RECEIVERLESS, so the surrogate must be the receiver AND the barrier must
/// stay down.
/// Ruby's `VM_CALL_FCALL`: the caller class that means "run no visibility
/// check". The runtime spells it the same way (`dispatch::caches::FCALL`).
pub(crate) const FCALL: u32 = u32::MAX;

/// The caller class as a COMPILE-TIME constant, or `None` when only the
/// run time can answer -- which is the whole gate on caching a site.
pub(crate) fn static_caller(fx: &Fx, bypass: bool) -> Option<u32> {
    if bypass {
        return Some(FCALL);
    }
    if fx.self_is_dynamic {
        return None;
    }
    Some(lexical_caller(fx))
}

/// The caller identity a `protected` target is measured against: the CLASS OF
/// `self`, which is CRuby's `rb_obj_is_kind_of(self, defined_class)`.
///
/// In an instance method that is the enclosing class. In a CLASS method it is
/// `Class` (or `Module`), because `self` there IS the class object -- and a
/// class object is no kind of the class whose instances the protected method
/// belongs to. Answering `method_class` for both made `def self.peek(a);
/// a.balance; end` reach a protected `balance` that ruby refuses.
fn lexical_caller(fx: &Fx) -> u32 {
    if !fx.self_is_class {
        return fx.method_class.map_or(0, |c| c.0);
    }
    match fx.method_class.is_some_and(|c| {
        fx.an
            .compiler
            .classes
            .get(c.0 as usize)
            .is_some_and(|ci| ci.is_module)
    }) {
        true => zeo_abi::MODULE_CLASS.0,
        false => zeo_abi::CLASS_CLASS.0,
    }
}

pub(crate) fn caller_class(fx: &mut Fx, bypass: bool) -> cranelift_codegen::ir::Value {
    if bypass {
        return fx.b.ins().iconst(types::I32, i64::from(u32::MAX));
    }
    // A scope whose `self` only the run time knows (a `Class.new` body's
    // method, an `instance_eval` block) has no lexical class to compare a
    // `protected` target against -- the enclosing one is `Object`, which is
    // no kind of the class the body will belong to, and the call was
    // refused. Ask `self` instead, per call.
    if fx.self_is_dynamic {
        let slf = fx.self_ptr.expect("self_ptr is set in the prologue");
        return fx.call_status("zeo_rt_class_of", &[slf]);
    }
    let cid = lexical_caller(fx);
    fx.cid_value(cid)
}

/// An explicit-receiver dynamic send through the uncached entry.
pub(crate) fn dynamic_send(
    fx: &mut Fx,
    site: NodeId,
    recv: NodeId,
    name: &str,
    args: &[ArrayElem],
) -> CResult<Operand> {
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
) -> CResult<Operand> {
    let recv_class = recv_op.class_id();
    let recv_ptr = ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    let argv_ptr = build_argv(fx, site, args)?;
    super::stmt::stamp_call_line(fx, site);
    dynamic_send_argv(fx, recv_ptr, recv_class, name, argv_ptr, args.len(), bypass)
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
) -> CResult<Operand> {
    let recv_class = recv_op.class_id();
    let recv_ptr = ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    dynamic_send_argv(fx, recv_ptr, recv_class, name, argv_ptr, argc, false)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one send's own shape: receiver, its static class, name, argv, arity, barrier"
)]
fn dynamic_send_argv(
    fx: &mut Fx,
    recv_ptr: cranelift_codegen::ir::Value,
    recv_class: Option<u32>,
    name: &str,
    argv_ptr: cranelift_codegen::ir::Value,
    argc: usize,
    bypass: bool,
) -> CResult<Operand> {
    let sym = fx.sym_id(name);
    let zero_box = fx.box_v();
    let argc_v = fx.b.ins().iconst(fx.em.ptr, argc as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    // A site's caller class is a per-site CONSTANT the cache is vetted
    // against once, so only a statically known caller can cache. A body
    // whose `self` only the run time knows asks `self` per call
    // (`Caller::Runtime`) and keeps the uncached entry.
    let caller = static_caller(fx, bypass);
    // A CLASS receiver never fills the value cache -- `send_value_cached`
    // rules it out on purpose, since a class value's methods resolve
    // through a singleton-chain arm of its own -- so `Foo.new` walks that
    // chain on every call unless it gets the class-method cache instead.
    // The emitted `cid` is compared against the receiver at run time, so a
    // constant reassigned since is a MISS, never a wrong answer.
    // `send_class_cached` falls back through box 0, so a boxed body keeps
    // the ordinary route.
    let class_cached = match (recv_class, caller) {
        (Some(cid), Some(caller)) if fx.box_id == 0 => Some((cid, caller)),
        _ => None,
    };
    let status = match (class_cached, caller) {
        (Some((cid, caller)), _) => {
            let site = fx.cm_site_ptr();
            let cid_v = fx.cid_value(cid);
            let caller_v = fx.cid_value(caller);
            fx.call(
                "zeo_rt_send_class_cached",
                &[
                    site, cid_v, recv_ptr, sym, argv_ptr, argc_v, null, caller_v, out,
                ],
            )
        }
        (None, Some(caller)) => {
            let cache = fx.callsite_ptr(caller);
            fx.call(
                "zeo_rt_send_value_cached",
                &[cache, zero_box, recv_ptr, sym, argv_ptr, argc_v, null, out],
            )
        }
        (None, None) => {
            // A DYNAMIC caller: the receiver-keyed cache still serves the
            // site (define_method bodies, instance_exec re-homed blocks),
            // with the visibility vet split so the per-call caller is
            // asked only its caller-dependent remainder.
            let site = fx.dyn_site_ptr();
            let caller = caller_class(fx, bypass);
            fx.call(
                "zeo_rt_send_value_dyn_cached",
                &[
                    site, zero_box, recv_ptr, sym, argv_ptr, argc_v, null, caller, out,
                ],
            )
        }
    }
    .expect("send returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A TYPED direct call: `obj.m(a, b)` where analyze proved the
/// receiver's static class (`Compiler::typed_call_sites`) and the
/// emitter holds that class's compiled body (`Emitter::typed_methods`).
/// `None` = the site does not qualify and the caller lowers as before.
///
/// The guard is four questions, `indexed_send`'s ladder: the receiver
/// tag must be Object; no gate in
/// [`zeo_abi::abi::GATE_TYPED_DIRECT_SLOW`] may be armed; the NOMINATED
/// CLASS's own bit in `zeo_rt_patched_bits` must be clear; and
/// `zeo_rt_class_of` must answer the nominated class EXACTLY -- a
/// subclass instance takes the slow arm, so a wrong static type is a
/// slow path, never a wrong answer. Visibility needs no runtime half:
/// nomination proved the method compile-time public, and a runtime
/// `private :m` patches the class, which the bit test routes away.
///
/// The third question used to be part of the second: the guard tested the
/// whole gate word for ZERO, so one `define_method` on an unrelated class
/// -- one `attr_accessor`, one `alias_method` -- sent every typed call in
/// the program down dispatch for good, measured at +54%. The bit test
/// costs no more than the zero test did: `zeo_rt_patched_bits` is a fixed
/// static array, so the word's address and the mask are both settled at
/// compile time.
///
/// Every operand is evaluated ONCE, in ruby's order, into the same
/// argv both arms read -- the slow arm never re-lowers. The body pushes
/// its own frame (every compiled body's prologue does), so backtraces
/// are call-path-identical.
pub(crate) fn typed_direct_send(
    fx: &mut Fx,
    site: NodeId,
    recv: NodeId,
    name: &str,
    args: &[ArrayElem],
) -> CResult<Option<Operand>> {
    use cranelift_codegen::ir::MemFlagsData;
    use cranelift_codegen::ir::condcodes::IntCC;
    use zeo_abi::abi::{TAG_OFFSET, ValueTag};
    if crate::debug_flags::debug(crate::debug_flags::DebugFlag::NoTypedCalls) {
        return Ok(None);
    }
    let Some(&cid) = fx.an.compiler.typed_call_sites.get(&site) else {
        return Ok(None);
    };
    let trace = crate::debug_flags::debug(crate::debug_flags::DebugFlag::TraceTyped);
    if trace {
        eprintln!("typed_direct_send: site {site:?} cid {} name {name}", cid.0);
    }
    // The guard reads this class's bit out of a fixed-size bitmap; a class id
    // past its span has no bit to read, so the site keeps dispatch.
    if cid.0 >= zeo_abi::abi::PATCHED_BITS_IDS {
        return Ok(None);
    }
    // A box may carry its own overlay patch; boxed callers keep dispatch.
    if fx.box_id != 0 {
        return Ok(None);
    }
    let Some(decl) = fx.em.typed_methods.get(&(cid.0, name.to_string())) else {
        if trace {
            eprintln!(
                "typed_direct_send: no typed_methods entry for ({}, {name})",
                cid.0
            );
        }
        return Ok(None);
    };
    if trace {
        eprintln!(
            "typed_direct_send: decl plain={} reopen={} concealed={} arity={} args={} kw={:?}",
            decl.plain,
            decl.reopen_flagged,
            decl.concealed,
            decl.arity,
            args.len(),
            decl.kw_direct
        );
    }
    // Plain positional bodies only, count-matched -- the receiverless
    // direct path's own gate. An arity MISMATCH must raise through
    // dispatch (the runtime owns the error), not bind wrong.
    if !decl.plain
        || decl.reopen_flagged
        || decl.concealed
        || decl.arity != args.len()
        || decl.kw_direct.is_some()
    {
        return Ok(None);
    }
    let (body_id, has_blk) = (decl.body, decl.has_blk);
    if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
        return Ok(None);
    }
    if trace {
        eprintln!(
            "typed_direct_send: emitting direct call for {name} on class {}",
            cid.0
        );
    }

    let bypass = super::expr::bypasses_visibility(fx, Some(recv));
    let later = super::expr::later_nodes(args, &[], None);
    let recv_op = lower_expr(fx, recv)?;
    let recv_op = super::expr::park_reassignable(fx, Some(recv), recv_op, &later);
    let recv_class = recv_op.class_id();
    let recv_ptr = ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    let argv_ptr = build_argv(fx, site, args)?;
    super::stmt::stamp_call_line(fx, site);

    // Packaged id mode: an own-band id is a table LOAD (see
    // `Fx::cid_value`), so the bitmap word offset, the bit mask, and the
    // class comparison below all compute from the loaded value instead of
    // being baked. Emitted here, in the entry flow, so the value dominates
    // both guard blocks. `None` keeps today's immediates byte for byte.
    let dyn_cid = match fx.em.id_mode {
        super::module::IdMode::Packaged { first } if cid.0 >= first => Some(fx.cid_value(cid.0)),
        _ => None,
    };

    let gates_chk = fx.b.create_block();
    let patch_chk = fx.b.create_block();
    let class_chk = fx.b.create_block();
    let fast = fx.b.create_block();
    let slow = fx.b.create_block();
    let join = fx.b.create_block();
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let fl = MemFlagsData::trusted();

    let tag = fx.b.ins().load(types::I8, fl, recv_ptr, TAG_OFFSET as i32);
    let is_obj =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, tag, i64::from(ValueTag::Object as u8));
    fx.b.ins().brif(is_obj, gates_chk, &[], slow, &[]);

    fx.b.switch_to_block(gates_chk);
    let gbase = fx.gates_base();
    let gates = fx.b.ins().load(types::I16, fl, gbase, 0);
    let wide =
        fx.b.ins()
            .band_imm_u(gates, i64::from(zeo_abi::abi::GATE_TYPED_DIRECT_SLOW));
    fx.b.ins().brif(wide, slow, &[], patch_chk, &[]);

    fx.b.switch_to_block(patch_chk);
    let pbase = fx.patched_bits_base();
    let bit = match dyn_cid {
        None => {
            let word =
                fx.b.ins()
                    .load(types::I64, fl, pbase, ((cid.0 / 64) * 8) as i32);
            fx.b.ins().band_imm_u(word, (1u64 << (cid.0 % 64)) as i64)
        }
        Some(cv) => {
            let widx = fx.b.ins().ushr_imm_u(cv, 6);
            let widx64 = fx.b.ins().uextend(types::I64, widx);
            let boff = fx.b.ins().ishl_imm_u(widx64, 3);
            let addr = fx.b.ins().iadd(pbase, boff);
            let word = fx.b.ins().load(types::I64, fl, addr, 0);
            let sh = fx.b.ins().band_imm_u(cv, 63);
            let sh64 = fx.b.ins().uextend(types::I64, sh);
            let shifted = fx.b.ins().ushr(word, sh64);
            fx.b.ins().band_imm_u(shifted, 1)
        }
    };
    fx.b.ins().brif(bit, slow, &[], class_chk, &[]);

    fx.b.switch_to_block(class_chk);
    let live_cid = fx.call_status("zeo_rt_class_of", &[recv_ptr]);
    let hit = match dyn_cid {
        None => {
            fx.b.ins()
                .icmp_imm_u(IntCC::Equal, live_cid, i64::from(cid.0))
        }
        Some(cv) => fx.b.ins().icmp(IntCC::Equal, live_cid, cv),
    };
    fx.b.ins().brif(hit, fast, &[], slow, &[]);

    fx.b.switch_to_block(fast);
    let fref = fx.em.module.declare_func_in_func(body_id, fx.b.func);
    let mut call_args = Vec::with_capacity(args.len() + 3);
    call_args.push(recv_ptr);
    for i in 0..args.len() {
        let p = if i == 0 {
            argv_ptr
        } else {
            fx.b.ins()
                .iadd_imm_u(argv_ptr, i64::from(i as u32 * VALUE_SIZE))
        };
        call_args.push(p);
    }
    if has_blk {
        call_args.push(fx.b.ins().iconst(fx.em.ptr, 0));
    }
    call_args.push(out);
    let inst = fx.b.ins().call(fref, &call_args);
    let status = fx.b.func.dfg.inst_results(inst)[0];
    fx.fallible(status);
    fx.owned_created += 1;
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(slow);
    let r = dynamic_send_argv(fx, recv_ptr, recv_class, name, argv_ptr, args.len(), bypass)?;
    ownership::write_move_into(fx, &r, out);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(join);
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// `a[i]` / `a[i] = v` under a run-time Array + Int guard: the aref/aset
/// core called directly, with today's cached dynamic send as the other
/// arm. `None` = the site does not qualify (wrong shape, or the program
/// itself touches `Array#[]`/`#[]=`) and the caller lowers as before.
///
/// The guard is four questions, and it is `typed_direct_send`'s ladder with
/// `Array` for the nominated class. Receiver tag == Array and index tag ==
/// Int prove the shapes; no gate in [`zeo_abi::abi::GATE_ITER_INLINE_SLOW`]
/// may be armed; and `Array`'s own bit in `zeo_rt_patched_bits` must be
/// clear. Both rows sit in `SPECIALIZED` (no synthetic frame), so the
/// frameless core is backtrace-identical.
///
/// The last two used to be one test for a ZERO gate word, with a capi call
/// to `iter_inline_ok_for` behind it -- so a program that ran any runtime
/// definition paid a CALL on every array index, measured at +14%. The two
/// inline tests cost the same as the zero test did.
///
/// A boxed site declines outright rather than emitting a guard that can
/// never pass: `iter_inline_ok_for` refused a nonzero box anyway, and the
/// box is a per-site constant.
///
/// Every operand is evaluated ONCE, in ruby's order, into the same argv
/// both arms read -- the slow arm never re-lowers.
pub(crate) fn indexed_send(
    fx: &mut Fx,
    site: NodeId,
    recv: NodeId,
    name: &str,
    args: &[ArrayElem],
) -> CResult<Option<Operand>> {
    use cranelift_codegen::ir::MemFlagsData;
    use cranelift_codegen::ir::condcodes::IntCC;
    use zeo_abi::abi::{PAYLOAD_OFFSET, TAG_OFFSET, ValueTag};
    let aset = match (name, args.len()) {
        ("[]", 1) => false,
        ("[]=", 2) => true,
        _ => return Ok(None),
    };
    if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) || fx.box_id != 0 {
        return Ok(None);
    }
    // The program's own text touching the rows stands the whole site down:
    // a document-position reopen applies POSITIONALLY, which only the real
    // dispatch route honors.
    let compiler = &fx.an.compiler;
    if compiler
        .method_in_chain(crate::compiler::ARRAY_CLASS, name)
        .is_some()
        || compiler.may_be_patched_at_runtime(name)
    {
        return Ok(None);
    }

    let bypass = super::expr::bypasses_visibility(fx, Some(recv));
    let later = super::expr::later_nodes(args, &[], None);
    let recv_op = lower_expr(fx, recv)?;
    let recv_op = super::expr::park_reassignable(fx, Some(recv), recv_op, &later);
    let recv_class = recv_op.class_id();
    let recv_ptr = ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    let argv_ptr = build_argv(fx, site, args)?;
    super::stmt::stamp_call_line(fx, site);

    let idx_chk = fx.b.create_block();
    let gates_chk = fx.b.create_block();
    let patch_chk = fx.b.create_block();
    let fast = fx.b.create_block();
    let slow = fx.b.create_block();
    let join = fx.b.create_block();
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let fl = MemFlagsData::trusted();

    let tag = fx.b.ins().load(types::I8, fl, recv_ptr, TAG_OFFSET as i32);
    let is_arr =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, tag, i64::from(ValueTag::Array as u8));
    fx.b.ins().brif(is_arr, idx_chk, &[], slow, &[]);

    fx.b.switch_to_block(idx_chk);
    let itag = fx.b.ins().load(types::I8, fl, argv_ptr, TAG_OFFSET as i32);
    let is_int =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, itag, i64::from(ValueTag::Int as u8));
    fx.b.ins().brif(is_int, gates_chk, &[], slow, &[]);

    fx.b.switch_to_block(gates_chk);
    let gbase = fx.gates_base();
    let gates = fx.b.ins().load(types::I16, fl, gbase, 0);
    let wide =
        fx.b.ins()
            .band_imm_u(gates, i64::from(zeo_abi::abi::GATE_ITER_INLINE_SLOW));
    fx.b.ins().brif(wide, slow, &[], patch_chk, &[]);

    fx.b.switch_to_block(patch_chk);
    let acid = crate::compiler::ARRAY_CLASS.0;
    let pbase = fx.patched_bits_base();
    let word =
        fx.b.ins()
            .load(types::I64, fl, pbase, ((acid / 64) * 8) as i32);
    let bit = fx.b.ins().band_imm_u(word, (1u64 << (acid % 64)) as i64);
    fx.b.ins().brif(bit, slow, &[], fast, &[]);

    fx.b.switch_to_block(fast);
    let idx =
        fx.b.ins()
            .load(types::I64, fl, argv_ptr, PAYLOAD_OFFSET as i32);
    if aset {
        let vptr = fx.b.ins().iadd_imm_u(argv_ptr, i64::from(VALUE_SIZE));
        let status = fx.call_status("zeo_rt_array_aset_int", &[recv_ptr, idx, vptr, out]);
        fx.fallible(status);
    } else {
        fx.call("zeo_rt_array_aref_int", &[recv_ptr, idx, out]);
    }
    fx.owned_created += 1;
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(slow);
    let r = dynamic_send_argv(fx, recv_ptr, recv_class, name, argv_ptr, args.len(), bypass)?;
    ownership::write_move_into(fx, &r, out);
    fx.b.ins().jump(join, &[]);

    fx.b.switch_to_block(join);
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

/// What a BARE `super` forwards: the enclosing method's parameters read by
/// NAME at the moment it runs -- rest splatted in place, keywords gathered
/// into one hash, and `unmark` saying whether a splatted tail keeps its
/// keyword mark (the `ruby2_keywords` opt-out). Answers
/// `(args Array, kw Hash or null, unmark)`.
///
/// Shared with the eval home (`emit::method_body`): a snippet's own bare
/// `super` forwards the same list, published when the enclosing method
/// starts because the snippet has no parameter list of its own.
pub(crate) fn build_zsuper_args(
    fx: &mut Fx,
    params: &crate::hir::Params,
) -> CResult<(
    cranelift_codegen::ir::Value,
    cranelift_codegen::ir::Value,
    bool,
)> {
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
        let status = fx.call_status("zeo_rt_array_push_splat", &[out, p]);
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
            let status = fx.call_status("zeo_rt_hash_set", &[kw, sptr, vp]);
            fx.fallible(status);
        }
        if let Some(Some(krest)) = &params.keyword_rest {
            let op = ownership::read_local(fx, krest).expect("a param is always bound");
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            let status = fx.call_status("zeo_rt_kw_splat_into", &[kw, p]);
            fx.fallible(status);
        }
        kw
    } else {
        fx.b.ins().iconst(fx.em.ptr, 0)
    };
    let unmark = matches!(params.rest, Some(Some(_))) && !fx.ruby2_keywords;
    Ok((out, kw_ptr, unmark))
}

/// `super` -- the instance-method channels only
/// (class-method/singleton-chain `super` and `super` inside a block still
/// refuse). One runtime mechanism: `send_super_from`'s per-position MRO
/// walk resumes AFTER the class the `def` was WRITTEN in
/// (`fx.defining_class` -- a module method's copy keeps the module, which
/// sits in the receiver's ancestry); a value-builtin subclass whose walk
/// finds no user definition above bridges into the native root instead
/// (`value_super`). Bare `super` forwards the current method's own params
/// by NAME (splat rest, keywords as one marked hash); the current
/// block forwards unless the site writes one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_super(
    fx: &mut Fx,
    site: NodeId,
    args: &[ArrayElem],
    kwargs: &[crate::hir::KwArg],
    zsuper: bool,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> CResult<Operand> {
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
        let status = fx.call_status("zeo_rt_raise_error", &[cid, mptr, mlen]);
        fx.fallible(status);
        return Ok(Operand::Nil);
    }
    // A snippet has no method of its own, and yet the eval may sit inside
    // one -- the walk below resumes from the (class, name) pair the
    // method-frame stack recorded, which is exactly what a `super` written
    // in an eval must do. A BARE `super` still cannot: it forwards the
    // enclosing method's own arguments, and no channel hands a snippet
    // those, so it says so rather than forwarding an empty list.
    let params = match fx.method_params.clone() {
        Some(p) => p,
        None if fx.eval_mode.is_some() && !zsuper => crate::hir::Params::default(),
        // A BARE `super` forwards the enclosing METHOD's arguments, which
        // that method published for the length of the eval.
        None if fx.eval_mode.is_some() => {
            let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
            let ss = fx.temp_slot();
            let out = fx.slot_addr(ss, 0);
            let status = fx.call_status("zeo_rt_eval_super", &[self_ptr, out]);
            fx.fallible(status);
            fx.owned_created += 1;
            return Ok(Operand::Slot {
                ss,
                owned: true,
                tag: TagInfo::Unknown,
            });
        }
        // A block with no method of its own may still BECOME one at run
        // time (`K.define_method(:m) { }`, and the same call through `send`,
        // which the compiler cannot recognize as a definition at all). An
        // explicit-argument `super` there travels the dynamic walk below,
        // which raises for itself when there is no method frame; a BARE one
        // cannot, and the run time picks which of the two refusals ruby
        // gives.
        None if zsuper => {
            let status = fx.call_status("zeo_rt_bare_super_outside_a_method", &[]);
            fx.fallible(status);
            return Ok(Operand::Nil);
        }
        None => crate::hir::Params::default(),
    };

    // The argument Array (rustc's `__super_args` Vec) + kw hash + unmark.
    let (args_ptr, kw_ptr, unmark) = if zsuper {
        build_zsuper_args(fx, &params)?
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
            let status = fx.call_status("zeo_rt_block_arg_to_proc", &[vp, conv]);
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
    if fx.runtime_method_body || fx.method_name.is_none() {
        let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let unmark_v =
            fx.b.ins()
                .iconst(cranelift_codegen::ir::types::I8, i64::from(unmark));
        let status = fx.call_status(
            "zeo_rt_send_super_dynamic_args",
            &[self_ptr, args_ptr, unmark_v, kw_ptr, blk_ptr, out],
        );
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
        return super_outside_a_method(fx);
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
                fx.call_status(
                    "zeo_rt_call_singleton_super_target_args",
                    &[
                        t_v, mi, recv_v, sym, args_ptr, unmark_v, kw_ptr, blk_ptr, out,
                    ],
                )
            }
            None => {
                let def_v =
                    fx.b.ins()
                        .iconst(cranelift_codegen::ir::types::I32, i64::from(resume.0));
                fx.call_status(
                    "zeo_rt_send_super_class_from_args",
                    &[recv_v, def_v, sym, args_ptr, unmark_v, kw_ptr, blk_ptr, out],
                )
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
        fx.call_status(
            "zeo_rt_value_super_args",
            &[
                self_ptr, nptr, nlen, args_ptr, unmark_v, kw_ptr, blk_ptr, out,
            ],
        )
    } else {
        let def_v =
            fx.b.ins()
                .iconst(cranelift_codegen::ir::types::I32, i64::from(def_class.0));
        let sym = fx.sym_id(&mname);
        fx.call_status(
            "zeo_rt_send_super_from_args",
            &[
                self_ptr, def_v, sym, args_ptr, unmark_v, kw_ptr, blk_ptr, out,
            ],
        )
    };
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// `super` with no enclosing method at all -- top level, a top-level block,
/// a class body. Real Ruby raises at RUNTIME and the raise is rescuable
/// (`vm_insnhelper.c`), so emit it instead of refusing to compile; rustc's
/// `super_calls` arm does the same, message verbatim.
fn super_outside_a_method(fx: &mut Fx) -> CResult<Operand> {
    let cid = fx.b.ins().iconst(
        cranelift_codegen::ir::types::I32,
        i64::from(zeo_abi::NO_METHOD_ERROR_CLASS.0),
    );
    let (mptr, mlen) = super::expr::rodata_name(fx, "super called outside of method");
    let status = fx.call_status("zeo_rt_raise_error", &[cid, mptr, mlen]);
    fx.fallible(status);
    Ok(Operand::Nil)
}
