//! The definition watermark -- what `method_added`-style hooks may observe
//! of a half-built class, and the machinery that defers them.

use super::*;

//
// Ruby's definition hook sees a HALF-BUILT class: at `method_added(:a)`,
// `instance_methods(false)` answers `[:a]` alone, and `method_defined?(:b)` is
// false for a `def b` written below. zeo installs every method table before the
// program's first statement runs, so there is no such moment to observe.
//
// Rather than defer registration -- which would cost every program -- the
// compiler works out which names are still in the future at each announcement
// (it knows them statically) and hands them over for the duration of the hook
// body. The reflection rows subtract the set; DISPATCH deliberately does not,
// so calling a not-yet-defined method from inside a hook succeeds here and
// raises `NoMethodError` in ruby. Truncating dispatch would put a thread-local
// check on the hot path for a case no real code exercises; it is recorded as
// `tests/gaps/method_added_calls_later_method.rb`.

thread_local! {
    /// One frame per definition hook currently on the stack. A hook body that
    /// defines another method nests, which is why this is a stack.
    static PENDING_DEFS: std::cell::RefCell<Vec<(ClassId, Vec<Symbol>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// How many hook frames are live across ALL threads. [`GATE_PENDING`] tracks
/// whether this is non-zero, so the gate lifts again once the last hook
/// returns rather than de-optimizing the rest of the program.
///
/// Two threads defining methods at the same time can interleave the decrement
/// and the increment such that the gate clears while the other thread is still
/// inside a hook. The consequence is exactly the behaviour zeo had before the
/// truncation existed -- that thread's hook body resolves a method it has not
/// been told about -- and no shape zeo compiles today fires definition hooks on
/// two threads at once. The thread-local stack stays the authority for WHICH
/// names are pending; this word only decides whether asking is worthwhile.
static PENDING_DEPTH: AtomicUsize = AtomicUsize::new(0);

/// Runs `f` -- a definition hook's send -- with `names` marked as not yet
/// defined on `class`.
pub fn with_pending_defs<R>(class: ClassId, names: &[Symbol], f: impl FnOnce() -> R) -> R {
    pending_defs_begin(class, names);
    let out = f();
    pending_defs_end();
    out
}

/// [`with_pending_defs`]' two halves, for a caller that cannot pass a
/// closure across the C boundary. They must pair exactly.
pub fn pending_defs_begin(class: ClassId, names: &[Symbol]) {
    PENDING_DEPTH.fetch_add(1, Ordering::AcqRel);
    GATES.fetch_or(GATE_PENDING, Ordering::Release);
    PENDING_DEFS.with(|p| p.borrow_mut().push((class, names.to_vec())));
}

pub fn pending_defs_end() {
    PENDING_DEFS.with(|p| {
        p.borrow_mut().pop();
    });
    if PENDING_DEPTH.fetch_sub(1, Ordering::AcqRel) == 1 {
        GATES.fetch_and(!GATE_PENDING, Ordering::Release);
    }
}

/// Whether a definition hook is running with a non-empty pending set -- the
/// gate every truncation site reads before touching the thread-local.
#[inline(always)]
pub fn any_pending() -> bool {
    GATES.load(Ordering::Relaxed) & GATE_PENDING != 0
}

/// Whether `name` is a method of `class` that the definition hook now running
/// has not seen defined yet.
#[inline]
pub fn not_yet_defined(class: ClassId, name: Symbol) -> bool {
    any_pending() && listed_pending(class, name) && !inherited(class, name)
}

/// Every name of `class` the running hook has not seen defined, for the callers
/// that enumerate rather than ask. Empty (and free) outside a hook body.
///
/// `inherit` picks which question is being asked, and the two differ. "Does
/// this name exist AT ALL?" -- `instance_methods(true)`, `method_defined?` --
/// is answered by an ancestor's copy, so a pending name an ancestor also
/// defines is not hidden. "Is it defined DIRECTLY here?" --
/// `instance_methods(false)` -- is not: `Sub#shared` written below the hook is
/// absent from Sub's OWN list even though `Base#shared` exists.
pub fn pending_defs_for(class: ClassId, inherit: bool) -> Vec<Symbol> {
    if !any_pending() {
        return Vec::new();
    }
    PENDING_DEFS.with(|p| {
        p.borrow()
            .iter()
            .filter(|(c, _)| *c == class)
            .flat_map(|(_, ns)| ns.iter().copied())
            .filter(|&n| !inherit || !inherited(class, n))
            .collect()
    })
}

/// Whether `class` itself has a `def name` still ahead of the running hook --
/// [`not_yet_defined`] without the ancestor rule, which is the question DISPATCH
/// asks. The two differ on an override: `Sub#shared` written below the hook is
/// not installed yet, so a call resolves to `Base#shared` rather than missing,
/// while `method_defined?(:shared)` was already true through `Base`.
#[inline]
pub fn pending_here(class: ClassId, name: Symbol) -> bool {
    any_pending() && listed_pending(class, name)
}

fn listed_pending(class: ClassId, name: Symbol) -> bool {
    PENDING_DEFS.with(|p| {
        p.borrow()
            .iter()
            .any(|(c, ns)| *c == class && ns.contains(&name))
    })
}

/// An ANCESTOR beyond `class` defines `name` too, so it exists no matter what
/// this class has reached: ruby hides only what does not exist ANYWHERE yet,
/// and a `Sub#to_s` written below the hook still leaves `Object#to_s`
/// reachable. Only ever asked inside a hook body.
fn inherited(class: ClassId, name: Symbol) -> bool {
    crate::dispatch::chain_index_of(class, class)
        .and_then(|at| crate::dispatch::method_owner_after(class, at + 1, name))
        .is_some()
}

/// `Module#const_added` -- ruby announces a constant right after it becomes
/// readable, on the module it was set on. Unlike the `method_*` family this one
/// has no singleton rerouting: a constant set inside `class << K` announces on
/// `#<Class:K>` itself (oracle-verified).
pub fn fire_const_added(owner: ClassId, name: &str) -> Result<(), Signal> {
    let hook = Symbol::intern("const_added");
    if !global_def_hook(hook) && crate::dispatch::class_method_owner(owner, hook).is_none() {
        return Ok(());
    }
    crate::dispatch::send_value(
        &RubyValue::Class(owner),
        hook,
        &[RubyValue::Symbol(Symbol::intern(name))],
        None,
    )?;
    Ok(())
}

/// The seven hook names that become GLOBAL when defined on `Module`/`Class`
/// (the `method_*` trio) or on `BasicObject` (the `singleton_method_*` trio).
pub fn global_def_hook_owner(id: ClassId, name: Symbol) -> bool {
    match name.name_str() {
        "method_added" | "method_removed" | "method_undefined" | "const_added" => {
            id == zeo_abi::MODULE_CLASS || id == zeo_abi::CLASS_CLASS
        }
        "singleton_method_added" | "singleton_method_removed" | "singleton_method_undefined" => {
            id == zeo_abi::BASIC_OBJECT_CLASS
        }
        _ => false,
    }
}

/// One chain's version of the splice: CRuby's `include_modules_at`, run
/// against the flat chain `current`.
///
/// The insertion point walks: `mid`'s own ancestry is inserted element by
/// element, keeping ITS relative order, and an element the chain already
/// carries is not added -- it MOVES the insertion point past itself instead,
/// so the next new module lands after it. That is what makes
/// `include A; include M(includes A)` answer `[C, M, A]` while
/// `include A; include M(prepends A)` answers `[C, A, M]`.
///
/// The search scope is what the two verbs disagree about, and it is the whole
/// of the difference between them. `rb_include_module` passes
/// `search_super = TRUE`, so an `include` skips a module already ANYWHERE in
/// the chain -- inherited from a superclass included. `rb_prepend_module`
/// passes `FALSE`, so a `prepend` looks only at the PREPEND AREA and adds a
/// module that is merely included below, giving the chain two occurrences of
/// it. Document order therefore decides: whichever verb runs first finds an
/// empty area and takes effect.
///
/// `None` when the chain doesn't pass through `target` or gains nothing.
fn splice_into_chain(
    current: &[ClassId],
    target: ClassId,
    fresh_src: &[ClassId],
    placement: Placement,
) -> Option<Vec<ClassId>> {
    // Not `current[0]`: after a prepend, self is no longer first.
    let at = current.iter().position(|&a| a == target)?;
    let mut new_anc = current.to_vec();
    // The target's PREPEND AREA inside this chain. In the target's own chain
    // that is everything ahead of it; in a SUBCLASS's chain the entries ahead
    // of the target also hold the subclass and the subclass's own prepends,
    // which belong to a different origin -- so the area is named by the
    // target's own chain rather than by position alone.
    let own = ancestors_of_value(target);
    let own_prepends: HashSet<ClassId> = own.iter().copied().take_while(|&a| a != target).collect();
    let mut area: Vec<usize> = (0..at)
        .filter(|&p| own_prepends.contains(&new_anc[p]))
        .collect();
    let mut ins = if placement == Placement::Before {
        area.first().copied().unwrap_or(at)
    } else {
        at + 1
    };
    let mut added = false;
    for &m in fresh_src {
        let seen = if placement == Placement::Before {
            area.iter().copied().find(|&p| new_anc[p] == m)
        } else {
            new_anc.iter().position(|&a| a == m)
        };
        match seen {
            Some(p) => ins = p + 1,
            None => {
                new_anc.insert(ins, m);
                if placement == Placement::Before {
                    for p in &mut area {
                        if *p >= ins {
                            *p += 1;
                        }
                    }
                    area.push(ins);
                    area.sort_unstable();
                }
                ins += 1;
                added = true;
            }
        }
    }
    added.then_some(new_anc)
}

/// Splices `mid`'s ancestry into `cid`'s -- and into EVERY chain that passes
/// through `cid`. CRuby's ancestry is a shared linked structure, so a later
/// `include` into a superclass is visible to subclasses minted earlier, and
/// (since Ruby 3.0) an `include` into a module already mixed in elsewhere
/// reaches its hosts too. zeo chains are flat leaked snapshots, so the splice
/// must visit each: the target's own chain, every overlay chain containing the
/// target (runtime-minted subclasses -- rspec's describe-groups gaining the
/// mock adapter their base class was given at configure time is the corpus
/// case), and every REGISTERED class whose frozen chain contains the target
/// (those mint an overlay chain here; the frozen row stays untouched).
///
/// Lock discipline: candidate ids are collected under the read lock reading
/// `ancestors` fields directly; chains are recomputed lock-free through
/// `ancestors_of_value` (overlay-first); one write installs them all. Old
/// slices leak, matching this runtime's no-GC policy for interned ancestries.
pub(super) fn splice_module_into(cid: ClassId, mid: ClassId, placement: Placement) {
    let fresh_src: Vec<ClassId> = ancestors_of_value(mid).to_vec();
    let mut candidates: Vec<u32> = vec![cid.0];
    {
        let r = maps().classes.read().unwrap();
        for (&id, e) in r.iter() {
            if id != cid.0 && e.ancestors.contains(&cid) {
                candidates.push(id);
            }
        }
    }
    for id in crate::dispatch::classes_with_ancestor(cid) {
        // A registered class with a live overlay chain was already considered
        // above; only frozen-chain classes join here.
        if overlay_ancestors(ClassId(id)).is_none() && id != cid.0 {
            candidates.push(id);
        }
    }
    let mut updates: Vec<(u32, &'static [ClassId])> = Vec::new();
    for id in candidates {
        let current = ancestors_of_value(ClassId(id));
        if let Some(new_anc) = splice_into_chain(current, cid, &fresh_src, placement) {
            crate::dispatch::note_chain(&new_anc);
            updates.push((id, Box::leak(new_anc.into_boxed_slice())));
        }
    }
    let mut w = maps().classes.write().unwrap();
    for (id, leaked) in updates {
        w.entry(id).or_insert_with(OverlayEntry::delta).ancestors = leaked;
    }
}

/// The public/protected instance-method names a module contributes to a host
/// via `extend`/`include` -- registry methods plus any runtime-overlay ones a
/// `Module.new` added.
pub(super) fn module_extendable_method_names(mid: ClassId) -> Vec<Symbol> {
    // PRIVATE methods extend too -- ruby copies the module's whole instance
    // set onto the singleton, private ones staying private there, which is how
    // `singleton`'s own `set_mutex` writer travels. The filter said
    // `NotPrivate`, which went unnoticed only because a `private` inside a
    // module body never reached the registry to begin with.
    let mut names =
        crate::dispatch::instance_method_names(mid, crate::dispatch::VisFilter::All, false);
    for n in overlay_own_method_names(mid) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names
}

/// One `extend`ed module method for a BARE HEAP VALUE receiver
/// (`ARGV.extend(OptionParser::Arguable)`). Both the native module tables and
/// a compiled user module's own bridge already take a `&RubyValue` receiver --
/// which an Array/String/Hash IS -- so the value passes straight through, with
/// no surrogate instance anywhere. A module method that only exists as an
/// `&RObj` body has no way to run against a bare value and is skipped, the same
/// posture `extended_class_method` takes for its own unreachable cases.
pub(super) fn extended_value_method(mid: ClassId, name: Symbol) -> Option<RProc> {
    let f = crate::builtins::class_table(mid)
        .and_then(|t| t(&name.name()))
        .map(crate::dispatch::ValueImpl::Rust)
        .or_else(|| crate::dispatch::value_method(mid, 0, name))?;
    Some(RProc::with_self_and_block(
        f.into_fn(),
        RubyValue::Nil,
        -1,
        true,
    ))
}

/// One `extend`ed module method, wrapped as a class method (a value-receiver
/// `RProc`, invoked with the Class as `self`).
pub(super) fn extended_class_method(mid: ClassId, name: Symbol) -> Option<RProc> {
    let sname = name.name();
    // A NATIVE module method (`Random::Formatter`, `Comparable`, ...) takes a
    // value receiver, so it runs correctly with a Class `self` and can
    // redispatch to it. Pass `self` straight through.
    if let Some(f) = crate::builtins::class_table(mid).and_then(|t| t(&sname)) {
        return Some(RProc::with_self_and_block(f, RubyValue::Nil, -1, true));
    }
    // A compiled user module ALSO emits a value bridge per method, which takes
    // `self` as a plain `RubyValue` -- so a Class receiver passes straight
    // through and `self` really is the class. That is what makes an
    // implicit-self CLASS-method call resolve (the singleton gem's
    // `@singleton__instance__ ||= new`, where an object receiver would look
    // for an instance method `new` and find none) and what routes `@x` to the
    // class's own store. Preferred over the `&RObj` body below.
    if let Some(f) = crate::dispatch::value_method(mid, 0, name) {
        return Some(RProc::with_self_and_block(
            f.into_fn(),
            RubyValue::Nil,
            -1,
            true,
        ));
    }
    // A RUNTIME-defined body (`define_method`, or a method an eval'd `def`
    // installed) is already value-shaped -- it takes `self` as a plain
    // `RubyValue`. Preferred over the surrogate below for the same reason the
    // compiled value bridge is: `self` really is the class, so `self.class`
    // answers Module and a sibling call resolves through the singleton
    // ancestry rather than looking for an instance method on the class.
    if let Some(body) = overlay_value_body(mid, name) {
        return Some(RProc::with_self_and_block(
            move |self_val: &RubyValue, args: &[RubyValue], block| {
                call_value_body(mid, name, &body, self_val, args, block)
            },
            RubyValue::Nil,
            -1,
            true,
        ));
    }
    // A USER module method is a compiled `&RObj` body: it needs an object
    // receiver. When invoked with a Class `self`, run it against a
    // `ClassSurrogate` -- an `RObj` shell whose ivars ARE the class's own
    // class-level store, so `@x = 1` in the module's body lands where
    // `Klass.instance_variable_get(:@x)` and a `def self.x` reading `@x` both
    // look. (A blank throwaway instance stood here, and swallowed every such
    // write: the singleton gem's `klass.extend SingletonClassMethods` then
    // `klass.instance_eval { set_mutex(Thread::Mutex.new) }` left the mutex
    // nil, and `Singleton#instance` raised on it.)
    // An INCLUDED module's method counts: `module_function :greet` after
    // `include Greeting` promotes the mixed-in body, and `extend self` reaches
    // the same way. Own definitions win, so the ancestry walk is only the
    // fallback.
    let m = module_own_method_impl(mid, name).or_else(|| snapshot_instance_method(mid, name))?;
    Some(RProc::with_self_and_block(
        move |self_val, args, block| match self_val {
            RubyValue::Object(o) => m.call(o, args, block),
            RubyValue::Class(cid) => {
                let surrogate: crate::dispatch::RObj = Arc::new(ClassSurrogate { class_id: *cid });
                m.call(&surrogate, args, block)
            }
            other => Err(type_error!(
                "can't run an extended method with {} as self",
                immediate_kind(other)
            )),
        },
        RubyValue::Nil,
        -1,
        true,
    ))
}

/// The `MethodImpl` a module id defines for `name` directly -- overlay delta
/// first (a runtime `Module.new`/`define_method`), then the frozen registry (a
/// compile-time `module M; def hi; end`).
/// The overlay's own runtime-defined instance-method names for a class/module
/// id (empty if it has no overlay entry) -- the names the registry-based
/// `instance_method_names` can't see.
fn overlay_own_method_names(id: ClassId) -> Vec<Symbol> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .map(|e| e.methods.keys().copied().collect())
        .unwrap_or_default()
}

/// Every instance-method name `id`'s own overlay entry has an opinion about,
/// paired with its visibility -- `None` for an `undef_method` tombstone, which
/// carries no method but still CLAIMS the name so an ancestor's definition
/// can't answer for it.
///
/// This is the overlay half of `dispatch::instance_method_names`, whose other
/// half reads the frozen registry: a class minted by `Class.new` has all of its
/// methods here and none of them there.
pub fn overlay_instance_method_names(
    id: ClassId,
) -> Vec<(Symbol, Option<crate::dispatch::MethodVisibility>)> {
    let c = maps().classes.read().unwrap();
    let Some(e) = c.get(&id.0) else {
        return Vec::new();
    };
    // A visibility mark can name a method the overlay carries no body for
    // (`class_eval { private :compiled_method }`), so the marks contribute
    // names of their own rather than just annotating `methods`.
    let named: HashSet<Symbol> = e
        .methods
        .keys()
        .chain(e.prepended.keys())
        .chain(e.methods_vis.keys())
        .copied()
        // A `Class#dup` copy's inherited bodies are dispatch-only -- see
        // `OverlayEntry::inherited_names`.
        .filter(|n| !e.undefs.contains(n) && !e.inherited_names.contains(n))
        .collect();
    // Intern-id order: hash-set order is RANDOM PER PROCESS (found by the
    // typed-diff leg -- two runs of one program listed `attr_accessor`'s
    // pair both ways), and first-intern order is definition order for
    // runtime-defined names, which is CRuby's listing order.
    let mut named: Vec<Symbol> = named.into_iter().collect();
    named.sort_unstable_by_key(|s| s.to_u32());
    let mut undefs: Vec<Symbol> = e.undefs.iter().copied().collect();
    undefs.sort_unstable_by_key(|s| s.to_u32());
    undefs
        .into_iter()
        .map(|n| (n, None))
        .chain(named.into_iter().map(|n| {
            let vis = e.methods_vis.get(&n).copied();
            let vis = vis.unwrap_or(crate::dispatch::MethodVisibility::Public);
            (n, Some(vis))
        }))
        .collect()
}

/// The overlay's own CLASS-method names for `id` -- the class-level counterpart
/// of [`overlay_instance_method_names`], and likewise invisible to the frozen
/// registry that `dispatch::class_method_names` otherwise reads.
/// The runtime `define_method` body `id` ITSELF holds for `name`. Value-shaped:
/// see [`OverlayEntry::value_bodies`] for why an `RObj` receiver takes a
/// different route.
///
/// Own-only, because the caller interleaves it with the registry and builtin
/// probes ancestor by ancestor -- ruby's placement rule. A whole-ancestry walk
/// here would let `Enumerable.define_method(:map)` beat `Array`'s own `map`,
/// which it must not.
pub fn overlay_value_body(id: ClassId, name: Symbol) -> Option<RProc> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.value_bodies.get(&name).cloned())
}

/// Run an [`overlay_value_body`] with the method frame around it.
///
/// Not just `body.call_with_self_and_block`: `dynamic_from_proc`'s wrapper is
/// what pushes `METHOD_FRAMES`, and a bare `super` inside a runtime-defined
/// method reads exactly that. Calling the proc raw made prism's
/// `Module.new { def unpack1(..) ... super ... }` raise "super called outside
/// of method" the moment String dispatch started consulting the overlay.
pub fn call_value_body(
    defining: ClassId,
    name: Symbol,
    body: &RProc,
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    push_method_frame(defining, name);
    let out = body.call_with_self_and_block(recv, args, block);
    pop_method_frame();
    out
}

/// `own_only` drops the names an `extend` copied in, which is
/// `singleton_methods(false)`'s narrowing -- see
/// [`OverlayEntry::extended_class_methods`].
/// Whether the overlay's class method `name` on `id` was copied in by an
/// `extend` rather than written on the class itself -- see
/// [`OverlayEntry::extended_class_methods`]. What tells reflection to report
/// the MODULE as the owner.
pub fn overlay_class_method_is_extended(id: ClassId, name: Symbol) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.extended_class_methods.contains(&name))
}

pub fn overlay_class_method_names(id: ClassId, own_only: bool) -> Vec<Symbol> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .map(|e| {
            let own = e
                .class_methods
                .keys()
                .copied()
                .filter(|n| !own_only || !e.extended_class_methods.contains(n));
            // A singleton-prepend copy dispatches on the class, so the WIDE
            // list reports it; like an extend it lives in the singleton's
            // chain, not on the class itself, so the narrow list skips it.
            match own_only {
                true => own.collect(),
                false => {
                    let mut names: Vec<Symbol> = own.collect();
                    for n in e.prepended_class_methods.keys() {
                        if !names.contains(n) {
                            names.push(*n);
                        }
                    }
                    names
                }
            }
        })
        .unwrap_or_default()
}

pub(super) fn module_own_method_impl(mid: ClassId, name: Symbol) -> Option<MethodImpl> {
    overlay_own_method(mid, name)
        .or_else(|| crate::dispatch::registry_lookup_cloned(mid, name))
        .or_else(|| crate::dispatch::registry_value_method_impl(mid, name))
        .or_else(|| builtin_module_method_impl(mid, name))
}

/// The `MethodImpl` a BUILTIN module (`Comparable`/`Enumerable`/`Math`, or any
/// module with a hardcoded `class_table`) defines for `name`. These don't live
/// in the registry -- their bodies are static method-table rows -- so each is
/// wrapped in a `Dynamic` closure that re-dispatches by name. This is what lets
/// `obj.extend(Comparable)` install `clamp`/`between?` etc. (`Math`'s module
/// functions are ordinary instance-table rows now, reached the same way.)
fn builtin_module_method_impl(mid: ClassId, name: Symbol) -> Option<MethodImpl> {
    let f = crate::builtins::class_table(mid)?(&name.name())?;
    Some(MethodImpl::Dynamic(std::sync::Arc::new(
        move |recv: &RObj, args: &[RubyValue], b| f(&RubyValue::Object(recv.clone()), args, b),
    )))
}

/// `obj.singleton_class` -- the per-object singleton class as a real `Class`
/// value (minted once per object identity, cached). Defining a method on it
/// routes back to `obj`'s singleton table via the `singleton_owner` hook in
/// `runtime_define_method`; its ancestry is `[singleton, *obj.class.ancestors]`.
/// For a `Class` receiver the owner is the class itself, so defs become class
/// methods (`class << Foo` semantics).
pub fn runtime_singleton_class(recv: &RubyValue) -> Result<RubyValue, Signal> {
    // An IMMEDIATE (Integer/Float/Symbol/nil/true/false and the numeric towers)
    // has no singleton class -- CRuby raises `TypeError: can't define singleton`.
    // Object/Class get a CACHED singleton class whose method defs redirect to the
    // owner (`singleton_owner`); other heap values (String/Array/...) get a fresh
    // singleton class good for `.class`/`.superclass`/reflection (defining on one
    // isn't supported, matching this runtime's singleton-storage limits).
    // `nil`/`true`/`false` are the exception CRuby carves out: each is the sole
    // instance of its class, so its singleton class IS that class, and
    // `nil.singleton_class.equal?(NilClass)` is true. Only the numeric and
    // Symbol immediates raise.
    match recv {
        RubyValue::Nil => return Ok(RubyValue::Class(zeo_abi::NIL_CLASS)),
        RubyValue::Bool(true) => return Ok(RubyValue::Class(zeo_abi::TRUE_CLASS)),
        RubyValue::Bool(false) => return Ok(RubyValue::Class(zeo_abi::FALSE_CLASS)),
        _ => {}
    }
    if matches!(
        recv,
        RubyValue::Int(_)
            | RubyValue::BigInt(_)
            | RubyValue::Float(_)
            | RubyValue::Rational(_)
            | RubyValue::Complex(_)
            | RubyValue::Symbol(_)
    ) {
        return Err(type_error!("can't define singleton"));
    }
    // A shared object refuses a singleton -- see `refuse_shared_singleton`.
    super::api::refuse_shared_singleton(recv)?;
    let cache_key = singleton_class_key(recv);
    let real = recv.class_id();
    let owner = recv.clone();
    // The guard is dropped before the rebase below: an `if let` chain holds
    // its temporaries for the whole body, and the rebase walks the metaclass
    // chain, which mints -- and minting takes this same lock for writing.
    let cached = cache_key.and_then(|k| maps().singleton_classes.read().unwrap().get(&k).copied());
    if let Some(sid) = cached {
        // A COMPILED surrogate carries the compiler's Object-rooted ancestry,
        // not ruby's metaclass chain. This is the first ask, so it is where
        // the rebase belongs -- a minted singleton pays the same gate below.
        super::rebase_compiled_surrogate(sid, recv);
        return Ok(RubyValue::Class(sid));
    }
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    // The chain carries every module `extend` mixed in, ahead of the receiver
    // class's own -- CRuby files an extended module between the singleton and
    // the class, which is what makes `o.extend(M)` show up here.
    let mut anc = vec![new_id];
    anc.extend(singleton_super_chain(recv));
    let leaked: &'static [ClassId] = Box::leak(anc.into_boxed_slice());
    // A CLASS receiver's singleton is named after the class itself --
    // CRuby's `#<Class:Melody>`. A plain object's is named after the OBJECT,
    // in its address form (`#<Class:#<Object:0xADDR>>`, CRuby's
    // `rb_any_to_s` of the attached object -- never its class, which would
    // collapse every instance's singleton to one name).
    let singleton_name = match recv {
        RubyValue::Class(cid) => {
            let n = crate::dispatch::class_name(*cid).unwrap_or_else(|| "Object".to_string());
            format!("#<Class:{n}>")
        }
        _ => {
            let cname = crate::dispatch::class_name(real).unwrap_or_else(|| "Object".to_string());
            match value_identity(recv) {
                Some(addr) => format!("#<Class:#<{cname}:0x{addr:016x}>>"),
                None => format!("#<Class:{cname}>"),
            }
        }
    };
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                name: RwLock::new(Some(singleton_name)),
                ancestors: leaked,
                ..Default::default()
            },
        );
    }
    if let Some(k) = cache_key {
        maps().singleton_classes.write().unwrap().insert(k, new_id);
        maps()
            .singleton_owner
            .write()
            .unwrap()
            .insert(id_num, owner);
    }
    mark_singletons();
    mark_live();
    Ok(RubyValue::Class(new_id))
}

/// `SomeModule.dup`/`.clone` -- a real, independent copy: a fresh runtime
/// module id carrying its own snapshot of the original's OWN instance
/// methods. Handing the original's handle back instead is silently
/// destructive, because the copy is made in order to be EDITED. delegate.rb
/// opens with `kernel = ::Kernel.dup` and then undefines
/// `to_s`/`inspect`/`!~`/`===`/`<=>`/`hash` on it -- which, sharing one id,
/// stripped them from the real `Kernel` and from every object in the program.
///
/// Only the method table is copied. Constants and class-level ivars stay with
/// the original, and the copy is anonymous until a constant names it.
/// `Class#dup` / `Class#clone` -- CRuby's `rb_mod_init_copy` for a CLASS.
///
/// The copy is a REAL new class: anonymous (`name` is nil, so naming it does
/// not rename the source), sharing the source's superclass and mixins, and
/// carrying its own copies of the source's OWN instance methods, class
/// methods, constants, class-level ivars and class variables. Nothing is
/// shared -- writing a constant on the copy leaves the source alone.
///
/// `clone` differs from `dup` in exactly one observable way here: it carries
/// the frozen state over (oracle-verified -- the singleton class, which CRuby
/// documents as clone-only, is copied by both because a class's singleton
/// methods ARE its class methods and `rb_mod_init_copy` moves those either way).
pub fn runtime_class_dup(cid: ClassId, clone: bool) -> Result<RubyValue, Signal> {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    // The source's chain with the source's own head replaced: same superclass,
    // same includes and prepends, a new identity at the front.
    let src_chain = ancestors_of_value(cid);
    let mut anc = Vec::with_capacity(src_chain.len());
    anc.push(new_id);
    anc.extend(src_chain.iter().copied().filter(|&a| a != cid));
    let leaked: &'static [ClassId] = Box::leak(anc.into_boxed_slice());

    // Own instance methods, per visibility filter, exactly as
    // `runtime_module_dup` collects a module's.
    let mut methods = crate::FMap::default();
    let mut methods_vis = crate::FMap::default();
    for (filter, vis) in [
        (
            crate::dispatch::VisFilter::Public,
            crate::dispatch::MethodVisibility::Public,
        ),
        (
            crate::dispatch::VisFilter::Protected,
            crate::dispatch::MethodVisibility::Protected,
        ),
        (
            crate::dispatch::VisFilter::Private,
            crate::dispatch::MethodVisibility::Private,
        ),
    ] {
        for name in crate::dispatch::instance_method_names(cid, filter, false) {
            if let Some(m) = module_own_method_impl(cid, name) {
                methods.insert(name, m);
                methods_vis.insert(name, vis);
            }
        }
    }

    // A method the source INHERITS from a compiled ancestor has to be copied
    // too, resolved AT the source: zeo materializes a layout-correct body per
    // class, so the ancestor's own copy downcasts to the ANCESTOR's struct and
    // aborts on the copy's instances (which carry the source's). Reflection
    // must not see these as the copy's own, so they are recorded apart.
    //
    // Only compiled ancestors below `Object` need it -- a builtin row and every
    // Kernel/BasicObject universal take a receiver generically, and the copy's
    // chain still reaches them.
    let mut inherited_names = FSet::default();
    for &anc in ancestors_of_value(cid).iter().skip(1) {
        if anc == ClassId(0) || crate::dispatch::registry_allocator(anc).is_none() {
            continue;
        }
        for name in
            crate::dispatch::instance_method_names(anc, crate::dispatch::VisFilter::All, false)
        {
            if methods.contains_key(&name) {
                continue;
            }
            if let Some(m) = snapshot_instance_method(cid, name) {
                methods.insert(name, m);
                inherited_names.insert(name);
            }
        }
    }

    // Own CLASS methods. `extended_class_method` is the right wrapper: it
    // hands back an `RProc` whose `self` is the receiving class VALUE, so the
    // copy's `def self.x` reads the COPY's class-level ivars.
    let mut class_methods = crate::FMap::default();
    let mut class_methods_vis = crate::FMap::default();
    for name in crate::dispatch::class_method_names_in(cid, false) {
        if let Some(p) = overlay_class_method(cid, name).or_else(|| class_method_as_proc(cid, name))
        {
            class_methods.insert(name, p);
            if crate::dispatch::class_method_is_private(cid, name) {
                class_methods_vis.insert(name, true);
            }
        }
    }

    // NOT the source's own constructor: it stamps the SOURCE's class id, so
    // `K.dup.new.class` answered `K` and `K.dup.new.is_a?(K)` was true. The
    // struct the copied bodies expect travels separately -- see
    // `OverlayEntry::allocator`.
    let allocator = crate::dispatch::ancestor_allocator_of(cid);
    let constructor = Some(match allocator {
        Some(_) => compiled_subclass_construct,
        None => minted_constructor(leaked),
    });
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                ancestors: leaked,
                methods,
                methods_vis,
                class_methods,
                class_methods_vis,
                constructor,
                allocator,
                inherited_names,
                ..Default::default()
            },
        );
    }
    // A CLIF program's instances share one concrete type whose per-class
    // shape is a `LAYOUTS` row keyed by class id, and the copy's chain does
    // not contain its source -- so the row has to be copied across with the
    // allocator, or the copy's `new` finds nothing to allocate. (The rustc
    // backend needs no equivalent: its allocator IS the source's per-class
    // `__allocate`, which knows the struct.)
    if let Some(layout) = crate::compiled_object::alloc_layout_of(cid) {
        crate::compiled_object::register_layout(new_id, layout);
    }
    // Constants, class-level ivars and class variables are stored outside the
    // overlay entry, keyed by class id -- copied by value, so the two classes
    // diverge from here.
    for name in crate::constants::const_names_of(cid.0) {
        if let Some(v) = crate::constants::const_get_own(cid.0, &name) {
            crate::constants::const_set(id_num, &name, v);
        }
    }
    for name in crate::civars::class_ivar_names(cid.0) {
        let v = crate::civars::class_ivar_get(cid.0, &name);
        crate::civars::class_ivar_set(id_num, &name, v)?;
    }
    for name in crate::cvars::cvar_names_of(cid.0) {
        let v = crate::cvars::cvar_get(cid.0, &name);
        crate::cvars::cvar_set(id_num, &name, v)?;
    }
    super::copy_class_extensions(cid, new_id);
    mark_live();
    if clone && crate::dispatch::class_frozen(cid) {
        crate::dispatch::class_set_frozen(new_id);
    }
    Ok(RubyValue::Class(new_id))
}

/// The allocator a `Class#dup` copy recorded for itself -- see
/// [`OverlayEntry::allocator`].
pub(crate) fn overlay_allocator(id: ClassId) -> Option<crate::dispatch::AllocatorFn> {
    if !is_live() {
        return None;
    }
    maps().classes.read().unwrap().get(&id.0)?.allocator
}

/// One of `cid`'s registered CLASS methods as an `RProc` whose `self` is the
/// receiving class value -- the shape [`OverlayEntry::class_methods`] holds.
fn class_method_as_proc(cid: ClassId, name: Symbol) -> Option<RProc> {
    let f = crate::dispatch::own_class_method_fn(cid, name)?;
    Some(RProc::with_self_and_block(
        f.into_fn(),
        RubyValue::Nil,
        -1,
        true,
    ))
}

pub fn runtime_module_dup(mid: ClassId) -> Result<RubyValue, Signal> {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let leaked: &'static [ClassId] = Box::leak(vec![new_id].into_boxed_slice());
    // Own names only (`inherit: false`), private included: delegate.rb's
    // second pass walks `private_instance_methods` on the copy.
    // Visibility comes from the same per-filter name queries
    // `private_instance_methods` and friends answer with, so the copy reports
    // exactly what the original does. Every name is marked, including the
    // public ones: an unmarked overlay entry reads as public anyway, but a
    // mark is what survives a later `private :m` lookup on the copy alone.
    let mut methods = crate::FMap::default();
    let mut methods_vis = crate::FMap::default();
    let record =
        |name: Symbol, vis, methods: &mut crate::FMap<_, _>, vism: &mut crate::FMap<_, _>| {
            if let Some(m) = module_own_method_impl(mid, name) {
                methods.insert(name, m);
                vism.insert(name, vis);
            }
        };
    for (filter, vis) in [
        (
            crate::dispatch::VisFilter::Public,
            crate::dispatch::MethodVisibility::Public,
        ),
        (
            crate::dispatch::VisFilter::Protected,
            crate::dispatch::MethodVisibility::Protected,
        ),
        (
            crate::dispatch::VisFilter::Private,
            crate::dispatch::MethodVisibility::Private,
        ),
    ] {
        for name in crate::dispatch::instance_method_names(mid, filter, false) {
            record(name, vis, &mut methods, &mut methods_vis);
        }
    }
    // A builtin module's rows are not in the name queries above (`Kernel`'s
    // table is the whole of it), and they are public unless the runtime says
    // otherwise.
    for n in crate::builtins::class_table_names(mid) {
        let sym = Symbol::intern(n);
        if !methods.contains_key(&sym) {
            let vis = crate::dispatch::instance_method_visibility(mid, sym)
                .unwrap_or(crate::dispatch::MethodVisibility::Public);
            record(sym, vis, &mut methods, &mut methods_vis);
        }
    }
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                is_module: true,
                ancestors: leaked,
                methods,
                methods_vis,
                ..Default::default()
            },
        );
    }
    super::copy_class_extensions(mid, new_id);
    mark_live();
    Ok(RubyValue::Class(new_id))
}

/// `Module.new { body }` -- allocate a runtime MODULE id and run the optional
/// body block with `self` bound to it.
///
/// A module's ancestors are just itself. It has no superclass and no
/// constructor, so `Module.new.new` raises NoMethodError. The result composes
/// with `obj.extend`/`include`: its `define_method`-installed methods are
/// retrievable by id from the overlay.
pub fn runtime_module_new(body: Option<RProc>) -> Result<RubyValue, Signal> {
    runtime_module_new_owned(body, None)
}

/// The class a minted module is an INSTANCE of -- `Some(X)` only for one
/// `X.new` where `class X < Module`. `None` for every ordinary `Module.new`
/// and for every frozen id, so the hot `.class` path pays one overlay probe
/// and only once anything has been minted at run time at all.
///
/// This is the whole of the "class-valued instance": the value is still a real
/// module id, so `include`, `Module#===`, `ancestors` and constant lookup are
/// untouched. Only its class differs.
pub fn module_owner_class(id: ClassId) -> Option<ClassId> {
    if !is_live() {
        return None;
    }
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.owner_class)
}

/// Register a user `class X < Module`: name + linearized ancestors + the
/// shared [`module_subclass_construct`]. Like the other struct-less shapes it
/// installs no methods -- `X`'s own `def`s register as `RubyValue`-self
/// `define_method` deltas (`Compiler::is_native_backed` covers it).
pub fn register_module_subclass(
    registry: &mut crate::dispatch::ClassRegistry,
    id: ClassId,
    name: &str,
    ancestors: Vec<ClassId>,
) {
    registry.register(
        id,
        name,
        false,
        ancestors,
        Some(module_subclass_construct as crate::dispatch::ConstructorFn),
    );
}

/// Whether the module subclass `owner` (or an ancestor of it BELOW `Module`)
/// defines `name` as one of its own instance methods.
///
/// `value_method`, not `has_instance_method`: a module subclass's methods take
/// a `RubyValue::Class` self, so they register into `value_methods`, which the
/// `methods` table `has_instance_method` reads never sees. Stopping at `Module`
/// is what keeps `Module`'s own defaults out -- they are not what the user
/// wrote, and running them here would be wrong for both callers.
pub(crate) fn module_subclass_defines(owner: ClassId, name: Symbol) -> bool {
    crate::dispatch::ancestors_of_value(owner)
        .iter()
        .take_while(|&&a| a != zeo_abi::MODULE_CLASS)
        .any(|&a| crate::dispatch::value_method(a, 0, name).is_some())
}

/// The `ConstructorFn` behind every `class X < Module`. Mints a real runtime
/// module tagged as an instance of `X`, then runs `X`'s own `initialize`
/// against it with the MODULE as `self` -- which is what Rails'
/// `DeprecatedConstantProxy#initialize` expects when it stores `@old_const`.
///
/// The result is a `RubyValue::Class`, so `include X.new(...)` reaches
/// `runtime_include` unchanged and the user's `included` hook fires from
/// `fire_mixin_hook` like any module's.
pub fn module_subclass_construct(
    class_id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let val = runtime_module_new_owned(None, Some(class_id))?;
    let init = Symbol::intern("initialize");
    // A USER `initialize` only: `Module`'s own takes no arguments and would
    // raise on `X.new(attrs)`.
    if module_subclass_defines(class_id, init) {
        crate::dispatch::send_value_in(0, &val, init, args, block)?;
    }
    Ok(val)
}

/// [`runtime_module_new`] with the minted module tagged as an instance of
/// `owner` -- what `X.new` runs for a `class X < Module`.
pub fn runtime_module_new_owned(
    body: Option<RProc>,
    owner: Option<ClassId>,
) -> Result<RubyValue, Signal> {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let leaked: &'static [ClassId] = Box::leak(vec![new_id].into_boxed_slice());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                is_module: true,
                ancestors: leaked,
                owner_class: owner,
                ..Default::default()
            },
        );
    }
    mark_live();
    let val = RubyValue::Class(new_id);
    if let Some(b) = body {
        // `Module.new` reaches its body through `Class#new` -> `Module#initialize`
        // -- the allocator is Class's, the initializer Module's. See
        // `runtime_class_new` for the pair it mirrors.
        let _new = crate::frames::synthetic_c_frame("Class#new");
        let _init = crate::frames::synthetic_c_frame("Module#initialize");
        with_body_frame(new_id, || b.call_with_self(&val, &[]))?;
    }
    Ok(val)
}

/// A RUNTIME `refine(target) { body }` (Module's private `refine`, reached
/// from a `Module.new` body or any other runtime module context): mints the
/// holder module, marks it a refinement of `(module, target)`, and runs the
/// body with the holder as self and definee -- so its `def`s land on the
/// holder, exactly like a module body. Definition only: `Module#refinements`
/// reports it and nothing changes dispatch until a `using`.
pub fn runtime_refine(
    module: ClassId,
    target: ClassId,
    body: &crate::RProc,
) -> Result<RubyValue, Signal> {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let leaked: &'static [ClassId] = Box::leak(vec![new_id].into_boxed_slice());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                is_module: true,
                ancestors: leaked,
                refinement_of: Some((module, target)),
                ..Default::default()
            },
        );
    }
    mark_live();
    let val = RubyValue::Class(new_id);
    with_body_frame(new_id, || body.call_with_self(&val, &[]))?;
    Ok(val)
}

/// `Refinement#import_methods(*modules)` -- CRuby copies each module's OWN
/// method entries into the refinement, re-compiled under the refinement's
/// cref (eval.c:1863), refusing methods not defined in Ruby. zeo's bodies
/// are Rust fns with no cref to re-bind, so the copy is the resolved body
/// itself and every method qualifies; the module doc records that copied
/// bodies do not see the refinement's own refinements. Ancestors are NOT
/// imported, with CRuby's warning.
pub fn refinement_import_methods(
    holder: &RubyValue,
    modules: &[RubyValue],
) -> Result<RubyValue, Signal> {
    let RubyValue::Class(hid) = holder else {
        return Err(runtime_error!("import_methods on a non-module refinement"));
    };
    // Every argument is validated BEFORE anything imports (CRuby's shape).
    for m in modules {
        let ok = matches!(m, RubyValue::Class(id)
            if crate::dispatch::class_is_module(*id) == Some(true));
        if !ok {
            return Err(crate::builtins::type_error!(
                "wrong argument type {} (expected Module)",
                crate::builtins::check_type_name(m)
            ));
        }
    }
    for m in modules {
        let RubyValue::Class(mid) = m else {
            unreachable!()
        };
        if ancestors_of_value(*mid).len() > 1
            && let Some((file, line)) = crate::frames::current_location()
        {
            eprintln!(
                "{file}:{line}: warning: {} has ancestors, but Refinement#import_methods doesn't import their methods",
                m.try_display_string()?
            );
        }
        let private: std::collections::HashSet<Symbol> = crate::dispatch::instance_method_names(
            *mid,
            crate::dispatch::VisFilter::Private,
            false,
        )
        .into_iter()
        .collect();
        let protected: std::collections::HashSet<Symbol> = crate::dispatch::instance_method_names(
            *mid,
            crate::dispatch::VisFilter::Protected,
            false,
        )
        .into_iter()
        .collect();
        for name in
            crate::dispatch::instance_method_names(*mid, crate::dispatch::VisFilter::All, false)
        {
            let Some(body) = snapshot_instance_method(*mid, name) else {
                continue;
            };
            // The VALUE-shaped copy as well: a refinement of `String` is
            // reached with a `RubyValue::Str` receiver, which has no `RObj` for
            // the `MethodImpl` above to bind against. A compiled user module
            // emits a value bridge per method, and it takes `self` as a plain
            // `RubyValue`, so it passes straight through.
            let value_body = crate::dispatch::value_method(*mid, 0, name)
                .map(|f| RProc::with_self_and_block(f.into_fn(), RubyValue::Nil, -1, true));
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(hid.0).or_insert_with(OverlayEntry::delta);
            e.methods.insert(name, body);
            if let Some(vb) = value_body {
                e.value_bodies.insert(name, vb);
            }
            e.undefs.remove(&name);
            e.removed.remove(&name);
            if private.contains(&name) {
                e.methods_vis
                    .insert(name, crate::dispatch::MethodVisibility::Private);
            } else if protected.contains(&name) {
                e.methods_vis
                    .insert(name, crate::dispatch::MethodVisibility::Protected);
            } else {
                e.methods_vis.remove(&name);
            }
        }
        patch_class(*hid);
    }
    mark_live();
    Ok(holder.clone())
}

/// The overlay twin of `dispatch::refinement_of` -- `(refining module,
/// refined target)` for a holder a runtime `refine` minted.
pub fn overlay_refinement_of(id: ClassId) -> Option<(ClassId, ClassId)> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.refinement_of)
}

/// The holders a runtime `refine` minted for `module`, in declaration order
/// (ids are handed out sequentially) -- `Module#refinements`' overlay half.
pub fn overlay_refinements_of(module: ClassId) -> Vec<ClassId> {
    let mut holders: Vec<ClassId> = maps()
        .classes
        .read()
        .unwrap()
        .iter()
        .filter(|(_, e)| e.refinement_of.is_some_and(|(m, _)| m == module))
        .map(|(&id, _)| ClassId(id))
        .collect();
    holders.sort_by_key(|c| c.0);
    holders
}

/// `Class.new(superclass) { body }` -- allocate a runtime class id, register
/// its overlay entry (ancestors linearized from the superclass, a generic
/// name-keyed-object constructor), then run the body block with `self` bound to
/// the new class so `define_method`/`include`/const-assign inside populate it.
/// The `ConstructorFn` a class MINTED at run time takes, chosen from its
/// linearized chain -- shared by `Class.new(Base)` and `Class#dup`, which have
/// the same problem: instances must carry the RUNTIME id, so the source's own
/// constructor (which stamps the source's id) is exactly wrong.
fn minted_constructor(chain: &'static [ClassId]) -> ConstructorFn {
    if chain
        .iter()
        .copied()
        .any(crate::builtins::value_subclass::is_payload_root)
    {
        crate::builtins::value_subclass::value_subclass_construct
    } else if chain.contains(&zeo_abi::EXCEPTION_CLASS) {
        crate::builtins::exception::exception_construct
    } else if chain
        .iter()
        .any(|&a| crate::dispatch::registry_allocator(a).is_some())
    {
        // `Class.new(CompiledBase)`: instances must be the compiled
        // ancestor's real struct (stamped with the runtime id) or every
        // inherited compiled method's downcast aborts.
        compiled_subclass_construct
    } else {
        dyn_object_construct
    }
}

pub fn runtime_class_new(
    superclass: Option<RubyValue>,
    body: Option<RProc>,
) -> Result<RubyValue, Signal> {
    // CRuby's `rb_check_inheritable`, in its order. Note what is LEGAL:
    // `Class.new(Module)` and `Class.new(BasicObject)` both work -- `Module`
    // is a Class, it is just not a module INSTANCE. Only `Class` itself and
    // a singleton class are refused, and both were minted here before.
    let super_id = match &superclass {
        None => ClassId(0), // default super is Object
        Some(RubyValue::Class(cid)) => {
            if crate::dispatch::class_is_module(*cid).unwrap_or(false) {
                return Err(type_error!(
                    "superclass must be an instance of Class (given an instance of Module)"
                ));
            }
            if *cid == zeo_abi::CLASS_CLASS {
                return Err(type_error!("can't make subclass of Class"));
            }
            if singleton_owner_value(*cid).is_some() {
                return Err(type_error!("can't make subclass of singleton class"));
            }
            *cid
        }
        Some(other) => {
            return Err(type_error!(
                "superclass must be an instance of Class (given an instance of {})",
                crate::builtins::class_name_of(other)
            ));
        }
    };

    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);

    // ancestors = [self, *superclass.ancestors], leaked to 'static.
    let super_chain = ancestors_of_value(super_id);
    let mut anc = Vec::with_capacity(super_chain.len() + 1);
    anc.push(new_id);
    anc.extend_from_slice(super_chain);
    let leaked: &'static [ClassId] = Box::leak(anc.into_boxed_slice());

    // `Class.new(String)` needs the payload-carrying instance the value bridge
    // dispatches against, not a name-keyed `DynObject` -- exactly as a compiled
    // `class Tag < String` registers. Read off `leaked` rather than through
    // `value_root_of`, which resolves the ancestry via an overlay this entry
    // isn't in yet.
    // An EXCEPTION subclass needs the native `RubyException` allocator for the
    // same reason: every `Exception` method it inherits reads that payload, so a
    // name-keyed `DynObject` would satisfy `is_a?` and then fail on `#message`.
    let constructor = minted_constructor(leaked);
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                ancestors: leaked,
                constructor: Some(constructor),
                ..Default::default()
            },
        );
    }
    mark_live();

    let class_val = RubyValue::Class(new_id);
    // CRuby fires `inherited` on the superclass at creation -- before the
    // body block runs and before any constant names the class (the hook sees
    // `name == nil`). minitest's whole Runnable registry IS this hook, fired
    // by every `describe` block's `Class.new(Minitest::Spec)`. The default
    // `Class#inherited` is a no-op row, so an unconditional send is safe.
    crate::dispatch::send_value(
        &RubyValue::Class(super_id),
        Symbol::intern("inherited"),
        std::slice::from_ref(&class_val),
        None,
    )?;
    if let Some(b) = body {
        // The body runs two C frames deep in CRuby (`Class.new` calls
        // `Class#initialize`, which yields), and a raise from inside it shows
        // both -- so a backtrace here has to as well.
        let _new = crate::frames::synthetic_c_frame("Class#new");
        let _init = crate::frames::synthetic_c_frame("Class#initialize");
        with_body_frame(new_id, || b.call_with_self(&class_val, &[]))?;
    }
    Ok(class_val)
}

/// `Class.allocate` -- a class with NO superclass, because `Class#initialize`
/// is what installs one and it has not run. Its ancestry is just itself, so it
/// inherits nothing, answers no instance method, and cannot instantiate.
///
/// Only `Marshal` and the reflection corners reach this. It exists so
/// `Class.allocate` answers the object ruby answers rather than the TypeError
/// the generic `Class#allocate` raises for a class with no allocator.
pub fn runtime_class_allocate() -> RubyValue {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let leaked: &'static [ClassId] = Box::leak(vec![new_id].into_boxed_slice());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                ancestors: leaked,
                uninitialized: true,
                ..Default::default()
            },
        );
    }
    mark_live();
    RubyValue::Class(new_id)
}

/// Whether `id` is a class `Class.allocate` handed out and nothing initialized.
pub fn class_is_uninitialized(id: ClassId) -> bool {
    is_live()
        && maps()
            .classes
            .read()
            .unwrap()
            .get(&id.0)
            .is_some_and(|e| e.uninitialized)
}

/// Mint a fresh runtime class rooted at `root` (e.g. `STRUCT_CLASS`/
/// `DATA_CLASS`), pre-populated with `methods` and a native `constructor` --
/// the counterpart of `runtime_class_new` for a native-backed class the user
/// never wrote a `class` body for (Batch E's `Struct.new`/`Data.define`). The
/// ancestry is `[new_id, *root.ancestors]`, leaked to `'static` like every
/// other runtime class's.
pub fn intern_native_class(
    root: ClassId,
    methods: FMap<Symbol, MethodImpl>,
    constructor: ConstructorFn,
) -> ClassId {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let super_chain = ancestors_of_value(root);
    let mut anc = Vec::with_capacity(super_chain.len() + 1);
    anc.push(new_id);
    anc.extend_from_slice(super_chain);
    let leaked: &'static [ClassId] = Box::leak(anc.into_boxed_slice());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                ancestors: leaked,
                methods,
                constructor: Some(constructor),
                ..Default::default()
            },
        );
    }
    mark_live();
    new_id
}

/// Name a runtime class the first time it's assigned to a constant
/// (`Foo = Class.new`). A no-op for a frozen id or an already-named runtime
/// class (CRuby names on FIRST binding only).
pub fn name_runtime_class_if_anonymous(id: ClassId, name: &str) {
    if id.0 < RUNTIME_CLASS_ID_BASE || !is_live() {
        return;
    }
    if let Some(entry) = maps().classes.read().unwrap().get(&id.0) {
        let mut slot = entry.name.write().unwrap();
        if slot.is_none() {
            *slot = Some(name.to_string());
            // A named class is reachable as a nested constant of its namespace
            // (`constants::nested_class_of` asks the registry, not the table),
            // so naming one changes what a constant lookup can find.
            crate::constants::bump_const_epoch();
        }
    }
}

/// The runtime classes whose current name came from `set_temporary_name`
/// rather than from a constant binding. CRuby's rule is that only a name
/// reachable by a CONSTANT PATH is permanent, and a temporary one may be
/// replaced or cleared; nothing else in this runtime needs to tell the two
/// apart, so the distinction lives in this side set rather than in the entry.
static TEMPORARY_NAMES: std::sync::LazyLock<parking_lot::Mutex<crate::FSet<u32>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(crate::FSet::default()));

/// `Module#set_temporary_name`'s storage half. `false` when the class already
/// carries a PERMANENT name (every compile-time class, and any runtime class
/// already bound to a constant), which the caller turns into CRuby's
/// `RuntimeError: can't change permanent name`. `None` clears the name, making
/// the class anonymous again.
pub fn set_temporary_class_name(id: ClassId, name: Option<String>) -> bool {
    if id.0 < RUNTIME_CLASS_ID_BASE || !is_live() {
        return false;
    }
    let classes = maps().classes.read().unwrap();
    let Some(entry) = classes.get(&id.0) else {
        return false;
    };
    let mut temporary = TEMPORARY_NAMES.lock();
    let mut slot = entry.name.write().unwrap();
    if slot.is_some() && !temporary.contains(&id.0) {
        return false;
    }
    match &name {
        Some(_) => temporary.insert(id.0),
        None => temporary.remove(&id.0),
    };
    *slot = name;
    drop(slot);
    drop(temporary);
    drop(classes);
    crate::constants::bump_const_epoch();
    true
}
