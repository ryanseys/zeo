//! Method-name coercion and the lookup probes: the Kernel reflection
//! entries (`reflect_dispatch_in`), the value-method probe, the cloned
//! registry lookups, and `class_name`.

use super::*;

/// Coerces a dynamic method-name value the way `send`/`__send__` do:
/// Symbol or String (real Ruby accepts both), anything else raising
/// CRuby's exact TypeError shape.
pub fn method_name_symbol(v: &RubyValue) -> Result<Symbol, Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(*s),
        RubyValue::Str(s) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            other.inspect_string()
        )),
    }
}

/// The Kernel entry a [`reflect_dispatch_in`] site was written as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reflect {
    Send,
    PublicSend,
    RespondTo,
    Method,
}

impl Reflect {
    /// The name as WRITTEN, which is both what the MRO is asked about and
    /// what an ordinary call dispatches when the answer is "shadowed".
    fn symbol(self) -> Symbol {
        match self {
            Reflect::Send => crate::symbol::wk::send(),
            Reflect::PublicSend => crate::symbol::wk::public_send(),
            Reflect::RespondTo => crate::symbol::wk::respond_to(),
            Reflect::Method => crate::symbol::wk::method(),
        }
    }
}

/// `recv.send(name, …)` / `#respond_to?` / `#method` where the compiler could
/// not prove WHICH of them the receiver's chain resolves to.
///
/// CRuby has no intrinsic for any of the three: each is an ordinary method on
/// `Kernel`, so a class defining its own -- `BasicSocket#send` writing bytes,
/// `Ractor#send` passing a message, any `def respond_to?` of your own --
/// simply wins the lookup because it sits earlier in the MRO. Only when the
/// lookup lands on Kernel's does the first argument get reinterpreted as a
/// method name. This asks that question once, then does whichever the answer
/// calls for.
///
/// `candidates` are the refinements active at the site, empty where there are
/// none. All three entries honour a refinement in real Ruby, and a receiver
/// whose class is only known at run time is exactly where the compiler cannot
/// fold that away.
pub fn reflect_dispatch_in(
    box_id: u32,
    recv: &RubyValue,
    entry: Reflect,
    args: &[RubyValue],
    block: Option<RubyValue>,
    candidates: &[(ClassId, ClassId, bool)],
) -> Result<RubyValue, Signal> {
    let written = entry.symbol();
    let shadowed = !matches!(
        method_owner(recv.class_id(), written),
        Some(KERNEL_CLASS) | Some(BASIC_OBJECT_CLASS) | None
    );
    if shadowed {
        return send_value_in(box_id, recv, written, args, block);
    }
    let Some((target, rest)) = args.split_first() else {
        return Err(match entry {
            Reflect::Send | Reflect::PublicSend => {
                crate::builtins::arg_error!("no method name given")
            }
            _ => crate::builtins::arg_error!("wrong number of arguments (given 0, expected 1)"),
        });
    };
    let target = method_name_symbol(target)?;
    // `public_send` re-wraps the block as a PROC handler where `send`
    // forwards the iseq handler unchanged -- a CRuby wart, and the only
    // thing that makes the two disagree about the same block
    // (`Kernel.send(:lambda) { }` is accepted, `public_send` is not).
    if entry == Reflect::PublicSend
        && let Some(RubyValue::Proc(p)) = &block
    {
        p.clear_literal_block();
    }
    match entry {
        Reflect::Send | Reflect::PublicSend => refined_send_dynamic(
            box_id,
            recv,
            target,
            rest,
            block,
            candidates,
            entry == Reflect::PublicSend,
        ),
        Reflect::RespondTo => Ok(RubyValue::Bool(refined_responds_to(
            recv,
            target,
            rest.first().is_some_and(|v| v.truthy()),
            candidates,
        )?)),
        Reflect::Method => refined_method(recv, target, candidates),
    }
}

/// A registry-OPTIONAL probe for a builtin-reopen method --
/// `None` when no registry is installed (this crate's own unit tests) or
/// the class carries no such method. The lookup key is the receiver's own
/// `class_id()`; see `ValueMethodFn`'s docs for why no ancestor walk is
/// needed. Used by `send_value`'s override-first stage and by
/// `display_with`/`inspect_with`'s `to_s`/`inspect` probes in `value.rs`.
pub(crate) fn value_method(id: ClassId, box_id: u32, name: Symbol) -> Option<ValueImpl> {
    REGISTRY.get()?.lookup_value_method(id, box_id, name)
}

/// Whether a NON-NATIVE `name` wins the lookup for `id` -- a program's reopen
/// of a builtin, rather than the class's own Rust row.
///
/// `Class#new` asks this of `initialize` to decide whether to unfuse into
/// `allocate` plus `initialize`. The answer must be the same one dispatch
/// would give, so the walk is the walk: most-derived first, and the FIRST
/// ancestor owning the name decides. A native row found before any reopen
/// means the constructor still owns construction.
///
/// Both channels are read, because a reopen can arrive either way -- a
/// compile-time `class Pathname; def initialize` registers a value row, and a
/// runtime `define_method` lands in the overlay.
/// Whether a builtin's own `new` row must GIVE WAY to `Class#new`.
///
/// The row FUSES allocate and initialize, so it cannot run a body it does not
/// know about. Once a program reopens the class with its own `initialize`,
/// ruby's `new` is allocate plus THAT and the native construction is gone --
/// which is why `String.new("x")` answers `""` from then on. `Class#new`
/// unfuses; this is what stops the fused row from answering ahead of it.
///
/// A `def self.new` the program wrote at run time is a different thing and
/// still wins.
pub(crate) fn builtin_new_gave_way(id: ClassId, name: Symbol) -> bool {
    name.name_str() == "new"
        && crate::builtins::class_method_table(id).is_some_and(|t| t("new").is_some())
        && crate::builtins::rclass::can_builtin_allocate(id)
        && !(crate::runtime_meta::is_live()
            && crate::runtime_meta::overlay_class_method(id, name).is_some())
        && reopened_initialize_in_chain(id, crate::symbol::wk::initialize())
}

pub(crate) fn reopened_initialize_in_chain(id: ClassId, name: Symbol) -> bool {
    let live = crate::runtime_meta::is_live();
    for &anc in ancestors_of_value(id) {
        if live {
            if crate::runtime_meta::overlay_is_undefined(anc, name) {
                return false;
            }
            if crate::runtime_meta::overlay_is_removed(anc, name) {
                continue;
            }
            if crate::runtime_meta::overlay_own_method(anc, name).is_some() {
                return true;
            }
        }
        // The VALUE channel only. A compile-time reopen of a non-bootstrap
        // builtin registers here, and nothing native does.
        //
        // The object channel (`registry().lookup`) cannot be asked: a
        // BOOTSTRAP builtin keeps its native rows there too, so every
        // `RuntimeError.new` read as a reopen and lost its own constructor.
        // Exceptions reopen through that same channel as deltas, so an
        // exception's `initialize` reopen is a separate question with a
        // separate answer -- `exception_construct` owns it.
        if value_method(anc, 0, name).is_some() {
            return true;
        }
        // The class's OWN native row, reached before any reopen: the
        // constructor keeps construction.
        if crate::builtins::side_of(anc, crate::builtins::Side::Instance)
            .is_some_and(|t| (t.lookup)(name.name_str()).is_some())
        {
            return false;
        }
    }
    false
}

/// The frozen registry's own instance method for `id`, CLONED out (a `fn`
/// copy or an `Arc` bump). The runtime overlay (`runtime_meta`) uses this to
/// walk a runtime class's frozen ancestors -- a `RuntimeClass < SomeUserClass`
/// instance inherits `SomeUserClass`'s materialized methods, which live in the
/// frozen table and can't be reached by the overlay's own maps. `None` when no
/// registry is installed (this crate's unit tests) or the class has no such
/// method.
pub(crate) fn registry_lookup_cloned(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    REGISTRY.get()?.lookup(id, name).cloned()
}

/// [`registry_lookup_cloned`]'s own-implementation twin: what `id` itself
/// DEFINES, rather than what it answers.
///
/// A compile-time MODULE's `methods` table is emptied once materialization has
/// copied its bodies onto every includer, so the flat lookup above finds
/// nothing there. That is fine for a compile-time includer, which got a copy --
/// and wrong for a class that includes or prepends the module at RUNTIME, which
/// did not. Such a class's walk reached the module's position, found an empty
/// table, and carried on to the class itself: a prepended module correctly
/// listed FIRST in `ancestors` yet never reached by dispatch.
///
/// `own_impls` is where those bodies survive -- it is the set `super` already
/// walks for exactly this reason.
pub(crate) fn registry_own_impl_cloned(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    REGISTRY.get()?.super_target(id, name).cloned()
}

/// A module/class's OWN value-method (the `ValueMethodFn` shape builtin modules
/// -- `Comparable`/`Enumerable`/`Kernel` -- register their instance methods
/// as), wrapped as a `MethodImpl` so `Object#extend` can copy it into an
/// object's singleton table. Read for the RUNNING box: a module compiled
/// inside a `Ruby::Box` registers its rows under that box's id.
pub(crate) fn registry_value_method_impl(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    let f = REGISTRY.get()?.lookup_value_method(id, crate::boxes::current_box(), name)?;
    Some(value_fn_impl(f))
}

/// An ancestor's NATIVE builtin row (`Struct#size`, `Comparable#between?`) as
/// an object-channel `MethodImpl` -- the last probe at each position of a
/// runtime class's walk, mirroring the step `send_value_in`'s own walk makes
/// after the value methods.
///
/// Without it a native row was invisible to a class born at runtime, and the
/// walk carried on to a FARTHER ancestor: `class T < Struct.new(:b)` with a
/// `module Enumerable; def size` reopen answered `Enumerable#size`, stepping
/// over `Struct#size` two positions nearer.
pub(crate) fn builtin_row_impl(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    // The root `BasicObject#method_missing` is not an answer. It exists so a
    // USER override's `super` can reach it, and `super` gets there through its
    // own walk; resolving it HERE means nothing overrode `method_missing`, and
    // the miss path would then raise through a row that carries its own
    // generic reason -- losing the VCall/private/protected distinction the
    // caller was given, and turning `NameError: undefined local variable or
    // method` back into a plain `NoMethodError`.
    if id == zeo_abi::BASIC_OBJECT_CLASS && name == crate::symbol::wk::method_missing() {
        return None;
    }
    let f = crate::builtins::class_table(id)?(name.name_str())?;
    Some(MethodImpl::Dynamic(std::sync::Arc::new(
        move |recv: &RObj, args: &[RubyValue], block: Option<RubyValue>| {
            // The same payload bridge the main walk runs: a value-subclass
            // instance (`Class.new(Array)`) carries a `RubyValue::Array`
            // payload, and at its payload root the builtin row runs against
            // that value -- handing it the boxed object instead panicked the
            // row's own receiver downcast.
            if let Some(root) = recv.builtin_root()
                && crate::builtins::value_subclass::payload_owns(root, id)
                && let Some(p) = recv.builtin_payload()
            {
                if let Some(k) = crate::builtins::value_subclass::wrapper_row(name) {
                    return k(&RubyValue::Object(recv.clone()), args, block);
                }
                let result = f(&p, args, block)?;
                return Ok(crate::builtins::value_subclass::rewrap_self_return(
                    result,
                    &p,
                    recv,
                    name.name_str(),
                ));
            }
            f(&RubyValue::Object(recv.clone()), args, block)
        },
    )))
}

pub(super) fn value_fn_impl(f: ValueImpl) -> MethodImpl {
    MethodImpl::Dynamic(std::sync::Arc::new(
        move |recv: &RObj, args: &[RubyValue], block: Option<RubyValue>| {
            f.call(&RubyValue::Object(recv.clone()), args, block)
        },
    ))
}

/// The registered Ruby-visible (fully-qualified) name of `id` -- `None`
/// when the registry isn't installed yet or has no such entry (this
/// crate's own unit tests; a real generated program registers everything
/// before any statement runs). See `RubyValue::Class`'s display arms for
/// the fallback rendering.
pub fn class_name(id: ClassId) -> Option<String> {
    // A `refine` holder renders as ruby renders it, from the pair it refines
    // -- not as the unwritable `#refinement:String` the compiler keys it by.
    // Still leading-`#`, so `Module#name` keeps answering nil.
    if let Some((module, target)) = refinement_of(id) {
        return Some(format!(
            "#<refinement:{}@{}>",
            class_name(target).unwrap_or_default(),
            class_name(module).unwrap_or_default(),
        ));
    }
    if let Some(name) = REGISTRY
        .get()
        .and_then(|r| r.entries.get(&id.0))
        .map(|e| e.name.clone())
    {
        return Some(name);
    }
    if crate::runtime_meta::is_live() {
        return crate::runtime_meta::overlay_class_name(id);
    }
    None
}

/// The name `Module#name` reports: `None` unless the class is really reachable
/// by a CONSTANT PATH. [`class_name`] always answers something printable, so an
/// anonymous or singleton class gets a `#<Class:...>` rendering there -- which
/// is exactly what tells the two apart, since no constant path begins with `#`.
pub fn class_real_name(id: ClassId) -> Option<String> {
    class_name(id).filter(|n| !n.starts_with('#'))
}
