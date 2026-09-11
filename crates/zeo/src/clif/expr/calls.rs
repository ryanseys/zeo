//! Lowering one call: the five written shapes the big match dispatches
//! on, and the questions each asks about its receiver and its site.

use super::*;

/// `recv&.name(...)`: the nil-test diamond. A nil receiver answers nil
/// without evaluating the arguments or building the block (ruby's rule,
/// oracle-verified) -- so the whole argument build sits in the call arm.
#[expect(clippy::too_many_arguments, reason = "one lowering fact per parameter")]
pub(super) fn safe_nav_call(
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
        crate::clif::call::splat_send(
            fx,
            id,
            crate::clif::call::Recv::maybe(through, bypass),
            &name,
            &args,
            &kwargs,
            blk,
        )?
    } else if !kwargs.is_empty() {
        crate::clif::call::kw_send(
            fx,
            id,
            crate::clif::call::Recv::maybe(through, bypass),
            &name,
            &args,
            &kwargs,
            blk,
        )?
    } else if blk.is_open() {
        crate::clif::blocks::send_with_block_ptr_ops(fx, id, through, &name, &args, blk, bypass)?
    } else if let Some(op) = through {
        crate::clif::call::dynamic_send_value(fx, id, op, &name, &args, bypass)?
    } else {
        crate::clif::call::implicit_send(fx, id, &name, &args)?
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
pub(super) fn literal_block_call(
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
    if let Some(kind) = fx.an.compiler.inline_iter_sites.get(&blk)
        && crate::clif::iter::fusable_block(fx, blk, kind.max_fused_params())
        && let Some(r) = receiver
        && match kind {
            // `inject` fuses with exactly its explicit seed argument.
            crate::compiler::InlineIterKind::ArrayInject => {
                matches!(args.as_slice(), [ArrayElem::Single(_)])
            }
            _ => args.is_empty(),
        }
        && let Some((acc, bind)) = match kind {
            crate::compiler::InlineIterKind::ArrayEach => Some((
                crate::clif::iter::Acc::None,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArrayEachWithIndex => Some((
                crate::clif::iter::Acc::None,
                crate::clif::iter::Bind::ElementIndex,
            )),
            crate::compiler::InlineIterKind::ArrayCount => Some((
                crate::clif::iter::Acc::Count,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArrayAll => Some((
                crate::clif::iter::Acc::All,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArrayAny => Some((
                crate::clif::iter::Acc::Any,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArrayNone => Some((
                crate::clif::iter::Acc::NonePred,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArrayFind => Some((
                crate::clif::iter::Acc::Find,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArrayMap => Some((
                crate::clif::iter::Acc::Map,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArraySelect => Some((
                crate::clif::iter::Acc::Select,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArrayReject => Some((
                crate::clif::iter::Acc::Reject,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArraySum => Some((
                crate::clif::iter::Acc::Sum,
                crate::clif::iter::Bind::Element,
            )),
            crate::compiler::InlineIterKind::ArrayInject => Some((
                crate::clif::iter::Acc::Inject,
                crate::clif::iter::Bind::AccElement,
            )),
            _ => None,
        }
    {
        // The slow arm dispatches the NAME AS WRITTEN (`detect` stays
        // `detect` -- a runtime singleton may define only one alias).
        return Ok(crate::clif::iter::lower_array_each(
            fx, id, r, blk, true, acc, bind, &args, &name,
        )?
        .expect("a wanted result is always built"));
    }
    if args.is_empty()
        && crate::clif::iter::fusable_block(fx, blk, 1)
        && let Some(r) = receiver
        && fx.an.compiler.inline_iter_sites.get(&blk)
            == Some(&crate::compiler::InlineIterKind::TimesInt)
    {
        return Ok(crate::clif::iter::lower_counted_int(fx, id, r, blk, true)?
            .expect("a wanted result is always built"));
    }
    if crate::clif::iter::fusable_block(fx, blk, 1)
        && let Some(r) = receiver
        && let Some(down) = match fx.an.compiler.inline_iter_sites.get(&blk) {
            Some(crate::compiler::InlineIterKind::UptoInt) => Some(false),
            Some(crate::compiler::InlineIterKind::DowntoInt) => Some(true),
            _ => None,
        }
        && let [arg] = args.as_slice()
        && matches!(
            arg,
            ArrayElem::Single(a) if matches!(
                fx.an.compiler.hir[*a],
                crate::hir::HirNode::IntegerLit(_)
                    | crate::hir::HirNode::FloatLit(_)
                    | crate::hir::HirNode::LocalRead(_)
            )
        )
    {
        return Ok(
            crate::clif::iter::lower_up_down_int(fx, id, r, arg, blk, true, down, &name)?
                .expect("a wanted result is always built"),
        );
    }
    if args.is_empty()
        && crate::clif::iter::fusable_block(fx, blk, 1)
        && let Some(counted) = crate::clif::iter::counted_of(fx, receiver, &name, true)
    {
        let ss = fx.temp_slot();
        let dst = fx.slot_addr(ss, 0);
        crate::clif::iter::lower_counted(
            fx,
            id,
            &counted,
            blk,
            Some(dst),
            crate::clif::iter::Acc::None,
            crate::clif::iter::Bind::Element,
            None,
        )?;
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
        && !decl.reopen_flagged
        && !decl.concealed
        && decl.arity == args.len()
        && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
        && !method_class_shadows(fx, &name)
    {
        return crate::clif::call::direct_call(fx, id, &name, &args, Some(blk));
    }
    // A splatted argument list builds its Array in the runtime, so
    // it takes the args entry with the block on the same channel.
    if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
        let recv = match receiver {
            Some(r) => Some(lower_expr(fx, r)?),
            None => None,
        };
        let bypass = bypasses_visibility(fx, receiver);
        return crate::clif::call::splat_send(
            fx,
            id,
            crate::clif::call::Recv::maybe(recv, bypass),
            &name,
            &args,
            &[],
            crate::clif::blocks::BlockChannel::Literal(blk),
        );
    }
    crate::clif::blocks::block_send(fx, id, receiver, &name, &args, blk)
}

/// A plain send (no block, no keywords): the compile-time folds
/// (`Module.nesting`, `Ruby::Box.current`, `__method__`, method
/// captures, `binding`, `eval`), then the splat entry, the operator
/// fast path, the direct call, and the dynamic sends.
pub(super) fn plain_call(
    fx: &mut Fx,
    id: NodeId,
    receiver: Option<NodeId>,
    name: String,
    args: Vec<ArrayElem>,
) -> CResult<Operand> {
    if let Some(op) = crate::clif::boxes::module_nesting(fx, receiver, &name, &args)? {
        return Ok(op);
    }
    if let Some(op) = crate::clif::boxes::box_current(fx, receiver, &name, &args)? {
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
        return crate::clif::consts::symbol_value(fx, &origin);
    }
    // `__callee__`'s half. The frame label carries the name the body was
    // BORN as, which is what a backtrace shows. A compile-time alias is
    // emitted as its own body, whose `method_name` IS the alias; a run-time
    // alias copies this body and hands the name it was called through to
    // the frame, so the frame answers first and the known name stands in.
    if receiver.is_none()
        && args.is_empty()
        && name == "__callee__"
        && let Some(called) = fx.method_name.clone()
    {
        let fallback = fx.sym_id(&called);
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        fx.call("zeo_rt_frame_callee", &[fallback, out]);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Known(ValueTag::Symbol as u8),
        });
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
        return crate::clif::call::splat_send(
            fx,
            id,
            crate::clif::call::Recv::maybe(recv, bypass),
            &name,
            &args,
            &[],
            crate::clif::blocks::BlockChannel::None,
        );
    }
    match receiver {
        Some(recv) if crate::clif::binop::operator_fast_path(fx, &name) && args.len() == 1 => {
            let [ArrayElem::Single(arg)] = args.as_slice() else {
                return fx.unsupported(id, "a splat operand");
            };
            crate::clif::binop::binop(fx, id, &name, recv, *arg)
        }
        Some(recv) if name == "nil?" && args.is_empty() => match nil_p_call(fx, id, recv)? {
            Some(fold) => Ok(fold),
            None => crate::clif::call::dynamic_send(fx, id, recv, &name, &args),
        },
        Some(recv) if let Some(site) = fx.an.compiler.accessor_sites.get(&id).copied() => {
            crate::clif::boxes::explicit_accessor(fx, id, recv, &name, &args, site)
        }
        Some(recv) if fx.an.compiler.typed_call_sites.contains_key(&id) => {
            match crate::clif::call::typed_direct_send(fx, id, recv, &name, &args)? {
                Some(fast) => Ok(fast),
                None => crate::clif::call::dynamic_send(fx, id, recv, &name, &args),
            }
        }
        Some(recv) => match crate::clif::call::indexed_send(fx, id, recv, &name, &args)? {
            Some(fast) => Ok(fast),
            None => crate::clif::call::dynamic_send(fx, id, recv, &name, &args),
        },
        None if let Some(folded) =
            crate::clif::boxes::inline_accessor(fx, id, &name, &args, &[], None, None) =>
        {
            folded
        }
        None => match fx.em.methods.get(&name) {
            Some(decl)
                if decl.plain
                    && !decl.reopen_flagged
                    && !decl.concealed
                    && decl.arity == args.len()
                    && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
                    && !method_class_shadows(fx, &name) =>
            {
                crate::clif::call::direct_call(fx, id, &name, &args, None)
            }
            // Unknown names and arity mismatches go through the
            // implicit-self dynamic send (the runtime raises the
            // NoMethodError/ArgumentError).
            Some(_) | None => crate::clif::call::implicit_send(fx, id, &name, &args),
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
pub(super) fn nil_p_call(fx: &mut Fx, id: NodeId, recv: NodeId) -> CResult<Option<Operand>> {
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
    let r = crate::clif::call::dynamic_send_value(fx, id, borrowed, "nil?", &[], bypass)?;
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
#[expect(clippy::too_many_arguments, reason = "one lowering fact per parameter")]
pub(super) fn keyword_call(
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
    if receiver.is_none()
        && block_arg.is_none()
        && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
        && !method_class_shadows(fx, &name)
        && fx
            .em
            .methods
            .get(&name)
            .is_some_and(|d| d.arity == args.len())
        && let Some(order) = crate::clif::call::kw_direct_order(fx, &name, &kwargs)
    {
        return crate::clif::call::direct_call_kw(
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
        crate::clif::call::splat_send(
            fx,
            id,
            crate::clif::call::Recv::maybe(recv, bypass),
            &name,
            &args,
            &kwargs,
            blk,
        )
    } else {
        crate::clif::call::kw_send(
            fx,
            id,
            crate::clif::call::Recv::maybe(recv, bypass),
            &name,
            &args,
            &kwargs,
            blk,
        )
    }
}

/// The block channel a call opens, if any: a literal block builds its
/// proc, a `&expr` argument converts through `to_proc` (nil = no block).
/// Ruby's grammar admits only one of the two.
pub(super) fn block_channel(
    fx: &mut Fx,
    id: NodeId,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> CResult<crate::clif::blocks::BlockChannel> {
    use crate::clif::blocks::BlockChannel;
    match (block, block_arg) {
        (None, None) => Ok(BlockChannel::None),
        // The literal stays UNBUILT; the `&expr` conversion cannot, since
        // it runs ruby code of its own and belongs in written order.
        (Some(blk), None) => Ok(BlockChannel::Literal(blk)),
        (None, Some(ba)) => Ok(BlockChannel::Ready(crate::clif::blocks::block_arg_ptr(
            fx, ba,
        )?)),
        (Some(_), Some(_)) => fx.unsupported(id, "a literal block beside a `&` block argument"),
    }
}

/// `K.method(:name)` captured BEFORE every own `def self.name` in the
/// document: CRuby resolves at capture time, so the Method binds the
/// INHERITED entry and a by-name capture would recurse forever through
/// the later override (rspec-support's `NEW_MUTEX_METHOD =
/// Mutex.method(:new)` / `def self.new = NEW_MUTEX_METHOD.call` pair).
pub(super) fn method_capture_intrinsic(
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
    let Some(target) = crate::clif::boxes::resolve_class_here(fx, &path) else {
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
    let recv_ptr = crate::clif::ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        crate::clif::ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    let sym_op = lower_expr(fx, sym_id)?;
    let sym_ptr = crate::clif::ownership::borrow_ptr(fx, &sym_op);
    if sym_op.owned() {
        crate::clif::ownership::pool_owned(fx, sym_ptr, sym_op.tag());
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
    if !matches!(fx.locals.get(&name), Some(crate::clif::ctx::Local::Slot(_))) {
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

pub(super) fn self_receiver(fx: &Fx, recv: Option<NodeId>) -> Option<NodeId> {
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
    // A PACKAGE never binds a top-level body directly: this tier carries
    // no guard, and the program the object links into may redefine the
    // name. Standing the tier down routes the call through dispatch, which
    // reads the merged Object row -- and the overlay after a
    // redefinition -- at its document position.
    if fx.em.pkg.is_some() {
        return true;
    }
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
            crate::clif::boxes::resolve_class_here(fx, target) == Some(zeo_abi::RACTOR_CLASS)
        })
}

/// `blk.call(a, b)` where `blk` is the scope's own `&block` parameter --
/// a value that is a Proc or nil, and nothing else. A Proc reaches
/// `RProc::call` DIRECTLY rather than Proc's dispatch row, which is what
/// keeps a `break` inside an iterator's block a `Signal::Break` for the
/// iterator to catch instead of the `LocalJumpError` a proc-closure's
/// break raises. `Enumerable#first` driving a user `each` that forwards
/// its block is the corpus shape.
pub(super) fn block_param_call(fx: &mut Fx, id: NodeId) -> CResult<Option<Operand>> {
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
    let argv = crate::clif::call::build_argv(fx, id, &args)?;
    let sym = fx.sym_id(&name);
    let argc = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let caller = crate::clif::call::caller_class(fx, false);
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
