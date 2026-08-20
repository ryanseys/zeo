//! Escaping blocks: the `BlockFn` body, the cell plumbing, `proc_new`, and
//! the block-passing send with its catch_break landing. The slice's block
//! shape: at most one required parameter (nil-filled/extra-dropped, the
//! non-lambda rule; auto-splat and multi-param binding land at M1-2), no
//! `return`-from-proc, no bare `yield` inside a block.

use super::ctx::{Fx, Local, VALUE_SIZE};
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::analyze::{captures, class_query};
use crate::hir::{ArrayElem, HirNode, NodeId};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::Module;
use zeo_abi::abi::SignalKind;

/// Whether `body` can raise a `Signal::Return` at its own level --
/// nested blocks/lambdas recurse, `def`/`class` bodies stop (the rustc
/// emitter's rule, verbatim).
fn body_contains_return(hir: &crate::hir::Hir, body: &[NodeId]) -> bool {
    fn scan(hir: &crate::hir::Hir, id: NodeId) -> bool {
        match &hir[id] {
            HirNode::Return(_) => return true,
            HirNode::DefMethod { .. } | HirNode::ClassDef { .. } => return false,
            _ => {}
        }
        let mut found = false;
        hir[id].for_each_child(&mut |n| {
            if !found {
                found = scan(hir, n);
            }
        });
        found
    }
    body.iter().any(|&n| scan(hir, n))
}

/// Whether `body` takes a `binding` of its own -- the question
/// `captures::binding_scope_names` answers, asked here without keeping its
/// result (the block's own context recomputes it).
fn body_takes_a_binding(fx: &Fx, params: &crate::hir::Params, body: &[NodeId]) -> bool {
    let mut probe = captures::Captures::default();
    captures::binding_scope_names(&fx.an.compiler, body, params, &mut probe, false).is_some()
}

/// The names an escaping closure with `params`/`body` captures from the
/// enclosing scope, in deterministic order.
fn captured_names(
    fx: &Fx,
    site: NodeId,
    params: &crate::hir::Params,
    body: &[NodeId],
) -> Result<Vec<String>, String> {
    let block = site;
    let caps = captures::block_captures(
        &fx.an.compiler,
        params,
        body,
        class_query::SelfClass::new(fx.method_class, None),
    );
    // A BARE `super` inside the block forwards the ENCLOSING method's
    // parameters by name, and those reads exist only in the emitted
    // forwarding list -- there is no HIR node to walk, which is what
    // `zsuper_forwards` records. The enclosing scope already promoted them
    // to cells for exactly this (`collect_escaping_captures`).
    let zsuper_params = caps
        .zsuper_forwards
        .then_some(fx.method_params.as_ref())
        .flatten()
        .map(captures::own_param_names)
        .unwrap_or_default();
    // A `binding` taken INSIDE the block reports the enclosing scope's
    // locals too, and CLIF lifts a block to its own function -- so a name
    // the binding names has to be captured even where nothing else in the
    // body reads it. The rustc backend needs no equivalent: its block is a
    // Rust closure written inside the enclosing function, where every outer
    // cell is already in scope by name.
    let binding_reach: std::collections::BTreeSet<String> =
        match body_takes_a_binding(fx, params, body) {
            true => fx
                .binding_names
                .iter()
                .flat_map(|names| names.iter())
                .cloned()
                .collect(),
            false => std::collections::BTreeSet::default(),
        };
    let mut names: Vec<String> = caps
        .locals
        .union(&zsuper_params)
        .chain(binding_reach.iter())
        .filter(|n| fx.locals.contains_key(n.as_str()))
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    for n in &names {
        if fx.shadowed.contains(n) {
            return fx.unsupported(
                block,
                "a block capturing a fused-loop parameter (the shadow dies with the loop)",
            );
        }
    }
    // The enclosing scope decides what is SHARED, and it decided textually:
    // ruby makes a name first assigned inside a block block-local unless the
    // outer scope assigned it EARLIER in the source, so `b = proc { x = 1 };
    // x = 5` gives the block an `x` of its own. This walk only sees which
    // names the body mentions; a name the enclosing scope hoisted as a plain
    // slot is one it found unshared, and the block declares its own.
    names.retain(|n| matches!(fx.locals.get(n), Some(Local::Cell { .. })));
    Ok(names)
}

/// Build the block's `RProc` into a fresh slot; the caller passes it as
/// the moved-in `blk`.
pub(crate) fn build_proc(
    fx: &mut Fx,
    site: NodeId,
    block: NodeId,
) -> Result<(ir::StackSlot, Vec<String>), String> {
    let HirNode::Block { params, body } = &fx.an.compiler.hir[block] else {
        return fx.unsupported(block, "a non-literal block");
    };
    let (params, body) = (params.as_ref().clone(), body.clone());
    // `instance_eval`/`instance_exec` rebinds this block's `self` at run
    // time, so nothing lexical can name its class.
    let rehomed = fx
        .an
        .compiler
        .hir
        .has_flag(block, crate::hir::NodeFlag::REHOMED_BLOCK);
    // A computed-name `define_method` block IS a method body at run time:
    // `super` inside it reads the frame stack and a bare one raises ruby's
    // define_method refusal -- the two markers the literal `DefMethod` form
    // carries (rustc's `DYNAMIC_DEFINE_METHOD_BLOCK` arm).
    if fx
        .an
        .compiler
        .hir
        .has_flag(block, crate::hir::NodeFlag::DYNAMIC_DEFINE_METHOD_BLOCK)
    {
        return build_closure_with(
            fx,
            site,
            &params,
            &body,
            false,
            Some(MethodBody::DefineMethod),
            FrameName::Block,
            rehomed,
        );
    }
    build_closure_with(
        fx,
        site,
        &params,
        &body,
        false,
        None,
        FrameName::Block,
        rehomed,
    )
}

/// A lambda literal (`->() {}` and friends): the same closure machinery
/// with lambda semantics -- strict arity, no auto-splat, `return`/`break`
/// fold at the lambda's own boundary.
pub(crate) fn build_lambda(
    fx: &mut Fx,
    site: NodeId,
    params: &crate::hir::Params,
    body: &[NodeId],
) -> Result<(ir::StackSlot, Vec<String>), String> {
    build_closure(fx, site, params, body, true)
}

/// A METHOD-BODY lambda: the proc a runtime `def`/`define_method` installs.
/// Same closure machinery, one semantic difference -- a bare `yield` /
/// `block_given?` in the body reaches the block the INSTALLED METHOD is
/// called with (this fn's own `blk` parameter), never the lexically
/// enclosing method's, so no lexical block rides in the env.
pub(crate) fn build_method_body(
    fx: &mut Fx,
    site: NodeId,
    params: &crate::hir::Params,
    body: &[NodeId],
    frame: FrameName,
    kind: MethodBody,
) -> Result<ir::StackSlot, String> {
    let (ss, _) = build_closure_with(fx, site, params, body, true, Some(kind), frame, false)?;
    Ok(ss)
}

/// What a closure's backtrace frame is called. Ruby names a block after the
/// scope it was WRITTEN in and counts the nesting -- `block in Foo#m`,
/// `block (2 levels) in Foo#m` -- while a real `def` creates an ordinary
/// method however it is installed, so the method IS the frame and blocks
/// inside its body count from zero again.
pub(crate) enum FrameName {
    Block,
    Method(String),
}

/// Ruby's spelling for a block nested `depth` levels under `base`.
fn block_label(base: &str, depth: usize) -> String {
    match depth {
        0 | 1 => format!("block in {base}"),
        n => format!("block ({n} levels) in {base}"),
    }
}

/// Which runtime-install form a method-body closure came from. A real
/// `def` forwards its own parameters for a bare `super`; a literal
/// `define_method` has no parameter list to forward, and ruby refuses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodBody {
    Def,
    DefineMethod,
}

fn build_closure(
    fx: &mut Fx,
    site: NodeId,
    params: &crate::hir::Params,
    body: &[NodeId],
    is_lambda: bool,
) -> Result<(ir::StackSlot, Vec<String>), String> {
    build_closure_with(
        fx,
        site,
        params,
        body,
        is_lambda,
        None,
        FrameName::Block,
        false,
    )
}

fn build_closure_with(
    fx: &mut Fx,
    site: NodeId,
    params: &crate::hir::Params,
    body: &[NodeId],
    is_lambda: bool,
    method_body: Option<MethodBody>,
    frame: FrameName,
    rehomed: bool,
) -> Result<(ir::StackSlot, Vec<String>), String> {
    let names = captured_names(fx, site, params, body)?;
    let arity = super::params::proc_arity(params, is_lambda);
    // Bare `yield`/`block_given?` in the body targets the LEXICALLY
    // enclosing method's block, cloned into the env -- unless the closure
    // declares its own `&b`, which owns the channel.
    let bare_block_use = crate::analyze::scan_bare_block_use_body(&fx.an.compiler.hir, body)
        && params.block.is_none()
        && method_body.is_none();
    let lexical_blk = if bare_block_use { fx.blk_ptr } else { None };
    // A body that can raise `Signal::Return` at its own level captures its
    // home (dead home -> LocalJumpError, the runtime's resolution). A
    // lambda folds its own returns and needs none.
    let wants_home = !is_lambda && body_contains_return(&fx.an.compiler.hir, body);
    let f_id = define_block_fn(
        fx,
        site,
        params,
        body,
        &names,
        is_lambda,
        method_body,
        frame,
        rehomed,
    )?;
    let f_ref = fx.em.module.declare_func_in_func(f_id, fx.b.func);
    let ptr_ty = fx.em.ptr;
    let f_addr = fx.b.ins().func_addr(ptr_ty, f_ref);

    // The cell-pointer array the runtime copies (retaining each).
    let cells_slot = (!names.is_empty()).then(|| {
        fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            names.len() as u32 * 8,
            3,
        ))
    });
    for (i, name) in names.iter().enumerate() {
        let Some(&Local::Cell { ss, .. }) = fx.locals.get(name) else {
            unreachable!("checked in captured_names");
        };
        let ptr = fx.cell_ptr(ss);
        let dst = fx.slot_addr(cells_slot.expect("names non-empty"), (i * 8) as i32);
        fx.b.ins().store(MemFlagsData::trusted(), ptr, dst, 0);
    }
    let cells_ptr = match cells_slot {
        Some(ss) => fx.slot_addr(ss, 0),
        None => fx.b.ins().iconst(ptr_ty, 0),
    };
    let n_cells = fx.b.ins().iconst(ptr_ty, names.len() as i64);
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let null = fx.b.ins().iconst(ptr_ty, 0);
    let lex_blk = lexical_blk.unwrap_or(null);
    let arity_v = fx.b.ins().iconst(types::I32, i64::from(arity));
    // PROC_LAMBDA = 1, PROC_HOME = 2 (the runtime's bits).
    let flag_bits = u32::from(is_lambda) | (u32::from(wants_home) << 1);
    let flags = fx.b.ins().iconst(types::I32, i64::from(flag_bits));
    // `Proc#parameters` and `#source_location` (the middle of `#inspect`):
    // the shape is compile-time knowledge, handed over per construction as
    // a stack-built row array whose names point into `.rodata`.
    let entries = super::emit::param_entries(params, true);
    let params_slot = (!entries.is_empty()).then(|| {
        fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            entries.len() as u32 * param_c_size(),
            3,
        ))
    });
    for (i, (kind, name)) in entries.iter().enumerate() {
        let base = i as i32 * param_c_size() as i32;
        let ss = params_slot.expect("entries non-empty");
        let fl = MemFlagsData::trusted();
        let k = fx.b.ins().iconst(types::I8, i64::from(*kind));
        let at = fx.slot_addr(ss, base + kind_off());
        fx.b.ins().store(fl, k, at, 0);
        let off = fx.em.intern_rodata(name.as_bytes());
        let p = fx.rod(off);
        let at = fx.slot_addr(ss, base + name_ptr_off());
        fx.b.ins().store(fl, p, at, 0);
        let n = fx.b.ins().iconst(ptr_ty, name.len() as i64);
        let at = fx.slot_addr(ss, base + name_len_off());
        fx.b.ins().store(fl, n, at, 0);
    }
    let params_ptr = match params_slot {
        Some(ss) => fx.slot_addr(ss, 0),
        None => fx.b.ins().iconst(ptr_ty, 0),
    };
    let n_params = fx.b.ins().iconst(ptr_ty, entries.len() as i64);
    let (file, line) = fx
        .location(site)
        .map_or((String::new(), 0), |(f, l)| (f.to_string(), l));
    let foff = fx.em.intern_rodata(file.as_bytes());
    let file_ptr = fx.rod(foff);
    let file_len = fx.b.ins().iconst(ptr_ty, file.len() as i64);
    let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
    // `Proc#binding` -- the scope the proc was BUILT in, captured here
    // because the block's own locals do not exist until it runs. Only a
    // program that can ask pays: `uses_proc_binding` is what promoted this
    // scope's locals to cells, so without it there is nothing to capture and
    // `#binding` is left to raise CRuby's C-level-Proc `ArgumentError`.
    let binding_ptr = match fx.an.compiler.hir.uses_proc_binding() && fx.binding_names.is_some() {
        true => {
            let op = super::expr::binding_value(fx, site)?.expect("binding_names is Some");
            let p = ownership::borrow_ptr(fx, &op);
            ownership::pool_owned(fx, p, op.tag());
            p
        }
        false => null,
    };
    let proc_ss = fx.temp_slot();
    let proc_addr = fx.slot_addr(proc_ss, 0);
    fx.call(
        "zeo_rt_proc_new",
        &[
            f_addr,
            cells_ptr,
            n_cells,
            self_ptr,
            lex_blk,
            binding_ptr,
            arity_v,
            flags,
            params_ptr,
            n_params,
            file_ptr,
            file_len,
            line_v,
            proc_addr,
        ],
    );
    // The proc is owned until a send/call consumes it (moved-in blk).
    fx.owned_created += 1;
    Ok((proc_ss, names))
}

/// `ParamC`'s size and field offsets -- the stack rows a proc's parameter
/// list is built into must match what the runtime reads back.
fn param_c_size() -> u32 {
    u32::try_from(std::mem::size_of::<zeo_abi::abi::ParamC>()).expect("ParamC fits a u32")
}
fn kind_off() -> i32 {
    i32::try_from(std::mem::offset_of!(zeo_abi::abi::ParamC, kind)).expect("offset fits")
}
fn name_ptr_off() -> i32 {
    i32::try_from(
        std::mem::offset_of!(zeo_abi::abi::ParamC, name)
            + std::mem::offset_of!(zeo_abi::abi::Str, ptr),
    )
    .expect("offset fits")
}
fn name_len_off() -> i32 {
    i32::try_from(
        std::mem::offset_of!(zeo_abi::abi::ParamC, name)
            + std::mem::offset_of!(zeo_abi::abi::Str, len),
    )
    .expect("offset fits")
}

/// The block body as a `BlockFn`: env cells become (unowned) cell locals,
/// params bind through `zeo_rt_bind_block_params` (ruby's lenient block
/// rules, full shape), `next` is the ok-exit, `break` arms the Break
/// signal, `redo` re-enters at the binding head.
#[allow(
    clippy::too_many_arguments,
    reason = "one closure-emission entry: the block's own shape, its capture list, and the two method-body distinctions each say something different"
)]
fn define_block_fn(
    fx: &mut Fx,
    site: NodeId,
    params: &crate::hir::Params,
    body: &[NodeId],
    captured: &[String],
    is_lambda: bool,
    method_body: Option<MethodBody>,
    frame: FrameName,
    rehomed: bool,
) -> Result<cranelift_module::FuncId, String> {
    let params = params.clone();
    let body = body.to_vec();
    let layout = super::params::layout_of(&params)?;
    let auto_splat = !is_lambda && super::params::auto_splats(&params);
    let (label, base, depth) = match frame {
        FrameName::Method(name) => (name.clone(), name, 0),
        FrameName::Block => {
            let depth = fx.block_depth + 1;
            (
                block_label(&fx.frame_label, depth),
                fx.frame_label.clone(),
                depth,
            )
        }
    };
    let (line, file) = {
        let loc = fx.location(site);
        (
            loc.map_or(0, |(_, l)| l),
            fx.an.compiler.hir.files.first().map(|f| f.name.clone()),
        )
    };

    // A fresh function; the enclosing Fx lends its Emitter.
    let em = &mut *fx.em;
    let an = fx.an;
    let method_class = fx.method_class;
    let self_is_dynamic = fx.self_is_dynamic;
    // A NATIVE-BACKED owner has no compiled slot layout, so its ivars are
    // name-keyed -- and a block written in one of its methods reads the same
    // storage (`@items` inside `synchronize { }` answered nil).
    let dyn_ivars = fx.dyn_ivars;
    // A `super` written inside a block targets the ENCLOSING method (ruby:
    // blocks have no `super` of their own), so the block fn carries that
    // method's identity -- its defining class, name and parameter list.
    let enclosing = (
        fx.defining_class,
        fx.method_name.clone(),
        fx.method_params.clone(),
        fx.self_is_class,
        fx.ruby2_keywords,
    );

    let desc_id = super::statics::define_param_desc(
        em,
        &super::params::ParamDescSpec {
            params: &params,
            name: "block",
            file: file.as_deref(),
            label: &label,
            line,
            end_line: line,
        },
    )?;

    let mut sig = em.module.make_signature();
    for _ in 0..6 {
        sig.params.push(AbiParam::new(em.ptr));
    }
    sig.returns.push(AbiParam::new(types::I32));
    let idx = em.next_fn_index();
    let f_id = em
        .module
        .declare_function(
            &format!("zeo_blk_{idx}"),
            cranelift_module::Linkage::Local,
            &sig,
        )
        .map_err(|e| format!("declaring a block fn: {e}"))?;

    let mut func = ir::Function::with_name_signature(UserFuncName::user(2, idx), sig);
    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let b = FunctionBuilder::new(&mut func, &mut fbc);
    let mut bfx = Fx::new(em, an, b, |em, b| {
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let rodata_gv = em.module.declare_data_in_func(em.rodata_id, b.func);
        let syms_gv = em.module.declare_data_in_func(em.syms_id, b.func);
        let rodata = b.ins().symbol_value(em.ptr, rodata_gv);
        let syms = b.ins().symbol_value(em.ptr, syms_gv);
        (rodata, syms)
    });
    let entry = bfx.b.current_block().expect("entry is current");
    let ep: Vec<ir::Value> = bfx.b.block_params(entry).to_vec();
    let (env, self_p, argv, argc, blk, out) = (ep[0], ep[1], ep[2], ep[3], ep[4], ep[5]);
    bfx.self_ptr = Some(self_p);
    bfx.method_class = method_class;
    bfx.dyn_ivars = dyn_ivars;
    match method_body {
        // A RUNTIME-installed method is a scope of its own: its defining
        // class is minted at run time, so a `super` inside it reads the
        // frame stack, and a BARE one forwards ITS OWN parameters.
        Some(kind) => {
            bfx.method_params = Some(params.clone());
            bfx.runtime_method_body = true;
            bfx.define_method_body = kind == MethodBody::DefineMethod;
            bfx.self_is_dynamic = true;
        }
        None => {
            (
                bfx.defining_class,
                bfx.method_name,
                bfx.method_params,
                bfx.self_is_class,
                bfx.ruby2_keywords,
            ) = enclosing;
        }
    }
    bfx.frame_label = base;
    bfx.block_depth = depth;
    // A re-homed block (`recv.instance_eval { }`) runs under a `self` only
    // the run time knows, and so does every block written inside one.
    bfx.self_is_dynamic =
        bfx.self_is_dynamic || rehomed || (method_body.is_none() && self_is_dynamic);
    // Bare `yield`/`block_given?` targets the env's lexical block (the
    // enclosing method's) -- unless this closure declares its own `&b`,
    // which owns the channel and takes the CALL-SITE block.
    // A method body's bare `yield` reaches the block the INSTALLED method
    // is called with -- this fn's own `blk` -- so it binds even without a
    // declared `&b`.
    bfx.blk_ptr = (params.block.is_some() || method_body.is_some()).then_some(blk);

    // Env cells -> unowned cell locals.
    let fl = MemFlagsData::trusted();
    let cells_off = std::mem::offset_of!(zeo_rt::capi::ProcEnv, cells) as i32;
    let ptr_ty = bfx.em.ptr;
    let cells_base = bfx.b.ins().load(ptr_ty, fl, env, cells_off);
    if params.block.is_none()
        && method_body.is_none()
        && crate::analyze::scan_bare_block_use_body(&an.compiler.hir, &body)
    {
        let lex_off = std::mem::offset_of!(zeo_rt::capi::ProcEnv, lexical_blk) as i32;
        let lex = bfx.b.ins().load(ptr_ty, fl, env, lex_off);
        bfx.blk_ptr = Some(lex);
    }
    for (i, name) in captured.iter().enumerate() {
        let cellp = bfx.b.ins().load(ptr_ty, fl, cells_base, (i * 8) as i32);
        let ss =
            bfx.b
                .create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
        let dst = bfx.slot_addr(ss, 0);
        bfx.b.ins().store(fl, cellp, dst, 0);
        bfx.locals
            .insert(name.clone(), Local::Cell { ss, owned: false });
    }

    // Storage for every name the block binds -- params (all kinds, `&b`
    // and destructured names included), body locals, block-locals -- as
    // owned cells when a NESTED block captures them, plain slots
    // otherwise. Created once, before the redo loop.
    // ...and as cells for every name a `binding` taken in the block would
    // report, which is the only storage a binding can share.
    let mut body_caps = captures::collect_escaping_captures(
        &an.compiler,
        &body,
        &params,
        class_query::SelfClass::new(method_class, None),
    );
    // What a `binding` in this body sees: the block's OWN names first, then
    // the enclosing scope's -- CRuby's innermost-scope-first
    // `local_variables` order.
    bfx.binding_names =
        captures::binding_scope_names(&an.compiler, &body, &params, &mut body_caps, false).map(
            |own| {
                let mut names = (*own).clone();
                for n in fx.binding_names.iter().flat_map(|outer| outer.iter()) {
                    if !names.contains(n) {
                        names.push(n.clone());
                    }
                }
                std::rc::Rc::new(names)
            },
        );
    let body_captured = body_caps.locals;
    let mut names: Vec<String> = params.bound_names();
    {
        let mut locals = crate::analyze::local_storage::Locals::default();
        for &stmt in &body {
            crate::analyze::local_storage::collect_locals(&an.compiler, stmt, &mut locals);
        }
        for id in params.default_ids() {
            crate::analyze::local_storage::collect_locals(&an.compiler, id, &mut locals);
        }
        names.extend(locals.names().iter().cloned());
    }
    for name in names {
        if bfx.locals.contains_key(&name) {
            continue;
        }
        if body_captured.contains(&name) {
            let null = bfx.b.ins().iconst(ptr_ty, 0);
            let cellp = bfx
                .call("zeo_rt_cell_new", &[null])
                .expect("cell_new returns the cell");
            let ss = bfx.b.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                8,
                3,
            ));
            let dst = bfx.slot_addr(ss, 0);
            bfx.b.ins().store(fl, cellp, dst, 0);
            bfx.locals.insert(name, Local::Cell { ss, owned: true });
        } else {
            let ss = bfx.new_value_slot();
            bfx.locals.insert(name, Local::Slot(ss));
        }
    }

    if let Some(file) = &file {
        let off = bfx.em.intern_rodata(file.as_bytes());
        let label_off = bfx.em.intern_rodata(label.as_bytes());
        let file_ptr = bfx.rod(off);
        let file_len = bfx.b.ins().iconst(ptr_ty, file.len() as i64);
        let label_ptr = bfx.rod(label_off);
        let label_len = bfx.b.ins().iconst(ptr_ty, label.len() as i64);
        let line_v = bfx.b.ins().iconst(types::I32, i64::from(line));
        bfx.call(
            "zeo_rt_frame_push",
            &[file_ptr, file_len, label_ptr, label_len, line_v, line_v],
        );
    }
    let status = bfx
        .call("zeo_rt_check_ints", &[])
        .expect("check_ints returns a status");
    bfx.fallible(status);

    // The binding head: `redo` re-enters here (the bindings re-run,
    // ruby's rule -- rustc's 'redo loop starts at the same point).
    let redo_head = bfx.b.create_block();
    bfx.b.ins().jump(redo_head, &[]);
    bfx.b.switch_to_block(redo_head);
    bfx.block_redo = Some(redo_head);

    // Bind through the runtime (the full block rules); slot values are
    // POOLED (a default can raise mid-sequence -- the pool keeps the
    // error edge clean; under `redo` the pool grows per iteration until
    // frame pop, an accepted rarity).
    let n_slots = layout.n_slots;
    let slots_ss = (n_slots > 0).then(|| {
        bfx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            n_slots as u32 * VALUE_SIZE,
            3,
        ))
    });
    let present_ss =
        bfx.b
            .create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let slots_ptr = match slots_ss {
        Some(ss) => bfx.slot_addr(ss, 0),
        None => bfx.b.ins().iconst(ptr_ty, 0),
    };
    let present_ptr = bfx.slot_addr(present_ss, 0);
    let desc_gv = bfx.em.module.declare_data_in_func(desc_id, bfx.b.func);
    let desc_ptr = bfx.b.ins().symbol_value(ptr_ty, desc_gv);
    // BLOCK_BIND_AUTO_SPLAT = 1, BLOCK_BIND_LAMBDA = 2 (the runtime's bits).
    let bind_flags = u8::from(auto_splat) | (u8::from(is_lambda) << 1);
    let flags_v = bfx.b.ins().iconst(types::I8, i64::from(bind_flags));
    let status = bfx
        .call(
            "zeo_rt_bind_block_params",
            &[desc_ptr, flags_v, argv, argc, slots_ptr, present_ptr],
        )
        .expect("bind_block_params returns a status");
    bfx.fallible(status);
    for s in 0..n_slots {
        let ss = slots_ss.expect("n_slots > 0 when slots exist");
        let addr = bfx.slot_addr(ss, s as i32 * 24);
        bfx.owned_created += 1;
        ownership::pool_owned(&mut bfx, addr, TagInfo::Unknown);
    }
    let present = (n_slots > 0).then(|| bfx.b.ins().load(types::I64, fl, present_ptr, 0));

    // Copy the bound slots into their locals, in declared order; absent
    // optionals run their defaults (user code, frame already up).
    {
        let mut bound: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut s = 0usize;
        let slot_addr =
            |bfx: &mut Fx, s: usize| bfx.slot_addr(slots_ss.expect("slots exist"), s as i32 * 24);
        let always =
            |bfx: &mut Fx, bound: &mut std::collections::HashSet<String>, name: &str, s: usize| {
                if !bound.insert(name.to_string()) {
                    return;
                }
                let addr = slot_addr(bfx, s);
                let op = Operand::Ptr {
                    addr,
                    owned: false,
                    tag: TagInfo::Unknown,
                };
                ownership::write_local(bfx, name, &op);
            };
        let optional = |bfx: &mut Fx,
                        bound: &mut std::collections::HashSet<String>,
                        name: &str,
                        default: NodeId,
                        s: usize|
         -> Result<(), String> {
            let duplicate = !bound.insert(name.to_string());
            let p = present.expect("optionals imply slots");
            let bit = bfx.b.ins().band_imm_u(p, (1u64 << s) as i64);
            let got = bfx.b.ins().icmp_imm_u(IntCC::NotEqual, bit, 0);
            let given = bfx.b.create_block();
            let absent = bfx.b.create_block();
            let join = bfx.b.create_block();
            bfx.b.ins().brif(got, given, &[], absent, &[]);
            bfx.b.switch_to_block(given);
            if !duplicate {
                let addr = slot_addr(bfx, s);
                let op = Operand::Ptr {
                    addr,
                    owned: false,
                    tag: TagInfo::Unknown,
                };
                ownership::write_local(bfx, name, &op);
            }
            bfx.b.ins().jump(join, &[]);
            bfx.b.switch_to_block(absent);
            let op = super::expr::lower_expr(bfx, default)?;
            ownership::write_local(bfx, name, &op);
            bfx.b.ins().jump(join, &[]);
            bfx.b.switch_to_block(join);
            Ok(())
        };
        for name in &params.required {
            always(&mut bfx, &mut bound, name, s);
            s += 1;
        }
        for (name, default) in &params.optional {
            optional(&mut bfx, &mut bound, name, *default, s)?;
            s += 1;
        }
        if let Some(Some(name)) = &params.rest {
            always(&mut bfx, &mut bound, name, s);
            s += 1;
        } else if matches!(params.rest, Some(None)) {
            // anonymous `*`: no slot
        }
        for name in &params.post {
            always(&mut bfx, &mut bound, name, s);
            s += 1;
        }
        for kw in &params.keywords {
            match kw {
                crate::hir::KeywordParam::Required(name) => always(&mut bfx, &mut bound, name, s),
                crate::hir::KeywordParam::Optional(name, default) => {
                    optional(&mut bfx, &mut bound, name, *default, s)?;
                }
            }
            s += 1;
        }
        if let Some(Some(name)) = &params.keyword_rest {
            always(&mut bfx, &mut bound, name, s);
            s += 1;
        }
        debug_assert_eq!(s, n_slots);
    }
    // Destructured params replay as multi-assignments.
    for (read, group) in &params.destructures {
        let op = super::expr::lower_expr(&mut bfx, *read)?;
        let tag = op.tag();
        let ptr = ownership::borrow_ptr(&mut bfx, &op);
        if op.owned() {
            ownership::pool_owned(&mut bfx, ptr, tag);
        }
        super::stmt::lower_multi_group(&mut bfx, *read, group, ptr)?;
    }
    // Block-locals (and implicit ones): fresh nil EVERY invocation --
    // and every `redo` iteration (they sit inside the loop).
    for name in params
        .block_locals
        .iter()
        .chain(&params.implicit_block_locals)
    {
        ownership::write_local(&mut bfx, name, &Operand::Nil);
    }
    // `&b`: nil when the block was called blockless, else the call-site
    // block.
    if let Some(Some(bname)) = &params.block {
        ownership::write_local(&mut bfx, bname, &Operand::Nil);
        let got = bfx.b.ins().icmp_imm_u(IntCC::NotEqual, blk, 0);
        let yes = bfx.b.create_block();
        let join = bfx.b.create_block();
        bfx.b.ins().brif(got, yes, &[], join, &[]);
        bfx.b.switch_to_block(yes);
        let op = Operand::Ptr {
            addr: blk,
            owned: false,
            tag: TagInfo::Unknown,
        };
        ownership::write_local(&mut bfx, bname, &op);
        bfx.b.ins().jump(join, &[]);
        bfx.b.switch_to_block(join);
    }

    let ret_ok = bfx.b.create_block();
    bfx.block_next = Some((out, ret_ok));
    if is_lambda {
        // `return` inside a lambda terminates the LAMBDA call (a closure
        // boundary, like a method).
        bfx.ret = Some((out, ret_ok));
    }
    super::stmt::lower_value_body_into(&mut bfx, &body, out)?;
    bfx.b.ins().jump(ret_ok, &[]);

    let has_frame = file.is_some();
    let epilogue = |bfx: &mut Fx, status: i64| {
        let local_slots: Vec<Local> = bfx.locals.values().copied().collect();
        for l in local_slots {
            match l {
                Local::Slot(ss) => {
                    let addr = bfx.slot_addr(ss, 0);
                    bfx.call("zeo_rt_release", &[addr]);
                }
                Local::Cell { owned: false, .. } => {}
                Local::Cell { ss, owned: true } => {
                    let ptr = bfx.cell_ptr(ss);
                    bfx.call("zeo_rt_cell_release", &[ptr]);
                }
            }
        }
        // The call-site block was MOVED in (`call_block_fn` hands over its
        // reference), so this fn owns it -- exactly as a compiled method
        // body owns the blk its trampoline passes. `&b` took its own
        // retained copy; this releases ours. A null channel is skipped.
        let got = bfx.b.ins().icmp_imm_u(IntCC::NotEqual, blk, 0);
        let rel = bfx.b.create_block();
        let cont = bfx.b.create_block();
        bfx.b.ins().brif(got, rel, &[], cont, &[]);
        bfx.b.switch_to_block(rel);
        bfx.call("zeo_rt_release", &[blk]);
        bfx.b.ins().jump(cont, &[]);
        bfx.b.switch_to_block(cont);
        if has_frame {
            bfx.call("zeo_rt_frame_pop", &[]);
        }
        let code = bfx.b.ins().iconst(types::I32, status);
        bfx.b.ins().return_(&[code]);
    };
    bfx.b.switch_to_block(ret_ok);
    epilogue(&mut bfx, 0);
    let land = bfx.land;
    bfx.b.switch_to_block(land);
    if is_lambda {
        // A lambda folds a propagating `Return`/`Break` (a nested block's)
        // into its own normal return -- the value lands in `out` and the
        // ok-epilogue runs.
        let kind = bfx.call("zeo_rt_signal_kind", &[]).expect("kind answers");
        let is_ret =
            bfx.b
                .ins()
                .icmp_imm_u(IntCC::Equal, kind, i64::from(SignalKind::Return as u8));
        let is_brk = bfx
            .b
            .ins()
            .icmp_imm_u(IntCC::Equal, kind, i64::from(SignalKind::Break as u8));
        let folds = bfx.b.ins().bor(is_ret, is_brk);
        let fold = bfx.b.create_block();
        let normal = bfx.b.create_block();
        bfx.b.ins().brif(folds, fold, &[], normal, &[]);
        bfx.b.switch_to_block(fold);
        bfx.call("zeo_rt_signal_take", &[out]);
        bfx.b.ins().jump(ret_ok, &[]);
        bfx.b.switch_to_block(normal);
        epilogue(&mut bfx, 1);
    } else {
        epilogue(&mut bfx, 1);
    }

    super::verify::check(&bfx, &label);
    let Fx { mut b, .. } = bfx;
    b.seal_all_blocks();
    b.finalize(cfg);
    em.record_clif(&label, &func);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(f_id, &mut ctx)
        .map_err(|e| format!("compiling {label}: {e}"))?;
    Ok(f_id)
}

/// A dynamic send carrying a literal block: build the proc, pass it moved,
/// and catch a Break -- the Break value IS the send's value (rustc's
/// `catch_break`).
/// [`block_send`] with the receiver ALREADY lowered -- what a `Foo.new
/// { .. }` needs, whose receiver is a Class immediate rather than a HIR
/// node.
pub(crate) fn block_send_op(
    fx: &mut Fx,
    site: NodeId,
    recv: Operand,
    name: &str,
    args: &[ArrayElem],
    block: NodeId,
) -> Result<Operand, String> {
    let blk_ptr = literal_block_ptr(fx, site, block)?;
    send_with_block_ptr_ops(fx, site, Some(recv), name, args, blk_ptr, false)
}

/// The block channel a literal `{ .. }`/`do .. end` opens: the proc is
/// built here and MOVED to the callee, error path included.
pub(crate) fn literal_block_ptr(
    fx: &mut Fx,
    site: NodeId,
    block: NodeId,
) -> Result<ir::Value, String> {
    let (proc_ss, _names) = build_proc(fx, site, block)?;
    let blk_ptr = fx.slot_addr(proc_ss, 0);
    fx.owned_consumed += 1;
    Ok(blk_ptr)
}

/// The block channel a `&expr` argument opens: `nil` is "no block" (a
/// null blk), anything else converts through `to_proc` and moves to the
/// callee.
pub(crate) fn block_arg_ptr(fx: &mut Fx, block_arg: NodeId) -> Result<ir::Value, String> {
    let op = super::expr::lower_expr(fx, block_arg)?;
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
    let fl = MemFlagsData::trusted();
    let tag = fx.b.ins().load(types::I8, fl, conv, 0);
    let is_nil = fx.b.ins().icmp_imm_u(IntCC::Equal, tag, 0);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let blk_ptr = fx.b.ins().select(is_nil, null, conv);
    fx.owned_created += 1;
    fx.owned_consumed += 1; // moved to the callee, or an immediate nil
    Ok(blk_ptr)
}

pub(crate) fn block_send(
    fx: &mut Fx,
    site: NodeId,
    recv: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    block: NodeId,
) -> Result<Operand, String> {
    let blk_ptr = literal_block_ptr(fx, site, block)?;
    send_with_block_ptr(fx, site, recv, name, args, blk_ptr)
}

/// A call site's `&expr` block argument: convert (Proc through, Symbol to
/// proc, nil to "no block", `to_proc` duck-typing), then the same
/// block-passing send. The converted value moves to the callee when it is
/// a Proc; a nil conversion is an immediate and needs nothing.
pub(crate) fn block_arg_send(
    fx: &mut Fx,
    site: NodeId,
    recv: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    block_arg: NodeId,
) -> Result<Operand, String> {
    // The receiver evaluates FIRST (ruby's order), then the block arg.
    let bypass = super::expr::bypasses_visibility(fx, recv);
    let recv_op = match recv {
        Some(r) => Some(super::expr::lower_expr(fx, r)?),
        None => None,
    };
    let blk_ptr = block_arg_ptr(fx, block_arg)?;
    send_with_block_ptr_ops(fx, site, recv_op, name, args, blk_ptr, bypass)
}

/// [`block_send`]'s tail with the receiver already lowered.
fn send_with_block_ptr(
    fx: &mut Fx,
    site: NodeId,
    recv: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    blk_ptr: ir::Value,
) -> Result<Operand, String> {
    let bypass = super::expr::bypasses_visibility(fx, recv);
    let recv_op = match recv {
        Some(r) => Some(super::expr::lower_expr(fx, r)?),
        None => None,
    };
    send_with_block_ptr_ops(fx, site, recv_op, name, args, blk_ptr, bypass)
}

pub(crate) fn send_with_block_ptr_ops(
    fx: &mut Fx,
    site: NodeId,
    recv: Option<Operand>,
    name: &str,
    args: &[ArrayElem],
    blk_ptr: ir::Value,
    bypass: bool,
) -> Result<Operand, String> {
    let recv_ptr = match recv {
        Some(recv_op) => {
            let p = ownership::borrow_ptr(fx, &recv_op);
            if recv_op.owned() {
                ownership::pool_owned(fx, p, recv_op.tag());
            }
            (p, true)
        }
        None => (fx.self_ptr.expect("self_ptr is set in the prologue"), false),
    };
    // A splatted list builds its Array in the runtime, so it takes the
    // args entry with the block on the same channel.
    if args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
        let recv = recv_ptr.1.then_some(Operand::Ptr {
            addr: recv_ptr.0,
            owned: false,
            tag: TagInfo::Unknown,
        });
        return super::call::splat_send(
            fx,
            site,
            super::call::Recv::maybe(recv, bypass),
            name,
            args,
            &[],
            Some(blk_ptr),
        );
    }
    let argv_ptr = super::call::build_argv(fx, site, args)?;
    let sym = fx.sym_id(name);
    let zero_box = fx.b.ins().iconst(types::I32, 0);
    let argc_v = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = if recv_ptr.1 {
        let caller = super::call::caller_class(fx, bypass);
        fx.call(
            "zeo_rt_send_value_explicit_in",
            &[
                zero_box, recv_ptr.0, sym, argv_ptr, argc_v, blk_ptr, caller, out,
            ],
        )
        .expect("send returns a status")
    } else {
        fx.call(
            "zeo_rt_send_value_in",
            &[zero_box, recv_ptr.0, sym, argv_ptr, argc_v, blk_ptr, out],
        )
        .expect("send returns a status")
    };
    catch_break(fx, status, out);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The block-passing send's landing: a `Break` signal's value IS the
/// send's value (ruby's `break` out of a block), everything else
/// propagates. Every send that opened a block channel needs it -- the
/// plain shape, and the keyword and splat entries alike.
pub(crate) fn catch_break(fx: &mut Fx, status: ir::Value, out: ir::Value) {
    let ok = fx.b.create_block();
    let signalled = fx.b.create_block();
    fx.b.ins().brif(status, signalled, &[], ok, &[]);
    fx.b.switch_to_block(signalled);
    let kind = fx.call("zeo_rt_signal_kind", &[]).expect("kind answers");
    let is_break =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, kind, i64::from(SignalKind::Break as u8));
    let take = fx.b.create_block();
    fx.b.ins().brif(is_break, take, &[], fx.land, &[]);
    fx.b.switch_to_block(take);
    fx.call("zeo_rt_signal_take", &[out]);
    fx.b.ins().jump(ok, &[]);
    fx.b.switch_to_block(ok);
}
