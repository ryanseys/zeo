//! Reflection over the method tables: `respond_to?`, owner scans,
//! `instance_methods`/visibility, class-method names/privacy -- every
//! "which rows exist and who owns them" question. The census and the
//! `builtins::resolve` accessor (Phase 0 item 8) read through here.

use super::*;

/// Whether `recv` carries a not-implemented stub named `name` (see
/// [`is_notimplement_row`]). Reflection that asks whether a row EXISTS --
/// `method`, `instance_method` -- must answer yes exactly where `respond_to?`
/// answers no, which is the whole shape of `rb_f_notimplement`.
pub fn has_notimplement_row(recv: &RubyValue, name: Symbol) -> bool {
    is_notimplement_row(recv.class_id(), name)
        || matches!(recv, RubyValue::Class(cid) if is_notimplement_row(*cid, name))
}

/// `respond_to?` on a VALUE receiver. Like [`responds_to`], but it also
/// honours a per-object singleton method. Those are keyed by object identity,
/// so the class-id-only [`responds_to`] cannot see them. Codegen's
/// `respond_to?` fast path routes here, so a `def obj.foo` singleton answers
/// true.
pub fn responds_to_value(recv: &RubyValue, name: Symbol, include_all: bool) -> bool {
    // Asked HERE and not in `responds_to`, which doubles as an EXISTENCE
    // predicate -- `instance_method(:syscall)` must still build an
    // `UnboundMethod`, and report arity 0, for a row that exists but refuses.
    if has_notimplement_row(recv, name) {
        return false;
    }
    // Retired by an `undef` through the singleton class, checked before any
    // table -- the same order `send_value_in` uses, and for the same reason:
    // the instance walk over `Class`/`Module` at the bottom would otherwise
    // find the very definition the undef shadows. `Guarded.singleton_class.
    // undef_method :extend_object` has to make `Guarded.respond_to?
    // (:extend_object, true)` false, not just make the call raise.
    if let RubyValue::Class(cid) = recv
        && crate::runtime_meta::is_live()
        && crate::runtime_meta::class_method_undefined(*cid, name)
    {
        return false;
    }
    // The per-object twin of the class-method undef just above: `g.singleton_
    // class.undef_method(:close)` has to make `g.respond_to?(:close)` false
    // while `Foo.new.respond_to?(:close)` stays true, and the class walk below
    // would find the very definition the tombstone shadows.
    if crate::runtime_meta::is_live() && crate::runtime_meta::object_method_undefined(recv, name) {
        return false;
    }
    if crate::runtime_meta::is_live()
        && crate::runtime_meta::object_has_singleton_method(recv, name)
    {
        return true;
    }
    // A class/module receiver also responds to its class methods -- a user
    // `def self.x`, a `module_function`, or a `class << self` accessor (stored
    // as class methods), plus a builtin class-method table (`File.read`) -- none
    // of which the instance-method MRO walk over Class/Module below can see.
    // Mirrors the class-receiver dispatch order in `send_value_in`.
    if let RubyValue::Class(cid) = recv
        && class_receiver_responds(*cid, name)
    {
        // A `private_class_method` one is invisible to the default
        // `respond_to?`, the same rule the instance walk below applies.
        return include_all || !class_method_is_private(*cid, name);
    }
    // `private_class_method :new` marks a name the class does NOT define -- it
    // inherits `Class#new` -- so `class_receiver_responds` above says no and the
    // instance walk below would find the public `Class#new` and answer true.
    // The mark is what ruby reports, whichever table the body lives in.
    if let RubyValue::Class(cid) = recv
        && !include_all
        && class_method_is_private(*cid, name)
    {
        return false;
    }
    // A class-method ALIAS is a name indirection rather than a table row, so
    // ask the whole question again under the source name. Re-entering HERE and
    // not inside `class_receiver_responds` is what makes `alias [] new` answer:
    // `Class#new` is not a class method of the aliasing class at all, it is an
    // instance method of `Class`, which only the walk below finds. Rows are
    // terminal, so the re-entry cannot loop.
    if let RubyValue::Class(cid) = recv
        && let Some(old) = class_alias_target(*cid, name)
    {
        return responds_to_value(recv, old, include_all);
    }
    // ...and the payload root's own class methods, which a value subclass
    // inherits but no registry entry records. Same reason this sits out here
    // and not in `class_receiver_responds`: the row lives on the ROOT.
    if let RubyValue::Class(cid) = recv
        && crate::builtins::value_subclass::root_class_method_target(*cid, name.name_str())
            .is_some()
    {
        return true;
    }
    responds_to(recv.class_id(), name, include_all)
}

/// `respond_to?`'s FULL protocol (CRuby's `rb_obj_respond_to`): the plain
/// value-aware walk, then -- on a miss -- the receiver's USER-DEFINED
/// `respond_to_missing?(name, include_all)` hook (never the builtin
/// default row, which only exists for an override's `super` to reach).
/// Fallible because the hook is user code whose raise must propagate.
/// Wired at the `respond_to?` surfaces (codegen's two folds + the Kernel
/// row); internal duck-type probes (convert protocol, `Array#zip`'s
/// `:each` check, ...) keep the plain bool walk -- CRuby's own internal
/// probes are a mix, and the hook firing there is not oracle-pinned.
pub fn responds_to_or_missing(
    recv: &RubyValue,
    name: Symbol,
    include_all: bool,
) -> Result<bool, Signal> {
    if responds_to_value(recv, name, include_all) {
        return Ok(true);
    }
    let rtm = crate::symbol::wk::respond_to_missing();
    let args = [RubyValue::Symbol(name), RubyValue::Bool(include_all)];
    let id = recv.class_id();
    if let RubyValue::Object(o) = recv {
        if crate::runtime_meta::is_live()
            && let Some(m) = crate::runtime_meta::resolve_dynamic(o, id, rtm)
        {
            return Ok(m.call(o, &args, None)?.truthy());
        }
        if let Some(f) = registry().lookup_mro(id, rtm) {
            return Ok(f.call(o, &args, None)?.truthy());
        }
    } else if let RubyValue::Class(cid) = recv
        && class_defines_user_hook(*cid, rtm)
    {
        // A CLASS receiver's hook is a CLASS-level `respond_to_missing?`
        // (`def self.respond_to_missing?`, faker's Base) -- probed through
        // the same singleton-chain resolution its `method_missing` twin uses.
        return Ok(crate::dispatch::send_class_chain(*cid, rtm, &args, None)?.truthy());
    }
    // The VALUE channel, for both receiver shapes: a builtin REOPEN's hook
    // (`class Integer; def respond_to_missing?...`), and every method of a
    // namespace-slot builtin whose body is Ruby -- `WeakRef`, which inherits
    // `Delegator`'s hook and registers it here rather than on the object
    // channel `lookup_mro` reads.
    for &anc in ancestors_of_value(id) {
        if let Some(f) = value_method(anc, 0, rtm) {
            return Ok(f.call(recv, &args, None)?.truthy());
        }
    }
    Ok(false)
}

/// CRuby's `rb_obj_dig` (object.c): recurse `cur.dig(rest)` after a container's
/// own first-level lookup. A nil short-circuits to nil; an intermediate that
/// doesn't respond to `dig` raises TypeError -- which is why
/// `[1,[2]].dig(1,0,3)` raises (the `2` Integer has no `#dig`) instead of
/// silently indexing an Integer's bits through `[]`. Callers (`Array#dig`,
/// `Hash#dig`) perform their own first index, then hand the remaining keys here.
pub(crate) fn obj_dig(cur: RubyValue, rest: &[RubyValue]) -> Result<RubyValue, Signal> {
    if cur.is_nil() {
        return Ok(RubyValue::Nil);
    }
    let dig = crate::symbol::wk::dig();
    if !responds_to_value(&cur, dig, false) {
        return Err(type_error!(
            "{} does not have #dig method",
            crate::class_name_of_value(&cur)
        ));
    }
    send_value(&cur, dig, rest, None)
}

/// Whether a class/module VALUE responds to `name` via a class-method source
/// (runtime singleton overlay, registered `def self.x`/`module_function`/
/// `class << self` methods, a builtin class-method table, or a native struct
/// class's own methods) -- the reflection counterpart of `send_value_in`'s
/// `RubyValue::Class` dispatch probes.
pub(super) fn class_receiver_responds(cid: ClassId, name: Symbol) -> bool {
    // The ANCESTRY, not just `cid`: a subclass's singleton class inherits its
    // parent's, so a `Base.extend Store` answers on `Sub` too. The frozen half
    // needs no walk -- materialization already copied a `def self.x` onto every
    // subclass's entry -- but nothing copies a runtime overlay.
    if crate::runtime_meta::is_live()
        && ancestors_of_value(cid)
            .iter()
            .any(|&anc| crate::runtime_meta::overlay_class_method(anc, name).is_some())
    {
        return true;
    }
    // The frozen half: a REGISTERED class's entry is COMPLETE -- materialization
    // flattened every inherited `def self.x` onto it and left `undef`'d names
    // out -- so its own entry is the whole answer, and walking past it would
    // resurrect what a singleton-body `undef_method` retired. Only a RUNTIME
    // subclass (`Class.new(Base)`, a `describe` group) has no entry at all;
    // ITS inherited class methods live on the nearest registered ancestor, so
    // the walk runs for exactly the entry-less case.
    if let Some(r) = REGISTRY.get() {
        match r.entries.get(&cid.0) {
            Some(e) => {
                if e.class_methods.contains_key(&name) {
                    return true;
                }
            }
            None => {
                if ancestors_of_value(cid).iter().any(|&anc| {
                    r.entries
                        .get(&anc.0)
                        .is_some_and(|e| e.class_methods.contains_key(&name))
                }) {
                    return true;
                }
            }
        }
    }
    let n = name.name_str();
    if crate::builtins::class_method_table(cid).is_some_and(|lookup| lookup(n).is_some()) {
        return true;
    }
    // A module the class EXTENDED answers too -- the same edge dispatch runs
    // the call through, and the same two probes in the same order, so the
    // predicate cannot say no to a name the call would answer.
    if let Some(owner) = class_method_extend_source(cid, name)
        && ((crate::runtime_meta::is_live()
            && crate::runtime_meta::overlay_value_body(owner, name).is_some())
            || extended_class_method_body(owner, name).is_some())
    {
        return true;
    }
    crate::builtins::rstruct::is_struct_class(cid)
        && crate::builtins::rstruct::class_lookup(n).is_some()
}

/// Whether `cid` DEFINES instance method `name` itself, as opposed to
/// inheriting it or receiving a materialized copy from a module it included.
/// `remove_method` asks this: CRuby removes only a class's own definition and
/// raises `NameError` for anything else.
pub fn class_defines_own_instance_method(cid: ClassId, name: Symbol) -> bool {
    registry().defines_own(cid, name)
}

/// [`class_defines_own_instance_method`] for a caller that can run before the
/// registry is installed -- the overlay resolver, which the runtime's own unit
/// tests drive with no program registered. No registry means no frozen
/// definitions, so the answer is no.
pub fn class_defines_own_instance_method_if_registered(cid: ClassId, name: Symbol) -> bool {
    REGISTRY.get().is_some_and(|r| r.defines_own(cid, name))
}

/// Whether `cid` has an OWN class method `name` -- a `def self.x` materialized
/// into the registry, or a runtime singleton def -- as opposed to one merely
/// inherited from `Class`/`Module`. `extend` consults this so a receiver's own
/// singleton methods keep outranking a mixed-in module's copy (CRuby's "closest
/// singleton" rule); the inherited builtin `Class`/`Module` methods deliberately
/// don't count, because an extended module SHOULD override those.
pub fn class_defines_own_class_method(cid: ClassId, name: Symbol) -> bool {
    (crate::runtime_meta::is_live()
        && crate::runtime_meta::overlay_class_method(cid, name).is_some())
        || REGISTRY
            .get()
            .and_then(|r| r.entries.get(&cid.0))
            .is_some_and(|e| e.class_methods.contains_key(&name))
}

/// The class in `cid`'s ancestry whose `def self.<name>` a `cid.<name>` call
/// reaches -- `Method#owner` for a class-method lookup, and what tells a class
/// receiver's `method(:x)` that it found a class method rather than an
/// instance method of `Class`/`Module`. `None` when nothing up the chain
/// defines one.
pub fn class_method_owner(cid: ClassId, name: Symbol) -> Option<ClassId> {
    scan_class_method_owner(cid, 0, name).map(|(owner, _)| owner)
}

/// Whether `cid`'s `def self.<name>` is written HERE rather than materialized
/// down from an ancestor -- the distinction the flattened `class_methods` map
/// cannot make. A runtime definition counts; a class that recorded no own-set
/// falls back to "it has the row".
pub fn class_method_defined_here(cid: ClassId, name: Symbol) -> bool {
    if crate::runtime_meta::is_live()
        && crate::runtime_meta::overlay_class_method_below_prepends(cid, name).is_some()
    {
        return true;
    }
    REGISTRY
        .get()
        .is_some_and(|r| r.class_method_is_own(cid, name))
}

/// [`class_method_owner`]'s REPORTING twin -- what `Method#owner` answers.
///
/// A module PREPENDED into a singleton class wins the lookup, and ruby names
/// it as the owner. zeo flattens a singleton prepend into the host's own
/// class-method rows (the module's body becomes the winner, the host's own
/// `def self.x` becomes its super target), so the scan below correctly finds
/// the HOST and reflection must name the module instead.
///
/// Only reflection takes this route. `class_method_owner` stays the
/// dispatch answer, because `class_method_fn` reads the owner's own row
/// table -- and the flattened winner lives on the host, not on the module.
pub fn class_method_owner_reported(cid: ClassId, name: Symbol) -> Option<ClassId> {
    let owner = class_method_owner(cid, name)?;
    Some(crate::runtime_meta::singleton_prepend_owner(owner, name).unwrap_or(owner))
}

/// The module an `extend` supplies class method `name` from, or `None` when a
/// `def self.<name>` up the chain gets there first.
///
/// An extended module's row is an INSTANCE method seated in the singleton
/// class's ancestry, not a class method minted on the singleton -- which is
/// why reflection reports it differently at every turn: `#owner` names the
/// module bare where an own definition names a singleton class, and
/// `#inspect` qualifies the singleton home with it.
pub fn class_method_extend_source(cid: ClassId, name: Symbol) -> Option<ClassId> {
    match scan_class_method_owner(cid, 0, name) {
        Some((owner, true)) => Some(owner),
        _ => None,
    }
}

/// The row `owner` -- a module some class `extend`ed -- supplies for `name`.
/// Only a value-receiver row qualifies: a compiled `&RObj` body has no object
/// to bind a Class receiver to, and the module's own `extend`-time wrapper
/// (`runtime_meta::extended_class_method`) is the path that serves those.
///
/// The owner is resolved by the caller ([`class_method_extend_source`]) so
/// that one scan can serve both this and the overlay probe beside it.
pub(super) fn extended_class_method_body(owner: ClassId, name: Symbol) -> Option<ValueImpl> {
    crate::builtins::class_table(owner)
        .and_then(|t| t(name.name_str()))
        .map(ValueImpl::Rust)
        .or_else(|| value_method(owner, 0, name))
}

/// The module a class `extend`ed that supplies instance method `name`, with
/// the LAST `extend` winning -- the order [`runtime_meta::singleton_super_chain`]
/// seats them in, and the order their bodies were installed in.
fn extended_class_method_owner(cid: ClassId, name: Symbol) -> Option<ClassId> {
    crate::runtime_meta::extended_modules(&RubyValue::Class(cid))
        .into_iter()
        .rev()
        .find_map(|m| method_owner(m, name))
}

/// The compiled CLASS method `name` resolves to for `cid`, from whichever
/// ancestor defines it -- the `class_methods` table, which is a different one
/// from `value_method`'s. Used to COPY a class method (a runtime `alias` inside
/// `class << self`); dispatch itself walks the chain inline.
pub(crate) fn class_method_fn(cid: ClassId, name: Symbol) -> Option<ValueImpl> {
    let owner = class_method_owner(cid, name)?;
    registry()
        .entries
        .get(&owner.0)
        .and_then(|e| e.class_methods.get(&name).copied())
        .or_else(|| {
            crate::builtins::class_method_table(owner)
                .and_then(|l| l(name.name_str()))
                .map(ValueImpl::Rust)
        })
}

/// [`class_method_owner`]'s `#super_method` companion: the next definer
/// strictly after position `from` on `cid`'s SINGLETON chain.
///
/// Over the singleton walk, not the instance ancestry -- an `extend`ed module
/// is seated in the first and absent from the second, so
/// `OwnFirst.method(:tag).super_method` answered nil where ruby names the
/// module. The index it returns is a walk position, which is what makes a
/// module seated twice re-seat past the copy it is on rather than back onto
/// the first.
pub fn class_method_owner_after(
    cid: ClassId,
    from: usize,
    name: Symbol,
) -> Option<(ClassId, usize)> {
    crate::dispatch::singleton_owner_from(cid, from, name)
}

/// The singleton-chain position of `after`, for a `#super_method` walk with
/// no recorded seat -- [`chain_index_of`]'s class-method twin.
pub fn singleton_chain_index_of(recv_class: ClassId, after: ClassId) -> Option<usize> {
    crate::dispatch::singleton_position_of(recv_class, after)
}

/// The shared scan, answering `(owner, reached through an extend)`. Wider than
/// [`class_defines_own_class_method`], which deliberately ignores a builtin's
/// own `def self.x` rows so an `extend`ed module can override them --
/// reflection has no such stake and must report
/// `Array.method(:try_convert).owner` too.
///
/// Each ancestor is asked for its OWN class methods first and for the modules
/// it `extend`ed second, which is the order CRuby's singleton ancestry seats
/// them in: `#<Class:K>`, then K's extends, then `#<Class:Object>`.
pub(crate) fn scan_class_method_owner(
    cid: ClassId,
    skip: usize,
    name: Symbol,
) -> Option<(ClassId, bool)> {
    let n = name.name_str();
    // An `undef` written inside `class << self` retires the name here and for
    // every subclass, so the walk must stop rather than reach an ancestor's
    // still-live `def self.x`. Same rule the instance side applies for `undef`.
    if crate::runtime_meta::is_live() && crate::runtime_meta::class_method_undefined(cid, name) {
        return None;
    }
    ancestors_of_value(cid)
        .iter()
        .skip(skip)
        .copied()
        .find_map(|anc| {
            // `remove_method` in `class << self` empties THIS ancestor's OWN
            // position without ending the walk -- an ancestor's `def self.x`
            // is meant to answer now, which is what separates it from the
            // `undef` above. It empties only the own layer: a module this
            // ancestor `extend`ed sits at a position of its own, behind it,
            // and `remove_method` on the singleton never touched that one.
            let removed_here = crate::runtime_meta::is_live()
                && crate::runtime_meta::overlay_class_removed(anc, name);
            if removed_here {
                return extended_class_method_owner(anc, name).map(|m| (m, true));
            }
            // A RUNTIME `extend` copies the module's rows into the overlay, so
            // the overlay hit alone cannot tell the two apart -- the set the
            // copies were recorded in can.
            let copied = crate::runtime_meta::is_live()
                && crate::runtime_meta::overlay_class_method_is_extended(anc, name);
            let own = !copied
                && ((crate::runtime_meta::is_live()
                    && crate::runtime_meta::overlay_class_method(anc, name).is_some())
                    || REGISTRY
                        .get()
                        .and_then(|r| r.entries.get(&anc.0))
                        .is_some_and(|e| e.own_class_methods.contains(&name))
                    || crate::builtins::class_method_table(anc)
                        .is_some_and(|lookup| lookup(n).is_some()));
            match own {
                true => Some((anc, false)),
                false => extended_class_method_owner(anc, name).map(|m| (m, true)),
            }
        })
}

/// `Module#method_defined?` -- true when `name` resolves to a public OR
/// protected instance method of `recv_class` (private and nonexistent answer
/// false). Unlike `respond_to?`'s default, protected counts. `instance_method_
/// visibility` supplies the private/public/protected verdict for a registered
/// method; a builtin/Enumerable method (never private) has no registry entry,
/// so its `None` verdict is correctly treated as "not private".
pub fn method_defined(recv_class: ClassId, name: Symbol) -> bool {
    responds_to(recv_class, name, true)
        && instance_method_visibility(recv_class, name) != Some(MethodVisibility::Private)
}

/// `Module#method_defined?(name, inherit)` -- with `inherit` false the lookup is
/// restricted to methods `recv_class` defines DIRECTLY (its own `def`s and attr
/// accessors), skipping the ancestor walk. A private own method still answers
/// false, matching the inherit-true contract.
pub fn method_defined_inherit(recv_class: ClassId, name: Symbol, inherit: bool) -> bool {
    if crate::runtime_meta::not_yet_defined(recv_class, name) {
        return false;
    }
    if inherit {
        return method_defined(recv_class, name);
    }
    let Some(r) = REGISTRY.get() else {
        return false;
    };
    r.defines_own(recv_class, name)
        && r.own_method_visibility(recv_class, name) != Some(MethodVisibility::Private)
}

/// A row this build DEFINES but cannot perform -- CRuby's `rb_f_notimplement`.
/// Such an entry is listed like any other (`private_instance_methods` and
/// `singleton_methods` both carry it, and its arity answers), but `respond_to?`
/// reports FALSE, which is how a program is meant to detect the absence before
/// calling. CRuby tells them apart by the entry's implementation identity; the
/// bodies here are ordinary rows, so they are named instead.
///
/// `tests/notimplement_stub_respond_to.rb` calls every name below and asserts
/// each one raises `NotImplementedError`, so an entry cannot go stale by having
/// its body implemented out from under it.
fn is_notimplement_row(cid: ClassId, name: Symbol) -> bool {
    /// One owning module and the stub it declares. The id is a THUNK because a
    /// `ClassId` const is not usable in a const initializer here -- the same
    /// shape `exception.rs`'s `BY_OWNER` table uses.
    type Stub = (fn() -> ClassId, &'static str);
    const STUBS: &[Stub] = &[
        // `crates/zeo-rt/src/builtins/kernel.rs`
        (|| KERNEL_CLASS, "syscall"),
        // `crates/zeo-rt/src/builtins/process.rs`
        (|| zeo_abi::PROCESS_SYS_MODULE, "setresuid"),
        (|| zeo_abi::PROCESS_SYS_MODULE, "setresgid"),
    ];
    let n = name.name_str();
    STUBS
        .iter()
        .any(|(owner, stub)| *stub == n && ancestors_of_value(cid).contains(&owner()))
}

/// The visibility of a PER-OBJECT singleton method (`def obj.x`). Public as
/// defined -- the identity-keyed table carries no visibility -- unless the
/// singleton CLASS was told otherwise (`obj.singleton_class.send(:private,
/// :x)`), which marks that class id's own overlay.
pub(crate) fn value_singleton_visibility(sclass: ClassId, name: Symbol) -> MethodVisibility {
    crate::runtime_meta::overlay_method_visibility(sclass, name).unwrap_or(MethodVisibility::Public)
}

/// Does `recv_class`, or any ancestor, provide `name`?
///
/// Walks the receiver's real ancestor chain. Per ancestor it probes the
/// registry, the builtin method tables, and the Enumerable/Comparable name
/// sets. Materialized user methods live flat on the OWN class, the first
/// ancestor; builtin reopens hang off whichever ancestor was reopened.
///
/// `include_all` is the Ruby method's own second parameter. False, the
/// default, skips private methods, as in CRuby. Kernel's private functions
/// (`puts` and friends) stay invisible for the same reason.
///
/// This does not consult `method_missing`/`respond_to_missing?`, which this
/// runtime does not model.
pub fn responds_to(recv_class: ClassId, name: Symbol, include_all: bool) -> bool {
    // A definition hook on the stack has not seen the rest of its class yet --
    // see `runtime_meta::not_yet_defined`.
    if crate::runtime_meta::not_yet_defined(recv_class, name) {
        return false;
    }
    // A SINGLETON class's instance methods ARE its owner's class methods --
    // the same redirection `instance_method_visibility` makes, so
    // `Foo.singleton_class.instance_method(:a)` finds `def self.a`.
    if crate::runtime_meta::is_live() {
        match crate::runtime_meta::singleton_owner_value(recv_class) {
            // A name an `undef` inside `class << self` retired answers
            // nothing, however live the registry row behind it still is.
            Some(RubyValue::Class(owner))
                if crate::runtime_meta::class_method_undefined(owner, name) =>
            {
                return false;
            }
            Some(RubyValue::Class(owner)) => {
                if class_receiver_responds(owner, name) {
                    return include_all || !class_method_is_private(owner, name);
                }
            }
            // A PER-OBJECT singleton (`def obj.x`) is identity-keyed, so the
            // class-id walk below cannot see it -- the singleton class has no
            // rows of its own.
            Some(owner) if crate::runtime_meta::object_has_singleton_method(&owner, name) => {
                return include_all
                    || value_singleton_visibility(recv_class, name) == MethodVisibility::Public;
            }
            _ => {}
        }
    }
    let n = name.name_str();
    let overlay_live = crate::runtime_meta::is_live();
    for &anc in ancestors_of_value(recv_class) {
        // A method defined at runtime (`define_method`, a runtime class's
        // own method) answers `respond_to?` on every ancestor it lands on.
        // An explicit runtime visibility mark (`class_eval { private :m }`)
        // is checked first: it can target a frozen-registry or builtin
        // method the overlay carries no body for, and only ever exists for
        // a name that resolved when the mark was made -- so it is
        // authoritative for both existence and visibility.
        if overlay_live {
            if crate::runtime_meta::overlay_is_undefined(anc, name) {
                return false;
            }
            // Removed here, so this position answers nothing -- but an
            // ancestor still may, which is what separates it from an undef.
            if crate::runtime_meta::overlay_is_removed(anc, name) {
                continue;
            }
            if let Some(v) = crate::runtime_meta::overlay_method_visibility(anc, name) {
                return include_all || v == MethodVisibility::Public;
            }
            if crate::runtime_meta::overlay_has_instance_method(anc, name) {
                return true;
            }
        }
        if let Some(r) = REGISTRY.get() {
            // `undef` TERMINATES the walk at the class that wrote it --
            // an ancestor's still-live definition must not answer for a
            // descendant that undef'd the name.
            if r.is_undefined(anc, name) {
                return false;
            }
            if r.lookup(anc, name).is_some() || r.lookup_value_method(anc, 0, name).is_some() {
                // ...unless the row was FLATTENED in from an ancestor a runtime
                // `undef_method` has since retired. Same question `lookup_mro`
                // asks before trusting its own flattened hit.
                if overlay_live && r.retired_before_owner(anc, name) {
                    return false;
                }
                // This is the NEAREST ancestor defining `name` (materialization
                // flattens the resolved method, with its effective visibility,
                // onto the receiver's own class), so its visibility is
                // authoritative -- a private/protected redefinition here shadows
                // a public copy farther up the chain (CRuby's nearest-wins rule).
                if !include_all && (r.is_private(anc, name) || r.is_protected(anc, name)) {
                    return false;
                }
                return true;
            }
        }
        if let Some(table) = crate::builtins::class_table(anc)
            && table(n).is_some()
        {
            // A builtin private (Kernel's print family, BasicObject's
            // `initialize`, every `module_function`'s instance copy) is
            // reachable via implicit self / `send` / `super` -- all
            // through the table -- but `respond_to?`'s default ignores
            // privates, so it answers false unless `include_all`. A
            // PROTECTED row (`Pathname#path`) is hidden the same way:
            // CRuby's default `respond_to?` reports public only.
            if !include_all
                && (is_hidden_builtin_private(anc, n)
                    || crate::builtins::class_method_is_private(anc, n)
                    || crate::builtins::class_method_is_protected(anc, n))
            {
                continue;
            }
            return true;
        }
    }
    // A builtin-alias row answers through its SOURCE name -- which also
    // carries the source's visibility (a Ruby alias copies it): `dup!` for
    // `dup` answers true, `raise!` for the hidden-private `raise` answers
    // false unless `include_all`. See `alias_target`.
    if let Some(old) = alias_target(recv_class, name) {
        return responds_to(recv_class, old, include_all);
    }
    false
}

/// The class or module that actually defines `name` for an instance of
/// `recv_class` -- the first ancestor in the MRO carrying a definition.
/// Backs `Method#owner`/`UnboundMethod#owner`. Reflection sees privates, so
/// this is the `include_all` walk. `None` when nothing in the chain defines
/// it (callers construct the method only after a `responds_to` check, so this
/// is the belt-and-suspenders arm).
pub fn method_owner(recv_class: ClassId, name: Symbol) -> Option<ClassId> {
    scan_owner(recv_class, 0, name)
}

/// `Module#undefined_instance_methods` -- the names THIS class's own body
/// `undef`'d, which the MRO walk treats as a lookup terminator. Sorted, so the
/// listing is stable across runs (the set is a hash set).
pub fn undefined_method_names(class: ClassId) -> Vec<Symbol> {
    let Some(entry) = REGISTRY.get().and_then(|r| r.entries.get(&class.0)) else {
        return Vec::new();
    };
    let mut names: Vec<Symbol> = entry.undefined_methods.iter().copied().collect();
    names.sort_by_key(|s| s.name_str());
    names
}

/// The next class up that defines `name` -- the one strictly AFTER position
/// `from` in `recv_class`'s MRO, WITH its own position. Backs
/// `Method#super_method`: `None` once the chain runs out.
///
/// The position rides back out because a module the chain holds twice would
/// otherwise re-seat onto its first copy forever -- see
/// `builtins::method::Seat`.
pub fn method_owner_after(
    recv_class: ClassId,
    from: usize,
    name: Symbol,
) -> Option<(ClassId, usize)> {
    let owner = scan_owner(recv_class, from, name)?;
    let at = ancestors_of_value(recv_class)
        .iter()
        .enumerate()
        .skip(from)
        .find(|&(_, &a)| a == owner)
        .map(|(i, _)| i)?;
    Some((owner, at))
}

/// The chain index of `after` in `recv_class`'s MRO -- where a
/// `#super_method` walk that has no recorded position starts.
pub fn chain_index_of(recv_class: ClassId, after: ClassId) -> Option<usize> {
    ancestors_of_value(recv_class)
        .iter()
        .position(|&a| a == after)
}

/// The shared MRO scan: the first ancestor from `skip` positions in that
/// carries a definition of `name` itself.
pub(super) fn scan_owner_from(recv_class: ClassId, skip: usize, name: Symbol) -> Option<ClassId> {
    scan_owner(recv_class, skip, name)
}

fn scan_owner(recv_class: ClassId, skip: usize, name: Symbol) -> Option<ClassId> {
    let n = name.name_str();
    let overlay_live = crate::runtime_meta::is_live();
    for &anc in ancestors_of_value(recv_class).iter().skip(skip) {
        if overlay_live {
            if crate::runtime_meta::overlay_is_undefined(anc, name) {
                return None;
            }
            if crate::runtime_meta::overlay_is_removed(anc, name) {
                continue;
            }
            if crate::runtime_meta::overlay_has_instance_method(anc, name) {
                return Some(anc);
            }
        }
        if let Some(r) = REGISTRY.get() {
            if r.is_undefined(anc, name) {
                return None;
            }
            // `defines_own`, not `lookup`: materialization flattens an inherited
            // method onto every descendant's table, so only the direct-def set
            // pins down where it actually originated.
            if r.defines_own(anc, name) {
                return Some(anc);
            }
        }
        // A row marked `inherits` answers the CALL here but names an ancestor
        // as its owner, so the scan walks past it -- the ancestor that really
        // declares the method is further along this same chain.
        if let Some(table) = crate::builtins::class_table(anc)
            && table(n).is_some()
            && !crate::builtins::builtin_row_inherits(anc, n, false)
        {
            return Some(anc);
        }
    }
    None
}

/// The ancestor chain the MRO walk runs over: the registry's (richer --
/// user classes, and reopens may have `include`d user modules into a
/// builtin) when installed and populated for `id`, else the ABI-derived
/// fallback chain (this crate's own unit tests; identical for builtins by
/// construction -- both derive from `zeo_abi::BUILTINS`).
/// `id`'s chain as the COMPILE-TIME tables have it -- what the frozen flat
/// lookup can reach, ignoring anything a run-time `include`/`prepend` spliced
/// in. Empty when nothing registered `id`.
pub(crate) fn frozen_ancestors(id: ClassId) -> &'static [ClassId] {
    REGISTRY.get().map_or(&[], |r| r.ancestors_of(id))
}

pub(crate) fn ancestors_of_value(id: ClassId) -> &'static [ClassId] {
    // The OVERLAY wins when it has a chain: a class born at runtime
    // (`Class.new`) has no frozen entry at all, and a runtime `include`/
    // `prepend` into a COMPILE-TIME class splices a new chain there while
    // the frozen one keeps the original -- reading frozen first would hide
    // the mix-in from `ancestors`, `is_a?` and constant lookup. Gated on
    // `is_live`, so a program that never mutates a hierarchy pays one
    // relaxed load.
    if crate::runtime_meta::is_live()
        && let Some(chain) = crate::runtime_meta::overlay_ancestors(id)
    {
        return chain;
    }
    // `REGISTRY` is a `static OnceLock`, so `get()` hands out `&'static`
    // borrows directly -- no lifetime gymnastics needed.
    if let Some(r) = REGISTRY.get() {
        let chain = r.ancestors_of(id);
        if !chain.is_empty() {
            return chain;
        }
    }
    crate::builtins::fallback_ancestors(id)
}

/// The resolved visibility of a single method (`private`/`protected`/public).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MethodVisibility {
    Public,
    Private,
    Protected,
}

/// A reflection filter over method visibility -- what each `*_instance_methods`
/// / `*_method_defined?` query keeps. `instance_methods` and `Object#methods`
/// keep public+protected (`NotPrivate`); the prefixed forms match exactly one
/// visibility.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VisFilter {
    NotPrivate,
    Public,
    Protected,
    Private,
    /// Every visibility -- what `extend` copies, since ruby carries a module's
    /// private instance methods onto the singleton as private ones.
    All,
}

impl VisFilter {
    fn matches(self, v: MethodVisibility) -> bool {
        match self {
            VisFilter::All => true,
            VisFilter::NotPrivate => v != MethodVisibility::Private,
            VisFilter::Public => v == MethodVisibility::Public,
            VisFilter::Protected => v == MethodVisibility::Protected,
            VisFilter::Private => v == MethodVisibility::Private,
        }
    }
}

/// Builtin-table methods that CRuby defines as PRIVATE, so they are reachable
/// via implicit self / `send` / `super` (all of which go through the table)
/// but are invisible to `respond_to?` and raise `NoMethodError: private
/// method` on an explicit-receiver call.
///
/// A row that knows it is private says so (`private def`). What is left here
/// is the set no single row can declare, because MANY tables define the name
/// and every one of them is private. `initialize` (`BasicObject`'s,
/// `object.c`'s `rb_obj_dummy`) is the load-bearing case: `super` from any
/// `initialize` must reach it, but `obj.initialize` must raise. Kernel's
/// print family is the same shape.
fn is_hidden_builtin_private(owner: ClassId, name: &str) -> bool {
    // Private on EVERY class, wherever a table defines one -- except
    // `Ractor::MovedObject`, whose whole raising surface (`method_missing`
    // included) is PUBLIC in CRuby's `instance_methods(false)`.
    if matches!(
        name,
        "initialize" | "method_missing" | "respond_to_missing?"
    ) {
        return !(owner == zeo_abi::RACTOR_MOVED_OBJECT_CLASS && name == "method_missing");
    }
    // Private on `Kernel` ALONE. `IO#puts`, `StringIO#print` and
    // `Thread#raise` are ordinary public methods of their own classes, so
    // hiding these by name regardless of owner dropped them from every
    // listing -- which is what made `IO#puts` read as missing.
    if owner == zeo_abi::KERNEL_CLASS {
        return matches!(
            name,
            "puts" | "print" | "p" | "pp" | "warn" | "system" | "spawn" | "`" | "raise" | "fail"
        );
    }
    false
}

/// The instance-method names of `class` and -- when `inherit` -- its
/// ancestors, as interned Symbols in MRO order, deduped (a nearer class's
/// definition shadows a farther one). Combines the registry (user methods +
/// builtin reopens) with the builtin method tables and the
/// Enumerable/Comparable/Math name sets. `vis` selects public(+protected) or
/// private-only.
///
/// The builtin tables are a SUBSET of CRuby's (this runtime implements a
/// subset of each class's methods), so a builtin class's list won't equal
/// CRuby's exactly -- reflection callers assert membership, not equality. A
/// user class's own list (`inherit=false`) is exact.
pub fn instance_method_names(class: ClassId, filter: VisFilter, inherit: bool) -> Vec<Symbol> {
    // A singleton class's instance methods are its owner's CLASS methods --
    // see `instance_method_visibility`, which resolves their visibility the
    // same way.
    if crate::runtime_meta::is_live() {
        match crate::runtime_meta::singleton_owner_value(class) {
            Some(RubyValue::Class(owner)) => {
                // `inherit` rides along: a singleton class's OWN instance
                // methods are the owner's own `def self.x` rows, and neither an
                // ancestor's nor a module the owner `extend`ed is one of them.
                return class_method_names_in(owner, inherit)
                    .into_iter()
                    .filter(|&n| {
                        filter.matches(match class_method_is_private(owner, n) {
                            true => MethodVisibility::Private,
                            false => MethodVisibility::Public,
                        })
                    })
                    .collect();
            }
            // A PER-OBJECT singleton's methods live identity-keyed, which is
            // the table `Object#singleton_methods` reads -- so without this the
            // two views of the same method disagreed.
            Some(owner) => {
                return crate::runtime_meta::singleton_method_names(&owner)
                    .into_iter()
                    .filter(|&n| filter.matches(value_singleton_visibility(class, n)))
                    .collect();
            }
            None => {}
        }
    }
    let chain: Vec<ClassId> = if inherit {
        ancestors_of_value(class).to_vec()
    } else {
        vec![class]
    };
    let reg = REGISTRY.get();
    let mut seen = HashSet::new();
    // What a definition hook on the stack has not seen defined yet. Claimed
    // like an `undef` tombstone -- marked SEEN so no layer can put the name
    // back. `pending_defs_for` has already ruled out the names an ANCESTOR
    // defines, which do exist.
    for n in crate::runtime_meta::pending_defs_for(class, inherit) {
        seen.insert(n);
    }
    let mut out = Vec::new();
    let live = crate::runtime_meta::is_live();
    for anc in chain {
        // A `remove_method` here empties only THIS position, so the name is
        // dropped from what `anc` contributes and the walk carries on -- an
        // ancestor that still defines it says so later in this same loop.
        // (An `undef` is the other rule and claims the name outright, which
        // is why it rides in the overlay list below instead.)
        let removed = |n: Symbol| live && crate::runtime_meta::overlay_is_removed(anc, n);
        // The overlay first, and it CLAIMS every name it has an opinion about
        // (`seen.insert` before the filter): a runtime `private :m` has to beat
        // the registry's compile-time public flag below, and an `undef_method`
        // tombstone has to keep an ancestor's definition off the list entirely.
        if crate::runtime_meta::is_live() {
            for (name, vis) in crate::runtime_meta::overlay_instance_method_names(anc) {
                if seen.insert(name) && vis.is_some_and(|v| filter.matches(v)) {
                    out.push(name);
                }
            }
        }
        if let Some(r) = reg {
            // `inherit=false` restricts the user-method set to this class's own
            // definitions (materialization otherwise flattens inherited in).
            for (name, vis) in r.own_instance_method_names(anc, !inherit) {
                if !removed(name) && filter.matches(vis) && seen.insert(name) {
                    out.push(name);
                }
            }
        }
        // A builtin table row carries its own visibility now (`module_function`
        // defines the instance copy private, CRuby's rule), so the filter reads
        // it per name rather than assuming public.
        for &n in crate::builtins::class_table_names(anc) {
            // A row ruby owns further up (`Module#instance_variable_get` is
            // really Kernel's) sits in this table only so dispatch reaches it.
            // Skip it for the OWN set; the wide set still gets the name from
            // the ancestor that really declares it, later in this same chain.
            if !inherit && crate::builtins::builtin_row_inherits(anc, n, false) {
                continue;
            }
            // A builtin private (Kernel's print family, BasicObject's
            // `initialize`) is CLASSIFIED private, not skipped: CRuby keeps it
            // out of `instance_methods` and inside `private_instance_methods`,
            // and dropping it entirely lost the second half.
            let vis = if crate::builtins::class_method_is_private(anc, n)
                || is_hidden_builtin_private(anc, n)
            {
                MethodVisibility::Private
            } else if crate::builtins::class_method_is_protected(anc, n) {
                MethodVisibility::Protected
            } else {
                MethodVisibility::Public
            };
            if !filter.matches(vis) {
                continue;
            }
            let sym = Symbol::intern(n);
            if !removed(sym) && seen.insert(sym) {
                out.push(sym);
            }
        }
        // A builtin alias is stored as a NAME INDIRECTION rather than a copied
        // entry (see `ClassInfo::builtin_aliases`), so nothing above lists it --
        // but `alias_method :gems, :specs` defines `gems` as far as Ruby is
        // concerned, and reflection has to say so. Aliases are public.
        if filter.matches(MethodVisibility::Public)
            && let Some(r) = reg
        {
            for new in r.alias_names(anc) {
                if seen.insert(new) {
                    out.push(new);
                }
            }
        }
    }
    out
}

/// The resolved visibility of instance method `name` on `class` (walking the
/// MRO, nearest definition winning), or `None` if `class` has no such instance
/// method. Backs `private_method_defined?`/`public_method_defined?`/
/// `protected_method_defined?`.
pub fn instance_method_visibility(class: ClassId, name: Symbol) -> Option<MethodVisibility> {
    // A definition hook on the stack has not seen the rest of its class yet --
    // this is what backs `public_method_defined?` and friends.
    if crate::runtime_meta::not_yet_defined(class, name) {
        return None;
    }
    // `Foo.singleton_class`'s instance methods ARE `Foo`'s class methods, so
    // its visibility is theirs -- the table `def self.x` and
    // `private_class_method` actually write to.
    if crate::runtime_meta::is_live() {
        match crate::runtime_meta::singleton_owner_value(class) {
            // Only when the owner really HAS a class method by this name. A
            // singleton class also inherits `Class`/`Module`'s own instance
            // methods, and those carry their own visibility -- answering `None`
            // here read as "not private" and made
            // `C.singleton_class.method_defined?(:private)` true, where CRuby
            // says false because `Module#private` is private.
            // ...or when a `private_class_method` MARK names it. The mark can
            // name a method the class only INHERITS (`private_class_method
            // :new` retires `Class#new` for this one class), which
            // `class_receiver_responds` alone answers no for.
            // ...and a RETIRED one answers nothing at all.
            Some(RubyValue::Class(owner))
                if crate::runtime_meta::class_method_undefined(owner, name) =>
            {
                return None;
            }
            Some(RubyValue::Class(owner))
                if class_receiver_responds(owner, name)
                    || crate::runtime_meta::overlay_class_method_private(owner, name).is_some() =>
            {
                return Some(match class_method_is_private(owner, name) {
                    true => MethodVisibility::Private,
                    false => MethodVisibility::Public,
                });
            }
            // A per-object singleton method. Same "only when it really has one"
            // rule, for the same reason.
            Some(owner)
                if !matches!(owner, RubyValue::Class(_))
                    && crate::runtime_meta::object_has_singleton_method(&owner, name) =>
            {
                return Some(value_singleton_visibility(class, name));
            }
            _ => {}
        }
    }
    let reg = REGISTRY.get()?;
    // `name_str`, not `name()`: this walk is on the explicit-receiver barrier's
    // path, so a `String` per call would be a heap allocation on every dynamic
    // call a site's inline cache does not serve. `name_str` itself is two
    // indexes into the published symbol slab -- no lock (see `symbol.rs`).
    let name_str = name.name_str();
    for anc in ancestors_of_value(class) {
        // Runtime marks first: an explicit `class_eval { private :m }` (or an
        // alias's inherited visibility) on the nearest ancestor beats the
        // frozen registry's compile-time flags, and a runtime-defined method
        // with no mark is public.
        if crate::runtime_meta::is_live() {
            if crate::runtime_meta::overlay_is_undefined(*anc, name) {
                return None;
            }
            if crate::runtime_meta::overlay_is_removed(*anc, name) {
                continue;
            }
            if let Some(vis) = crate::runtime_meta::overlay_method_visibility(*anc, name) {
                return Some(vis);
            }
            if crate::runtime_meta::overlay_has_instance_method(*anc, name) {
                return Some(MethodVisibility::Public);
            }
        }
        // A user/reopen definition on this ancestor carries its own visibility;
        // `undef` here terminates the search with "no such method".
        if reg.is_undefined(*anc, name) {
            return None;
        }
        if let Some(vis) = reg.own_method_visibility(*anc, name) {
            return Some(vis);
        }
        // A builtin-table method carries its own visibility now; the hidden
        // list still covers the tables that are not macro-generated.
        // Answering `Private` rather than falling through matters: the name
        // IS defined, so `private_method_defined?` must say so, and the walk
        // must not keep looking for a public copy farther up.
        // The generated `lookup`, not a linear scan of `names`: this walk is
        // the explicit-receiver barrier's, and `String`/`Array` carry ~200
        // rows each, so `contains` was up to a few hundred string compares per
        // ancestor on every call the inline cache did not serve. `lookup` is a
        // match rustc lowers to a length switch, and the macro builds both
        // from the same def list, so they answer the same question.
        if crate::builtins::class_table(*anc).is_some_and(|f| f(name_str).is_some()) {
            if is_hidden_builtin_private(*anc, name_str)
                || crate::builtins::class_method_is_private(*anc, name_str)
            {
                return Some(MethodVisibility::Private);
            }
            if crate::builtins::class_method_is_protected(*anc, name_str) {
                return Some(MethodVisibility::Protected);
            }
            return Some(MethodVisibility::Public);
        }
    }
    None
}

/// Whether class method `name` on `class` is PRIVATE -- what
/// `private_class_method` marked, found on the nearest ancestor that says
/// anything about the name (so `public_class_method` in a subclass promotes it
/// back). `false` for a name no ancestor marks, which is every ordinary
/// `def self.x`.
pub fn class_method_is_private(class: ClassId, name: Symbol) -> bool {
    for anc in ancestors_of_value(class) {
        if crate::runtime_meta::is_live()
            && let Some(private) = crate::runtime_meta::overlay_class_method_private(*anc, name)
        {
            return private;
        }
        if REGISTRY
            .get()
            .and_then(|r| r.entries.get(&anc.0))
            .is_some_and(|e| e.private_class_methods.contains(&name))
        {
            return true;
        }
        // A definition here with no mark is public, and stops the walk: an
        // ancestor's `private_class_method` must not reach past an override.
        if class_defines_own_class_method(*anc, name) {
            return false;
        }
        // A builtin class-method row carries its own `private def self.` mark
        // (prism's `serialize_parse` and friends), which no registry set knows
        // about. Answering here also STOPS the walk, for the same reason an
        // own definition does.
        let n = name.name_str();
        if let Some(lookup) = crate::builtins::class_method_table(*anc)
            && lookup(n).is_some()
        {
            return crate::builtins::builtin_class_method_is_private(*anc, n);
        }
    }
    false
}

/// The explicit-receiver visibility guard for a class method a STATIC site
/// resolved -- `Foo.new` where `private_class_method :new` may arrive at run
/// time (singleton.rb's `included` hook writes it on its includer). The
/// compile-time half is `emit_private_new_error`; this is the same refusal for
/// a mark codegen could not see.
pub fn guard_public_class_method(cid: ClassId, name: Symbol) -> Result<(), Signal> {
    if !crate::runtime_meta::is_live() || !class_method_is_private(cid, name) {
        return Ok(());
    }
    Err(raise_error(
        "NoMethodError",
        format!(
            "private method '{}' called for class {}",
            name.name(),
            class_name(cid).unwrap_or_else(|| "?".to_string())
        ),
    ))
}

/// The CLASS-method (`def self.x` + builtin class-method) names of `class`,
/// deduped -- backs `SomeClass.singleton_methods` and the class-method half
/// of `SomeClass.methods`.
///
/// `inherit` is the flag `singleton_methods(false)` passes. Narrowing has to
/// reach all three sources: the overlay stops
/// walking ancestors, the registry answers from `own_class_methods` instead
/// of the flattened map materialization filled, and a builtin table is a
/// class's own by construction.
pub fn class_method_names_in(class: ClassId, inherit: bool) -> Vec<Symbol> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    // A name an `undef` inside `class << self` retired is claimed before any
    // table can offer it -- the registry still carries the row it took back.
    let live = crate::runtime_meta::is_live();
    if live {
        for &anc in ancestors_of_value(class) {
            for name in crate::runtime_meta::overlay_class_undefs(anc) {
                seen.insert(name);
            }
        }
    }
    // A class minted at runtime keeps its `def self.x`/`define_singleton_method`/
    // `module_function` methods in the overlay, never in the frozen registry.
    if crate::runtime_meta::is_live() {
        // Ancestors included, so the listing agrees with what dispatch actually
        // answers -- see `class_receiver_responds` for why only the overlay
        // half needs the walk.
        let chain: &[ClassId] = match inherit {
            true => ancestors_of_value(class),
            false => std::slice::from_ref(&class),
        };
        for &anc in chain {
            for name in crate::runtime_meta::overlay_class_method_names(anc, !inherit) {
                if seen.insert(name) {
                    out.push(name);
                }
            }
        }
    }
    if let Some(r) = REGISTRY.get() {
        for name in r.own_class_method_names(class, !inherit) {
            if seen.insert(name) {
                out.push(name);
            }
        }
    }
    for &n in crate::builtins::class_method_table_names(class) {
        // `Hash.new` and friends: the row lives on the class so a `Hash(...)`
        // receiver reaches an allocator, but ruby owns `new` on `Class`, which
        // the singleton chain reaches anyway. Keep it out of the OWN set.
        if !inherit && crate::builtins::builtin_row_inherits(class, n, true) {
            continue;
        }
        let sym = Symbol::intern(n);
        if seen.insert(sym) {
            out.push(sym);
        }
    }
    // A MINTED struct/data class answers `members`/`keyword_init?` through
    // `rstruct::class_lookup`, a table of its own that no ClassId indexes -- so
    // the loop above cannot see it, and `S.methods` omitted what `S.members`
    // plainly answers.
    if crate::builtins::rstruct::is_struct_class(class) {
        for &n in crate::builtins::rstruct::class_method_names() {
            let sym = Symbol::intern(n);
            if seen.insert(sym) {
                out.push(sym);
            }
        }
    }
    out
}

/// [`class_method_names`] without the ones `private_class_method` marked --
/// what `singleton_methods` and the class-method half of `methods` report,
/// both of which list public names only.
pub fn public_class_method_names(class: ClassId, inherit: bool) -> Vec<Symbol> {
    class_method_names_in(class, inherit)
        .into_iter()
        .filter(|&n| !class_method_is_private(class, n))
        .collect()
}
