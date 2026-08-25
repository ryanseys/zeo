//! The inline caches: `CallSite`/`ClassMethodSite`/`DynCallerSite` (one
//! static slot per emitted call site), what they remember (`Cached`), the
//! cached send entries, and the explicit-receiver visibility barrier
//! (`Vet`/`FCALL`). Track 1 adds the `CValue`/`CObj` arms here.

use super::*;

/// What one call site remembered: the receiver class, and the method that
/// class resolved to in whichever of the two dispatch shapes served it.
#[derive(Clone, Copy)]
enum Cached {
    /// A generated object -- resolved through the registry's own method table.
    Obj(MethodFn),
    /// A Cranelift-compiled object method (`MethodImpl::CValue`) -- the
    /// receiver is already the `&RubyValue` the C body borrows.
    CObj(crate::capi::ValueFn),
    /// A builtin value (`Int`, `Str`, `Array`, ...) -- resolved through the
    /// flattened one-probe walk, with the row's synthetic-frame label so a
    /// cached hit shows the same backtrace as the uncached resolution.
    Value(ValueImpl, Option<&'static str>),
}

/// One dynamic call site's monomorphic inline cache.
///
/// Neither resolution shape is walking anything on the hot path -- `lookup_mro`
/// finds the method on the receiver's own class because materialization put it
/// there, and `flat_value_hit` is a prebuilt map -- but both are two hash
/// probes per call, and that was a third of `bm_rbtree`. A site that keeps
/// seeing one receiver class remembers the answer and compares a `u32`.
///
/// Filled ONCE and never replaced. A site that sees a second class simply
/// misses forever and pays what it paid before, which is the trade that keeps
/// this a plain `OnceLock` -- no tearing to reason about, no `unsafe`, and no
/// write traffic on the hot path.
/// Two plain function pointers and a class id, so one site costs 24 bytes of
/// BSS. A `MethodImpl::Dynamic` -- the `Arc<dyn Fn>` a user MODULE's bridged
/// method registers as -- is deliberately not cacheable: storing one would put
/// a fat pointer in every site in the program to serve a minority of them.
pub struct CallSite {
    hit: std::sync::OnceLock<(u32, Cached)>,
    /// The class whose body this site sits in -- [`FCALL`] where ruby asks no
    /// visibility question at all. It rides in the site rather than in a call
    /// argument because it is a per-site CONSTANT, and because the hot path
    /// never reads it: a filled site was vetted when it filled.
    caller_class: u32,
}

impl Default for CallSite {
    fn default() -> Self {
        Self::new(FCALL)
    }
}

impl CallSite {
    pub const fn new(caller_class: u32) -> CallSite {
        CallSite {
            hit: std::sync::OnceLock::new(),
            caller_class,
        }
    }
}

/// [`send_value_in`] with a call-site cache in front of it.
///
/// The cache is consulted only where resolution is a pure function of the
/// receiver's CLASS: box 0 with a dormant overlay, the same gate
/// `flat_value_hit` already uses. Once anything is defined at runtime -- a
/// singleton, a `define_method`, a spliced ancestry, a tombstone -- `is_live`
/// turns it off wholesale, so no invalidation edge has to be maintained.
///
/// Two receiver shapes never cache. `Object` the CLASS, because `ENV` is an
/// ordinary object whose class IS `Object` and its own methods are probed by
/// identity ahead of the registry, so a class-keyed answer would serve
/// `ENV.dup` the wrong body. And a `Class` receiver, whose class methods and
/// reflection set are resolved by an arm of their own further down.
///
/// NOT `#[inline]`: a generated program has tens of thousands of dynamic call
/// sites, and inlining this body into each cost uri 153% more rustc time for
/// no runtime gain -- the work it saves is two hash probes, not a call.
/// The site's own `caller_class` carries the visibility question (see
/// [`explicit_call_barrier`]). It is asked only where the cache does NOT
/// answer: a filled site was already vetted for that receiver class when it
/// filled, and visibility is a function of the same `(receiver class, name)`
/// the cache is keyed by -- so a hit reads neither the field nor the tables.
pub fn send_value_cached(
    site: &'static CallSite,
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // A cache HIT calls the target without ever reaching
    // `send_value_in_reason`, which is where the dynamic entry's stack
    // check lives -- so the check has to happen here too, or a recursion
    // whose only carrier is a cached send runs the NATIVE stack out and
    // aborts the process instead of raising a rescuable SystemStackError.
    // One TLS read and a compare, beside an atomic load this path already
    // pays.
    crate::stack_guard::stack_check()?;
    // ONE gate-byte load serves both questions this path asks -- the same
    // single atomic load `is_live()` always cost. The moved branch is
    // never taken until a `move: true` send poisons something; from then on
    // a husk receiver raises here, BEFORE the cache can serve a container
    // husk its old class's method (its class id never changed).
    let gates = crate::runtime_meta::gates();
    if crate::runtime_meta::gates_moved(gates) && value_moved(recv) {
        return Err(crate::ractor::moved_object_error());
    }
    // A `Class` receiver is ruled out FIRST, before the class id is computed
    // or the cache is even read: it resolves through a class-method arm of its
    // own further down, so a site that only ever sees one (`Math.sin`) would
    // otherwise pay the lookup on every call and never fill.
    // The live gates also cover a running definition hook, whose class is only
    // part-built: a filled cache line would answer for a method that does not
    // exist yet. Sending the whole call down the slow route while a hook runs
    // puts that question where it is already asked (`send_in_reason`).
    if !matches!(recv, RubyValue::Class(_))
        && box_id == 0
        && !crate::runtime_meta::gates_live(gates)
    {
        let id = recv.class_id();
        if let Some((cached, target)) = site.hit.get() {
            if *cached == id.0 {
                note_dispatch_gated(gates, name);
                return match (target, recv) {
                    (Cached::Obj(f), RubyValue::Object(o)) => f(o, args, block),
                    (Cached::CObj(f), RubyValue::Object(_)) => {
                        crate::capi::dispatch::call_value_fn(*f, recv, args, block)
                    }
                    (Cached::Value(f, label), _) => {
                        with_c_frame(*label, || f.call(recv, args, block))
                    }
                    // A class id cannot be both shapes, so this is unreachable
                    // in practice; falling through is still the right answer.
                    _ => send_value_in(box_id, recv, name, args, block),
                };
            }
        } else {
            // Empty. Resolve once and remember, BEFORE the call, so a
            // recursive method hits its own site on the way down rather than
            // only after the outermost frame returns -- which is the shape
            // `bm_rbtree` and `bm_splay` actually have. The site is vetted
            // before it fills, so every later hit on it is vetted too.
            if let Some(reason) = explicit_call_barrier(recv, name, site.caller_class) {
                return missing_or_raise(recv, name, args, block, reason);
            }
            match recv {
                RubyValue::Object(o) if id != zeo_abi::OBJECT_CLASS => {
                    match REGISTRY.get().and_then(|r| r.lookup_mro(id, name)) {
                        Some(MethodImpl::Static(f)) => {
                            note_dispatch_gated(gates, name);
                            let _ = site.hit.set((id.0, Cached::Obj(*f)));
                            return f(o, args, block);
                        }
                        // A CValue hit that only fell through would make
                        // every compiled-object call uncached.
                        Some(MethodImpl::CValue(f)) => {
                            note_dispatch_gated(gates, name);
                            let _ = site.hit.set((id.0, Cached::CObj(*f)));
                            return crate::capi::dispatch::call_value_fn(*f, recv, args, block);
                        }
                        Some(MethodImpl::Dynamic(_)) | None => {}
                    }
                }
                RubyValue::Object(_) => {}
                _ => {
                    if let Some(Some(hit)) = REGISTRY.get().and_then(|r| r.flat_value_hit(id, name))
                    {
                        note_dispatch_gated(gates, name);
                        let _ = site.hit.set((id.0, Cached::Value(hit.f, hit.frame_label)));
                        return with_c_frame(hit.frame_label, || hit.f.call(recv, args, block));
                    }
                }
            }
        }
    }
    // Every route the cache did not serve -- a `Class` receiver, a box, a live
    // overlay, a site that has already seen another class -- still has to ask.
    send_value_explicit_in(box_id, recv, name, args, block, site.caller_class)
}

/// One CLASS-method call site's monomorphic inline cache -- `Math.sin(x)`,
/// `Time.now`, `File.read(p)`.
///
/// [`CallSite`] rules a `RubyValue::Class` receiver out before it even reads
/// its cache, because a class value's methods resolve through an arm of their
/// own; the note there names this exact case. So a site that only ever sees
/// `Math` paid the full class-method lookup on every call and never filled --
/// `bm_partial_sums` runs 2.5M iterations of three such sends.
///
/// The receiver class is a COMPILE-TIME constant here (the type is
/// `TyKind::ClassObj(cid)`), so the cache needs no class key on the hot path;
/// the emitted `cid` is compared against the receiver anyway, which costs one
/// `u32` and makes a wrong static type a miss rather than a wrong answer.
/// The caller class is cached alongside, so a shared body -- whose caller
/// varies per call -- misses instead of skipping a visibility question that
/// was answered for somebody else.
/// A site whose flat probe MISSES remembers that too. `Foo.new` on a
/// compiled class is the shape that forced it: `new` is served by the
/// class's constructor rather than by a class-method row, so the probe
/// never hits, and an unfillable site otherwise re-ran the visibility
/// barrier and the failed probe on every single call -- strictly more work
/// than the uncached route it was meant to replace. A remembered miss goes
/// straight to [`send_value_in`], which is LESS work than the uncached
/// route: the barrier was answered when the site filled, and its answer is
/// a function of the same `(receiver class, name, caller)` the key is.
/// What one site remembers: the receiver class it filled for, and either the
/// resolved row and its owner, or `None` for a remembered MISS.
type ClassMethodHit = (u32, Option<(ValueImpl, Option<&'static str>)>);

pub struct ClassMethodSite {
    hit: std::sync::OnceLock<ClassMethodHit>,
}

impl Default for ClassMethodSite {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassMethodSite {
    pub const fn new() -> ClassMethodSite {
        ClassMethodSite {
            hit: std::sync::OnceLock::new(),
        }
    }
}

/// [`send_value_explicit_in`] with a class-method cache in front of it, for a
/// receiver whose class `cid` is statically known.
///
/// Fills from [`Registry::flat_class_hit`] ONLY. Under `!gates_live` every
/// probe that precedes it in [`send_value_in_reason`] -- the `class << self`
/// undef set, a runtime `define_singleton_method`, a value singleton -- is
/// itself gated off, so a flat hit IS the answer, and the routes that follow
/// it (an extended module, a minted struct class, the Class/Module table) are
/// only reached when the flat probe found nothing and so never fill a site.
pub fn send_class_cached(
    site: &'static ClassMethodSite,
    cid: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    caller_class: u32,
) -> Result<RubyValue, Signal> {
    // A hit skips the dynamic entry, and with it the stack check -- see
    // `send_value_cached`.
    crate::stack_guard::stack_check()?;
    let gates = crate::runtime_meta::gates();
    // One gate-byte load, same as `send_value_cached`: nothing caches while
    // anything is defined at runtime, and a poisoned (moved) program takes
    // the slow route so the husk raise stays in one place.
    if !crate::runtime_meta::gates_live(gates)
        && !crate::runtime_meta::gates_moved(gates)
        && matches!(recv, RubyValue::Class(c) if c.0 == cid)
    {
        if let Some((cached_caller, target)) = site.hit.get() {
            if *cached_caller == caller_class {
                return match target {
                    Some((f, label)) => {
                        note_dispatch_gated(gates, name);
                        with_c_frame(*label, || f.call(recv, args, block))
                    }
                    None => send_value_in(0, recv, name, args, block),
                };
            }
        } else {
            // Vetted BEFORE it fills, exactly as `send_value_cached` does, so
            // every later hit on the site is vetted too.
            if let Some(reason) = explicit_call_barrier(recv, name, caller_class) {
                return missing_or_raise(recv, name, args, block, reason);
            }
            let target = REGISTRY
                .get()
                .and_then(|r| r.flat_class_hit(ClassId(cid), name));
            let _ = site.hit.set((caller_class, target));
            return match target {
                Some((f, label)) => {
                    note_dispatch_gated(gates, name);
                    with_c_frame(label, || f.call(recv, args, block))
                }
                None => send_value_in(0, recv, name, args, block),
            };
        }
    }
    send_value_explicit_in(0, recv, name, args, block, caller_class)
}

/// [`send_value_in`] behind ruby's explicit-receiver barrier. The uncached
/// entry point, and the one every non-cacheable route funnels into.
pub fn send_value_explicit_in(
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    caller_class: u32,
) -> Result<RubyValue, Signal> {
    match explicit_call_barrier(recv, name, caller_class) {
        Some(reason) => missing_or_raise(recv, name, args, block, reason),
        None => send_value_in(box_id, recv, name, args, block),
    }
}

/// The `caller_class` that means "run no visibility check" -- ruby's
/// `VM_CALL_FCALL`. The compiler passes it for an implicit receiver, for a
/// literal `self` receiver (allowed to reach a private method since 2.7), and
/// for `send`/`__send__`, which are visibility-blind by design.
pub const FCALL: u32 = u32::MAX;

/// Ruby's barrier on an explicit-receiver call (`rb_method_call_status`,
/// `vm_eval.c:837`), or `None` to let the call through. A name nothing defines
/// is `None` too: that is `NoEntry`'s job, and it carries a different message.
///
/// `caller_class` stands in for CRuby's caller `self`. The rule there is
/// `self.is_a?(owner)`, and an instance of `caller_class` is a kind of `owner`
/// exactly when `caller_class` has `owner` in its ancestry -- so the class the
/// call site sits in decides it, with no runtime `self` to thread through.
fn explicit_call_barrier(
    recv: &RubyValue,
    name: Symbol,
    caller_class: u32,
) -> Option<MissingReason> {
    if caller_class == FCALL {
        return None;
    }
    // A CLASS receiver's class methods keep their own visibility table; the
    // instance walk below reads Class/Module's, which says nothing about them.
    if let RubyValue::Class(cid) = recv {
        if class_method_is_private(*cid, name) {
            return Some(MissingReason::Private);
        }
        // ...and a class method the class DOES provide is the one that answers,
        // so nothing below may overrule it. `File.open` is File's own public
        // singleton method; the instance walk would reach past it to the
        // private `Kernel#open`, which is not on File's singleton chain at all.
        if class_method_owner(*cid, name).is_some() {
            return None;
        }
    }
    // ENV's rows are SINGLETON methods on one object, and dispatch probes them
    // by identity because `ENV.class` is `Object` (see `builtins::env`). The
    // instance walk below reads `Object`'s table, which knows nothing about
    // them -- so `ENV.select` was refused as the private `Kernel#select` that
    // ENV's own public row shadows. One pointer compare, on the vet path only.
    if let RubyValue::Object(o) = recv
        && crate::builtins::env::is_env_obj(o)
        && crate::builtins::env::lookup(name.name_str()).is_some()
    {
        return None;
    }
    // A row installed on THIS OBJECT answers before its class does, so its own
    // mark decides. `recv.class_id()` names the ordinary class and knows
    // nothing about it. Asked here and not in `method_vet`, which caches by
    // `(class, name)`: a per-object mark is not a function of the class -- and
    // a program with any singleton at all is `is_live`, so no cache serves it.
    if let Some(vis) = object_singleton_visibility(recv, name) {
        return match vis {
            MethodVisibility::Public => None,
            MethodVisibility::Private => Some(MissingReason::Private),
            // The singleton class is the owner, and nothing else is kin to it.
            MethodVisibility::Protected => Some(MissingReason::Protected),
        };
    }
    match instance_method_visibility(recv.class_id(), name)? {
        MethodVisibility::Public => None,
        MethodVisibility::Private => Some(MissingReason::Private),
        MethodVisibility::Protected => {
            let owner = method_owner(recv.class_id(), name)?;
            let kin = caller_class == owner.0
                || ancestors_of_value(ClassId(caller_class)).contains(&owner);
            (!kin).then_some(MissingReason::Protected)
        }
    }
}

/// What a filled [`DynCallerSite`] must still ask on a hit, given the caller
/// class that arrives PER CALL. Everything else [`explicit_call_barrier`]
/// asks is a function of the `(receiver class, name)` pair the cache is
/// keyed by, so it is answered once, at fill.
#[derive(Clone, Copy)]
enum Vet {
    /// No visibility question at all -- the overwhelmingly common case; a
    /// hit does zero extra work.
    Public,
    /// Reachable only from an `FCALL` caller.
    Private,
    /// Reachable when the caller class is kin to the owner -- the one rule
    /// that still needs the per-call caller, so the owner rides along.
    Protected(u32),
}

/// The [`Vet`] for `(class, name)`: [`explicit_call_barrier`]'s instance
/// walk, split at the caller-dependent step. Only asked for a non-`Class`
/// receiver -- the class-method half of the barrier never reaches a cache.
fn method_vet(class: ClassId, name: Symbol) -> Vet {
    match instance_method_visibility(class, name) {
        None | Some(MethodVisibility::Public) => Vet::Public,
        Some(MethodVisibility::Private) => Vet::Private,
        // No owner row means the barrier waves the call through (its `?`).
        Some(MethodVisibility::Protected) => match method_owner(class, name) {
            Some(owner) => Vet::Protected(owner.0),
            None => Vet::Public,
        },
    }
}

/// Ask `vet` about one caller. `Public` answers before the `FCALL` compare,
/// so the common case is one branch and done.
#[inline]
fn vet_denies(vet: Vet, caller_class: u32) -> Option<MissingReason> {
    match vet {
        Vet::Public => None,
        _ if caller_class == FCALL => None,
        Vet::Private => Some(MissingReason::Private),
        Vet::Protected(owner) => {
            let kin = caller_class == owner
                || ancestors_of_value(ClassId(caller_class)).contains(&ClassId(owner));
            (!kin).then_some(MissingReason::Protected)
        }
    }
}

/// A dynamic call site whose CALLER class is a per-call fact rather than a
/// per-site constant: a site inside a shared body (one emitted function
/// serving a whole hierarchy, where the runtime `self.class` decides
/// visibility) or inside a re-homed block. [`CallSite`] bakes the caller in
/// and vets once at fill; here the vet's caller-independent half is cached
/// (see [`Vet`]) and the caller-dependent remainder is asked per hit.
///
/// Same fill-once discipline as [`CallSite`], for the same reasons; the
/// emitter mints one `zeo_cm_sites` slot per site (`Fx::cm_site_ptr`),
/// never shared between sites.
pub struct DynCallerSite {
    hit: std::sync::OnceLock<(u32, Vet, Cached)>,
}

impl DynCallerSite {
    pub const fn new() -> DynCallerSite {
        DynCallerSite {
            hit: std::sync::OnceLock::new(),
        }
    }
}

impl Default for DynCallerSite {
    fn default() -> Self {
        Self::new()
    }
}

/// [`send_value_cached`] for a [`DynCallerSite`]: the same cache gates and
/// fill discipline, with the visibility question split so a hit pays only
/// the caller-dependent remainder -- nothing at all for a public target.
/// Every route the cache does not serve funnels into
/// [`send_value_explicit_in`] unchanged. NOT `#[inline]`, for `CallSite`'s
/// reason.
pub fn send_value_dyn_cached(
    site: &'static DynCallerSite,
    box_id: u32,
    recv: &RubyValue,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
    caller_class: u32,
) -> Result<RubyValue, Signal> {
    let gates = crate::runtime_meta::gates();
    if crate::runtime_meta::gates_moved(gates) && value_moved(recv) {
        return Err(crate::ractor::moved_object_error());
    }
    if !matches!(recv, RubyValue::Class(_))
        && box_id == 0
        && !crate::runtime_meta::gates_live(gates)
    {
        let id = recv.class_id();
        if let Some((cached, vet, target)) = site.hit.get() {
            if *cached == id.0 {
                if let Some(reason) = vet_denies(*vet, caller_class) {
                    return missing_or_raise(recv, name, args, block, reason);
                }
                note_dispatch_gated(gates, name);
                return match (target, recv) {
                    (Cached::Obj(f), RubyValue::Object(o)) => f(o, args, block),
                    (Cached::CObj(f), RubyValue::Object(_)) => {
                        crate::capi::dispatch::call_value_fn(*f, recv, args, block)
                    }
                    (Cached::Value(f, label), _) => {
                        with_c_frame(*label, || f.call(recv, args, block))
                    }
                    _ => send_value_in(box_id, recv, name, args, block),
                };
            }
        } else {
            // The vet is computed BEFORE the resolution it guards, exactly
            // as `send_value_cached` runs the barrier before it fills -- and
            // a denied call fills nothing, so the deny is re-asked (and
            // re-raised) on every call, the shape the uncached path has.
            let vet = method_vet(id, name);
            if let Some(reason) = vet_denies(vet, caller_class) {
                return missing_or_raise(recv, name, args, block, reason);
            }
            match recv {
                RubyValue::Object(o) if id != zeo_abi::OBJECT_CLASS => {
                    match REGISTRY.get().and_then(|r| r.lookup_mro(id, name)) {
                        Some(MethodImpl::Static(f)) => {
                            note_dispatch_gated(gates, name);
                            let _ = site.hit.set((id.0, vet, Cached::Obj(*f)));
                            return f(o, args, block);
                        }
                        Some(MethodImpl::CValue(f)) => {
                            note_dispatch_gated(gates, name);
                            let _ = site.hit.set((id.0, vet, Cached::CObj(*f)));
                            return crate::capi::dispatch::call_value_fn(*f, recv, args, block);
                        }
                        Some(MethodImpl::Dynamic(_)) | None => {}
                    }
                }
                RubyValue::Object(_) => {}
                _ => {
                    if let Some(Some(hit)) = REGISTRY.get().and_then(|r| r.flat_value_hit(id, name))
                    {
                        note_dispatch_gated(gates, name);
                        let _ = site
                            .hit
                            .set((id.0, vet, Cached::Value(hit.f, hit.frame_label)));
                        return with_c_frame(hit.frame_label, || hit.f.call(recv, args, block));
                    }
                }
            }
        }
    }
    send_value_explicit_in(box_id, recv, name, args, block, caller_class)
}
