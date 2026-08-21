//! `Method`: the object `Kernel#method(:name)` answers -- a bound
//! (receiver, name) pair whose `#call` dispatches through the ordinary
//! `send` machinery. Its sibling `UnboundMethod` (an unbound `(class, name)`
//! pair) lives in `unbound_method.rs`; the two reference each other's payload
//! (`#unbind` builds an `UnboundMethod`, `UnboundMethod#bind` builds a
//! `Method`) plus this file's shared `resolve_method_name`/`method_source`.

use std::sync::Arc;

use crate::builtins::{inherited_row, name_error, type_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::method_meta::MethodKind;
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use zeo_abi::{ClassId, METHOD_CLASS};
use zeo_macros::ruby_class;

use crate::builtins::unbound_method::RUnboundMethod;

pub struct RMethod {
    pub recv: RubyValue,
    pub name: Symbol,
    /// Where the MRO lookup STARTS -- CRuby's `mklass`. Ordinarily the
    /// receiver's own class (paired with [`MethodKind::Instance`]), or the
    /// class itself when the receiver IS a class holding a `def self.x`
    /// (paired with [`MethodKind::Singleton`] -- zeo has no singleton-class
    /// objects, so the pair plays that role).
    ///
    /// `#super_method` re-seats it further down the chain. That single fact
    /// is what makes the super chain walkable, tells `#call` where to resume
    /// so a re-seated Method runs the ancestor's body rather than
    /// re-dispatching to the override, and tells `#inspect` when to stop
    /// qualifying the owner (`Sub(Base)#greet` vs. a plain `Base#greet`).
    pub home: ClassId,
    pub kind: MethodKind,
    /// The method entry as it resolved WHEN THIS OBJECT WAS MADE.
    ///
    /// CRuby copies the entry (`rb_method_entry_clone`), so a later
    /// redefinition cannot change what an already-taken Method calls. That is
    /// what makes the wrap-and-redefine idiom terminate -- the canonical thing
    /// a `method_added` hook is written to do:
    ///
    /// ```ruby
    /// orig = instance_method(name)
    /// define_method(name) { |*a| orig.bind(self).call(*a) }
    /// ```
    ///
    /// Re-dispatching by name instead finds the wrapper and recurses forever.
    ///
    /// `None` for the shapes a freeze cannot serve: a class-method lookup
    /// (whose body is an `RProc` keyed on a `Class` receiver, not an `RObj`),
    /// a non-object receiver, and a `#super_method` re-seat -- all of which
    /// keep the ordinary resolve-and-send.
    pub(crate) snapshot: Option<FrozenEntry>,
    /// The RESOLVED chain position, once a `#super_method` re-seat picked
    /// one. `#owner` answers it verbatim (re-scanning from `home` under
    /// `prepend` finds the prepended module again and the walk never
    /// advances), and `#call` runs exactly this ancestor's body.
    pub(crate) seat: Option<ClassId>,
}

/// Which LAYER answered when a `Method`/`UnboundMethod` was taken.
///
/// Keeping the resolved `MethodImpl` itself would be wrong: zeo materializes a
/// layout-correct copy of a compiled method PER CLASS, so `Foo#b`'s body
/// cannot run against a `Sub` instance -- it downcasts the receiver to Foo's
/// generated struct. Freezing the layer, and re-resolving from that layer for
/// the actual receiver, is both redefinition-proof and layout-correct.
#[derive(Clone)]
pub(crate) enum FrozenEntry {
    /// A runtime `define_method` body. Its `RProc` takes the receiver as an
    /// argument, so this one copy binds to any instance and is kept verbatim.
    Overlay(crate::dispatch::MethodImpl),
    /// A compiled body, or a builtin row. Re-resolved per receiver with the
    /// overlay skipped, so a later `define_method` cannot capture it.
    BelowOverlay,
}

impl FrozenEntry {
    /// Runs the frozen entry against `recv`, or `None` when the name no longer
    /// resolves below the overlay at all (an `undef` since it was taken).
    pub(crate) fn call_on(
        &self,
        recv: &RObj,
        name: Symbol,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Option<Result<RubyValue, Signal>> {
        match self {
            FrozenEntry::Overlay(m) => Some(m.call(recv, args, block)),
            FrozenEntry::BelowOverlay => {
                crate::runtime_meta::snapshot_below_overlay(recv.class_id(), name)
                    .map(|m| m.call(recv, args, block))
            }
        }
    }
}

impl RMethod {
    /// The MRO a lookup on this Method walks. It is the RECEIVER's, not
    /// `home`'s: after `#super_method` re-seats `home` onto an included
    /// module, that module's own chain is just itself and would lose the
    /// rest of the ancestry.
    pub(crate) fn chain(&self) -> ClassId {
        match (self.kind, &self.recv) {
            (MethodKind::Singleton, RubyValue::Class(cid)) => *cid,
            _ => self.recv.class_id(),
        }
    }

    /// The class or module that actually defines this method. `home` starts
    /// at the lookup root and, once `#super_method` has re-seated it, IS the
    /// definer -- so resolving from it answers both cases.
    pub(crate) fn owner(&self) -> Option<ClassId> {
        if let Some(seat) = self.seat {
            return Some(seat);
        }
        // A PER-OBJECT singleton method's owner IS its recorded home (the
        // extending module, or the object's own singleton class -- see
        // `home_of`): the generic scan below walks real class tables, which
        // cannot see the identity-keyed row and would skip past the
        // singleton class to whatever ancestor also defines the name.
        if self.kind == MethodKind::Instance
            && !matches!(self.recv, RubyValue::Class(_))
            && crate::runtime_meta::object_has_singleton_method(&self.recv, self.name)
        {
            return Some(self.home);
        }
        match self.kind {
            MethodKind::Instance => crate::dispatch::method_owner(self.home, self.name),
            MethodKind::Singleton => {
                crate::dispatch::class_method_owner_reported(self.home, self.name)
            }
        }
    }
}

/// The `(home, kind)` a freshly built `Method` starts at: a class or module
/// receiver that answers `name` from its CLASS-method chain is a singleton
/// lookup rooted at that class; everything else -- including `Api.method(
/// :name)`, which finds `Module#name` -- is an instance lookup rooted at the
/// receiver's class.
pub(crate) fn home_of(recv: &RubyValue, name: Symbol) -> (ClassId, MethodKind) {
    match recv {
        RubyValue::Class(cid) if crate::dispatch::class_method_owner(*cid, name).is_some() => {
            (*cid, MethodKind::Singleton)
        }
        _ => {
            // A PER-OBJECT singleton method roots at its real definer -- the
            // module a per-object `extend` copied it from, or the object's
            // own singleton class -- so `#owner`/`#unbind` report it and
            // `#call` seats there (`send_as_defined_in` reaches a module's
            // bridge, and degrades to the identity-keyed send for an own
            // def). Everything else keeps the receiver-class root.
            if crate::runtime_meta::object_has_singleton_method(recv, name)
                && let Some(home) = crate::runtime_meta::per_object_method_home(recv, name)
            {
                return (home, MethodKind::Instance);
            }
            (recv.class_id(), MethodKind::Instance)
        }
    }
}

/// A bound `Method` value over an already-resolved lookup position, taking the
/// entry snapshot [`RMethod::snapshot`] documents.
pub(crate) fn method_value(
    recv: RubyValue,
    name: Symbol,
    home: ClassId,
    kind: MethodKind,
) -> RubyValue {
    let snapshot = entry_snapshot(&recv, name, home, kind);
    method_value_with(recv, name, home, kind, None, snapshot)
}

/// [`method_value`] over an entry frozen ELSEWHERE -- `UnboundMethod#bind`,
/// which must hand on the copy the unbound method already froze rather than
/// re-resolving a name that may have been redefined since.
pub(crate) fn method_value_with(
    recv: RubyValue,
    name: Symbol,
    home: ClassId,
    kind: MethodKind,
    seat: Option<ClassId>,
    snapshot: Option<FrozenEntry>,
) -> RubyValue {
    RubyValue::Object(Arc::new(RMethod {
        recv,
        name,
        home,
        kind,
        snapshot,
        seat,
    }))
}

/// The entry to freeze into a new `Method`/`UnboundMethod` -- see
/// [`RMethod::snapshot`] for why, and for the shapes that get `None`.
pub(crate) fn entry_snapshot(
    recv: &RubyValue,
    name: Symbol,
    home: ClassId,
    kind: MethodKind,
) -> Option<FrozenEntry> {
    if kind != MethodKind::Instance || !matches!(recv, RubyValue::Object(_)) {
        return None;
    }
    // A `#super_method` re-seat has already picked a position the ordinary
    // walk would not reach; leave it on `send_as_defined_in`.
    (home == recv.class_id()).then(|| freeze_entry(recv.class_id(), name))
}

/// The layer that answers `name` for instances of `id`, right now.
pub(crate) fn freeze_entry(id: ClassId, name: Symbol) -> FrozenEntry {
    match crate::runtime_meta::resolves_through_overlay(id, name) {
        true => match crate::runtime_meta::snapshot_instance_method(id, name) {
            Some(m) => FrozenEntry::Overlay(m),
            None => FrozenEntry::BelowOverlay,
        },
        false => FrozenEntry::BelowOverlay,
    }
}

/// A bound `Method` over a fresh lookup (`obj.method(:name)`).
fn method_at_home(recv: &RubyValue, name: Symbol) -> RubyValue {
    let (home, kind) = home_of(recv, name);
    method_value(recv.clone(), name, home, kind)
}

impl RubyObject for RMethod {
    fn class_id(&self) -> ClassId {
        METHOD_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RMethod {
            recv: self.recv.clone(),
            name: self.name,
            home: self.home,
            kind: self.kind,
            snapshot: self.snapshot.clone(),
            seat: self.seat,
        })
    }
}

/// Resolve a Symbol/String method-name argument to a `Symbol`, or the
/// `TypeError` CRuby raises for anything else. Shared by
/// `method`/`public_method`/`instance_method`.
pub(crate) fn resolve_method_name(name_arg: &RubyValue) -> Result<Symbol, Signal> {
    match name_arg {
        RubyValue::Symbol(s) => Ok(*s),
        RubyValue::Str(s) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            other.inspect_string()
        )),
    }
}

/// Constructs the `Method` value for `recv.method(name_arg)` -- shared by
/// the Kernel table row and any codegen fast path. An unknown method is a
/// `NameError` at CONSTRUCTION time, as in CRuby: `method(:nope)` raises
/// immediately rather than deferring to `#call`. The lookup is
/// `respond_to?`'s, with `include_all` -- `method(:private_helper)` is
/// legal (privacy limits CALL sites, not reflection).
pub fn method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    // `responds_to_value`, not a bare instance-MRO `responds_to`: a
    // class/module receiver binds its CLASS methods (`Process.method(
    // :clock_gettime)` -- the timeout gem's `GET_TIME`), which only the
    // value-aware probe sees (it mirrors `send_value_in`'s class-receiver
    // dispatch order, singletons included).
    // `respond_to_missing?` counts: `obj.method(:dyn)` succeeds when the
    // hook admits `:dyn`, returning a Method that dispatches through
    // `method_missing` at call time -- CRuby's `rb_obj_method`.
    // ...but a NOT-IMPLEMENTED stub still exists, and ruby hands back a Method
    // for it (reporting arity 0) even though `respond_to?` denies it.
    if !crate::dispatch::responds_to_or_missing(recv, name, true)?
        && !crate::dispatch::has_notimplement_row(recv, name)
    {
        // CRuby's phrasing names the receiver's CLASS, not the receiver
        // ("undefined method 'nope' for class 'String'").
        return Err(name_error!(
            "undefined method '{}' for class '{}'",
            name.name(),
            crate::dispatch::class_name(recv.class_id()).unwrap_or_else(|| "Object".to_string())
        ));
    }
    Ok(method_at_home(recv, name))
}

/// `K.method(:name)` captured BEFORE `def self.name` runs in document order:
/// CRuby's capture-time resolution sees only the INHERITED entry, so the
/// Method must bind it -- rspec-support's
/// `NEW_MUTEX_METHOD = Mutex.method(:new)` captures `Class#new` and the
/// `def self.new` below it delegates through the capture; a by-name capture
/// finds the override and recurses forever. The COMPILER proves the position
/// (every own def of the name sits later in the document) and emits this
/// instead of the by-name capture. `home` = the first ancestor position that
/// can answer, so `Method#call`'s seat logic resumes the walk there, above
/// the override.
pub fn method_capture_inherited(
    recv: &RubyValue,
    name_arg: &RubyValue,
) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    if let RubyValue::Class(cid) = recv {
        let ancestors = crate::dispatch::ancestors_of_value(*cid);
        let seat = ancestors
            .iter()
            .skip(1)
            .copied()
            .find(|&anc| {
                crate::dispatch::class_defines_own_class_method(anc, name)
                    || crate::builtins::class_method_table(anc)
                        .is_some_and(|t| t(name.name().as_str()).is_some())
            })
            // No ancestor DECLARES it (a universal like `new` lives in the
            // dispatch fallback, not a table row): seat at the direct super
            // position and let the class-send walk resolve from there.
            .or_else(|| ancestors.get(1).copied());
        if let Some(seat) = seat {
            return Ok(method_value_with(
                recv.clone(),
                name,
                seat,
                MethodKind::Singleton,
                None,
                None,
            ));
        }
    }
    method_new(recv, name_arg)
}

/// `Object#singleton_method(:name)` -- a `Method` bound to the receiver, but
/// ONLY for a per-object singleton method (`def obj.name` /
/// `define_singleton_method`). A name that resolves to an ordinary class
/// method is a `NameError`, exactly like CRuby -- reflection here is limited
/// to the object's own singletons.
pub fn singleton_method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    if !crate::runtime_meta::object_has_singleton_method(recv, name) {
        return Err(name_error!(
            "undefined singleton method '{}' for '{}'",
            name.name(),
            recv.inspect_string()
        ));
    }
    Ok(method_at_home(recv, name))
}

/// `Kernel#public_method(:name)` -- like `method`, but a PRIVATE (or
/// protected) method raises `NameError` rather than binding: reflection here
/// is restricted to the public surface.
pub fn public_method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    let cid = recv.class_id();
    let class = crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string());
    if crate::dispatch::responds_to(cid, name, false) {
        return Ok(method_at_home(recv, name));
    }
    let msg = if crate::dispatch::responds_to(cid, name, true) {
        format!("method '{}' for class '{}' is private", name.name(), class)
    } else {
        format!("undefined method '{}' for class '{}'", name.name(), class)
    };
    Err(raise_error("NameError", msg))
}

/// If `v` is a `Method` or `UnboundMethod`, its source `(owning class, method
/// name)` -- what `define_method(name, method_obj)` copies the definition of.
/// `None` for anything else (a Proc, a bare value).
pub fn method_source(v: &RubyValue) -> Option<(ClassId, Symbol)> {
    let RubyValue::Object(o) = v else {
        return None;
    };
    if let Some(m) = o.as_any().downcast_ref::<RMethod>() {
        return Some((m.owner().unwrap_or(m.home), m.name));
    }
    if let Some(u) = o.as_any().downcast_ref::<RUnboundMethod>() {
        return Some((u.owner().unwrap_or(u.home), u.name));
    }
    None
}

fn recv_method(recv: &RubyValue) -> &RMethod {
    let RubyValue::Object(o) = recv else {
        unreachable!("the Method table only dispatches on METHOD_CLASS receivers")
    };
    o.as_any()
        .downcast_ref::<RMethod>()
        .expect("class_id guarantees this downcast")
}

/// Shared body of `>>`/`<<`: both sides go through `#call`, so a Method, a
/// Proc, or any object answering `call` composes uniformly.
fn compose(recv: &RubyValue, other: &RubyValue, forward: bool) -> Result<RubyValue, Signal> {
    let this = recv.clone();
    let other = other.clone();
    let call = Symbol::intern("call");
    Ok(RubyValue::Proc(crate::RProc::new(
        move |call_args: &[RubyValue]| {
            let (first, second) = if forward {
                (&this, &other)
            } else {
                (&other, &this)
            };
            let mid = crate::dispatch::send_value(first, call, call_args, None)?;
            crate::dispatch::send_value(second, call, &[mid], None)
        },
    )))
}

ruby_class! {
    Method = zeo_abi::METHOD_CLASS < zeo_abi::OBJECT_CLASS;

    // An ordinary Method dispatches like any other call. One re-seated by
    // `#super_method` must resume the walk AT its home instead, or it would
    // find the override it was reached through and recurse.
    def "call" | "[]" | "===" (recv, *args, &blk) {
        let m = recv_method(recv);
        // A method whose position was NAMED -- a `#super_method` re-seat, or
        // an ancestor's `instance_method` bound down to a descendant -- runs
        // exactly that ancestor's body. Bypassing the overrides (and
        // prepends) above it is what naming the position means, so this
        // outranks the frozen-entry probe, whose layer is chain-rooted.
        let seat = m.seat.or_else(|| (m.home != m.chain()).then_some(m.home));
        if let Some(seat) = seat {
            return match m.kind {
                MethodKind::Instance => match &m.snapshot {
                    // A frozen `define_method` body binds any instance
                    // directly; a frozen layer resolves below the overlay AT
                    // the seat, so a wrapper installed since can't capture it.
                    Some(FrozenEntry::Overlay(mi)) => match &m.recv {
                        RubyValue::Object(o) => mi.call(o, args, blk),
                        _ => crate::dispatch::send_as_defined_in(&m.recv, seat, m.name, args, blk),
                    },
                    Some(FrozenEntry::BelowOverlay) => {
                        crate::dispatch::send_below_overlay_at(&m.recv, seat, m.name, args, blk)
                    }
                    None => crate::dispatch::send_as_defined_in(&m.recv, seat, m.name, args, blk),
                },
                MethodKind::Singleton => {
                    crate::dispatch::send_class_from(m.chain(), seat, m.name, args, blk)
                }
            };
        }
        // The frozen entry: re-resolving by name would find whatever
        // holds the name NOW, which for a wrapped method is the wrapper
        // itself. See `RMethod::snapshot`.
        if let (Some(frozen), RubyValue::Object(o)) = (&m.snapshot, &m.recv)
            && let Some(out) = frozen.call_on(o, m.name, args, blk.clone()) {
                return out;
            }
        crate::dispatch::send_value(&m.recv, m.name, args, blk)
    }
    def "name"(recv) {
        Ok(RubyValue::Symbol(recv_method(recv).name))
    }
    def "receiver"(recv) {
        Ok(recv_method(recv).recv.clone())
    }
    // `Method#to_proc` yields a lambda (`lambda? == true`) carrying the
    // method's arity, which is what `#curry` needs to know how many arguments
    // to gather before invoking.
    def "to_proc" as m_to_proc (recv) {
        let m = recv_method(recv);
        let (target, name) = (m.recv.clone(), m.name);
        let arity = crate::method_meta::arity(Some(&m.recv), m.home, m.kind, m.name).unwrap_or(-1) as i32;
        let p = crate::RProc::with_meta(
            move |args: &[RubyValue]| crate::dispatch::send_value(&target, name, args, None),
            arity,
            true,
        );
        // The proc reports the METHOD's own source location (CRuby's
        // method_to_proc carries the method, and source_location delegates).
        let p = match crate::method_meta::source_pair(m.home, m.kind, m.name) {
            Some((file, line)) => p.with_location(file, line),
            None => p,
        };
        Ok(RubyValue::Proc(p))
    }
    def "arity"(recv) {
        let m = recv_method(recv);
        // A user `def` has a baked descriptor; a builtin has none, so `-1`
        // (var-args) stays the honest catch-all there.
        Ok(RubyValue::Int(
            crate::method_meta::arity(Some(&m.recv), m.home, m.kind, m.name).unwrap_or(-1),
        ))
    }
    def "parameters"(recv) {
        let m = recv_method(recv);
        // Builtins have no baked signature -- CRuby reports them as a lone rest;
        // mirror that so `#parameters` is always an Array.
        Ok(crate::method_meta::parameters(Some(&m.recv), m.home, m.kind, m.name)
            .unwrap_or_else(|| RubyValue::Array(crate::array_new(vec![]))))
    }
    // CRuby unbinds to the OWNER, not to the class the method was reached
    // through: `Sub.new.method(:greet).unbind` is `Base`'s.
    def "unbind"(recv) {
        let m = recv_method(recv);
        let owner = m.owner().unwrap_or(m.home);
        // A class method an `extend` supplied unbinds to the MODULE's instance
        // method, which is what it has been all along -- the same
        // UnboundMethod `Ext.instance_method(:hi)` hands back.
        let extended = m.kind == MethodKind::Singleton
            && crate::dispatch::class_method_extend_source(m.home, m.name) == Some(owner);
        Ok(RubyValue::Object(Arc::new(RUnboundMethod {
            class_id: if extended { owner } else { m.chain() },
            name: m.name,
            home: owner,
            kind: if extended { MethodKind::Instance } else { m.kind },
            // Unbinding does not re-resolve: the entry stays the one this
            // Method froze, and a re-seat keeps its position.
            snapshot: m.snapshot.clone(),
            seat: m.seat,
        })))
    }
    // `Method#owner` -- the class or module in the receiver's ancestry that
    // actually defines the method (which may be an ancestor of the receiver's
    // class, not the class itself).
    def "owner"(recv) {
        let m = recv_method(recv);
        crate::builtins::unbound_method::owner_value(
            m.home, m.owner().unwrap_or(m.home), m.name, m.kind,
        )
    }
    // `Method#original_name` -- the name the method was DEFINED under, which
    // differs from `#name` only for one reached through an alias.
    def "original_name"(recv) {
        let m = recv_method(recv);
        Ok(RubyValue::Symbol(crate::method_meta::original_name(m.home, m.kind, m.name)))
    }
    // `Method#source_location` -- the `[file, line]` codegen baked from the
    // `def` keyword's own span. `nil` for a method with no Ruby source this
    // AOT runtime tracks (a builtin, a `define_method` body), matching CRuby's
    // `nil` for C-defined methods.
    def "source_location"(recv) {
        let m = recv_method(recv);
        Ok(crate::method_meta::source_location(m.home, m.kind, m.name))
    }
    // `Method#super_method` -- the same method as the NEXT ancestor up defines
    // it, or `nil` at the end of the chain. The result is re-seated onto that
    // ancestor, so calling it runs the ancestor's body.
    def "super_method"(recv) {
        let m = recv_method(recv);
        let Some(owner) = m.owner() else {
            return Ok(RubyValue::Nil);
        };
        let next = match m.kind {
            MethodKind::Instance => {
                crate::dispatch::method_owner_after(m.chain(), owner, m.name)
            }
            MethodKind::Singleton => {
                crate::dispatch::class_method_owner_after(m.chain(), owner, m.name)
            }
        };
        Ok(match next {
            // The re-seat records its position in `seat`: `#owner` must
            // answer it verbatim (a fresh scan from `home` under `prepend`
            // finds the prepended module again and the walk never advances).
            Some(home) => method_value_with(m.recv.clone(), m.name, home, m.kind, Some(home), None),
            None => RubyValue::Nil,
        })
    }
    // `meth >> other` -- a Proc running `meth` then piping its result into
    // `other` (`other.call(meth.call(*args))`). `other` is any callable.
    def ">>"(recv, other) {
        compose(recv, other, true)
    }
    // `meth << other` -- the reverse pipe: `meth.call(other.call(*args))`.
    def "<<"(recv, other) {
        compose(recv, other, false)
    }
    // `Method#==`/`#eql?` -- same defining method (name + owner) bound to the
    // SAME receiver. CRuby compares receivers by identity, not by `==`, so two
    // Methods over two equal-but-distinct Strings are unequal.
    def "==" | "eql?" (recv, other) {
        let m = recv_method(recv);
        let RubyValue::Object(o) = other else {
            return Ok(RubyValue::Bool(false));
        };
        let Some(other) = o.as_any().downcast_ref::<RMethod>() else {
            return Ok(RubyValue::Bool(false));
        };
        Ok(RubyValue::Bool(
            m.name == other.name
                && m.owner() == other.owner()
                && crate::builtins::basic_object::value_identity(&m.recv, &other.recv),
        ))
    }
    // `Method#hash` -- consistent with `#==`: name, owner, and the receiver's
    // IDENTITY, since two Methods over distinct receivers are never equal. The
    // receiver folds in exactly the way `value_identity` COMPARES it -- an
    // allocation address for a heap value, the value itself for an immediate
    // -- so two Methods that are `==` can never hash apart.
    def "hash"(recv) {
        use std::hash::{Hash, Hasher};
        let m = recv_method(recv);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        m.name.hash(&mut h);
        m.owner().unwrap_or(m.home).hash(&mut h);
        match crate::runtime_meta::value_identity(&m.recv) {
            Some(addr) => addr.hash(&mut h),
            None => crate::collections::hash_key(&m.recv).hash(&mut h),
        }
        Ok(RubyValue::Int(h.finish() as i64))
    }
    // `Method#box` -- the namespace this method was defined in. zeo has no
    // namespaces, so every method it can hand back belongs to none: nil, the
    // same answer ruby gives for a method defined outside any.
    def "box"(recv) {
        let _ = recv;
        Ok(RubyValue::Nil)
    }
    // `Method#curry` -- curries the equivalent Proc (`to_proc.curry`).
    def "curry"(recv, *args, &_blk) {
        let proc = m_to_proc(recv, &[], None)?;
        crate::dispatch::send_value(&proc, Symbol::intern("curry"), args, None)
    }
    // The four homes a bound Method can print (see `method_meta::Inspect`):
    // a per-object singleton names the RECEIVER, a class method names its
    // class with a `.`, an instance method of `Class`/`Module` reached
    // through a class receiver names the singleton class that method hangs
    // off, and everything else names the receiver's class -- qualified with
    // the owner whenever an ancestor is the one that actually defines it.
    def "inspect" | "to_s" (recv) {
        let m = recv_method(recv);
        // An alias names the class its SOURCE came from, not its own owner:
        // `alias_method :w, :x` in `B < A`, where `A` defines `x`, prints
        // `B(A)#w(x)` even though the alias itself is owned by `B`.
        let original = crate::method_meta::alias_origin(m.home, m.kind, m.name);
        let owner = match m.kind {
            MethodKind::Instance => {
                crate::dispatch::method_owner(m.home, original.unwrap_or(m.name))
            }
            MethodKind::Singleton => {
                crate::dispatch::class_method_owner(m.home, original.unwrap_or(m.name))
            }
        };
        let per_object = !matches!(m.recv, RubyValue::Class(_))
            && crate::runtime_meta::object_has_singleton_method(&m.recv, m.name);
        let extended = m.kind == MethodKind::Singleton
            && crate::dispatch::class_method_extend_source(m.home, m.name).is_some();
        let (home, separator, qualifier) = if per_object {
            (m.recv.inspect_string(), '.', None)
        } else if extended {
            // The row is the module's INSTANCE method, seated in the singleton
            // class's ancestry -- so CRuby prints the singleton as the home and
            // a `#`, then names the module: `#<Class:K>(Ext)#hi`.
            (format!("#<Class:{}>", class_name(m.home)), '#', owner)
        } else if m.kind == MethodKind::Singleton {
            (class_name(m.home), '.', owner.filter(|&o| o != m.home))
        } else if let RubyValue::Class(cid) = m.recv {
            (format!("#<Class:{}>", class_name(cid)), '#', owner)
        } else {
            (class_name(m.home), '#', owner.filter(|&o| o != m.home))
        };
        Ok(RubyValue::Str(crate::collections::string_new(
            crate::method_meta::Inspect {
                label: "Method",
                home,
                owner: qualifier.map(class_name),
                separator,
                name: m.name,
                original,
                params: crate::method_meta::printable_params(Some(&m.recv), m.home, m.kind, m.name),
                source: crate::method_meta::source_of(m.home, m.kind, m.name),
            }
            .render(),
        )))
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "clone"(recv) { inherited_row!(kernel, "clone", recv, __args, None) }
    def "dup"(recv) { inherited_row!(kernel, "dup", recv, __args, None) }
}

/// A class's Ruby name, or `Object` for one the registry can't name (never
/// expected -- reflection only ever holds registered classes).
fn class_name(cid: ClassId) -> String {
    crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string())
}
