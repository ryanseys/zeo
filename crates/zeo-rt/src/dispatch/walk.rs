//! The instance-channel MRO walk: `super` dispatch (`send_super_from`,
//! `send_walking`), the re-seated `Method` entries, the `defined?(super)`
//! probe, and the `MroResume` cell that tells two copies of a doubled
//! module apart.

use super::*;

/// Where a `super` written in a DUPLICATED module must resume, and which
/// copy it belongs to.
///
/// A module both `include`d and `prepend`ed into one class occupies TWO
/// positions in the chain, and ruby runs its body once per position. The
/// baked `defining_class` a compiled body hands `super` names the module,
/// which cannot say WHICH copy is running -- so a first-match walk resumes
/// past the first copy forever. CRuby has the answer on the control frame
/// (`cfp->cme->defined_class` is the iclass, not the module); this cell is
/// the same fact, published by the walk that entered the body.
#[derive(Clone, Copy)]
pub(crate) struct MroResume {
    defining: ClassId,
    next: usize,
}

std::thread_local! {
    /// The resume for the body currently running, or `None` when it was
    /// entered by ordinary dispatch (which always finds the FIRST copy, so
    /// the first-match walk is already right). Saved and restored around
    /// every invocation that can change the answer -- see [`MRO_DUPLICATES`]
    /// for why almost no program ever touches it.
    static MRO_RESUME: std::cell::Cell<Option<MroResume>> = const {
        std::cell::Cell::new(None)
    };
}

/// Arms `runtime_meta`'s duplicate gate when `chain` holds a repeat. Called
/// wherever a linearized chain is installed: registration, `set_ancestors`,
/// and the run-time splice.
pub(crate) fn note_chain(chain: &[ClassId]) {
    if mro_duplicates() {
        return;
    }
    let mut seen = FSet::default();
    if chain.iter().any(|&a| !seen.insert(a)) {
        crate::runtime_meta::mark_mro_duplicates();
    }
}

/// Whether any chain in this process holds a class twice.
#[inline]
pub(super) fn mro_duplicates() -> bool {
    crate::runtime_meta::mro_duplicates()
}

/// Runs `f` with the resume cell set to `value`, restoring the caller's on
/// every exit path. `None` is what ORDINARY dispatch installs: it always
/// lands on the first copy, so a `super` from the body it entered resumes
/// past that one.
pub(super) fn with_mro_resume<T>(value: Option<MroResume>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<MroResume>);
    impl Drop for Restore {
        fn drop(&mut self) {
            MRO_RESUME.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(MRO_RESUME.with(|c| c.replace(value)));
    f()
}

/// The ancestry index a `super` written in `defining_class` resumes at, or
/// `None` when this chain does not pass through `defining_class` at all.
///
/// The published resume when it names this very copy, and otherwise the
/// position after the first occurrence -- which is the whole answer for
/// every chain that holds `defining_class` once.
fn super_resume(ancestors: &[ClassId], defining_class: ClassId) -> Option<usize> {
    if mro_duplicates()
        && let Some(r) = MRO_RESUME.with(|c| c.get())
        && r.defining == defining_class
    {
        return Some(r.next);
    }
    ancestors
        .iter()
        .position(|&a| a == defining_class)
        .map(|p| p + 1)
}

/// The fiber swap for [`MRO_RESUME`] -- see `crate::ec`.
pub(crate) fn swap_mro_resume(v: Option<MroResume>) -> Option<MroResume> {
    MRO_RESUME.with(|c| c.replace(v))
}

/// `defined?(super)`'s probe: whether a super target exists for `name` past
/// `defining_class` on `recv`'s chain -- the same resolution
/// [`send_super_from`] walks, answered as a boolean instead of a call.
pub fn super_defined(recv: &RubyValue, defining_class: ClassId, name: Symbol) -> bool {
    // A CLASS receiver's `super` walks the CLASS-method chain -- the same
    // split [`send_super_from`] makes one function below, and for the same
    // reason: the object channel asks `Class`'s own instance ancestry, which
    // holds no `def self.x` and so answers no every time.
    if let RubyValue::Class(cid) = recv {
        return super_class_defined(*cid, defining_class, name);
    }
    // An unrecognized `defining_class` -- a `define_method` body installed on
    // a module, which has no defining class of its own -- answers NO. Walking
    // from the top instead would find the body's own row and say yes.
    let ancestors = ancestors_of_value(recv.class_id());
    super_resume(ancestors, defining_class)
        .and_then(|from| crate::dispatch::reflect::scan_owner_from(recv.class_id(), from, name))
        .is_some()
}

/// The `super` dispatch for an exception-backed receiver.
///
/// Resumes the MRO walk in the RECEIVER's own ancestors, at the entry after
/// `defining_class` -- the class whose body this `super` is written in -- and
/// invokes the first `name` registered there, whether a native default or a
/// user delta.
///
/// Codegen's `emit_super_inline` HIR splice cannot serve a `super` into a
/// native exception method. An exception's message lives in a hidden slot,
/// independent of any `@message` ivar, so the retained `Exception#initialize`
/// HIR would set a visible ivar and leave the real message untouched. Walking
/// the registry, where every exception id carries the native `exc_*` fns,
/// runs the true behaviour instead.
///
/// This is also the shape a run-time `eval` needs for `super`, so it lands here
/// rather than in codegen.
pub fn send_super_from(
    recv: &RubyValue,
    defining_class: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // A CLASS object in the OBJECT channel means the defining body ran with a
    // class as `self` -- a module instance method serving as a class method
    // through a runtime extend/singleton-prepend wrapper. Its `super` walks
    // the receiver's CLASS-method chain; the object channel would walk
    // `Class`'s own instance ancestry and miss.
    if let RubyValue::Class(cid) = recv {
        return send_super_class_from(*cid, defining_class, name, args, block);
    }
    // Resume AFTER the class this `super` is written in; an unrecognized
    // `defining_class` (never expected) degrades to a full walk from the top.
    // A module the chain holds TWICE resumes past the copy that is actually
    // running -- see `super_resume`.
    let ancestors = ancestors_of_value(recv.class_id());
    let start = super_resume(ancestors, defining_class).unwrap_or(0);
    send_walking(recv, start, name, args, block)
}

/// [`send_as_defined_in`] with the runtime overlay skipped -- the body
/// `owner` defines BELOW any later runtime redefinition. What a frozen
/// `Method`/`UnboundMethod` position calls, so a wrapper installed since
/// (the `method_added` wrap idiom) cannot capture it and recurse.
pub fn send_below_overlay_at(
    recv: &RubyValue,
    owner: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    if let RubyValue::Object(obj) = recv
        && let Some(m) = registry().super_target(owner, name)
    {
        return m.call(obj, args, block);
    }
    if let Some(r) = probe_generic_row(recv, owner, name, args, block.clone()) {
        return r;
    }
    send_value(recv, name, args, block)
}

/// Invoke `name` AS `owner` defines it, against `recv` -- what a `Method`
/// that `#super_method` re-seated calls, so
/// `Sub.new.method(:greet).super_method.call` runs `Base#greet` rather than
/// re-dispatching to the `Sub#greet` it was reached through.
///
/// It reads the same `own_impls` set a `super` walk does, and for the same
/// reason: a user class's materialized `methods` row downcasts `self` to that
/// class's own generated struct, so `Base`'s row cannot run against a `Sub`
/// instance. `own_impls` holds the receiver-generic bridge instead. Codegen
/// emits those bridges for every method name a `super` -- or a
/// `#super_method` anywhere in the program -- can reach.
pub fn send_as_defined_in(
    recv: &RubyValue,
    owner: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    if let RubyValue::Object(obj) = recv {
        if crate::runtime_meta::is_live()
            && let Some(m) = crate::runtime_meta::overlay_own_method(owner, name)
        {
            return m.call(obj, args, block);
        }
        if let Some(m) = registry().super_target(owner, name) {
            return m.call(obj, args, block);
        }
    }
    if let Some(r) = probe_generic_row(recv, owner, name, args, block.clone()) {
        return r;
    }
    // Nothing receiver-generic at the owner: fall back to ordinary dispatch
    // rather than raising, so the call still answers SOMETHING.
    send_value(recv, name, args, block)
}

/// Probe `anc`'s receiver-generic rows -- its VALUE methods, then its
/// registered builtin table -- and run the hit. A value subclass reaching an
/// ancestor its payload owns runs the row against the PAYLOAD and re-wraps a
/// self-return, the same bridge ordinary dispatch's probes apply: a payload
/// root's native rows downcast the receiver, and the boxed subclass panicked
/// them (`super` through an included module into `Array#size`).
fn probe_generic_row(
    recv: &RubyValue,
    anc: ClassId,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Option<Result<RubyValue, Signal>> {
    // The FOREIGN rows on `anc` are skipped: a reopened builtin registers its
    // whole flattened table, so the row sitting at this position can be the
    // very prepended module the `super` came FROM. See
    // `ClassEntry::foreign_value_names`.
    let f = value_method(anc, 0, name)
        .filter(|_| !value_row_is_foreign(anc, name))
        .or_else(|| {
            crate::builtins::class_table(anc)
                .and_then(|t| t(name.name_str()))
                .map(ValueImpl::Rust)
        })?;
    if let RubyValue::Object(o) = recv
        && let Some(root) = o.builtin_root()
        && crate::builtins::value_subclass::payload_owns(root, anc)
        && let Some(p) = o.builtin_payload()
    {
        if let Some(k) = crate::builtins::value_subclass::wrapper_row(name) {
            return Some(k(recv, args, block));
        }
        let result = match f.call(&p, args, block) {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        return Some(Ok(crate::builtins::value_subclass::rewrap_self_return(
            result,
            &p,
            o,
            name.name_str(),
        )));
    }
    Some(f.call(recv, args, block))
}

/// Whether `anc`'s registered value row for `name` was contributed by an
/// ANCESTOR rather than defined on `anc` -- see
/// [`ClassEntry::foreign_value_names`].
fn value_row_is_foreign(anc: ClassId, name: Symbol) -> bool {
    REGISTRY.get().is_some_and(|r| {
        r.entries
            .get(&anc.0)
            .is_some_and(|e| e.foreign_value_names.contains(&name))
    })
}

/// The shared body of [`send_super_from`] and [`send_as_defined_in`]: walk
/// the receiver's ancestors from `start`, invoking the first OWN definition
/// of `name`. Only the entry position differs between them.
pub(super) fn send_walking(
    recv: &RubyValue,
    start: usize,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // The two `MethodImpl` arms below take an `RObj`; a non-Object receiver
    // (an Integer, a String) never has one, and never reaches them either --
    // its definitions live in the value-method and builtin tables.
    let obj = match recv {
        RubyValue::Object(o) => Some(o.clone()),
        _ => None,
    };
    let ancestors = ancestors_of_value(recv.class_id());
    // The body this walk enters must know WHICH copy of `anc` it is running
    // as, or its own `super` restarts past the first one. Only a chain with a
    // repeat can disagree, so the publication is gated.
    let publish = |at: usize| {
        mro_duplicates().then(|| MroResume {
            defining: ancestors[at],
            next: at + 1,
        })
    };
    for (at, &anc) in ancestors.iter().enumerate().skip(start) {
        // Each position contributes its OWN definitions only -- never the
        // flattened `methods` table, whose winner at a position can be a
        // PREPENDED module's copy sitting BEFORE this position in the MRO
        // (probing it made a prepended method's `super` find itself,
        // recursing forever). Per position, most-derived source first:
        // a runtime-defined own method (the overlay), the registered own
        // implementation (`own_impls`), a user module's bridge / builtin
        // reopen (`value_methods` on that id), then the ancestor's native
        // builtin table (`BasicObject#initialize` is the one every `super`
        // chain bottoms out on).
        // An `undef_method` at this position terminates the walk, exactly as
        // it does for ordinary dispatch -- `super` must not reach past it into
        // a still-live ancestor definition.
        if crate::runtime_meta::is_live() && crate::runtime_meta::overlay_is_undefined(anc, name) {
            break;
        }
        // A `remove_method` here empties only THIS position: `super` carries
        // on to the ancestor that still defines the name.
        if crate::runtime_meta::is_live() && crate::runtime_meta::overlay_is_removed(anc, name) {
            continue;
        }
        if let Some(obj) = &obj {
            if crate::runtime_meta::is_live()
                && let Some(m) = crate::runtime_meta::overlay_own_method(anc, name)
            {
                return with_mro_resume(publish(at), || m.call(obj, args, block));
            }
            if let Some(m) = registry().super_target(anc, name) {
                return with_mro_resume(publish(at), || m.call(obj, args, block));
            }
        }
        let found = with_mro_resume(publish(at), || {
            probe_generic_row(recv, anc, name, args, block.clone())
        });
        if let Some(r) = found {
            return r;
        }
    }
    Err(raise_method_missing(
        recv,
        name.name_str(),
        args,
        MissingReason::Super,
    ))
}
