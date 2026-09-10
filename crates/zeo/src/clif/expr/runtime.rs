//! The forms that hand a whole node to the runtime: `binding`, a `def`
//! written where the emitter cannot install it, `Ractor.new` and `eval`.

use super::*;

/// `Kernel#binding` -- this frame, captured. A builtin row cannot answer it:
/// it would have to see its CALLER's locals. The emitter can, so it builds
/// the value here, from the names `binding_scope_names` promoted to cells.
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
        .filter(|n| {
            matches!(
                fx.locals.get(*n),
                Some(crate::clif::ctx::Local::Cell { .. })
            )
        })
        .map(String::as_str)
        .collect();
    let (names_ptr, n) = crate::clif::statics::str_array(fx, &shared);
    let cells_ptr = crate::clif::statics::cell_array(fx, &shared);
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
    let cref = fx.cid_value(owner.map_or(u32::MAX, |c| c.0));
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
/// Three shapes: `def self.x` is always
/// `define_singleton_method` on `self`; a real `def` installs on the
/// DEFAULT DEFINEE (the cref's, unless an `*_eval`/`Class.new` on the
/// stack replaced it -- only the runtime can say, so both candidates go);
/// a literal `define_method(:m){}` is an ordinary `Module#define_method`
/// send, which raises `NoMethodError` when `self` is no Module.
#[expect(
    clippy::too_many_arguments,
    reason = "the DefMethod node's own fields, each deciding a different part of the install"
)]
pub(super) fn runtime_def(
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
        crate::clif::blocks::FrameName::Method(match owner {
            Some(owner) => format!("{owner}{sep}{name}"),
            None => format!("Object{sep}{name}"),
        })
    } else {
        crate::clif::blocks::FrameName::Block
    };
    let kind = if is_def {
        crate::clif::blocks::MethodBody::Def
    } else {
        crate::clif::blocks::MethodBody::DefineMethod
    };
    let proc_ss = crate::clif::blocks::build_method_body(fx, id, params, body, frame, kind)?;
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
        return crate::clif::call::dynamic_send_ptr(fx, recv, verb, a0, 2);
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
        let cid = fx.cid_value(cref_cid.0);
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
pub(super) fn ractor_new(fx: &mut Fx, id: NodeId) -> CResult<Option<Operand>> {
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
    if crate::clif::boxes::resolve_class_here(fx, target) != Some(zeo_abi::RACTOR_CLASS) {
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
    let argv = crate::clif::call::build_argv(fx, id, &args)?;
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
        (Some(b), _) => crate::clif::blocks::literal_block_ptr_rehomed(fx, id, b, true)?,
        (None, Some(ba)) => crate::clif::blocks::block_arg_ptr(fx, ba)?,
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
pub(super) fn runtime_eval(
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
        let args_ptr = crate::clif::call::build_array(fx, args)?;
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
