//! The value-channel dispatcher: `send_value_in_reason` and its wrappers
//! (`public_send`, vcall, the refinement entries), plus the visibility
//! miss protocol.

use super::*;

/// The general dispatcher -- reached only on Path 2 (see the `dispatch`
/// module docs).
/// Since every reachable method (own, inherited, or mixed-in) is already
/// MATERIALIZED directly onto its receiver's own class at zeo compile
/// time -- monomorphization, not cloning-with-shadow-names -- this is a
/// FLAT lookup on the receiver's
/// own class -- no ancestor walk needed here at all, only for `is_a`
/// (above), which real dispatch doesn't need.
///
/// `block` is threaded through to whichever `MethodFn` is actually found --
/// including the `method_missing` fallback, matching real Ruby's own
/// `method_missing(name, *args, &block)` protocol.
/// Dynamic dispatch against ANY `RubyValue` receiver (closing
/// the long-standing "Poly-dispatch gap"): an `Object` goes through `send`'s
/// registry exactly as before; a BUILT-IN receiver (Array/Hash/String/...)
/// dispatches against a curated method table over the existing collection
/// helpers -- what makes `@items << x`, `@hash[k] = v`, `ks.length` work on
/// ivars/params/any dynamically-typed value. Codegen's every Poly-receiver
/// fallback (ordinary calls, `send`/`public_send`, safe-nav, and the
/// runtime OPERATOR fallback's non-numeric case) routes here.
///
/// The table is deliberately curated, not exhaustive: entries exist for the
/// operations the static `try_collection_dispatch` fast path also supports
/// (extended as packages need them); anything else raises the same real,
/// rescuable `NoMethodError` an unknown Object method does -- with the
/// builtin class's REAL name in the message (better than `send`'s
/// documented class-id approximation, since builtin names are known here).
pub fn send_value(
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    send_value_in(0, recv, name, args, block)
}

/// `public_send`'s dispatch: [`send_value_in`] plus the visibility gate.
///
/// CRuby implements `public_send` as an ordinary send carrying the
/// `CALL_PUBLIC` scope, and `rb_method_call_status` (`vm_eval.c:837`) then
/// rejects private AND protected targets. Protected is rejected
/// unconditionally here -- normally it is receiver-sensitive, allowed when the
/// CALLER's `self` is a kind of the method's owner, but `public_send` passes
/// `Qundef` as that self (`vm_eval.c:1230`), a sentinel nothing can ever be a
/// kind of. Hence the plain `!= Public` test: no relatedness relaxation
/// applies, which is what makes `public_send` stricter than a plain
/// explicit-receiver call.
///
/// This is a RUNTIME check by necessity, not by preference: visibility can be
/// changed after the fact (`private :foo`), and the method name reaching
/// `public_send` is frequently a runtime value.
pub fn send_value_public_in(
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // A CLASS receiver's class methods have their own visibility table -- the
    // instance walk below reads Class/Module's, which says nothing about them.
    if let RubyValue::Class(cid) = recv {
        if class_method_is_private(*cid, name) {
            return missing_or_raise(recv, name, args, block, MissingReason::Private);
        }
        // A class-method row ANSWERS this call, so the instance walk below
        // must not veto it: that walk reads Class/Module's own ancestry,
        // Kernel included. `module_function` is exactly this shape -- the
        // name is a PRIVATE instance method of the module and a PUBLIC
        // method on its singleton, and `Kernel.public_send(:puts, ..)`
        // reaches the second.
        if class_method_owner(*cid, name).is_some() {
            return send_value_in(box_id, recv, name, args, block);
        }
    }
    // A row installed on THIS OBJECT answers before its class does, so its own
    // mark decides. `recv.class_id()` names the ordinary class and knows
    // nothing about it -- `obj.extend(SomeModuleFunction)` is the case that
    // shows it.
    let own = object_singleton_visibility(recv, name);
    let reason = match own.or_else(|| instance_method_visibility(recv.class_id(), name)) {
        Some(MethodVisibility::Private) => Some(MissingReason::Private),
        Some(MethodVisibility::Protected) => Some(MissingReason::Protected),
        _ => None,
    };
    match reason {
        Some(reason) => missing_or_raise(recv, name, args, block, reason),
        None => send_value_in(box_id, recv, name, args, block),
    }
}

/// [`send_value`] with the CALLER's box.
///
/// The box is the statically-known defining box every codegen
/// dynamic-dispatch site passes -- the AOT translation of CRuby's
/// `cme->def->box`, with no frame walk. Only the per-ancestor value-method
/// probe consumes it: a box's builtin patches resolve from that box's code,
/// and root patches resolve everywhere.
///
/// This crate's own internal callers, such as Enumerable driving `each` or
/// Comparable driving `<=>`, go through the box-0 wrapper instead. Builtins
/// run in ROOT, which faithfully reproduces CRuby's documented
/// builtins-call-builtins leak.
pub fn send_value_in(
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    send_value_in_reason(box_id, recv, name, args, block, MissingReason::NoEntry)
}

/// The holder module whose refined `name` answers for `recv`, or `None` to
/// fall through to ordinary dispatch. `candidates` is `(refined class,
/// holder, singleton)` in most-recently-activated-first order, decided at
/// compile time from the `using` scopes covering the call site; the only
/// runtime question left is whether the receiver is actually of the refined
/// class. A SINGLETON candidate (`refine Range.singleton_class`) refines
/// CLASS methods, so it matches a Class receiver descending from the target
/// where a plain one matches an instance of it.
pub fn refinement_home(
    recv: &RubyValue,
    name: Symbol,
    candidates: &[(ClassId, ClassId, bool)],
) -> Option<ClassId> {
    let cls = recv.class_id();
    candidates.iter().find_map(|&(target, holder, singleton)| {
        let applies = match singleton {
            false => is_a(cls, target),
            true => matches!(recv, RubyValue::Class(c) if is_a(*c, target)),
        };
        (applies && holder_defines(holder, name)).then_some(holder)
    })
}

/// Whether refinement holder `holder` supplies `name` -- its compiled rows, or
/// the ones a runtime `Refinement#import_methods` copied into the overlay.
fn holder_defines(holder: ClassId, name: Symbol) -> bool {
    value_method(holder, 0, name).is_some()
        || (crate::runtime_meta::is_live()
            && crate::runtime_meta::overlay_value_body(holder, name).is_some())
}

/// Run refinement holder `holder`'s `name` against `recv`, from whichever of
/// the two tables carries it.
fn call_refined(
    holder: ClassId,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Option<Result<RubyValue, Signal>> {
    if let Some(f) = value_method(holder, 0, name) {
        return Some(f.call(recv, args, block));
    }
    let body = crate::runtime_meta::overlay_value_body(holder, name)?;
    Some(crate::runtime_meta::call_value_body(
        holder, name, &body, recv, args, block,
    ))
}

/// Whether a USER-WRITTEN `method_missing` would answer for `recv`.
///
/// A BUILTIN row is not one. `BasicObject#method_missing` exists so an
/// override's `super` has something to reach, and `Exception#method_missing`
/// is a row of its own -- both raise for a name NOTHING defines, so handing a
/// VISIBILITY refusal to either answers "undefined method" where ruby says
/// "private method". Only a row the program wrote may intercept.
fn user_method_missing(recv: &RubyValue, mm: Symbol) -> bool {
    if crate::runtime_meta::is_live() && crate::runtime_meta::object_has_singleton_method(recv, mm)
    {
        return true;
    }
    let Some(owner) = method_owner(recv.class_id(), mm) else {
        return false;
    };
    // A CORE owner's row is a native one unless the program reopened the class
    // with its own. `class_table` alone cannot answer: a bootstrap builtin
    // (every Exception) keeps its native rows on the OBJECT channel, so
    // `Exception#method_missing` is invisible there -- and the exception ids
    // are not in `BUILTINS` either. `core_class` covers both tables.
    if !zeo_abi::is_core_class(owner) {
        return true;
    }
    crate::runtime_meta::is_live() && crate::runtime_meta::overlay_has_instance_method(owner, mm)
}

/// A refusal that must still go through the `method_missing` protocol.
///
/// CRuby refuses an explicit-receiver call to a private or protected method by
/// CALLING `method_missing` with that reason; its default body is what turns
/// the reason into "private method 'x' called for ...". An override sees the
/// call first, which is how a delegator forwards a name its target keeps
/// private. Building the error here instead would make the hook unreachable.
///
/// Every receiver shape, so the class-method side behaves like the instance
/// side. A receiver with no hook -- and the `method_missing` name itself, which
/// must not recurse -- falls through to the raise.
pub(crate) fn missing_or_raise(
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    reason: MissingReason,
) -> Result<RubyValue, Signal> {
    let mm = crate::symbol::wk::method_missing();
    if name != mm {
        match recv {
            // Only a USER hook. The root `BasicObject#method_missing` exists so
            // an override's `super` has something to reach, and its body raises
            // for a name NOTHING defines -- handing a visibility refusal to it
            // would answer "undefined method" where ruby says "private method".
            RubyValue::Object(o) if user_method_missing(recv, mm) => {
                return method_missing_or_raise(o, recv.class_id(), name, args, block, reason);
            }
            RubyValue::Class(cid) if class_defines_user_hook(*cid, mm) => {
                let mut full = Vec::with_capacity(args.len() + 1);
                full.push(RubyValue::Symbol(name));
                full.extend_from_slice(args);
                return send_class_chain(*cid, mm, &full, block);
            }
            _ => {}
        }
    }
    Err(raise_method_missing(recv, &name.to_string(), args, reason))
}

/// A call site the active refinements may answer. The refined body wins
/// outright when the receiver is of the refined class; otherwise this is an
/// ordinary send, which is what keeps an unrefined receiver -- and a name
/// no refinement defines for THIS receiver -- on its normal path.
pub fn refined_send_in(
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    candidates: &[(ClassId, ClassId, bool)],
    explicit: bool,
) -> Result<RubyValue, Signal> {
    if let Some(holder) = refinement_home(recv, name, candidates) {
        // A refined method is an ordinary method for visibility: `private def`
        // inside a `refine` block (or a private row `import_methods` copied in)
        // refuses an explicit receiver.
        if explicit && instance_method_visibility(holder, name) == Some(MethodVisibility::Private) {
            return missing_or_raise(recv, name, args, block, MissingReason::Private);
        }
        if let Some(r) = call_refined(holder, recv, name, args, block.clone()) {
            return r;
        }
    }
    send_value_in(box_id, recv, name, args, block)
}

/// `recv.send(name, ...)` / `recv.public_send(...)` at a site some `using`
/// covers. `name` is a runtime value here, so the whole active set rides
/// along and the match happens at the call.
pub fn refined_send_dynamic(
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    candidates: &[(ClassId, ClassId, bool)],
    public: bool,
) -> Result<RubyValue, Signal> {
    if let Some(r) = refinement_home(recv, name, candidates)
        .and_then(|h| call_refined(h, recv, name, args, block.clone()))
    {
        return r;
    }
    if public {
        send_value_public_in(box_id, recv, name, args, block)
    } else {
        send_value_in(box_id, recv, name, args, block)
    }
}

/// `recv.respond_to?(name)` at a site some `using` covers -- a refined name
/// answers `true` even though it appears in no `instance_methods` list.
pub fn refined_responds_to(
    recv: &RubyValue,
    name: Symbol,
    include_all: bool,
    candidates: &[(ClassId, ClassId, bool)],
) -> Result<bool, Signal> {
    if refinement_home(recv, name, candidates).is_some() {
        return Ok(true);
    }
    responds_to_or_missing(recv, name, include_all)
}

/// `recv.method(name)` at a site some `using` covers. A refined name binds
/// to the HOLDER, which is what makes `#owner` answer a `Refinement`.
pub fn refined_method(
    recv: &RubyValue,
    name: Symbol,
    candidates: &[(ClassId, ClassId, bool)],
) -> Result<RubyValue, Signal> {
    match refinement_home(recv, name, candidates) {
        Some(holder) => Ok(crate::builtins::method::method_value(
            recv.clone(),
            name,
            holder,
            crate::MethodKind::Instance,
        )),
        None => crate::builtins::method::method_new(recv, &RubyValue::Symbol(name)),
    }
}

/// A bareword VCALL (`foo` -- implicit self, no args, no parens, could have
/// been a local): a miss raises `NameError`, not `NoMethodError`, per Ruby.
/// Everything else (method_missing, alias re-dispatch) is identical to a
/// normal send; only the terminal "nothing matched" raise differs.
pub fn send_value_vcall_in(
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
) -> Result<RubyValue, Signal> {
    send_value_in_reason(box_id, recv, name, &[], None, MissingReason::VCall)
}

/// A send whose receiver is not an `Object` -- a class, a module, or a bare
/// value.
///
/// The wrapper exists for one reason: a CLASS-method body entered by ORDINARY
/// dispatch always lands on the FIRST position of the singleton chain, so it
/// must not inherit a resume an enclosing `super` walk published for a later
/// copy of the same module -- possibly on a different receiver's chain
/// entirely. The instance channel guards `send_in_reason` the same way.
pub(super) fn send_value_in_reason(
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    reason: MissingReason,
) -> Result<RubyValue, Signal> {
    match matches!(recv, RubyValue::Class(_)) {
        true => with_ordinary_class_dispatch(|| {
            send_value_in_reason_inner(box_id, recv, name, args, block, reason)
        }),
        false => send_value_in_reason_inner(box_id, recv, name, args, block, reason),
    }
}

/// [`send_value_in_reason`] past the resume guard.
fn send_value_in_reason_inner(
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    reason: MissingReason,
) -> Result<RubyValue, Signal> {
    // Recursion that never re-enters a compiled prologue (method_missing
    // self-forwarding, builtin-row cycles) still deepens the native stack --
    // check here as the prologues do.
    crate::stack_guard::stack_check()?;
    // The husk probe: EVERY send to a moved object -- `equal?`, `!`,
    // `__id__`, `class`, all of them -- raises `Ractor::MovedError`. A
    // container husk's class word cannot say so (Str/Array/Hash have none),
    // hence the flags probe; an object husk would resolve through its
    // retagged id anyway, this just makes the raise uniform.
    if crate::runtime_meta::any_moved() && value_moved(recv) {
        return Err(crate::ractor::moved_object_error());
    }
    if let RubyValue::Object(o) = recv {
        // The top-level self carries `include` as a PRIVATE SINGLETON in
        // CRuby (`ruby.c` installs it on `rb_vm_top_self`), where it mixes
        // into `Object`. zeo has no singleton on `main` --
        // installing one at startup would mark the overlay maps live for
        // every program -- so the routing is here instead, behind the
        // identity check that already exists for `to_s`.
        //
        // `include Mod` with a literal constant never reaches this: the
        // lowering turns that into a compile-time `HirNode::Include`. What
        // does reach it is a COMPUTED module -- `m = Module.new { ... };
        // include m` -- which is the shape `mkmf` ends on.
        // `main`'s class IS `Object` -- the id compare spares every typed
        // receiver the identity probe.
        if o.class_id() == zeo_abi::OBJECT_CLASS
            && is_main_object(o)
            && let Some(out) = main_mixin(name, args)
        {
            return out;
        }
        return send_in_reason(box_id, o, name, args, block, reason);
    }
    // A singleton method installed directly on this VALUE (`def SOME_ARRAY.[]`)
    // is closer than anything its class offers, so it is probed first -- and,
    // like every other overlay probe, only once something was defined at
    // runtime, so the ordinary path is untouched.
    if crate::runtime_meta::is_live()
        && let Some(m) = crate::runtime_meta::value_singleton_method(recv, name)
    {
        // Through the frame pusher, not `call_with_self_and_block` directly:
        // a `super` in the body needs a seat, and a bare value's singleton
        // has no `MethodImpl` wrapper to push one.
        return crate::runtime_meta::call_value_singleton(&m, recv, name, args, block);
    }
    note_dispatch(name);
    // The flat one-probe path below -- almost every send in almost every
    // program -- resolves on the `Symbol` alone, so the text is fetched only
    // where a by-NAME builtin table is actually consulted, never up front.
    // (`name_str` used to take the interner's global mutex, which made this
    // load-bearing; it is now two slab indexes, so the ordering is kept for
    // tidiness rather than for the lock.)
    // CLASS/MODULE-level methods (`File.read`, `Time.now`, `Math.sqrt`):
    // this runtime has no singleton-method tables, so a class value gets its
    // own table probed ahead of the walk. The walk itself describes INSTANCE
    // methods, and a `RubyValue::Class`'s own chain runs over Class/Module --
    // it would never reach File's or Math's rows.
    if let RubyValue::Class(cid) = recv {
        // Retired by an `undef` inside `class << self` -- checked before any
        // table, so an ancestor's still-live `def self.x` cannot answer past
        // it. See `OverlayEntry::class_undefs`.
        if crate::runtime_meta::is_live() && crate::runtime_meta::class_method_undefined(*cid, name)
        {
            return Err(raise_method_missing(recv, &name.to_string(), args, reason));
        }
        // A class/singleton method DEFINED AT RUNTIME
        // (`define_singleton_method` on a class, a runtime `def self.x`) wins
        // over both the frozen `def self.x` and the builtin `Class#new`/`#name`,
        // matching Ruby's "closest singleton" placement. Runs under the class
        // value itself. Only probed once something is defined at runtime.
        if crate::runtime_meta::is_live()
            && let Some(p) = crate::runtime_meta::overlay_class_method(*cid, name)
        {
            // The caller's block must ride along: `def M.wrap; yield; end`
            // desugars to a method-body lambda whose `yield` reads the
            // block the METHOD was called with. Dropping it here made
            // `M.wrap { "hi" }` raise LocalJumpError.
            //
            // Through `call_value_body` for the method FRAME: a body installed
            // at run time has no compile-time defining class, so a `super`
            // inside it reads the frame instead -- and without one, a
            // `Class.new { def self.x; super; end }` raised "super called
            // outside of method" where ruby answers.
            return crate::runtime_meta::call_value_body(*cid, name, &p, recv, args, block);
        }
        // A module prepended into an ANCESTOR's singleton class. A subclass's
        // singleton chain runs through its parent's, so the module sits ahead
        // of the parent's own `def self.x` -- which is what the flat probe
        // below would otherwise answer with.
        if crate::runtime_meta::is_live()
            && let Some(p) = crate::runtime_meta::inherited_singleton_prepend(*cid, name)
        {
            return p.call_with_self_and_block(recv, args, block);
        }
        // An ANCESTOR's class method as the overlay holds it NOW, probed
        // BEFORE the flat table below rather than after. Materialization
        // copies an inherited `def self.x` onto every descendant, so the flat
        // answer is a copy of the FINAL body -- and a parent's redefinition
        // TIMELINE was therefore lost for every subclass, which answered the
        // last body from the program's first line.
        //
        // Only for a name the receiver does not define ITSELF, and the walk
        // stops at the first ancestor that really defines one, so a nearer
        // `def self.x` still wins -- ruby's placement rule, unchanged.
        if crate::runtime_meta::is_live()
            && !crate::dispatch::class_method_defined_here(*cid, name)
            && let Some((anc, p)) = crate::runtime_meta::inherited_overlay_class_method(*cid, name)
        {
            return crate::runtime_meta::call_value_body(anc, name, &p, recv, args, block);
        }
        // A USER `def self.x` and the builtin Class/Module table, flattened
        // into one probe (user rows win -- real Ruby's placement rule: the
        // singleton method is strictly closer than one inherited from
        // Class/Module). A class with NO registry entry (a never-required
        // feature-gated ext) still gets the direct builtin probe below.
        // ...unless `remove_method` in `class << self` took the row back.
        // This probe is FLAT, so the receiver's row is still sitting in it;
        // skipping the position is what lets an ancestor's `def self.x`
        // answer, which is the whole difference from an `undef`.
        let class_removed_here = crate::runtime_meta::is_live()
            && crate::runtime_meta::overlay_class_removed(*cid, name);
        if !class_removed_here
            && let Some((f, label)) = REGISTRY.get().and_then(|r| r.flat_class_hit(*cid, name))
        {
            return with_c_frame(label, || f.call(recv, args, block));
        }
        // A module the class EXTENDED, whose row is its own INSTANCE method:
        // CRuby seats the module in the singleton ancestry, just past the
        // class's own rows, and the row takes a `&RubyValue` receiver -- so
        // the class value passes straight through as `self`.
        if let Some(owner) = class_method_extend_source(*cid, name) {
            // A body the module installed at RUN TIME -- a `def` inside a
            // `case` whose branch only the run time picks -- lives in the
            // overlay and in no registered row, so the row probe below finds
            // the OWNER and then no body at all. Probed first because the
            // overlay REPLACES within one owner, the rule the ancestor walk
            // further down already applies per ancestor.
            if crate::runtime_meta::is_live()
                && let Some(p) = crate::runtime_meta::overlay_value_body(owner, name)
            {
                return crate::runtime_meta::call_value_body(owner, name, &p, recv, args, block);
            }
            if let Some(f) = extended_class_method_body(owner, name) {
                return with_c_frame_ids(owner, name, '#', || {
                    f.call(recv, args, block)
                });
            }
        }
        // A MINTED struct/data class's OWN singleton methods (`Point.members`,
        // `Point[1, 2]`, and `Point.new` itself). CRuby defines these directly
        // on the new class's singleton -- `Struct.new(:a).method(:new).owner`
        // is `#<Class:#<Class:0x...>>`, not `#<Class:Struct>` -- precisely so
        // they shadow `Struct.new`'s member-list constructor.
        //
        // They must therefore be probed BEFORE the ancestor walk below. A
        // minted class has no registry entry, so that walk takes it, reaches
        // `Struct`, and answers `k.new(1, 2, 3)` with `Struct.new` -- "1 is not
        // a symbol nor a string". They are also not reached by the MRO walk
        // further down, which runs over the class VALUE's own ancestry
        // (Class/Module), not the struct's.
        if crate::builtins::rstruct::is_struct_class(*cid)
            && let Some(f) = crate::builtins::rstruct::class_lookup(name.name_str())
        {
            return f(recv, args, block);
        }
        // A class born at RUNTIME (`Class.new(Base)`, `class Sub < expr`) was
        // never seen by `mro::materialize_class_methods`, so nothing flattened
        // its ancestors' `def self.x` onto it -- and the probe above is FLAT,
        // with no walk of its own. Without this, `Class.new(Base).made` is a
        // NoMethodError however ordinary `made` is, and `class << self; undef
        // inherited; end` cannot even find a target to undefine.
        //
        // The instance side has had this insurance all along (`lookup_mro`);
        // this is its class-method twin. Gated on the overlay being live and on
        // the receiver having no frozen entry, so a compile-time class -- whose
        // rows ARE flattened -- pays nothing: the walk would only rediscover
        // what the flat probe just answered.
        //
        // Placed before the builtin `Class`/`Module` table below because that
        // is ruby's order: an ancestor's singleton sits nearer than `Class`.
        //
        // One known divergence stays: the ancestor's compiled body carries the
        // ANCESTOR's class id, so a class-level `@x` read through an inherited
        // class method reaches the ancestor's storage where ruby gives the
        // receiver its own. Materialization avoids that by emitting a copy per
        // subclass, which a runtime class has no compile-time site for.
        //
        // ...and never for a name the receiver's OWN constructor serves.
        // `new`/`allocate` are `Class`'s methods parameterized by the receiver,
        // not an ancestor's singleton method, but a builtin root publishes its
        // own `new` row -- so the walk read `Class.new(String).new("hi")` as
        // `String.new` and handed back a plain String, which then reported
        // `String` for `.class` and failed `is_a?(Tagged)`. `runtime_class_new`
        // had already chosen the right constructor for the minted class; this
        // just stops the walk from answering ahead of it.
        if crate::runtime_meta::is_live()
            && REGISTRY
                .get()
                .is_some_and(|r| !r.entries.contains_key(&cid.0))
            && !(matches!(name.name_str(), "new" | "allocate") && constructor_of(*cid).is_some())
        {
            // The first REGISTERED class ancestor's flattened row first, not
            // the owner's own: materialization re-resolves a class method's
            // self-sends per class, so the nearest ancestor's copy is the one
            // whose overrides the receiver inherits. Asking the OWNER's row
            // (as reflection rightly does) ran `Class.new(Spec).run_suite`
            // with `Runnable`'s viewpoint, whose `runnable_methods` raises
            // "subclass responsibility" -- `Test`'s override never dispatched.
            let flattened = ancestors_of_value(*cid).iter().skip(1).find_map(|&anc| {
                let e = registry().entries.get(&anc.0)?;
                match e.is_module {
                    true => None,
                    false => e.class_methods.get(&name).copied().map(|f| (anc, f)),
                }
            });
            if let Some((anc, f)) = flattened {
                return with_c_frame_ids(anc, name, '.', || f.call(recv, args, block));
            }
            if let Some(f) = class_method_fn(*cid, name) {
                return with_c_frame_ids(*cid, name, '.', || f.call(recv, args, block));
            }
        }
        if let Some(lookup) = crate::builtins::class_method_table(*cid)
            && let Some(f) = lookup(name.name_str())
        {
            return with_c_frame_ids(*cid, name, '.', || f(recv, args, block));
        }
        // The same table on a VALUE SUBCLASS's payload ROOT -- `DOSTime.local`,
        // `IOBuffer.open`. Nothing copies a builtin's class-method rows onto a
        // subclass entry the way materialization copies a user `def self.x`, so
        // this probe is the only place they can be found. It has to sit HERE
        // rather than in the miss tail: `B.open` must reach `StringIO.open`
        // before the MRO walk below finds `Kernel#open`, which is exactly
        // ruby's order (`#<Class:B>` is nearer than `Kernel`). The result comes
        // back re-tagged as the subclass, because CRuby allocates through the
        // receiver class. See `value_subclass::root_class_method_target`.
        {
            let n = name.name_str();
            if let Some(root) = crate::builtins::value_subclass::root_class_method_target(*cid, n) {
                return crate::builtins::value_subclass::call_root_class_method(
                    *cid, root, n, args, block,
                );
            }
            // ...and the RECEIVER-HONOURING roots, where the row allocates
            // through the receiver itself (`Date`'s do). The receiver passes
            // through unchanged and the result needs no re-tagging -- the row
            // already did it. See `value_subclass::recv_honouring_root`.
            if let Some(root) = crate::builtins::value_subclass::recv_honouring_root(*cid)
                && let Some(lookup) = crate::builtins::class_method_table(root)
                && let Some(f) = lookup(n)
                && !crate::builtins::builtin_class_method_is_private(root, n)
            {
                return with_c_frame_ids(root, name, '.', || f(recv, args, block));
            }
        }
        // An ANCESTOR's runtime class method -- what a `Base.extend Store` or a
        // `define_singleton_method` on a superclass installs. A subclass's
        // singleton class inherits its parent's in ruby, so `Sub.tag` answers;
        // zeo probed the receiver's own overlay alone and stopped. Last, so
        // nothing that already resolves changes order: a frozen `def self.x`
        // nearer the receiver still wins, which is ruby's placement rule too.
        // The same probe WITHOUT the own-row gate the early one carries, for
        // a receiver whose flat table never held the name at all (a class born
        // at run time). The ANCESTOR is the defining class and is pushed as
        // the method frame: an overlay body has no compile-time defining class
        // of its own, so a `super` in it reads the frame.
        if crate::runtime_meta::is_live()
            && let Some((anc, p)) = crate::runtime_meta::inherited_overlay_class_method(*cid, name)
        {
            return crate::runtime_meta::call_value_body(anc, name, &p, recv, args, block);
        }
    }
    // THE MRO WALK -- the receiver's real ancestor chain, most
    // derived first. Per ancestor: user reopens (the value
    // methods) beat that ancestor's builtin table -- real Ruby's placement
    // rule (a `class Numeric; def foo` reopen is found on `5`, but a
    // builtin `Integer#foo` row would beat it). Everything -- including
    // the Kernel universals, BasicObject's `==`, and the
    // `Enumerable`/`Comparable` module tables (whose rows drive the
    // receiver's own `each`/`<=>`) -- is an ordinary `class_table` hit.
    //
    // Box 0 with a dormant overlay -- almost every send in almost every
    // program -- takes the flattened one-probe form of the same walk
    // (`flat_value_hit`); a per-box patch or live overlay keeps the full
    // walk, whose per-ancestor `(box, name)` probe it needs.
    let cid = recv.class_id();
    if box_id == 0 && !crate::runtime_meta::is_live() {
        if let Some(hit) = REGISTRY.get().and_then(|r| r.flat_value_hit(cid, name)) {
            if let Some(hit) = hit {
                return with_c_frame(hit.frame_label, || hit.f.call(recv, args, block));
            }
            // A genuine flat miss: fall through to the alias tail below.
        } else {
            let n = name.name_str();
            for &anc in ancestors_of_value(cid) {
                if let Some(f) = value_method(anc, box_id, name) {
                    return f.call(recv, args, block);
                }
                if let Some(table) = crate::builtins::class_table(anc)
                    && let Some(f) = table(n)
                {
                    return with_c_frame_ids(anc, name, '#', || f(recv, args, block));
                }
            }
        }
    } else {
        // `Math`'s module functions reach here too when `Math` is mixed in
        // (`include Math` -> a private `sqrt(x)`), as an ordinary `class_table`
        // hit on its registered instance table -- no special arm needed.
        let live = crate::runtime_meta::is_live();
        let n = name.name_str();
        for &anc in ancestors_of_value(cid) {
            // An `undef` at this position TERMINATES the walk, before any
            // table -- including this ancestor's NATIVE one. A builtin's
            // `class_table` is a static list, so the compile-time registry
            // entry and the runtime overlay tombstone are the ONLY records
            // that a native row was retired. `respond_to?`/`method_defined?`
            // already read both, so without this the two disagreed: the
            // predicate said no and the call still answered.
            if (live && crate::runtime_meta::overlay_is_undefined(anc, name))
                || REGISTRY.get().is_some_and(|r| r.is_undefined(anc, name))
            {
                break;
            }
            // `remove_method` empties the position rather than ending the
            // walk -- an ancestor's definition is meant to answer now.
            if live && crate::runtime_meta::overlay_is_removed(anc, name) {
                continue;
            }
            // A runtime `define_method` REPLACES, so within one ancestor the
            // overlay outranks both the reopen and the builtin table -- the
            // placement `send_in_reason` gives an object receiver through
            // `resolve_dynamic`. PER ancestor, not ahead of the whole walk:
            // `Enumerable.define_method(:map)` must not beat `Array#map`.
            if live && let Some(p) = crate::runtime_meta::overlay_value_body(anc, name) {
                return crate::runtime_meta::call_value_body(anc, name, &p, recv, args, block);
            }
            if let Some(f) = value_method(anc, box_id, name) {
                return f.call(recv, args, block);
            }
            if let Some(table) = crate::builtins::class_table(anc)
                && let Some(f) = table(n)
            {
                return with_c_frame_ids(anc, name, '#', || f(recv, args, block));
            }
        }
    }
    // A CLASS-method alias row (`class << self; alias [] new`): the same
    // rewrite for a class-object receiver, whose `class_id()` is `Class` and so
    // would never find the row on the class itself. First, because a class
    // receiver's own singleton table is what `[]` should mean here.
    if let RubyValue::Class(cid) = recv
        && let Some(old) = class_alias_target(*cid, name)
    {
        if let Some(f) = builtin_class_row(*cid, old) {
            return with_c_frame_ids(*cid, old, '.', || f(recv, args, block));
        }
        return send_value_in_reason(box_id, recv, old, args, block, reason);
    }
    // A builtin-alias row (`alias_method :dup!, :dup`), after every real
    // method missed. See `builtin_row` for why it binds the body, not the name.
    if let Some(old) = alias_target(recv.class_id(), name) {
        let cid = recv.class_id();
        if let Some(f) = builtin_row(cid, old) {
            return with_c_frame_ids(cid, old, '#', || f(recv, args, block));
        }
        return send_value_in_reason(box_id, recv, old, args, block, reason);
    }
    // A CLASS-level `method_missing` (`def self.method_missing`, or one in
    // `class << self`) catches a missing CLASS method, exactly as an
    // instance's hook catches an instance miss (`method_missing_or_raise`)
    // -- the whole public API of Faker-style gems. Guarded on the hook
    // actually being user-defined so the plain miss below stays one raise,
    // and on the missing name not being `method_missing` itself.
    if let RubyValue::Class(cid) = recv {
        let mm = crate::symbol::wk::method_missing();
        if name != mm && class_defines_user_hook(*cid, mm) {
            let mut full_args = Vec::with_capacity(args.len() + 1);
            full_args.push(RubyValue::Symbol(name));
            full_args.extend_from_slice(args);
            return send_class_chain(*cid, mm, &full_args, block);
        }
    }
    // Every receiver shape -- class, module, immediate, object -- gets its
    // message from the one method-missing raiser, so the class/module form
    // ("for class Widget") and the instance form stay in step.
    Err(raise_method_missing(recv, &name.to_string(), args, reason))
}

/// Whether any ancestor supplies a USER-DEFINED class-level `name` -- a
/// runtime overlay row (defs, singleton prepends) or a registered flattened
/// class-method row. Builtin class-method tables are deliberately not
/// consulted: this probes user HOOKS (`method_missing`,
/// `respond_to_missing?`), which no builtin defines as a class method.
/// Module ancestors past the receiver itself contribute nothing to a
/// singleton chain and are skipped, as `singleton_walk` skips them.
pub(super) fn class_defines_user_hook(recv_class: ClassId, name: Symbol) -> bool {
    for &anc in ancestors_of_value(recv_class) {
        let entry = registry().entries.get(&anc.0);
        if anc != recv_class && entry.is_some_and(|e| e.is_module) {
            continue;
        }
        if crate::runtime_meta::is_live()
            && crate::runtime_meta::overlay_class_method(anc, name).is_some()
        {
            return true;
        }
        if entry.is_some_and(|e| e.class_methods.contains_key(&name)) {
            return true;
        }
    }
    false
}
