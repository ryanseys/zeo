//! `Method`: the object `Kernel#method(:name)` answers -- a bound
//! (receiver, name) pair whose `#call` dispatches through the ordinary
//! `send` machinery. Its sibling `UnboundMethod` (an unbound `(class, name)`
//! pair) lives in `unbound_method.rs`; the two reference each other's payload
//! (`#unbind` builds an `UnboundMethod`, `UnboundMethod#bind` builds a
//! `Method`) plus this file's shared `resolve_method_name`/`method_source`.

use std::sync::Arc;

use crate::builtins::{inherited_row, name_error, type_error};
use crate::dispatch::{RObj, RubyObject};
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
    pub(crate) seat: Option<Seat>,
    /// The REFLECTION row as it resolved when this object was made -- the
    /// metadata twin of [`RMethod::snapshot`].
    ///
    /// `#arity`, `#parameters` and `#source_location` are keyed by
    /// `(class, name)` with no position, and a redefinition timeline
    /// re-registers that key at each body's own line. So a handle taken
    /// before a reopen described the body that replaced it. CRuby's handle
    /// holds the entry it was built from, which makes every one of these a
    /// snapshot rather than a lookup.
    ///
    /// `None` where [`RMethod::snapshot`] is None, and for the same reason: a
    /// `#super_method` re-seat names a position the ordinary walk would not
    /// reach, so it resolves afresh.
    pub(crate) meta: Option<Arc<crate::method_meta::MethodMeta>>,
    /// `Kernel#freeze`'s own flag. A Method is a value snapshot, so
    /// freezing gates nothing -- but `frozen?` answers what was written,
    /// which a hardcoded `false` did not.
    frozen: std::sync::atomic::AtomicBool,
}

/// A `#super_method` re-seat's position: the ancestor AND its INDEX in the
/// chain.
///
/// The index is what makes the walk advance through a module the chain holds
/// TWICE (one both `include`d and `prepend`ed). The ancestor alone names both
/// copies, so the next re-seat would find the first one again and the chain
/// would never end.
#[derive(Clone, Copy)]
pub(crate) struct Seat {
    pub(crate) owner: ClassId,
    pub(crate) at: usize,
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
            return Some(seat.owner);
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
    let meta = crate::method_meta::lookup(home, kind, name);
    method_value_with(recv, name, home, kind, None, snapshot, meta)
}

/// [`method_value`] over an entry frozen ELSEWHERE -- `UnboundMethod#bind`,
/// which must hand on the copy the unbound method already froze rather than
/// re-resolving a name that may have been redefined since.
pub(crate) fn method_value_with(
    recv: RubyValue,
    name: Symbol,
    home: ClassId,
    kind: MethodKind,
    seat: Option<Seat>,
    snapshot: Option<FrozenEntry>,
    meta: Option<Arc<crate::method_meta::MethodMeta>>,
) -> RubyValue {
    RubyValue::Object(Arc::new(RMethod {
        recv,
        name,
        home,
        kind,
        snapshot,
        seat,
        meta,
        frozen: std::sync::atomic::AtomicBool::new(false),
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
        self.frozen.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        Arc::new(RMethod {
            recv: self.recv.clone(),
            name: self.name,
            home: self.home,
            kind: self.kind,
            snapshot: self.snapshot.clone(),
            seat: self.seat,
            meta: self.meta.clone(),
            frozen: std::sync::atomic::AtomicBool::new(copy_frozen && self.is_frozen()),
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
    Err(crate::builtins::name_error!("{}", msg))
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
    // At COMPOSE time, as `Proc#>>` does -- the two share ruby's rule.
    if !crate::dispatch::responds_to(other.class_id(), crate::symbol::wk::call(), true) {
        return Err(crate::builtins::type_error!("callable object is expected"));
    }
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
        // A Method object forwards the block as CRuby's PROC handler, so a
        // literal block written at THIS call is not one at the callee's.
        // `Kernel.method(:lambda).call { 1 }` is the case that shows it.
        if let Some(RubyValue::Proc(p)) = &blk {
            p.clear_literal_block();
        }
        // A method whose position was NAMED -- a `#super_method` re-seat, or
        // an ancestor's `instance_method` bound down to a descendant -- runs
        // exactly that ancestor's body. Bypassing the overrides (and
        // prepends) above it is what naming the position means, so this
        // outranks the frozen-entry probe, whose layer is chain-rooted.
        let seat = m
            .seat
            .map(|s| s.owner)
            .or_else(|| (m.home != m.chain()).then_some(m.home));
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
        let arity = crate::method_meta::arity(m.meta.as_ref(), Some(&m.recv), m.home, m.kind, m.name).unwrap_or(-1) as i32;
        let b = crate::rproc::ProcBuilder::from_rust(
            // The block travels with the call: `m.to_proc.call { .. }` and
            // `f(&method(:each))` both reach the method WITH their block, so a
            // block-taking method runs its block instead of answering an
            // Enumerator.
            move |_self: &RubyValue, args: &[RubyValue], block| {
                crate::dispatch::send_value(&target, name, args, block)
            },
            RubyValue::Nil,
            arity,
            true,
        );
        // The proc reports the METHOD's own source location (CRuby's
        // method_to_proc carries the method, and source_location delegates).
        let b = match crate::method_meta::source_pair(m.meta.as_ref(), Some(&m.recv), m.home, m.kind, m.name) {
            Some((file, line)) => b.location(file, line),
            None => b,
        };
        Ok(RubyValue::Proc(b.build()))
    }
    def "arity"(recv) {
        let m = recv_method(recv);
        // A user `def` has a baked descriptor; a builtin has none, so `-1`
        // (var-args) stays the honest catch-all there.
        Ok(RubyValue::Int(
            crate::method_meta::arity(m.meta.as_ref(), Some(&m.recv), m.home, m.kind, m.name).unwrap_or(-1),
        ))
    }
    def "parameters"(recv) {
        let m = recv_method(recv);
        // Builtins have no baked signature -- CRuby reports them as a lone rest;
        // mirror that so `#parameters` is always an Array.
        Ok(crate::method_meta::parameters(m.meta.as_ref(), Some(&m.recv), m.home, m.kind, m.name)
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
            meta: m.meta.clone(),
        })))
    }
    // `Method#owner` -- the class or module in the receiver's ancestry that
    // actually defines the method (which may be an ancestor of the receiver's
    // class, not the class itself).
    def "owner"(recv) {
        let m = recv_method(recv);
        // `main`'s private singletons are ROUTED by dispatch rather than
        // installed, so their home is Object. Ruby owns them on main's own
        // singleton class, and reflection must not drift from dispatch.
        if crate::dispatch::is_main_private_singleton(&m.recv, m.name) {
            return crate::runtime_meta::runtime_singleton_class(&m.recv);
        }
        // `chain()`, not `home`: after `#super_method` re-seats, `home` IS the
        // owner, and asking whether the OWNER extended the module answers no
        // for the module itself -- so a re-seated extend owner rendered as
        // `#<Class:M1>` where ruby names the module bare. The receiver's own
        // class is what the question is about, and it is what `chain()` keeps.
        crate::builtins::unbound_method::owner_value(
            m.chain(), m.owner().unwrap_or(m.home), m.name, m.kind,
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
        Ok(crate::method_meta::source_location(m.meta.as_ref(), Some(&m.recv), m.home, m.kind, m.name))
    }
    // `Method#super_method` -- the same method as the NEXT ancestor up defines
    // it, or `nil` at the end of the chain. The result is re-seated onto that
    // ancestor, so calling it runs the ancestor's body.
    def "super_method"(recv) {
        let m = recv_method(recv);
        let Some(owner) = m.owner() else {
            return Ok(RubyValue::Nil);
        };
        // Resume past THIS copy of the owner. A re-seat recorded which one it
        // picked; a Method never re-seated is at the owner's first position,
        // which is where ordinary dispatch found it.
        // A seat records the position a re-seat picked. Without one the
        // Method sits at the owner's FIRST position -- where ordinary
        // dispatch found it -- and the two channels index different
        // sequences, so each asks its own.
        let Some(at) = m.seat.map(|s| s.at).or_else(|| match m.kind {
            MethodKind::Instance => crate::dispatch::chain_index_of(m.chain(), owner),
            MethodKind::Singleton => {
                crate::dispatch::singleton_chain_index_of(m.chain(), owner)
            }
        }) else {
            return Ok(RubyValue::Nil);
        };
        let next = match m.kind {
            MethodKind::Instance => crate::dispatch::method_owner_after(m.chain(), at + 1, m.name),
            MethodKind::Singleton => {
                crate::dispatch::class_method_owner_after(m.chain(), at + 1, m.name)
            }
        };
        Ok(match next {
            // The re-seat records its position in `seat`: `#owner` must
            // answer it verbatim (a fresh scan from `home` under `prepend`
            // finds the prepended module again and the walk never advances).
            Some((home, at)) => method_value_with(
                m.recv.clone(),
                m.name,
                home,
                m.kind,
                Some(Seat { owner: home, at }),
                None,
                None,
            ),
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
                params: crate::method_meta::printable_params(m.meta.as_ref(), Some(&m.recv), m.home, m.kind, m.name),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::send_value;

    // Each test runs in its own nextest process -- see the crate README for
    // the with_core() bootstrap pattern.
    fn install_core() {
        crate::dispatch::install_class_registry(crate::dispatch::ClassRegistry::with_core());
    }

    fn call(recv: &RubyValue, name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
        send_value(recv, Symbol::intern(name), args, None)
    }

    #[test]
    fn a_method_binds_name_receiver_and_owner() {
        install_core();
        let five = RubyValue::Int(5);
        let m = method_new(&five, &RubyValue::Symbol(Symbol::intern("+"))).unwrap();

        assert!(matches!(
            call(&m, "name", &[]),
            Ok(RubyValue::Symbol(s)) if s == Symbol::intern("+")
        ));
        assert!(matches!(call(&m, "receiver", &[]), Ok(RubyValue::Int(5))));
        assert!(matches!(
            call(&m, "owner", &[]),
            Ok(RubyValue::Class(c)) if c == zeo_abi::INTEGER_CLASS
        ));
        assert!(matches!(
            call(&m, "call", &[RubyValue::Int(2)]),
            Ok(RubyValue::Int(7))
        ));
    }

    #[test]
    fn an_unknown_name_refuses_at_construction() {
        install_core();
        let err = method_new(
            &RubyValue::Int(5),
            &RubyValue::Symbol(Symbol::intern("nope")),
        );
        let Err(Signal::Raise(exc)) = err else {
            panic!("expected a NameError raise");
        };
        assert_eq!(
            exc.as_object_unchecked().class_id(),
            zeo_abi::NAME_ERROR_CLASS
        );
    }

    #[test]
    fn equality_compares_receivers_by_identity() {
        install_core();
        let a = RubyValue::Str(crate::string_new("a".to_string()));
        let a_twin = RubyValue::Str(crate::string_new("a".to_string()));
        let upcase = RubyValue::Symbol(Symbol::intern("upcase"));
        let m1 = method_new(&a, &upcase).unwrap();
        let m2 = method_new(&a, &upcase).unwrap();
        let m3 = method_new(&a_twin, &upcase).unwrap();

        // Same name, same owner, same receiver OBJECT: equal.
        assert!(matches!(
            call(&m1, "==", std::slice::from_ref(&m2)),
            Ok(RubyValue::Bool(true))
        ));
        // An equal-but-distinct receiver is a different Method.
        assert!(matches!(
            call(&m1, "==", std::slice::from_ref(&m3)),
            Ok(RubyValue::Bool(false))
        ));
    }
}
