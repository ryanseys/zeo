//! `UnboundMethod` -- `Module#instance_method`'s result, a `(class, name)` pair
//! not yet bound to a receiver. `#bind(obj)` re-binds it into a `Method` (obj
//! must be an instance of the owning class or a subclass); `#bind_call(obj,
//! *args)` binds and calls in one step. Its sibling `Method` lives in
//! `method.rs`, from which this file borrows `method_value_with` (the bind
//! result) and the shared `resolve_method_name`.

use std::sync::Arc;

use crate::builtins::method::resolve_method_name;
use crate::builtins::{inherited_row, name_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::method_meta::MethodKind;
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use zeo_abi::ClassId;
use zeo_macros::ruby_class;

pub struct RUnboundMethod {
    /// The class it was FETCHED from -- the `#bind` target check, and the MRO
    /// a `#super_method` walk runs over.
    pub class_id: ClassId,
    pub name: Symbol,
    /// Where the lookup sits in that chain -- see [`RMethod::home`]. Unlike a
    /// bound `Method`, an UnboundMethod prints its owner unqualified, so this
    /// only drives the super walk and the metadata lookup.
    pub home: ClassId,
    pub kind: MethodKind,
    /// The frozen LAYER -- see [`crate::builtins::method::FrozenEntry`]. It is
    /// deliberately not the resolved body: `#bind` may be handed an instance of
    /// a SUBCLASS, whose compiled copy of the method has a different layout.
    pub(crate) snapshot: Option<crate::builtins::method::FrozenEntry>,
    /// The RESOLVED chain position after a `#super_method` re-seat -- see
    /// [`RMethod::seat`](crate::builtins::method::RMethod::seat).
    pub(crate) seat: Option<crate::builtins::method::Seat>,
    /// The REFLECTION row as it resolved when this object was made -- see
    /// [`RMethod::meta`](crate::builtins::method::RMethod::meta).
    pub(crate) meta: Option<Arc<crate::method_meta::MethodMeta>>,
}

impl RUnboundMethod {
    /// The class or module that actually defines this method.
    pub(crate) fn owner(&self) -> Option<ClassId> {
        if let Some(seat) = self.seat {
            return Some(seat.owner);
        }
        match self.kind {
            // A singleton class's own instance methods ARE its owner's class
            // methods, and they live on the owner's tables -- so the ordinary
            // scan finds nothing at the chain head and reports the first
            // `extend`ed module instead. See `singleton_head_owns`.
            MethodKind::Instance if singleton_head_owns(self.home, self.name) => Some(self.home),
            MethodKind::Instance => crate::dispatch::method_owner(self.home, self.name),
            MethodKind::Singleton => {
                crate::dispatch::class_method_owner_reported(self.home, self.name)
            }
        }
    }
}

impl RubyObject for RUnboundMethod {
    fn class_id(&self) -> ClassId {
        zeo_abi::UNBOUND_METHOD_CLASS
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
        Arc::new(RUnboundMethod {
            class_id: self.class_id,
            name: self.name,
            home: self.home,
            kind: self.kind,
            snapshot: self.snapshot.clone(),
            seat: self.seat,
            meta: self.meta.clone(),
        })
    }
}

/// `Module#instance_method(:name)` -- the unbound method for `name` on `cid`.
/// Unknown name is a `NameError` at construction, as in CRuby.
pub fn unbound_method_new(cid: ClassId, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    if !crate::dispatch::responds_to(cid, name, true) {
        return Err(name_error!(
            "undefined method '{}' for class '{}'",
            name.name(),
            crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string())
        ));
    }
    Ok(RubyValue::Object(Arc::new(RUnboundMethod {
        class_id: cid,
        name,
        // `instance_method` always asks the INSTANCE chain -- `Foo.bar` is
        // reached through `Foo.singleton_class.instance_method(:bar)`.
        home: instance_method_home(cid, name),
        kind: MethodKind::Instance,
        snapshot: Some(crate::builtins::method::freeze_entry(cid, name)),
        seat: None,
        meta: crate::method_meta::lookup(
            instance_method_home(cid, name),
            MethodKind::Instance,
            name,
        ),
    })))
}

/// Where `instance_method` seats its lookup.
///
/// A SINGLETON class's instance methods ARE its owner's class methods, and
/// they live on the OWNER's tables rather than on the minted singleton -- so
/// the ordinary instance walk finds nothing at the chain head and reports the
/// first `extend`ed module instead. `Chained.singleton_class.instance_method(
/// :tag).owner` answered `S2` where ruby answers `#<Class:Chained>`, whose own
/// `def self.tag` sits ahead of every extend.
fn instance_method_home(cid: ClassId, name: Symbol) -> ClassId {
    match singleton_head_owns(cid, name) {
        true => cid,
        false => crate::dispatch::method_owner(cid, name).unwrap_or(cid),
    }
}

/// Whether `cid` is a minted SINGLETON class whose own owner really defines
/// class method `name` -- so `cid` itself is where the row lives.
///
/// Nothing is stored on the minted class: a `def self.x` lands on the owner's
/// tables. The ordinary instance walk therefore sees an empty head and reports
/// the nearest `extend`ed module, which is a position BEHIND the head.
///
/// It asks the CHAIN rather than "does the class define one", because a module
/// PREPENDED into the singleton sits AHEAD of the head and rightly owns the
/// lookup even when the class defines the name itself.
fn singleton_head_owns(cid: ClassId, name: Symbol) -> bool {
    matches!(
        crate::runtime_meta::singleton_owner_value(cid),
        Some(RubyValue::Class(owner)) if crate::dispatch::singleton_resolves_at_head(owner, name)
    )
}

/// The class an `#owner` reports. A SINGLETON lookup lives on the owner's
/// singleton class -- `Foo.method(:a).owner` is `#<Class:Foo>`, not `Foo` --
/// which is also where `def self.a` actually put it. Shared with `Method`,
/// whose two kinds answer the same way.
///
/// A module the class `extend`ed is the exception: its row is an ordinary
/// INSTANCE method, seated in the singleton's ancestry rather than minted on
/// it, so it reports itself. `module M; def self.x; end; end` is NOT that
/// shape -- `M` owns a real class method there, and answers `#<Class:M>`.
pub(crate) fn owner_value(
    home: ClassId,
    owner: ClassId,
    name: Symbol,
    kind: MethodKind,
) -> Result<RubyValue, crate::Signal> {
    match kind {
        MethodKind::Instance => Ok(RubyValue::Class(owner)),
        // A module reached through the singleton chain -- `extend`ed onto the
        // class, or prepended into its singleton -- is named BARE. Only a
        // real `def self.x` is reported as owned by a singleton class.
        MethodKind::Singleton
            if crate::dispatch::class_method_extend_source(home, name) == Some(owner)
                || crate::runtime_meta::singleton_prepend_owner(home, name) == Some(owner)
                // The by-NAME questions above cannot answer after
                // `#super_method` has re-seated onto a module the host also
                // defines the name on -- `OwnFirst` owns `tag`, so nothing
                // "extends" it there, and the re-seated `M1` rendered as
                // `#<Class:M1>`. Where the chain SEATS the owner settles it.
                || crate::dispatch::singleton_seats_as_mixin(home, owner) =>
        {
            Ok(RubyValue::Class(owner))
        }
        MethodKind::Singleton => {
            crate::runtime_meta::runtime_singleton_class(&RubyValue::Class(owner))
        }
    }
}

fn recv_unbound(recv: &RubyValue) -> &RUnboundMethod {
    let RubyValue::Object(o) = recv else {
        unreachable!("the UnboundMethod table only dispatches on UNBOUND_METHOD_CLASS receivers")
    };
    o.as_any()
        .downcast_ref::<RUnboundMethod>()
        .expect("class_id guarantees this downcast")
}

/// The shared `bind` check: `obj` must be an instance of the class that OWNS
/// the method -- not of the class it was fetched from, which may be a subclass
/// of the owner (`B.instance_method(:x)` where `x` is A's binds an `A`).
///
/// A MODULE owner imposes no check at all, since ruby 3.0: a module has no
/// instances of its own, so an unbound method taken from one binds to any
/// object.
///
/// A SINGLETON unbind is owned by `#<Class:C>`, whose instances are `C` and
/// `C`'s subclasses -- so the argument has to be a class in that ancestry,
/// not an instance of it. CRuby words that refusal differently, because it
/// reaches the same test through a singleton `methclass`.
fn bind_target(um: &RUnboundMethod, obj: &RubyValue) -> Result<RubyValue, Signal> {
    let owner = um.owner().unwrap_or(um.class_id);
    let ok = crate::dispatch::class_is_module(owner).unwrap_or(false)
        || match um.kind {
            // `is_a_value`, not `is_a`: reaching a class method through
            // `Foo.singleton_class.instance_method(:a)` gives an INSTANCE-kind
            // unbound method whose class is that singleton, and `Foo`
            // instantiates it without being an instance of any ordinary class
            // in its ancestry.
            MethodKind::Instance => crate::dispatch::is_a_value(obj, owner),
            MethodKind::Singleton => {
                matches!(obj, RubyValue::Class(cid) if crate::dispatch::is_a(*cid, um.class_id))
            }
        };
    if !ok {
        // CRuby picks the wording from the OWNER, not the lookup kind: any
        // singleton `methclass` gets the "different object" message.
        let singleton = um.kind == MethodKind::Singleton
            || (crate::runtime_meta::is_live()
                && crate::runtime_meta::singleton_owner_value(owner).is_some());
        return Err(match singleton {
            true => type_error!("singleton method called for a different object"),
            false => type_error!(
                "bind argument must be an instance of {}",
                crate::dispatch::class_name(owner).unwrap_or_else(|| "Object".to_string())
            ),
        });
    }
    // The bound Method inherits the UNBOUND one's frozen entry, seat and
    // reflection row: `#bind` must not re-resolve a name that has been
    // redefined since, nor forget a `#super_method` re-seat. The row travels
    // for the same reason the entry does -- `um.arity` and
    // `um.bind(o).arity` describe one definition.
    Ok(crate::builtins::method::method_value_with(
        obj.clone(),
        um.name,
        um.home,
        um.kind,
        um.seat,
        um.snapshot.clone(),
        um.meta.clone(),
    ))
}

ruby_class! {
    UnboundMethod = zeo_abi::UNBOUND_METHOD_CLASS < zeo_abi::OBJECT_CLASS;

    def "name"(recv) {
        Ok(RubyValue::Symbol(recv_unbound(recv).name))
    }
    def "arity"(recv) {
        let um = recv_unbound(recv);
        Ok(RubyValue::Int(
            crate::method_meta::arity(um.meta.as_ref(), None, um.home, um.kind, um.name).unwrap_or(-1),
        ))
    }
    def "parameters"(recv) {
        let um = recv_unbound(recv);
        Ok(crate::method_meta::parameters(um.meta.as_ref(), None, um.home, um.kind, um.name)
            .unwrap_or_else(|| RubyValue::Array(crate::array_new(vec![]))))
    }
    def "bind"(recv, arg) {
        bind_target(recv_unbound(recv), arg)
    }
    def "bind_call" cfunc (recv, receiver, *args, &blk) {
        let um = recv_unbound(recv);
        // The receiver must still be a valid bind target.
        bind_target(um, receiver)?;
        // A method taken from an ANCESTOR of the receiver's class (or
        // re-seated by `#super_method`) runs exactly that ancestor's body --
        // bypassing the overrides above it is what naming the position
        // means (`Base.instance_method(:x).bind_call(sub)` runs Base's).
        if um.kind == MethodKind::Instance {
            let seat = um
                .seat
                .map(|s| s.owner)
                .or_else(|| (um.home != receiver.class_id()).then_some(um.home));
            if let Some(seat) = seat {
                return match &um.snapshot {
                    Some(crate::builtins::method::FrozenEntry::Overlay(mi)) => match receiver {
                        RubyValue::Object(o) => mi.call(o, args, blk),
                        _ => crate::dispatch::send_as_defined_in(receiver, seat, um.name, args, blk),
                    },
                    Some(crate::builtins::method::FrozenEntry::BelowOverlay) => {
                        crate::dispatch::send_below_overlay_at(receiver, seat, um.name, args, blk)
                    }
                    None => crate::dispatch::send_as_defined_in(receiver, seat, um.name, args, blk),
                };
            }
        }
        // Then the FROZEN entry, exactly as `#bind(obj).call` would.
        if let (Some(frozen), RubyValue::Object(o)) = (&um.snapshot, receiver)
            && let Some(out) = frozen.call_on(o, um.name, args, blk.clone()) {
                return out;
            }
        crate::dispatch::send_value(receiver, um.name, args, blk)
    }
    // `UnboundMethod#owner` -- the defining class/module in the owning class's
    // ancestry (may differ from the class the unbound method was fetched from).
    def "owner"(recv) {
        let um = recv_unbound(recv);
        owner_value(um.home, um.owner().unwrap_or(um.home), um.name, um.kind)
    }
    def "source_location"(recv) {
        let um = recv_unbound(recv);
        Ok(crate::method_meta::source_location(um.meta.as_ref(), None, um.home, um.kind, um.name))
    }
    // The unbound twin of `Method#super_method`, walking the chain of the
    // class this method was fetched from.
    def "super_method"(recv) {
        let um = recv_unbound(recv);
        let Some(owner) = um.owner() else {
            return Ok(RubyValue::Nil);
        };
        // Resume past THIS copy of the owner -- see `builtins::method::Seat`.
        let Some(at) = um
            .seat
            .map(|s| s.at)
            .or_else(|| crate::dispatch::chain_index_of(um.class_id, owner))
        else {
            return Ok(RubyValue::Nil);
        };
        let next = match um.kind {
            MethodKind::Instance => {
                crate::dispatch::method_owner_after(um.class_id, at + 1, um.name)
            }
            MethodKind::Singleton => {
                crate::dispatch::class_method_owner_after(um.class_id, at + 1, um.name)
            }
        };
        Ok(match next {
            Some((home, at)) => RubyValue::Object(Arc::new(RUnboundMethod {
                class_id: um.class_id,
                name: um.name,
                home,
                kind: um.kind,
                // A `#super_method` re-seat picks a position the ordinary walk
                // would not reach, so it keeps the resolve-and-send path.
                snapshot: None,
                seat: Some(crate::builtins::method::Seat { owner: home, at }),
                meta: None,
            })),
            None => RubyValue::Nil,
        })
    }
    // `UnboundMethod#original_name` -- the name it was DEFINED under, which
    // differs from `#name` only for one reached through an alias.
    def "original_name"(recv) {
        let um = recv_unbound(recv);
        Ok(RubyValue::Symbol(crate::method_meta::original_name(um.home, um.kind, um.name)))
    }
    // Unbound, there is no receiver to qualify against: CRuby prints the
    // OWNER alone, never the class the method was fetched from.
    def "inspect" | "to_s" (recv) {
        let um = recv_unbound(recv);
        let owner = um.owner().unwrap_or(um.home);
        Ok(RubyValue::Str(crate::collections::string_new(
            crate::method_meta::Inspect {
                label: "UnboundMethod",
                home: crate::dispatch::class_name(owner)
                    .unwrap_or_else(|| "Object".to_string()),
                owner: None,
                separator: if um.kind == MethodKind::Singleton { '.' } else { '#' },
                name: um.name,
                original: crate::method_meta::alias_origin(um.home, um.kind, um.name),
                params: crate::method_meta::printable_params(um.meta.as_ref(), None, um.home, um.kind, um.name),
                source: crate::method_meta::source_of(um.home, um.kind, um.name),
            }
            .render(),
        )))
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "=="(recv, _other) { inherited_row!(basic_object, "==", recv, __args, None) }
    def "clone"(recv) { inherited_row!(kernel, "clone", recv, __args, None) }
    def "dup"(recv) { inherited_row!(kernel, "dup", recv, __args, None) }
    def "eql?"(recv, _other) { inherited_row!(kernel, "eql?", recv, __args, None) }
    def "hash"(recv) { inherited_row!(kernel, "hash", recv, __args, None) }
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

    fn rstr(s: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(s.to_string()))
    }

    #[test]
    fn an_unbound_method_reports_name_and_owner() {
        install_core();
        let upcase = RubyValue::Symbol(Symbol::intern("upcase"));
        let um = unbound_method_new(zeo_abi::STRING_CLASS, &upcase).unwrap();

        assert!(matches!(
            call(&um, "name", &[]),
            Ok(RubyValue::Symbol(s)) if s == Symbol::intern("upcase")
        ));
        assert!(matches!(
            call(&um, "owner", &[]),
            Ok(RubyValue::Class(c)) if c == zeo_abi::STRING_CLASS
        ));
    }

    #[test]
    fn bind_rebinds_and_bind_call_runs_in_one_step() {
        install_core();
        let upcase = RubyValue::Symbol(Symbol::intern("upcase"));
        let um = unbound_method_new(zeo_abi::STRING_CLASS, &upcase).unwrap();

        let bound = call(&um, "bind", &[rstr("hi")]).expect("bind succeeds");
        let out = call(&bound, "call", &[]).expect("call succeeds");
        assert!(matches!(&out, RubyValue::Str(s) if s.lock().to_utf8_lossy() == "HI"));

        let out = call(&um, "bind_call", &[rstr("ho")]).expect("bind_call succeeds");
        assert!(matches!(&out, RubyValue::Str(s) if s.lock().to_utf8_lossy() == "HO"));
    }

    #[test]
    fn bind_refuses_an_instance_of_another_class() {
        install_core();
        let upcase = RubyValue::Symbol(Symbol::intern("upcase"));
        let um = unbound_method_new(zeo_abi::STRING_CLASS, &upcase).unwrap();

        let err = call(&um, "bind", &[RubyValue::Int(5)]);
        let Err(Signal::Raise(exc)) = err else {
            panic!("expected a TypeError raise");
        };
        assert_eq!(
            exc.as_object_unchecked().class_id(),
            zeo_abi::TYPE_ERROR_CLASS
        );
    }

    #[test]
    fn an_unknown_name_refuses_at_construction() {
        install_core();
        let err = unbound_method_new(
            zeo_abi::STRING_CLASS,
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
}
