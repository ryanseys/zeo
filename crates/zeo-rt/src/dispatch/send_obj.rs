//! The object-channel dispatcher: `send_in_reason` over a generated
//! receiver, and the shared `method_missing` tail.

use super::*;

pub fn send(
    recv: &RObj,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    send_in(0, recv, name, args, block)
}

/// `send` with the caller's box -- see `send_value_in`'s docs.
pub fn send_in(
    box_id: u32,
    recv: &RObj,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    send_in_reason(box_id, recv, name, args, block, MissingReason::NoEntry)
}

pub(super) fn send_in_reason(
    box_id: u32,
    recv: &RObj,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    reason: MissingReason,
) -> Result<RubyValue, Signal> {
    // Ordinary dispatch always lands on the FIRST copy of whatever wins, so
    // the body it enters must not inherit a resume an enclosing `super` walk
    // published for a LATER copy of the same module.
    if mro_duplicates() {
        return with_mro_resume(None, || {
            send_in_reason_inner(box_id, recv, name, args, block, reason)
        });
    }
    send_in_reason_inner(box_id, recv, name, args, block, reason)
}

/// [`send_in_reason`] past the duplicate-chain guard.
fn send_in_reason_inner(
    box_id: u32,
    recv: &RObj,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    reason: MissingReason,
) -> Result<RubyValue, Signal> {
    let id = recv.class_id();
    // A retagged husk short-circuits: every `Ractor::MovedObject` row raises
    // this same error, so the direct raise is behaviorally identical and
    // covers arbitrary names without the method_missing detour. A plain
    // compare on the id already in hand -- free.
    if id == zeo_abi::RACTOR_MOVED_OBJECT_CLASS {
        return Err(crate::ractor::moved_object_error());
    }
    note_dispatch(name);
    // A definition hook sees the class only as far as it has been built, and
    // that governs DISPATCH as well as reflection: a method written below the
    // `def` the hook is reporting is not installed yet. Resolution therefore
    // resumes ABOVE this class, exactly as `super` does -- an inherited copy
    // still answers (`Base#shared` while `Sub#shared` is still ahead), and
    // nothing above is an ordinary miss. Outside a hook body -- every program
    // that defines none -- this is one relaxed atomic load.
    if crate::runtime_meta::pending_here(id, name) {
        return match chain_index_of(id, id).and_then(|at| method_owner_after(id, at + 1, name)) {
            Some(_) => send_super_from(&RubyValue::Object(recv.clone()), id, name, args, block),
            None => method_missing_or_raise(recv, id, name, args, block, reason),
        };
    }
    // Runtime metaprogramming: a per-object singleton, a runtime
    // `define_method` override, or a runtime-class instance's own/inherited
    // methods -- probed first (Ruby: a runtime `define_method` REPLACES), but
    // only once anything has been defined at runtime (`is_live`), so the frozen
    // lock-free fast path below is untouched for all existing code.
    if crate::runtime_meta::is_live()
        && let Some(m) = crate::runtime_meta::resolve_dynamic(recv, id, name)
    {
        return m.call(recv, args, block);
    }
    // A name THIS object retired through its own singleton class. The class
    // still defines it, so every walk below would answer -- the tombstone is
    // what stops them, and `method_missing` gets its turn exactly as it does
    // for a name nothing ever defined.
    if crate::runtime_meta::is_live() {
        let boxed = RubyValue::Object(recv.clone());
        if crate::runtime_meta::object_method_undefined(&boxed, name) {
            return method_missing_or_raise(recv, id, name, args, block, MissingReason::NoEntry);
        }
    }
    // ENV's methods are probed by IDENTITY, not by class: `ENV.class` is
    // `Object` (real Ruby -- it is a lone singleton with Hash-shaped methods,
    // not a Hash). Its OWN explicitly-defined methods must be checked BEFORE
    // the registry, because several (`dup`/`clone`/`freeze`) OVERRIDE Kernel
    // universals that ENV's `Object` identity would otherwise match first --
    // `ENV.dup` must raise, not shallow-copy. The Hash-snapshot fallback for
    // ENV's read-only Enumerable surface stays AFTER the registry walk, so ENV
    // still inherits `object_id`/`equal?`/etc. from Object.
    //
    // A pointer compare, and the boxed handle is built only past the
    // `lookup_mro` hit below -- the ~100%-hit path pays neither an Arc bump
    // nor a downcast. Gated on the class id already in hand: ENV's class IS
    // `Object`, so every typed receiver skips the identity probe entirely.
    let is_env = id == zeo_abi::OBJECT_CLASS && crate::builtins::env::is_env_obj(recv);
    if is_env && let Some(f) = crate::builtins::env::lookup(name.name_str()) {
        return f(&RubyValue::Object(recv.clone()), args, block);
    }
    if let Some(f) = registry().lookup_mro(id, name) {
        return f.call(recv, args, block);
    }
    let boxed = RubyValue::Object(recv.clone());

    // ENV's singleton class INCLUDES Enumerable (oracle: its ancestors are
    // `[#<Class:ENV>, Enumerable, Object, Kernel, BasicObject]`), and a fresh
    // Hash snapshot answers every one of those rows faithfully without a
    // per-method stub. Mutators are in `env::lookup` above -- they must write
    // the real environment.
    //
    // Only ENUMERABLE's names, though: a universal like `equal?`/`object_id`
    // has to resolve on ENV itself, or it compares two different snapshots
    // and `ENV.equal?(ENV)` answers false.
    if is_env
        && crate::builtins::class_table(zeo_abi::ENUMERABLE_CLASS)
            .is_some_and(|lookup| lookup(name.name_str()).is_some())
    {
        let snapshot = crate::builtins::env::snapshot();
        return send_value(&snapshot, name, args, block);
    }

    // The SAME MRO walk `send_value` runs, over a boxed
    // handle: Kernel's universals, BasicObject's `==`, and the
    // Enumerable/Comparable module tables all resolve as real ancestor
    // methods, so `method_missing` fires only AFTER them -- real Ruby finds
    // a real (module) method first, always.
    //
    // The method's TEXT is fetched only where a by-name builtin table or the
    // value-subclass rewrap needs it; the flat one-probe hit below resolves
    // on the `Symbol` alone. (`name_str` is two lock-free slab indexes now,
    // so the ordering is tidiness, not a lock.)
    // Value-subclass payload bridge (D3): `class Stack < Array` carries a
    // `RubyValue::Array` payload; at its payload root the inherited builtin
    // method runs against that value, not the boxed object. `None` for every
    // ordinary object, so this costs one field read on the miss path.
    let payload = recv.builtin_payload();
    let payload_root = recv.builtin_root();
    // Same flat-vs-walk split as `send_value_in_reason`: box 0 with a
    // dormant overlay takes the one-probe flattened walk; anything else
    // keeps the per-ancestor probes it needs.
    let flat = if box_id == 0 && !crate::runtime_meta::is_live() {
        REGISTRY.get().and_then(|r| r.flat_value_hit(id, name))
    } else {
        None
    };
    match flat {
        Some(Some(hit)) => {
            // At the payload root, a BUILTIN hit runs against the wrapped
            // value and re-wraps a self-return (`push`/`<<`) back to the
            // subclass; reopen rows always run against the boxed receiver.
            if hit.builtin
                && payload_root
                    .is_some_and(|r| crate::builtins::value_subclass::payload_owns(r, hit.owner))
                && let Some(ref p) = payload
            {
                if let Some(k) = crate::builtins::value_subclass::wrapper_row(name) {
                    return k(&RubyValue::Object(recv.clone()), args, block);
                }
                let result = with_c_frame(hit.frame_label, || hit.f.call(p, args, block))?;
                return Ok(crate::builtins::value_subclass::rewrap_self_return(
                    result,
                    p,
                    recv,
                    name.name_str(),
                ));
            }
            return with_c_frame(hit.frame_label, || hit.f.call(&boxed, args, block));
        }
        // A genuine flat miss: straight to the alias tail below.
        Some(None) => {}
        None => {
            let n = name.name_str();
            let live = crate::runtime_meta::is_live();
            for &anc in ancestors_of_value(id) {
                // An `undef_method` at this position TERMINATES the walk,
                // before this ancestor's own NATIVE table -- the tombstone is
                // the only record that a builtin row was retired, since
                // `class_table` itself is a static list. See the twin in
                // `send_value_in_reason`.
                if (live && crate::runtime_meta::overlay_is_undefined(anc, name))
                    || REGISTRY.get().is_some_and(|r| r.is_undefined(anc, name))
                {
                    break;
                }
                if live && crate::runtime_meta::overlay_is_removed(anc, name) {
                    continue;
                }
                if let Some(f) = value_method(anc, box_id, name) {
                    // The same payload bridge the `class_table` arm below
                    // runs: an ext builtin's rows register as VALUE methods
                    // (zlib, strscan), and handing one the boxed subclass
                    // instead of its payload panicked the row's receiver
                    // downcast (`TaggedInflate#inflate` died in `zs_of`).
                    if payload_root
                        .is_some_and(|r| crate::builtins::value_subclass::payload_owns(r, anc))
                        && let Some(ref p) = payload
                    {
                        if let Some(k) = crate::builtins::value_subclass::wrapper_row(name) {
                            return k(&boxed, args, block);
                        }
                        let result = f.call(p, args, block)?;
                        return Ok(crate::builtins::value_subclass::rewrap_self_return(
                            result, p, recv, n,
                        ));
                    }
                    return f.call(&boxed, args, block);
                }
                // `include Math` reaches its module functions here as an
                // ordinary `class_table` hit on Math's registered instance
                // table.
                if let Some(table) = crate::builtins::class_table(anc)
                    && let Some(f) = table(n)
                {
                    // At the payload root, run against the wrapped value
                    // and re-wrap a self-return (`push`/`<<`) back to the
                    // subclass.
                    if payload_root
                        .is_some_and(|r| crate::builtins::value_subclass::payload_owns(r, anc))
                        && let Some(ref p) = payload
                    {
                        if let Some(k) = crate::builtins::value_subclass::wrapper_row(name) {
                            return k(&boxed, args, block);
                        }
                        let result = with_c_frame_ids(anc, name, '#', || f(p, args, block))?;
                        return Ok(crate::builtins::value_subclass::rewrap_self_return(
                            result, p, recv, n,
                        ));
                    }
                    return with_c_frame_ids(anc, name, '#', || f(&boxed, args, block));
                }
            }
        }
    }

    // A builtin-alias row (`alias_method :raise!, :raise`): rewrite the name
    // and re-dispatch -- after every real method (a real definition of the
    // alias name wins) and BEFORE `method_missing` (an alias is a real method
    // in Ruby).
    if let Some(old) = alias_target(id, name) {
        // The payload bridge the flat arm above runs: a builtin row wants the
        // wrapped value, not the boxed subclass instance.
        if let Some(root) = payload_root
            && let Some(ref p) = payload
            && let Some(f) = builtin_row(root, old)
        {
            let r = with_c_frame_ids(root, old, '#', || f(p, args, block))?;
            return Ok(crate::builtins::value_subclass::rewrap_self_return(
                r,
                p,
                recv,
                old.name_str(),
            ));
        }
        return send_in_reason(box_id, recv, old, args, block, reason);
    }
    method_missing_or_raise(recv, id, name, args, block, reason)
}

/// The tail every unresolved send shares: `method_missing` if the receiver has
/// one, else CRuby's raise. Also where a send truncated by the definition
/// watermark lands, so a class with its own `method_missing` still gets it.
pub(super) fn method_missing_or_raise(
    recv: &RObj,
    id: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    reason: MissingReason,
) -> Result<RubyValue, Signal> {
    // method_missing fallback, with `name` prepended to args (mirrors
    // CRuby's own protocol) -- AFTER every real method, per real Ruby.
    let mm = crate::symbol::wk::method_missing();
    let mm_impl = crate::runtime_meta::is_live()
        .then(|| crate::runtime_meta::resolve_dynamic(recv, id, mm))
        .flatten()
        .or_else(|| registry().lookup(id, mm).cloned());
    if let Some(f) = mm_impl {
        let mut full_args = Vec::with_capacity(args.len() + 1);
        full_args.push(RubyValue::Symbol(name));
        full_args.extend_from_slice(args);
        return f.call(recv, &full_args, block);
    }

    // A real, catchable `NoMethodError` (replacing the original
    // eprintln-and-`process::exit(1)` shortcut): propagates like any other
    // raised exception -- rescuable at the call site, re-raised at a
    // Thread's `join`/`value` if uncaught there, and printed by the
    // top-level uncaught handler otherwise. Shares the one method-missing
    // raiser with every other failure mode.
    Err(raise_method_missing(
        &RubyValue::Object(recv.clone()),
        &name.to_string(),
        args,
        reason,
    ))
}
