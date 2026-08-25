//! Constant reads and writes: the cref walk, receiver-scoped and
//! runtime-scoped reads, the private-constant guard, `autoload`
//! touches, and the `const_added` announcement -- plus the Class and
//! Symbol immediates they materialize.

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::codegen_error::CResult;
use crate::hir::NodeId;
use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};
use zeo_abi::abi::{PAYLOAD_OFFSET, TAG_OFFSET, ValueTag};

/// A constant read: a statically-resolved class becomes a Class immediate;
/// anything else (a value constant like `ARGV`) reads through the uncached
/// runtime lookup, `NameError` on miss.
pub(crate) fn const_read(fx: &mut Fx, id: NodeId, name: &str) -> CResult<Operand> {
    if let Some(cid) = super::boxes::resolve_class_here(fx, name) {
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
    let top = super::boxes::box_top(fx);
    let defining = if top_level {
        top
    } else {
        super::boxes::lexical_class(fx).unwrap_or(top)
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
        .class_opt(defining)
        .and_then(|c| c.const_owners.get(name))
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
    let mut at = compiler.class_opt(owner).and_then(|c| c.cref_parent);
    while let Some(cid) = at {
        if cid != owner && cid != top {
            chain.push(cid.0);
        }
        at = compiler.class_opt(cid).and_then(|c| c.cref_parent);
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
) -> CResult<Operand> {
    let bytes: Vec<u8> = chain.iter().flat_map(|c| c.to_le_bytes()).collect();
    let ids_off = fx.em.intern_rodata_aligned(&bytes, 4);
    let ids_ptr = fx.rod(ids_off);
    let n_ids = fx.b.ins().iconst(fx.em.ptr, chain.len() as i64);
    let (nptr, nlen) = super::expr::rodata_name(fx, name);
    let (qptr, qlen) = super::expr::rodata_name(fx, qualified);
    let flags = u8::from(hook) | if fx.box_id == 0 { 0 } else { 2 };
    let hook_v = fx.b.ins().iconst(types::I8, i64::from(flags));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status(
        "zeo_rt_const_get_cref",
        &[ids_ptr, n_ids, nptr, nlen, qptr, qlen, hook_v, out],
    );
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
pub(crate) fn const_path_read(fx: &mut Fx, id: NodeId, path: &str) -> CResult<Operand> {
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
    let (nptr, nlen) = super::expr::rodata_name(fx, name);
    let private = fx.call_status("zeo_rt_const_private", &[owner_v, nptr, nlen]);
    let hidden = fx.b.create_block();
    let go = fx.b.create_block();
    fx.b.ins().brif(private, hidden, &[], go, &[]);
    fx.b.switch_to_block(hidden);
    let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner_cid.0));
    let (nptr, nlen) = super::expr::rodata_name(fx, name);
    let (pptr, plen) = super::expr::rodata_name(fx, &path);
    let st = fx.call_status(
        "zeo_rt_raise_private_constant",
        &[owner_v, nptr, nlen, pptr, plen],
    );
    fx.fallible(st);
    fx.b.ins().jump(go, &[]);
    fx.b.switch_to_block(go);
}

pub(super) fn scoped_const_read(
    fx: &mut Fx,
    id: NodeId,
    scope: &str,
    name: &str,
) -> CResult<Operand> {
    // The private guard comes FIRST: a private constant naming a nested
    // class would otherwise fold to a Class immediate below and never ask
    // (`M::Hidden` answered the class where ruby raises).
    if let Some(scope_cid) = super::boxes::resolve_class_here(fx, scope) {
        // The SCOPE can itself be an `autoload` target, and reading THROUGH
        // it is a read of it -- CRuby runs the target before it looks the
        // leaf up. Only `class_value_of` touched, so `Holder::Composed`
        // alone ran the unit and `Holder::Composed::MARK` raised
        // `uninitialized constant` for a constant the unit assigns.
        emit_autoload_touch(fx, scope_cid);
        emit_private_constant_guard(fx, scope_cid, name);
    }
    // `Scope::NAME` naming a nested class/module is a Class immediate.
    let path = format!("{scope}::{name}");
    if let Some(cid) = super::boxes::resolve_class_here(fx, &path) {
        return class_value_of(fx, id, &path, cid);
    }
    // A top-level anchor `::Name` lowers with scope "Object", where the
    // name may be an ordinary top-level class.
    if scope == "Object"
        && let Some(cid) = fx.an.compiler.resolve_class(name, &[], 0)
    {
        return class_value_of(fx, id, name, cid);
    }
    let Some(scope_cid) = super::boxes::resolve_class_here(fx, scope) else {
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
    let (nptr, nlen) = super::expr::rodata_name(fx, name);
    let (qptr, qlen) = super::expr::rodata_name(fx, &qualified);
    let hook_v = fx.b.ins().iconst(types::I8, i64::from(hook));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status(
        "zeo_rt_const_get_scoped",
        &[owner_v, nptr, nlen, qptr, qlen, hook_v, out],
    );
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
fn runtime_scope_const_read(fx: &mut Fx, id: NodeId, scope: &str, name: &str) -> CResult<Operand> {
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
    let (nptr, nlen) = super::expr::rodata_name(fx, name);
    let (qptr, qlen) = super::expr::rodata_name(fx, &qualified);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status(
        "zeo_rt_const_get_on_value",
        &[sptr, nptr, nlen, qptr, qlen, out],
    );
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// Runs the `autoload` target `cid`'s constant still owes, before the read
/// resolves. Emitted only for a constant a literal `autoload` named, and the
/// runtime call itself is one relaxed load when nothing is pending.
fn emit_autoload_touch(fx: &mut Fx, cid: crate::compiler::ClassId) {
    if fx.an.compiler.hir.loader.autoload_consts.is_empty() {
        return;
    }
    // Every PREFIX, outermost first. `autoload :OpenSSL, "openssl"` names the
    // namespace, and net/http reads `OpenSSL::SSL::SSLContext` -- so keying
    // only on the whole path never fired, and the read found a class whose
    // unit had not run.
    let fq = fx.an.compiler.fq_name(cid);
    let parts: Vec<&str> = fq.split("::").collect();
    for i in 1..=parts.len() {
        let prefix = parts[..i].join("::");
        if !fx.an.compiler.hir.loader.autoload_consts.contains(&prefix) {
            continue;
        }
        let owner = match i {
            1 => crate::compiler::OBJECT_CLASS,
            _ => match super::boxes::resolve_class_here(fx, &parts[..i - 1].join("::")) {
                Some(c) => c,
                None => continue,
            },
        };
        let leaf = parts[i - 1].to_string();
        let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner.0));
        let (nptr, nlen) = super::expr::rodata_name(fx, &leaf);
        let st = fx.call_status("zeo_rt_autoload_touch", &[owner_v, nptr, nlen]);
        fx.fallible(st);
    }
}

/// The Class-immediate materialization for an already-resolved id.
fn class_value_of(
    fx: &mut Fx,
    _id: NodeId,
    _name: &str,
    cid: crate::compiler::ClassId,
) -> CResult<Operand> {
    // A constant a literal `autoload` names: the READ is what runs the
    // target, and a compiled-in unit's classes are registered from startup,
    // so nothing misses and no hook can carry it. Gate the fold instead.
    emit_autoload_touch(fx, cid);
    // Registered but not PROMISED: whether a runtime-conditional class's
    // constant exists is settled by the guarded body having run, so the
    // reference asks -- `NameError` until `reveal_class` fires there.
    if fx.an.compiler.constant_is_positional(cid) {
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
        let (nptr, nlen) = super::expr::rodata_name(fx, &fq);
        let owner_v = fx.b.ins().iconst(types::I32, i64::from(owner.0));
        let st = fx.call_status(
            "zeo_rt_conditional_class_ref",
            &[cid_v, nptr, nlen, owner_v, dst],
        );
        fx.fallible(st);
        return Ok(Operand::Slot {
            ss,
            owned: false,
            tag: TagInfo::Known(ValueTag::Class as u8),
        });
    }
    Ok(class_immediate(fx, cid))
}

/// A Symbol value for `name`, interned by `zeo_unit_init` -- two inline
/// stores, like `class_immediate` (the tag byte and the `u32` id).
pub(crate) fn symbol_value(fx: &mut Fx, name: &str) -> CResult<Operand> {
    let sym = fx.sym_id(name);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let fl = MemFlagsData::trusted();
    let z = fx.b.ins().iconst(types::I64, 0);
    for off in [0, 8, 16] {
        fx.b.ins().store(fl, z, out, off);
    }
    let tag =
        fx.b.ins()
            .iconst(types::I8, i64::from(ValueTag::Symbol as u8));
    fx.b.ins().store(fl, tag, out, TAG_OFFSET as i32);
    fx.b.ins().store(fl, sym, out, PAYLOAD_OFFSET as i32);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(ValueTag::Symbol as u8),
    })
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
    fx.b.ins().store(fl, tag, dst, TAG_OFFSET as i32);
    let cid_v = fx.b.ins().iconst(types::I32, i64::from(cid.0));
    fx.b.ins().store(fl, cid_v, dst, PAYLOAD_OFFSET as i32);
    Operand::Slot {
        ss,
        owned: false,
        tag: TagInfo::Class(cid.0),
    }
}

/// `owner.const_added(:name)` -- ruby announces a constant the moment it
/// becomes readable. Emits nothing unless the owner's chain answers the
/// hook by this point in the file (`Module`'s own default is a no-op), so
/// a program without one is unchanged.
pub(crate) fn const_added_send(
    fx: &mut Fx,
    owner: u32,
    name: &str,
    at: Option<NodeId>,
) -> CResult<()> {
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
pub(crate) fn const_added_announce(fx: &mut Fx, owner: u32, name: &str) -> CResult<()> {
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
    let status = fx.call_status(
        "zeo_rt_send_value_in",
        &[zero_box, recv_ptr, sym, argv, argc, null, out],
    );
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

/// `Scope::NAME = v` where `Scope` is no compile-time class -- see
/// [`runtime_scope_const_read`], whose scope half this shares. The write
/// goes through the scope VALUE, so a runtime-minted namespace binds its
/// nested name exactly where ruby does.
pub(super) fn runtime_scope_const_write(
    fx: &mut Fx,
    id: NodeId,
    scope: &str,
    name: &str,
    value: NodeId,
) -> CResult<Operand> {
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
    let op = super::expr::lower_expr(fx, value)?;
    let vptr = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        ownership::pool_owned(fx, vptr, op.tag());
    }
    let (nptr, nlen) = super::expr::rodata_name(fx, name);
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
