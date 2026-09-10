//! `defined?` lowering: the statically-classified forms fold to a
//! constant answer, the rest probe at run time and answer the CRuby
//! prose (`"expression"`, `"method"`, ...) or nil.

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::diagnostics::clif::CResult;
use crate::hir::{HirNode, NodeId};
use cranelift_codegen::ir::{InstBuilder, types};
use zeo_abi::abi::{TAG_OFFSET, ValueTag};

/// `defined?(s)` -- a FRESH string answer or nil.
fn defined_str(fx: &mut Fx, dst: cranelift_codegen::ir::Value, s: &str) {
    let off = fx.em.intern_rodata(s.as_bytes());
    let ptr = fx.rod(off);
    let len_v = fx.b.ins().iconst(fx.em.ptr, s.len() as i64);
    let enc = fx.b.ins().iconst(types::I8, super::expr::ENC_UTF8);
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
fn defined_all_or_nil(fx: &mut Fx, site: NodeId, nodes: &[NodeId]) -> CResult<Operand> {
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
        let tag = fx.b.ins().load(types::I8, fl, p, TAG_OFFSET as i32);
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

/// The globals ruby predefines, which `defined?($x)` answers for at
/// compile time.
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

/// `defined?(expr)`. The runtime-probing forms call one capi each;
/// everything else classifies statically. The collection-literal recursion and dynamic-scope const
/// forms still refuse.
pub(super) fn lower_defined(fx: &mut Fx, site: NodeId, inner: NodeId) -> CResult<Operand> {
    // `defined?(yield)`: runtime -- the block channel is or isn't there.
    if matches!(&fx.an.compiler.hir[inner], HirNode::Yield(_)) {
        // A snippet's own level has no channel; the one it means is the
        // enclosing method's, published for the call.
        if fx.blk_ptr.is_none() && fx.eval_mode.is_some() {
            let hit = fx.call_status("zeo_rt_eval_block_given", &[]);
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
    // `defined?(super)`: probe the same walk `super` runs. A CLASS-method
    // body asks the same two questions `lower_super` asks, in the same
    // order, or the two disagree: a statically resolved singleton target is
    // a yes outright, and only a miss reaches the run-time walk -- resumed
    // from the ancestor that HOLDS the defining class, since an extended
    // module is not itself in the ancestry. A class BODY has no method name,
    // so it still falls through to the static tail.
    if matches!(&fx.an.compiler.hir[inner], HirNode::SuperCall { .. })
        && let (Some(dc), Some(m)) = (fx.defining_class, fx.method_name.clone())
    {
        let mut target = dc;
        if fx.self_is_class {
            let Some(owner) = fx.method_class else {
                return Ok(Operand::Nil);
            };
            let compiler = &fx.an.compiler;
            if crate::analyze::class_query::extended_singleton_super(compiler, owner, dc, &m)
                .is_some()
            {
                let ss = fx.temp_slot();
                let dst = fx.slot_addr(ss, 0);
                defined_str(fx, dst, "super");
                fx.owned_created += 1;
                return Ok(Operand::Slot {
                    ss,
                    owned: true,
                    tag: TagInfo::Unknown,
                });
            }
            target = crate::analyze::class_query::singleton_chain_host(compiler, owner, Some(dc))
                .unwrap_or(dc);
        }
        let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
        let dc_v = fx.b.ins().iconst(types::I32, i64::from(target.0));
        let sym = fx.sym_id(&m);
        let hit = fx.call_status("zeo_rt_super_defined", &[self_ptr, dc_v, sym]);
        return Ok(defined_cond(fx, hit, "super"));
    }
    // The same probe at a SNIPPET's own level, where the target is the
    // enclosing method's rather than this scope's.
    if matches!(&fx.an.compiler.hir[inner], HirNode::SuperCall { .. }) && fx.eval_mode.is_some() {
        let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
        let hit = fx.call_status("zeo_rt_eval_super_defined", &[self_ptr]);
        return Ok(defined_cond(fx, hit, "super"));
    }
    // A scope with no method NAME of its own is either outside any method --
    // the top level, a class body, where ruby answers nil -- or a block that
    // BECOMES one at run time (`K.define_method(:m) { }`, and the same call
    // through `send`, which the compiler cannot recognize as a definition at
    // all). Only the run time can tell those apart, and it does: the same
    // method-frame stack `send_super_dynamic` resumes from, empty outside a
    // method. Answering the static "method" for both reported a super target
    // where there was none.
    if matches!(&fx.an.compiler.hir[inner], HirNode::SuperCall { .. }) && fx.method_name.is_none() {
        let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
        let hit = fx.call_status("zeo_rt_super_defined_dynamic", &[self_ptr]);
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
    if let HirNode::Call {
        receiver,
        name,
        args,
        ..
    } = &fx.an.compiler.hir[inner]
    {
        let (receiver, name) = (*receiver, name.clone());
        // CRuby answers "method" only when the call resolves AND every
        // ARGUMENT is itself defined, so `defined?(puts(Missing))` is nil.
        let arg_ids: Vec<NodeId> = args
            .iter()
            .map(super::super::hir::ArrayElem::node_id)
            .collect();
        let ss = fx.temp_slot();
        let dst = fx.slot_addr(ss, 0);
        let hit_ss = fx.temp_slot();
        let hit_ptr = fx.slot_addr(hit_ss, 0);
        let sym = fx.sym_id(&name);
        let merge = fx.b.create_block();
        let check = fx.b.create_block();
        for a in arg_ids {
            let op = lower_defined(fx, site, a)?;
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            let fl = cranelift_codegen::ir::MemFlagsData::trusted();
            let tag = fx.b.ins().load(types::I8, fl, p, TAG_OFFSET as i32);
            let known = fx.b.ins().icmp_imm_u(
                cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                tag,
                i64::from(ValueTag::Nil as u8),
            );
            let next = fx.b.create_block();
            let undefined = fx.b.create_block();
            fx.b.ins().brif(known, next, &[], undefined, &[]);
            fx.b.switch_to_block(undefined);
            ownership::write_move_into(fx, &Operand::Nil, dst);
            fx.b.ins().jump(merge, &[]);
            fx.b.switch_to_block(next);
        }
        match receiver {
            None => {
                let self_ptr = super::ivars::dyn_ivar_recv(fx);
                let one = fx.b.ins().iconst(types::I8, 1);
                fx.call("zeo_rt_defined_method", &[self_ptr, sym, one, hit_ptr]);
                fx.b.ins().jump(check, &[]);
            }
            Some(rid) => {
                let swallow = fx.b.create_block();
                let saved = fx.land;
                fx.land = swallow;
                let op = super::expr::lower_expr(fx, rid)?;
                fx.land = saved;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let zero = fx.b.ins().iconst(types::I8, 0);
                fx.call("zeo_rt_defined_method", &[p, sym, zero, hit_ptr]);
                fx.b.ins().jump(check, &[]);
                // The swallow landing: drop the pending signal and answer
                // nil.
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
        // An autoload unit's own assignment does not exist until the unit
        // runs, so the write the compiler can see is not an answer -- ask.
        // A scope-less write is keyed by its LEAF, so both spellings count;
        // matching one too many only costs the fold, never the answer.
        let unrun = {
            let set = &fx.an.compiler.hir.loader.unrun_unit_consts;
            set.contains(&name) || set.contains(&format!("{scope}::{name}"))
        };
        // A POSITIONAL scope or target -- a class a spliced require reveals
        // at its own statement -- is undefined until that line runs, so a
        // static "constant" above the require is a claim ruby does not make.
        // The bare-name arm below already asks the run time; this is the
        // qualified spelling's half.
        let positional = [format!("{scope}::{name}"), scope.clone()].iter().any(|p| {
            super::boxes::resolve_class_here(fx, p)
                .is_some_and(|cid| fx.an.compiler.constant_is_positional(cid))
        });
        // While the path's ROOT is only defined LATER in the document, ruby
        // has no such constant at this line -- and the runtime registry
        // cannot say so (a spliced class registers at startup), so the
        // answer is a STATIC nil, not a probe.
        if crate::analyze::constfold::path_root_defined_only_later(&env, inner, &scope) {
            return Ok(defined_static(fx, None));
        }
        if unrun
            || positional
            || crate::analyze::constfold::const_form_resolves(&env, inner) != Some(true)
        {
            let Some(scope_id) = super::boxes::resolve_class_here(fx, &scope) else {
                // A scope only the run time can name (`Scoped = Module.new`)
                // -- or one nothing ever defines, where reading it raises
                // and the swallow answers nil, as ruby's does.
                return defined_const_under_runtime_scope(fx, &name, |fx| {
                    super::consts::const_path_read(fx, inner, &scope)
                });
            };
            let sid = fx.b.ins().iconst(types::I32, i64::from(scope_id.0));
            let (nptr, nlen) = super::expr::rodata_name(fx, &name);
            let private = fx.call_status("zeo_rt_const_private", &[sid, nptr, nlen]);
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
            let (nptr2, nlen2) = super::expr::rodata_name(fx, &name);
            let hit = fx.call_status("zeo_rt_defined_const_in", &[sid2, nptr2, nlen2]);
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
        return defined_const_under_runtime_scope(fx, &name, |fx| {
            super::expr::lower_expr(fx, scope)
        });
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
    scope: impl FnOnce(&mut Fx) -> CResult<Operand>,
) -> CResult<Operand> {
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
        let (nptr, nlen) = super::expr::rodata_name(fx, name);
        let hit = fx.call_status("zeo_rt_scope_const_defined", &[p, nptr, nlen]);
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
fn defined_rest(fx: &mut Fx, site: NodeId, inner: NodeId) -> CResult<Operand> {
    use crate::hir::LastMatch;
    if let HirNode::GlobalRead(name) = &fx.an.compiler.hir[inner] {
        let name = name.clone();
        if is_predefined_global(&name) {
            return Ok(defined_static(fx, Some("global-variable")));
        }
        let bx = fx.box_v();
        let (nptr, nlen) = super::expr::rodata_name(fx, &name);
        let hit = fx.call_status("zeo_rt_defined_gvar", &[bx, nptr, nlen]);
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
        let tag = fx.b.ins().load(types::I8, fl, mout, TAG_OFFSET as i32);
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
        let self_ptr = super::ivars::dyn_ivar_recv(fx);
        let (nptr, nlen) = super::expr::rodata_name(fx, &name);
        let hit_ss = fx.temp_slot();
        let hit_ptr = fx.slot_addr(hit_ss, 0);
        let status = fx.call_status("zeo_rt_defined_ivar", &[self_ptr, nptr, nlen, hit_ptr]);
        fx.fallible(status);
        let fl = cranelift_codegen::ir::MemFlagsData::trusted();
        let hit = fx.b.ins().load(types::I8, fl, hit_ptr, 0);
        return Ok(defined_cond(fx, hit, "instance-variable"));
    }
    if let HirNode::ClassVarRead(name) = &fx.an.compiler.hir[inner] {
        let name = name.clone();
        let owner = super::ivars::cvar_owner(fx, &name);
        let ov = fx.b.ins().iconst(types::I32, i64::from(owner));
        let (nptr, nlen) = super::expr::rodata_name(fx, &name);
        let hit = fx.call_status("zeo_rt_defined_cvar", &[ov, nptr, nlen]);
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
        // `!= Some(true)`, not `is_none()`: a bare name nothing defines folds
        // to `Some(false)`, which is the version-gate idiom's whole value at
        // ANALYZE time (`defined?(Ractor)` drops the branch). At EMIT time a
        // guard that folded is already gone, so the only `defined?` left here
        // is one whose ANSWER the program reads -- and for a program that can
        // load at run time, "provably absent" is a claim the compiler is not
        // entitled to make.
        if crate::analyze::constfold::const_form_resolves(&env, inner) != Some(true) {
            // While the name's ROOT is only defined LATER in the document,
            // ruby has no such constant at this line. The runtime registry
            // cannot say so -- a spliced class registers at startup -- so
            // the answer is a STATIC nil, ahead of every probe below.
            if crate::analyze::constfold::path_root_defined_only_later(&env, inner, &name) {
                return Ok(defined_static(fx, None));
            }
            // A positional CLASS answers the scope operator's question -- the
            // name is looked up IN its lexical parent. A value constant a
            // unit assigns answers the BARE one, which reaches a top-level
            // name the scoped search excludes; the entry point differs with
            // it (`zeo_rt_defined_const_bare`).
            let mut bare = false;
            let positional = super::boxes::resolve_class_here(fx, &name)
                .filter(|&cid| fx.an.compiler.constant_is_positional(cid))
                .map(|cid| {
                    (
                        fx.an
                            .compiler
                            .class(cid)
                            .lexical_parent
                            .unwrap_or(crate::compiler::OBJECT_CLASS),
                        fx.an.compiler.leaf_name(cid).to_string(),
                    )
                })
                // A VALUE constant a compiled-in unit assigns names no class
                // at all, so the class probe above cannot see it -- and it is
                // undefined until that file runs just the same.
                .or_else(|| {
                    let leaf = crate::constpath::ConstPath::parse(&name).base().to_string();
                    let set = &fx.an.compiler.hir.loader.unrun_unit_consts;
                    (set.contains(&leaf) || set.contains(&name)).then(|| {
                        bare = true;
                        (
                            fx.defining_class
                                .or(fx.method_class)
                                .unwrap_or(crate::compiler::OBJECT_CLASS),
                            leaf,
                        )
                    })
                });
            // A program that can load or compile at RUN time may define a
            // constant this compile never saw -- a require behind a
            // `$LOAD_PATH.unshift` is the everyday case. Answering a static
            // nil there is a claim the compiler is not entitled to make, so
            // the question goes to the run time like any other unresolved
            // one. A program with no such hatch keeps its static answer.
            let positional = positional.or_else(|| {
                fx.an.compiler.compiles_at_runtime().then(|| {
                    bare = true;
                    (
                        fx.defining_class
                            .or(fx.method_class)
                            .unwrap_or(crate::compiler::OBJECT_CLASS),
                        crate::constpath::ConstPath::parse(&name).base().to_string(),
                    )
                })
            });
            if let Some((owner, leaf)) = positional {
                let sid = fx.b.ins().iconst(types::I32, i64::from(owner.0));
                let (nptr, nlen) = super::expr::rodata_name(fx, &leaf);
                let entry = match bare {
                    true => "zeo_rt_defined_const_bare",
                    false => "zeo_rt_defined_const_in",
                };
                let hit = fx.call_status(entry, &[sid, nptr, nlen]);
                return Ok(defined_cond(fx, hit, "constant"));
            }
        }
    }
    // The static classification tail. Order is load-bearing.
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
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
        | HirNode::ImaginaryLit(_)
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
