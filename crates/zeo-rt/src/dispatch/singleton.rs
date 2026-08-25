//! The singleton chain -- ruby's parallel metaclass hierarchy, derived
//! from the instance ancestry per walk: `SingletonPos`, the class-method
//! `super` walk and its `ClassResume` cell, and the compile-resolved
//! singleton super targets.

use super::*;

/// [`super_defined`]'s class-method half: the question
/// [`send_super_class_from`] answers by calling, asked without calling.
///
/// It asks it the SAME way -- one singleton walk, resumed at the same
/// position -- because a probe and a walk that answer one question twice
/// drift, and this pair has drifted twice already. The only difference is
/// that a hit becomes `true` instead of a call.
pub(super) fn super_class_defined(
    recv_class: ClassId,
    defining_class: ClassId,
    name: Symbol,
) -> bool {
    let walk = singleton_walk(recv_class);
    let start = class_super_resume(&walk, defining_class, name)
        .unwrap_or_else(|| past_own_position(&walk, recv_class));
    resolve_from(&walk, start, name).is_some() || instance_tail_defines(recv_class, name)
}

/// The class-method walk's tail: `#<Class:BasicObject>` inherits from `Class`,
/// so a class-method `super` with nothing above it continues into `Class`'s
/// own INSTANCE methods. `send_walk_from` ends there, and the probe
/// has to as well.
fn instance_tail_defines(recv_class: ClassId, name: Symbol) -> bool {
    let recv = RubyValue::Class(recv_class);
    crate::dispatch::reflect::scan_owner_from(recv.class_id(), 0, name).is_some()
}

/// One position in a class's SINGLETON chain -- ruby's parallel metaclass
/// hierarchy, which zeo derives from the instance ancestry rather than
/// materializing.
///
/// `K.singleton_class.ancestors` is `[K's singleton prepends] #<Class:K>
/// [K's extends] [.. the same three for K's superclass ..]
/// #<Class:BasicObject> Class Module Object Kernel BasicObject`, and a class
/// method resolves down it exactly as an instance method resolves down an
/// ordinary ancestry.
///
/// It has to be POSITIONS rather than class ids, and that is the whole reason
/// this type exists. A module both `extend`ed and `singleton_class.prepend`ed
/// into one class holds TWO of them and its body runs once per position --
/// while the `defining_class` a compiled body hands `super` names the module,
/// which cannot say which copy is running. The walk therefore publishes the
/// position it entered ([`ClassResume`]), the same fact [`MroResume`] carries
/// for the instance side.
#[derive(Clone, Copy)]
enum SingletonPos {
    /// `#<Class:host>` -- the rows `host` itself owns.
    Own(ClassId),
    /// A module seated in a singleton class -- prepended (ahead of its
    /// host's `Own`) or extended (behind it). Which side it sits on is
    /// carried by its POSITION, which is the only thing the walk needs; both
    /// answer from the MODULE, never from the host's flattened copy of it.
    Mixin(ClassId),
}

impl SingletonPos {
    /// The class a body written at this position bakes as its
    /// `defining_class`, and so the class a `super` from it names.
    fn defining(self) -> ClassId {
        match self {
            SingletonPos::Own(c) => c,
            SingletonPos::Mixin(module) => module,
        }
    }
}

/// `recv_class`'s singleton chain.
///
/// Built per walk rather than cached: only `super` and `Method#super_method`
/// reach it, so it is off every path a program runs in a loop. Ordinary
/// class-method dispatch probes the same three layers in the same order
/// inline (`send_value_in_reason`'s Class arm), which is why it always lands
/// on the FIRST position and needs no walk of its own.
///
/// `include`d modules never join a singleton chain -- `class K; include M`
/// leaves `K.singleton_class.ancestors` alone -- so module ancestors are
/// skipped.
fn singleton_walk(recv_class: ClassId) -> Vec<SingletonPos> {
    let mut out: Vec<SingletonPos> = Vec::new();
    let mut extended_seen = FSet::default();
    let live = crate::runtime_meta::is_live();
    for &anc in ancestors_of_value(recv_class) {
        // A module INCLUDED into a class contributes nothing to that class's
        // singleton chain -- `class K; include M` leaves
        // `K.singleton_class.ancestors` alone. The RECEIVER's own layer always
        // counts, module or not: `module M; extend N; def self.x` has a
        // singleton class like any other, and skipping it left the walk empty
        // so `M.method(:x).super_method` answered nil.
        if anc != recv_class && registry().entries.get(&anc.0).is_some_and(|e| e.is_module) {
            continue;
        }
        if live {
            for module in crate::runtime_meta::singleton_prepends_of(anc) {
                out.push(SingletonPos::Mixin(module));
            }
        }
        out.push(SingletonPos::Own(anc));
        // Newest `extend` closest to the singleton head, each carrying its own
        // ancestry -- `runtime_meta::singleton_super_chain`'s order, which is
        // the order their bodies were installed in. An `include` searches the
        // whole chain, so an extend of a module already seated adds nothing;
        // a PREPEND of one does, which is why only this side dedups.
        for module in crate::runtime_meta::extended_modules(&RubyValue::Class(anc))
            .into_iter()
            .rev()
        {
            for &m in ancestors_of_value(module) {
                if extended_seen.insert(m) {
                    out.push(SingletonPos::Mixin(m));
                }
            }
        }
    }
    out
}

/// What one position holds for `name`, or nothing.
///
/// The three sources an `Own` position answers from are the three
/// `send_value_in_reason`'s Class arm probes, minus the copies that are now
/// positions of their own: `prepended_class_methods` sits ABOVE this position
/// and the `extend` copies BELOW it, so reading the host's flattened table
/// here would answer at the wrong place -- and, for a prepended module, would
/// answer with the very copy the walk arrived through.
fn resolve_at(pos: SingletonPos, name: Symbol) -> Option<ClassHit> {
    match pos {
        SingletonPos::Own(anc) => {
            // `remove_method` inside `class << self` empties exactly this
            // position and lets the walk carry on -- which is the whole
            // difference between it and an `undef`, and why it is asked here
            // rather than of the chain.
            if crate::runtime_meta::is_live()
                && crate::runtime_meta::overlay_class_removed(anc, name)
            {
                return None;
            }
            if crate::runtime_meta::is_live()
                && !crate::runtime_meta::overlay_class_method_is_extended(anc, name)
                && let Some(p) = crate::runtime_meta::overlay_class_method_below_prepends(anc, name)
            {
                return Some(ClassHit::Overlay(p));
            }
            // Only a row this ancestor really OWNS. Materialization flattens
            // an inherited `def self.x` onto every descendant's table, so
            // taking the nearest copy would step over a module seated in an
            // ancestor's singleton further up.
            if registry().class_method_is_own(anc, name)
                && let Some(f) = registry()
                    .entries
                    .get(&anc.0)
                    .and_then(|e| e.class_methods.get(&name).copied())
            {
                return Some(ClassHit::Row(f));
            }
            crate::builtins::class_method_table(anc)
                .and_then(|lookup| lookup(name.name_str()))
                .map(ClassHit::Builtin)
        }
        // Both mixin kinds answer from the module itself, wrapped to take the
        // Class as `self` -- which is what an `extend`ed instance method IS
        // when it serves as a class method.
        SingletonPos::Mixin(module) => {
            crate::runtime_meta::singleton_mixin_method(module, name).map(ClassHit::Overlay)
        }
    }
}

/// Where a `super` written in `defining_class` resumes, and which copy of it
/// is running.
#[derive(Clone, Copy)]
pub(crate) struct ClassResume {
    defining: ClassId,
    next: usize,
}

std::thread_local! {
    /// The class-method twin of [`MRO_RESUME`], and deliberately a SECOND
    /// cell: the two channels index different sequences (an instance ancestry
    /// against a singleton walk), so a module used both as an instance mixin
    /// and a singleton one would have them collide on `defining_class` and
    /// resume at a position belonging to the other chain.
    static CLASS_MRO_RESUME: std::cell::Cell<Option<ClassResume>> = const {
        std::cell::Cell::new(None)
    };
}

/// Runs `f` with the class-method resume cell set, restoring the caller's on
/// every exit path.
fn with_class_resume<T>(value: Option<ClassResume>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<ClassResume>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CLASS_MRO_RESUME.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(CLASS_MRO_RESUME.with(|c| c.replace(value)));
    f()
}

/// Clears the resume for a body ORDINARY dispatch enters. It always lands on
/// the first position, so it must not inherit a resume an enclosing `super`
/// walk published for a later copy of the same module.
pub(crate) fn with_ordinary_class_dispatch<T>(f: impl FnOnce() -> T) -> T {
    // One relaxed thread-local read on the ordinary path, where the cell is
    // empty in every program that never takes a class-method `super`. It
    // cannot be gated on `mro_duplicates`: the walk publishes a position for
    // an extended module's COPY too, which has nothing to do with duplicates.
    match CLASS_MRO_RESUME.with(|c| c.get()).is_some() {
        true => with_class_resume(None, f),
        false => f(),
    }
}

/// The fiber swap for [`CLASS_MRO_RESUME`] -- see `crate::ec`.
pub(crate) fn swap_class_mro_resume(v: Option<ClassResume>) -> Option<ClassResume> {
    CLASS_MRO_RESUME.with(|c| c.replace(v))
}

/// The position a `super` written in `defining_class` resumes at.
///
/// The published resume when it names this very copy -- the only thing that
/// can tell two copies of a doubled module apart -- and otherwise the position
/// after the one the body OCCUPIES.
///
/// Those are not the same as "the position named `defining_class`", and the
/// difference is materialization. zeo copies an `extend`ed module's methods
/// onto every class in the chain, and such a copy bakes the class it was
/// materialized ONTO as its `defining_class` while sitting at the MODULE's
/// position further down -- `Sub.inspect` runs a copy that says `Base` and
/// belongs after `NameDSL`. Resuming after `Own(Base)` re-entered the module
/// and ran its body twice.
///
/// So the occupied position is the first one at or after `defining_class`'s
/// own layer that really answers `name`. When that layer answers itself --
/// every ordinary `def self.x` -- this is exactly the old rule.
fn class_super_resume(
    walk: &[SingletonPos],
    defining_class: ClassId,
    name: Symbol,
) -> Option<usize> {
    if let Some(r) = CLASS_MRO_RESUME.with(|c| c.get())
        && r.defining == defining_class
    {
        return Some(r.next);
    }
    let from = walk.iter().position(|p| p.defining() == defining_class)?;
    let at = resolve_from(walk, from, name).map_or(from, |(i, _)| i);
    Some(at + 1)
}

/// `super` inside a CLASS method (`def self.x`) -- the class-method channel
/// `send_super_from` (object receivers only) cannot serve.
///
/// Resumes the singleton walk past the position this body is running AT. A
/// `defining_class` the chain does not seat at all is a body installed
/// somewhere the walk cannot name (a `define_method` on a module reached
/// through a runtime wrapper); it resumes past the receiver's OWN singleton
/// layer, which is the nearest true answer and what this did for every such
/// body before positions existed.
pub fn send_super_class_from(
    recv_class: ClassId,
    defining_class: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let walk = singleton_walk(recv_class);
    let start = class_super_resume(&walk, defining_class, name)
        .unwrap_or_else(|| past_own_position(&walk, recv_class));
    send_walk_from(recv_class, &walk, start, name, args, block)
}

/// Where a body the walk cannot place resumes: just past `#<Class:recv>`.
///
/// Such a body is one installed somewhere with no position of its own -- a
/// `Class.new` block's `def self.x`, a `define_method` on a module reached
/// through a runtime wrapper -- and the receiver's own singleton class is
/// where it almost always belongs. Past `Own` and no further: the receiver's
/// EXTENDS sit behind that position and are a legitimate super target, which
/// is exactly what a runtime class's own `def self.x` reaches.
fn past_own_position(walk: &[SingletonPos], recv_class: ClassId) -> usize {
    walk.iter()
        .position(|p| matches!(*p, SingletonPos::Own(c) if c == recv_class))
        .map_or(0, |own| own + 1)
}

/// Invoke the CLASS method `name` starting the walk AT `from` -- the
/// class-method twin of [`send_below_overlay_at`], for a `Method` that `#super_method`
/// re-seated onto an ancestor's `def self.x`.
pub fn send_class_from(
    recv_class: ClassId,
    from: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let walk = singleton_walk(recv_class);
    let start = walk.iter().position(|p| p.defining() == from).unwrap_or(0);
    send_walk_from(recv_class, &walk, start, name, args, block)
}

/// The owner and POSITION of the first thing that answers `name` at or after
/// position `from` on `recv_class`'s singleton chain -- `#super_method`'s
/// walk, sharing this module's resolution so it cannot drift from the call's.
pub(crate) fn singleton_owner_from(
    recv_class: ClassId,
    from: usize,
    name: Symbol,
) -> Option<(ClassId, usize)> {
    let walk = singleton_walk(recv_class);
    let (at, _) = resolve_from(&walk, from, name)?;
    Some((walk[at].defining(), at))
}

/// Whether `recv_class`'s singleton chain resolves `name` at its own HEAD --
/// `#<Class:recv_class>` -- rather than at a module seated ahead of it.
///
/// "Does the class own a `def self.x`" is not the question: a module PREPENDED
/// into the singleton sits ahead of the head and owns the lookup even when the
/// class does define one. Only the chain can say which position wins.
pub(crate) fn singleton_resolves_at_head(recv_class: ClassId, name: Symbol) -> bool {
    let walk = singleton_walk(recv_class);
    match resolve_from(&walk, 0, name) {
        Some((at, _)) => matches!(walk[at], SingletonPos::Own(c) if c == recv_class),
        None => false,
    }
}

/// Whether `recv_class`'s singleton chain seats `owner` as a MIXIN -- a module
/// `extend`ed onto some ancestor or prepended into its singleton, rather than
/// a class's own `def self.x` layer.
///
/// `Method#owner` names such a module BARE where an own class method reports a
/// singleton class, and the by-NAME question cannot answer it after
/// `#super_method` has re-seated: `OwnFirst` owns `tag` itself, so "what
/// extend supplies `tag`" is nothing, even though `M1` supplies the copy the
/// re-seat landed on. The position knows.
pub(crate) fn singleton_seats_as_mixin(recv_class: ClassId, owner: ClassId) -> bool {
    singleton_walk(recv_class)
        .iter()
        .any(|p| matches!(*p, SingletonPos::Mixin(m) if m == owner))
}

/// Where `after` sits on `recv_class`'s singleton chain.
pub(crate) fn singleton_position_of(recv_class: ClassId, after: ClassId) -> Option<usize> {
    singleton_walk(recv_class)
        .iter()
        .position(|p| p.defining() == after)
}

/// Invoke `name` down `recv_class`'s WHOLE singleton chain -- what a
/// class-level hook (`def self.respond_to_missing?`) is reached through, since
/// it must resolve exactly as an ordinary class-method send would.
pub fn send_class_chain(
    recv_class: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let walk = singleton_walk(recv_class);
    send_walk_from(recv_class, &walk, 0, name, args, block)
}

/// The shared body: walk `positions` from `start` and invoke the first hit,
/// publishing the position it entered so that body's own `super` resumes past
/// THIS copy rather than past the first one.
fn send_walk_from(
    recv_class: ClassId,
    positions: &[SingletonPos],
    start: usize,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let recv = RubyValue::Class(recv_class);
    if let Some((at, hit)) = resolve_from(positions, start, name) {
        let resume = Some(ClassResume {
            defining: positions[at].defining(),
            next: at + 1,
        });
        let defining = positions[at].defining();
        return with_class_resume(resume, || match hit {
            // Through `call_value_body` for the method frame -- see the
            // ordinary-dispatch arm in `send_value_in_reason_inner`.
            ClassHit::Overlay(p) => {
                crate::runtime_meta::call_value_body(defining, name, &p, &recv, args, block)
            }
            ClassHit::Row(f) => f.call(&recv, args, block),
            ClassHit::Builtin(f) => f(&recv, args, block),
        });
    }
    // The singleton chain does not stop at the last ancestor:
    // `#<Class:BasicObject>` inherits from `Class` itself, so a class-method
    // `super` with nothing above it continues into `Class`'s own INSTANCE
    // methods, with the class object as `self`. That is where the default
    // allocate-then-`initialize` lives -- what a `def self.new` wrapping
    // construction reaches by `super` (rubygems' `Gem::Package::TarWriter
    // .new`). `send_walking` raises the same `MissingReason::Super` if that
    // comes up empty too.
    //
    // Cleared across the boundary: a body down there is an ordinary instance
    // method of `Class`/`Module` and must not inherit a position from the
    // singleton walk it fell out of.
    with_class_resume(None, || send_walking(&recv, 0, name, args, block))
}

/// What a class-method walk found -- the three sources a position can answer
/// from.
enum ClassHit {
    Overlay(crate::rproc::RProc),
    Row(ValueImpl),
    Builtin(crate::BuiltinMethodFn),
}

/// The walk's RESOLUTION, factored out so `defined?(super)` asks the same
/// question the call answers.
///
/// It could not before positions existed: the probe scanned from the target's
/// own ancestor index, which INCLUDES a module prepended into that singleton,
/// while the walk skipped that layer wholesale (it got there THROUGH it). A
/// singleton-prepended module with no row beneath it therefore reported a
/// super target and then raised looking for it -- and with an `extend` of the
/// same module underneath, re-entered its own copy until the stack died.
fn resolve_from(
    positions: &[SingletonPos],
    start: usize,
    name: Symbol,
) -> Option<(usize, ClassHit)> {
    positions
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(i, &pos)| resolve_at(pos, name).map(|hit| (i, hit)))
}

/// Dispatch a `super` whose target the COMPILER resolved against the
/// receiver's singleton chain (the `extend M` shape -- sibling extends
/// interleave in an order only the compile-time chain knows; the runtime
/// registry records no per-class extends list). The target is either a
/// module's instance method serving as a class method (`module_instance`,
/// reached through the module's own value-method bridge) or an ancestor's
/// own `def self.x` (its registered `class_methods` row / builtin table).
/// `self` stays the RECEIVER class object throughout, like every `super`.
pub fn call_singleton_super_target(
    target: ClassId,
    module_instance: bool,
    recv_class: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    call_singleton_super_target_at(target, module_instance, recv_class, name, args, block)
}

/// Where the singleton walk seats the target this `super` resolved to at
/// compile time, or `None` when no chain in the process holds a class twice
/// (nothing can then disagree, and the walk is not worth building) or the
/// target sits somewhere the derived walk cannot name.
///
/// The search starts past the position the RUNNING body occupies, which the
/// resume cell names directly. With no cell the body was entered by ordinary
/// dispatch, which always lands on the first position that answers -- so that
/// position is computable from the name alone.
fn super_target_position(
    recv_class: ClassId,
    target: ClassId,
    name: Symbol,
    module_instance: bool,
) -> Option<usize> {
    // A MODULE target always needs it: the body is a copy of the module's
    // method, and whichever copy runs bakes a `defining_class` that names a
    // CLASS -- so the resume is the only thing that can seat it at the
    // module's position. A `def self.x` target sits at `Own(target)`, which
    // the fallback already finds, so it pays for this only when some chain
    // holds a class twice and the fallback cannot tell two copies apart.
    if !module_instance && !mro_duplicates() {
        return None;
    }
    let walk = singleton_walk(recv_class);
    let from = match CLASS_MRO_RESUME.with(|c| c.get()) {
        Some(r) => r.next,
        None => resolve_from(&walk, 0, name).map_or(0, |(i, _)| i + 1),
    };
    walk.iter()
        .enumerate()
        .skip(from)
        .find(|(_, p)| p.defining() == target)
        .map(|(i, _)| i)
}

/// [`call_singleton_super_target`] past the resume publication.
fn call_singleton_super_target_at(
    target: ClassId,
    module_instance: bool,
    recv_class: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let recv = RubyValue::Class(recv_class);
    let method_name = name.to_string();
    // This route runs a body the WALK also knows a position for, so it must
    // publish that position -- otherwise the body's own `super` starts the
    // walk over and reaches the same target a second time, which is one extra
    // run of a doubled module's body.
    //
    // The POSITION is the target's wherever the body comes from; the class it
    // reports as its `defining_class` is not. The receiver's own emitted copy
    // of a module method is compiled in the RECEIVER's context and bakes the
    // receiver, while the module's generic bridge bakes the module -- so the
    // resume is stamped per arm rather than once up front.
    let at = super_target_position(recv_class, target, name, module_instance);
    let resume = |defining: ClassId| {
        at.map(|i| ClassResume {
            defining,
            next: i + 1,
        })
    };
    if module_instance {
        // The RECEIVER's own emitted copy of the module's method first --
        // its `super` resolves the receiver's singleton chain, which the
        // module's generic bridge (emitted in the module's own context)
        // cannot. See `ClassEntry::singleton_super_targets`.
        if let Some(f) = registry()
            .entries
            .get(&recv_class.0)
            .and_then(|e| e.singleton_super_targets.get(&(target.0, name)).copied())
        {
            return with_class_resume(resume(recv_class), || f.call(&recv, args, block));
        }
        if let Some(f) = value_method(target, 0, name) {
            return with_class_resume(resume(target), || f.call(&recv, args, block));
        }
    } else {
        if crate::runtime_meta::is_live()
            && let Some(p) = crate::runtime_meta::overlay_class_method(target, name)
        {
            return with_class_resume(resume(target), || {
                p.call_with_self_and_block(&recv, args, block)
            });
        }
        if let Some(f) = registry()
            .entries
            .get(&target.0)
            .and_then(|e| e.class_methods.get(&name).copied())
        {
            return with_class_resume(resume(target), || f.call(&recv, args, block));
        }
        if let Some(f) = crate::builtins::class_method_table(target).and_then(|t| t(&method_name)) {
            return with_class_resume(resume(target), || f(&recv, args, block));
        }
    }
    Err(raise_method_missing(
        &recv,
        &method_name,
        args,
        MissingReason::Super,
    ))
}
