//! Resolver support -- called from `dispatch`, always behind `is_live()`.

use super::*;

/// Resolve an instance method for a `send_in` receiver through the overlay:
/// a per-object singleton first, then (for a runtime class) an ancestor walk
/// that consults both overlay entries and the frozen registry, else (for a
/// frozen class) just this class's runtime method-delta. `None` falls back to
/// `send_in`'s existing frozen fast path + builtin MRO walk.
pub fn resolve_dynamic(recv: &RObj, id: ClassId, name: Symbol) -> Option<MethodImpl> {
    // 1. Per-object singleton (identity-keyed).
    {
        let s = maps().singletons.read().unwrap();
        if !s.is_empty()
            && let Some(m) = s.get(&obj_identity(recv)).and_then(|t| t.get(&name))
        {
            return Some(m.clone());
        }
    }
    if id.0 >= RUNTIME_CLASS_ID_BASE {
        // 2a. Runtime class: walk its ancestors (overlay methods, then frozen
        // materialized methods on a frozen ancestor).
        walk_runtime_class(id, name)
    } else {
        // 2b. Frozen class: check this class AND every frozen ancestor for a
        // runtime method delta, so a `class_eval`/`define_method`/`class_exec`
        // that reopened a SUPERCLASS or an included MODULE is inherited (a
        // subclass instance / an includer sees it).
        //
        // The walk STOPS at the first ancestor that has a frozen definition of
        // its own, and answers nothing: that definition is closer than any
        // overlay delta further along the chain, and `send_in`'s MRO walk is
        // what runs it. Without the stop, `M.define_method(:m)` on an included
        // module beat the includer's own compiled `def m` -- ruby keeps the
        // class's own.
        //
        // Only a position the FROZEN walk reaches IN THE SAME ORDER can stop
        // it, which is the prefix the two chains share. A run-time
        // `include`/`prepend` splices a module the frozen flat table knows
        // nothing about, and from the splice onward the frozen walk's order is
        // a different one -- so those rows reach the receiver through the
        // host's `prepended` map, further along this same walk, and stopping
        // early would lose them.
        let chain = ancestors_of_value(id);
        let frozen = crate::dispatch::frozen_ancestors(id);
        let c = maps().classes.read().unwrap();
        // The walk itself never stops early: a run-time `prepend`'s body is
        // filed under the HOST, which sits BEHIND the module it spliced in.
        let mut trusted = true;
        for (at, anc) in chain.iter().enumerate() {
            if let Some(m) = c
                .get(&anc.0)
                .and_then(|e| e.prepended.get(&name).or_else(|| e.methods.get(&name)))
            {
                return Some(m.clone());
            }
            trusted &= frozen.get(at) == Some(anc);
            if trusted
                && crate::dispatch::class_defines_own_instance_method_if_registered(*anc, name)
            {
                return None;
            }
        }
        None
    }
}

/// The instance-method resolution for a RUNTIME class id: overlay-defined
/// methods per ancestor, then that ancestor's frozen materialized method.
pub(super) fn walk_runtime_class(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    let chain: &'static [ClassId] = {
        let c = maps().classes.read().unwrap();
        c.get(&id.0)?.ancestors
    };
    for &anc in chain {
        {
            let c = maps().classes.read().unwrap();
            let entry = c.get(&anc.0);
            // An undef here terminates the walk -- an ancestor's still-live
            // definition must not answer past it.
            if entry.is_some_and(|e| e.undefs.contains(&name)) {
                return None;
            }
            // A `remove_method` empties this position without ending the
            // walk -- the ancestor that still defines the name answers.
            if entry.is_some_and(|e| e.removed.contains(&name)) {
                continue;
            }
            if let Some(m) = entry.and_then(|e| {
                e.prepended
                    .get(&name)
                    .or_else(|| e.methods.get(&name))
                    .cloned()
            }) {
                return Some(m);
            }
        }
        if let Some(m) = registry_lookup_cloned(anc, name) {
            return Some(m);
        }
        // ...then what this ancestor DEFINES, which is a different question for
        // a module. A module's bodies are never in the flat table above:
        // materialization copies them onto each compile-time includer, and a
        // class that included or prepended the module at RUNTIME has no copy.
        // They survive in two places, and `super`'s own walk probes both --
        // `own_impls` for a class's super-reachable bridge, and a VALUE METHOD
        // on the module's own id, which is where `emit_user_module_bridges`
        // puts every module instance method.
        //
        // Missing the second is what made a prepended module correctly first in
        // `ancestors` and yet invisible to dispatch: the walk reached its
        // position, found three empty tables, and carried on to the class.
        if let Some(m) = crate::dispatch::registry_own_impl_cloned(anc, name) {
            return Some(m);
        }
        if let Some(m) = crate::dispatch::registry_value_method_impl(anc, name) {
            return Some(m);
        }
        // ...and finally this ancestor's NATIVE table, which is the only place
        // a builtin's own rows live. Last at each position and never skipped:
        // leaving it out let a FARTHER ancestor's reopen answer over a nearer
        // builtin row.
        if let Some(m) = crate::dispatch::builtin_row_impl(anc, name) {
            return Some(m);
        }
    }
    None
}

/// [`walk_runtime_class`] for callers outside this module: the instance method a
/// RUNTIME class id answers `name` with. The frozen registry cannot resolve one
/// of these ids at all -- it holds no entry, so not even the ancestor walk finds
/// the chain -- which is why a caller that starts from a class rather than a
/// receiver has to ask here as well.
pub(crate) fn runtime_class_method(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    walk_runtime_class(id, name)
}

/// A class-level method (`def self.x` / `define_singleton_method` on a class)
/// for a `RubyValue::Class` receiver -- the raw block, invoked under the class.
/// A `singleton_class.prepend(M)` copy outranks everything else, ruby's own
/// layering (see [`OverlayEntry::prepended_class_methods`]).
pub fn overlay_class_method(id: ClassId, name: Symbol) -> Option<RProc> {
    let c = maps().classes.read().unwrap();
    let e = c.get(&id.0)?;
    e.prepended_class_methods
        .get(&name)
        .or_else(|| e.class_methods.get(&name))
        .cloned()
}

/// A module prepended into an ANCESTOR's singleton class, for a receiver that
/// inherits it.
///
/// `prepended_class_methods` is keyed on the class the `prepend` named, and a
/// subclass's singleton chain runs through its parent's -- `A5.singleton_class
/// .ancestors` is `[#<Class:A5>, CM, #<Class:A2>]` when `CM` was prepended
/// into `A2`'s. Ordinary class-method dispatch probes the RECEIVER's own
/// entry and then the flat table, which holds the inherited `def self.x`, so
/// without this the parent's own row answered where the module should.
///
/// `None` as soon as a nearer ancestor defines the name itself: that row is
/// closer than any prepend further up, and the flat path already serves it.
pub fn inherited_singleton_prepend(id: ClassId, name: Symbol) -> Option<RProc> {
    for &anc in crate::dispatch::ancestors_of_value(id).iter() {
        if let Some(p) = maps()
            .classes
            .read()
            .unwrap()
            .get(&anc.0)
            .and_then(|e| e.prepended_class_methods.get(&name).cloned())
        {
            return Some(p);
        }
        // Stop at the first ancestor that DEFINES the name, since its own row
        // is closer than any prepend further up and the flat path serves it.
        // `class_methods` alone cannot answer that: materialization flattens
        // an inherited `def self.x` onto every descendant's table.
        if crate::dispatch::class_method_defined_here(anc, name) {
            return None;
        }
    }
    None
}

/// An ANCESTOR's class method as the OVERLAY currently holds it, for a
/// receiver that inherits it -- with the ancestor it came from.
///
/// Materialization copies an inherited `def self.x` onto every descendant's
/// flat table, so the flat probe answers with a copy of the FINAL body. A
/// parent whose class method has an observable redefinition TIMELINE keeps the
/// live body in its own overlay instead, and without this a subclass call
/// answered the final body from the program's first line.
///
/// `None` as soon as a nearer ancestor DEFINES the name itself: that row is
/// closer than any overlay further up, and the flat path already serves it.
pub fn inherited_overlay_class_method(id: ClassId, name: Symbol) -> Option<(ClassId, RProc)> {
    for &anc in crate::dispatch::ancestors_of_value(id).iter().skip(1) {
        if let Some(p) = overlay_class_method(anc, name) {
            return Some((anc, p));
        }
        if crate::dispatch::class_method_defined_here(anc, name) {
            return None;
        }
    }
    None
}

/// [`overlay_class_method`] with the singleton-PREPEND layer skipped -- the
/// resume point for a prepended module method's own `super`, which must reach
/// the shadowed `def self.x` rather than the copy of itself.
pub fn overlay_class_method_below_prepends(id: ClassId, name: Symbol) -> Option<RProc> {
    let c = maps().classes.read().unwrap();
    c.get(&id.0)?.class_methods.get(&name).cloned()
}

/// The modules prepended into `id`'s singleton class, LATEST FIRST -- the
/// order they occupy in `id.singleton_class.ancestors`, ahead of the
/// singleton head itself.
pub fn singleton_prepends_of(id: ClassId) -> Vec<ClassId> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .map(|e| e.singleton_prepends.iter().rev().copied().collect())
        .unwrap_or_default()
}

/// Whether `mid` was prepended into `id`'s singleton class
/// (`id.singleton_class.prepend(mid)`) -- how `send_super_class_from` learns
/// that a `defining_class` missing from the ancestry sits in the prepend
/// layer, whose `super` resumes AT `id` rather than past it.
pub fn has_singleton_prepend(id: ClassId, mid: ClassId) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.singleton_prepends.contains(&mid))
}

/// The instance method THIS class's OWN overlay delta defines -- no ancestor
/// walk (unlike `resolve_dynamic`), so it is safe to call from the
/// reentrancy-sensitive `call_user_method` (which needs only "does THIS class
/// define the method itself"). Lets a runtime-defined `inspect`/`to_s`/`hash`
/// -- e.g. a native `Struct`/`Data` class's -- be honoured by `p`/string
/// interpolation, not just by `send`.
pub fn overlay_own_method(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    let c = maps().classes.read().unwrap();
    c.get(&id.0)?.methods.get(&name).cloned()
}

/// Record which module supplied each name a per-object `extend` just copied
/// -- see `OverlayMaps::extended_names`.
pub(super) fn record_extended_names(key: usize, mid: ClassId, names: &[Symbol]) {
    if names.is_empty() {
        return;
    }
    let mut w = maps().extended_names.write().unwrap();
    let table = w.entry(key).or_default();
    for &name in names {
        table.insert(name, mid);
    }
}

/// Which module an `extend` copied `name` onto this object from, or `None`
/// for a row the object defines itself -- the per-object twin of
/// `overlay_class_method_is_extended`. CRuby files an extended module in the
/// singleton class's SUPER chain, so its rows are not the singleton's OWN.
pub fn extended_name_source(recv: &RubyValue, name: Symbol) -> Option<ClassId> {
    let key = value_identity(recv)?;
    maps()
        .extended_names
        .read()
        .unwrap()
        .get(&key)
        .and_then(|t| t.get(&name).copied())
}

/// Drop the extended-module record for one name -- every OWN definition (and
/// undef) of a per-object singleton owns the name from then on.
pub(super) fn clear_extended_name(key: usize, name: Symbol) {
    if let Some(t) = maps().extended_names.write().unwrap().get_mut(&key) {
        t.remove(&name);
    }
}

/// The class a PER-OBJECT singleton method is rooted at for reflection --
/// `obj.method(:x)`'s home: the module a per-object `extend` copied it from,
/// or the object's own singleton class for an own `def obj.x` (minted on
/// demand, as CRuby's `.owner` observably does).
pub fn per_object_method_home(recv: &RubyValue, name: Symbol) -> Option<ClassId> {
    let key = value_identity(recv)?;
    let recorded = maps()
        .extended_names
        .read()
        .unwrap()
        .get(&key)
        .and_then(|t| t.get(&name).copied());
    if let Some(mid) = recorded {
        return Some(mid);
    }
    match runtime_singleton_class(recv) {
        Ok(RubyValue::Class(sid)) => Some(sid),
        _ => None,
    }
}

/// Whether a singleton class EXISTS for `recv` -- CRuby's
/// `RCLASS_SINGLETON_P(CLASS_OF(obj))`, which is what decides how a
/// `NoMethodError` names its receiver (`#<K:0xADDR>` instead of `an
/// instance of K`).
///
/// Materializing one is the trigger, not having methods in it:
/// oracle-verified, a bare `obj.singleton_class` flips it, removing the
/// last `def obj.m` does NOT flip it back, and `freeze` never flips it.
/// So the question is asked of every place one can be materialized -- a
/// minted class, an own per-object def, a per-object `extend` -- rather
/// than of the method tables alone.
///
/// `nil`/`true`/`false` answer their own class from `singleton_class`,
/// so they have none of their own; they render bare anyway.
pub fn has_singleton_class(recv: &RubyValue) -> bool {
    if matches!(recv, RubyValue::Nil | RubyValue::Bool(_)) {
        return false;
    }
    if super::singleton_class_key(recv)
        .is_some_and(|k| maps().singleton_classes.read().unwrap().contains_key(&k))
    {
        return true;
    }
    let Some(key) = value_identity(recv) else {
        return false;
    };
    let own =
        |t: &FMap<usize, FMap<Symbol, MethodImpl>>| t.get(&key).is_some_and(|t| !t.is_empty());
    own(&maps().singletons.read().unwrap())
        || maps()
            .value_singletons
            .read()
            .unwrap()
            .get(&key)
            .is_some_and(|t| !t.is_empty())
        || maps()
            .extended
            .read()
            .unwrap()
            .get(&key)
            .is_some_and(|l| !l.is_empty())
}

/// Whether `recv` (an object) has a per-object singleton method `name` --
/// `respond_to?`'s identity-keyed probe, since the class-id walk can't see a
/// singleton installed on one specific object.
pub fn object_has_singleton_method(recv: &RubyValue, name: Symbol) -> bool {
    let Some(key) = value_identity(recv) else {
        return false;
    };
    let by_object = maps()
        .singletons
        .read()
        .unwrap()
        .get(&key)
        .is_some_and(|t| t.contains_key(&name));
    by_object
        || maps()
            .value_singletons
            .read()
            .unwrap()
            .get(&key)
            .is_some_and(|t| t.contains_key(&name))
}

/// Whether class `id`'s OWN overlay table defines instance method `name` -- a
/// runtime `define_method` delta on a frozen class, or a runtime class's own
/// method. `respond_to?`'s ancestor walk consults this per ancestor.
pub fn overlay_has_instance_method(id: ClassId, name: Symbol) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.methods.contains_key(&name) || e.prepended.contains_key(&name))
}

/// A runtime class's leaked ancestor chain -- `None` for a frozen id or a pure
/// method-delta (empty ancestors). Feeds `ancestors_of_value`.
pub fn overlay_ancestors(id: ClassId) -> Option<&'static [ClassId]> {
    let c = maps().classes.read().unwrap();
    let anc = c.get(&id.0)?.ancestors;
    if anc.is_empty() { None } else { Some(anc) }
}

/// A runtime class's Ruby-visible name (or the anonymous `#<Class:ID>` form).
pub fn overlay_class_name(id: ClassId) -> Option<String> {
    // Everything this needs from the entry, taken under one guard which then
    // ends. `class_name` below walks back into the overlay -- through
    // `refinement_of` and through this very function for a nested owner -- and
    // holding the guard across it re-enters `classes`. See `runtime_meta::lock`.
    let (owner_class, is_module, addr, name) = {
        let c = maps().classes.read().unwrap();
        let entry = c.get(&id.0)?;
        if entry.ancestors.is_empty() {
            return None; // a pure delta over a frozen class carries no name of its own
        }
        (
            entry.owner_class,
            entry.is_module,
            entry.addr,
            entry.name.read().unwrap().clone(),
        )
    };
    if let Some(name) = name {
        return Some(name);
    }
    // An anonymous value renders as `#<ITS CLASS:0xADDR>`, so a module a
    // `class X < Module` minted reads `#<X:0x...>` -- CRuby's rule, and the
    // reason the owner is consulted before the plain Class/Module split.
    let kind = match owner_class.and_then(crate::dispatch::class_name) {
        Some(owner) => owner,
        None if is_module => "Module".to_string(),
        None => "Class".to_string(),
    };
    Some(format!("#<{kind}:0x{addr:016x}>"))
}

/// Whether class `id`'s OWN entry undef'd `name` -- the terminator every MRO
/// walk probes per ancestor, alongside `ClassRegistry::is_undefined`.
pub fn overlay_is_undefined(id: ClassId, name: Symbol) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.undefs.contains(&name))
}

/// Whether class `id`'s OWN entry had `name` REMOVED -- the walk skips this
/// ancestor's tables and keeps going, where [`overlay_is_undefined`] stops it.
pub fn overlay_is_removed(id: ClassId, name: Symbol) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.removed.contains(&name))
}

/// [`overlay_is_removed`]'s class-method twin -- true for a name
/// `remove_method` took out of `class << self`, and for one whose
/// materialized copy no `extend` has seated yet. Both empty the class's own
/// position without ending the walk, which is the same answer every reader
/// needs; see [`OverlayEntry::class_deferred`] for why the two sets are kept
/// apart.
pub fn overlay_class_removed(id: ClassId, name: Symbol) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.class_removed.contains(&name) || e.class_deferred.contains(&name))
}

/// Reverse of [`overlay_class_name`]: the runtime class id whose Ruby-visible
/// name is `name` (so `Marshal.load` can resolve a `Struct.new`-minted or
/// otherwise runtime-defined class back to its id). Only real, named classes
/// match -- anonymous ids and pure frozen-class deltas never do.
pub fn runtime_class_id_by_name(name: &str) -> Option<ClassId> {
    if !is_live() {
        return None;
    }
    let c = maps().classes.read().unwrap();
    c.iter().find_map(|(&id, entry)| {
        if entry.ancestors.is_empty() {
            return None;
        }
        (entry.name.read().unwrap().as_deref() == Some(name)).then_some(ClassId(id))
    })
}

/// Whether a runtime id names a module (always `false` -- `Class.new` makes a
/// class). `None` for a frozen id / pure delta.
pub fn overlay_is_module(id: ClassId) -> Option<bool> {
    let c = maps().classes.read().unwrap();
    let entry = c.get(&id.0)?;
    if entry.ancestors.is_empty() {
        None
    } else {
        Some(entry.is_module)
    }
}

/// A runtime class's constructor -- feeds `constructor_of` so `RuntimeClass.new`
/// works through the same `construct_by_class_id` path frozen classes use.
pub fn overlay_constructor(id: ClassId) -> Option<ConstructorFn> {
    let c = maps().classes.read().unwrap();
    c.get(&id.0)?.constructor
}

/// A fresh uninitialized instance of a runtime (`Class.new`) class, backing
/// `Class#allocate` -- a name-keyed `DynObject` with no `initialize` run.
/// `None` if `id` is not a known runtime class.
pub fn runtime_allocate(id: ClassId) -> Option<RubyValue> {
    if !maps().classes.read().unwrap().contains_key(&id.0) {
        return None;
    }
    // Mirror of `runtime_class_new`'s constructor pick: a runtime
    // subclass of a compiled class allocates the ancestor's struct under
    // its own id.
    if let Some(alloc) = crate::dispatch::ancestor_allocator_of(id) {
        return Some(RubyValue::Object(alloc(id)));
    }
    // A NATIVE class with no allocator gets nothing. Its rows downcast to
    // their own payload, so a name-keyed `DynObject` is a landmine: it
    // allocates fine and the next method call panics on the downcast, which
    // takes the process rather than raising. `BigDecimal.allocate` did
    // exactly that, and `Marshal.load` of a BigDecimal walked into it.
    //
    // `None` here is what `Class#allocate` reports as ruby's `TypeError:
    // allocator undefined for X` -- which is also ruby's own answer for a
    // class whose allocator is undefined. A native class that SHOULD be
    // allocatable says so with `allocate` in its `ruby_class!` header.
    if crate::builtins::registered_table(id).is_some() {
        return None;
    }
    Some(RubyValue::Object(super::dyn_object::dyn_alloc(id)))
}

/// Coerce a `define_method`/`define_singleton_method` NAME argument (a Symbol
/// or String) to a `Symbol` -- CRuby's `rb_to_id`.
pub(crate) fn coerce_method_name(arg: Option<&RubyValue>) -> Result<Symbol, Signal> {
    match arg {
        Some(RubyValue::Symbol(s)) => Ok(*s),
        Some(RubyValue::Str(s)) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        // CRuby's `rb_to_id` message, which INSPECTS the value: `1 is not a
        // symbol nor a string`, the same shape the ivar and constant
        // coercions already use.
        Some(other) => Err(type_error!(
            "{} is not a symbol nor a string",
            other.inspect_string()
        )),
        None => Err(type_error!("nil is not a symbol nor a string")),
    }
}

/// The block that becomes the method body: the passed block, or a `Proc` given
/// as the second positional argument (`define_method(:x, some_proc)`). A
/// `Method`/`UnboundMethod` second argument is a documented fast-follow.
pub(crate) fn coerce_method_body(
    body: Option<&RubyValue>,
    block: &Option<RubyValue>,
) -> Result<RProc, Signal> {
    if let Some(RubyValue::Proc(p)) = block {
        return Ok(p.clone());
    }
    if let Some(RubyValue::Proc(p)) = body {
        return Ok(p.clone());
    }
    // A body that is present but WRONG is a TypeError naming what the row
    // takes; only an absent one is the "without a block" ArgumentError.
    // (`Method`/`UnboundMethod` never reach here -- `define_method` peels
    // them off first.)
    match body {
        Some(other) => Err(crate::builtins::type_error!(
            "wrong argument type {} (expected Proc/Method/UnboundMethod)",
            crate::builtins::class_name_of(other)
        )),
        None => Err(arg_error!("tried to create Proc object without a block")),
    }
}

pub(super) fn immediate_kind(v: &RubyValue) -> &'static str {
    match v {
        RubyValue::Nil => "nil",
        RubyValue::Bool(true) => "true",
        RubyValue::Bool(false) => "false",
        RubyValue::Int(_) | RubyValue::BigInt(_) => "Integer",
        RubyValue::Float(_) => "Float",
        RubyValue::Symbol(_) => "Symbol",
        _ => "this value",
    }
}

#[cfg(test)]
mod allocate_tests {
    use crate::ClassId;

    /// A NATIVE class with no allocator of its own gets NOTHING from
    /// `runtime_allocate`, and `Class#allocate` turns that `None` into ruby's
    /// `TypeError: allocator undefined for X`.
    ///
    /// The regression this guards is not a wrong answer but an ABORT: the
    /// generic blank is a name-keyed `DynObject`, a native class's rows
    /// downcast to their own payload, and the downcast panics. `BigDecimal`
    /// was the case, reached through `Marshal.load`.
    #[test]
    fn a_native_class_with_no_allocator_is_refused() {
        let mut refused = 0;
        for b in zeo_abi::BUILTINS.iter() {
            let id = ClassId(b.id.0);
            if crate::builtins::registered_table(id).is_none() {
                continue;
            }
            if crate::dispatch::ancestor_allocator_of(id).is_some() {
                continue;
            }
            assert!(
                super::runtime_allocate(id).is_none(),
                "{} would hand out a generic blank, which its own method \
                 table cannot downcast",
                b.name
            );
            refused += 1;
        }
        assert!(
            refused > 0,
            "no native class was examined, so this test proves nothing"
        );
    }

    /// A class the runtime map does not know is not this entry's business at
    /// all -- the caller's other arms answer.
    #[test]
    fn an_unknown_class_is_declined() {
        assert!(super::runtime_allocate(ClassId(u32::MAX)).is_none());
    }
}
