//! The protocol invoker (`call_user_method`), `run_initialize`, and the
//! `ZEO_ARITY_DEBUG` dispatch breadcrumb.

use super::*;

/// A registry probe-and-call for the runtime PROTOCOL dispatches:
/// user `==`/`<=>`/`to_s`/`inspect`/`hash` reached from deep inside
/// pure helpers (`rb_eq`, `display_with`, `hash_key`) that can't route
/// through `send` (whose `method_missing`/NoMethodError fallbacks must NOT
/// fire for a protocol probe). `None` = no such method (or no registry --
/// this crate's own unit tests), letting each caller apply its own
/// default; the method, when present, is the MATERIALIZED entry, so
/// inherited/mixed-in definitions resolve exactly like a real call.
pub(crate) fn call_user_method(
    recv: &RObj,
    name: &str,
    args: &[RubyValue],
) -> Option<Result<RubyValue, Signal>> {
    let id = recv.class_id();
    if let Some(f) = REGISTRY
        .get()
        .and_then(|r| r.lookup_mro(id, Symbol::intern(name)))
    {
        return Some(f.call(recv, args, None));
    }
    // A RUNTIME-defined method on the receiver's OWN class (a `define_method`
    // delta, or a native `Struct`/`Data` class's `inspect`/`to_s`/`hash`
    // forwarder) -- so `p`/interpolation honour it, not just `send`. THIS
    // class only, no ancestor walk (see the reentrancy note below).
    if crate::runtime_meta::is_live()
        && let Some(m) = crate::runtime_meta::overlay_own_method(id, Symbol::intern(name))
    {
        return Some(m.call(recv, args, None));
    }
    // A RUNTIME-RESIDENT class (`Time`, `File`, ... -- plan P-B) is an
    // `Object(RObj)` with no registry entry, but it does have a builtin
    // table. Without this probe its own `to_s`/`inspect`/`hash` would be
    // invisible to every caller here -- stringifying a Time would answer
    // `#<Time>` rather than running `Time#to_s`.
    //
    // The receiver's OWN class only, deliberately NOT its ancestors: this is
    // called from `display_with`/`inspect_with`, and `Kernel`'s own `to_s`
    // row renders via `to_display_string` -- walking up to it would recurse
    // until the stack died (it did). The ancestor walk belongs to `send_in`,
    // which has no such reentrancy; what this needs is only "does THIS class
    // define the method itself".
    if let Some(f) = crate::builtins::class_table(id).and_then(|lookup| lookup(name)) {
        return Some(f(&RubyValue::Object(recv.clone()), args, None));
    }
    // ...and an ANCESTOR's builtin table, for a builtin SUBCLASS of a builtin
    // (`DateTime < Date`, whose own table declares only the rows CRuby
    // publishes on the subclass -- `p dt` reached `Object`'s default repr
    // because `Date#inspect` sits one class up).
    //
    // Bounded at `Object`, which is the whole reason the probe above is
    // own-class-only: `Kernel`'s `to_s`/`inspect` rows render through
    // `to_display_string`, so walking into them recurses until the stack dies.
    // A value-subclass payload is excluded too -- `class Tag < String` renders
    // through the caller's payload arm, not through `String`'s rows.
    if recv.builtin_payload().is_none()
        && let Some(reg) = REGISTRY.get()
    {
        for &anc in reg.ancestors_of(id).iter().skip(1) {
            if anc == zeo_abi::OBJECT_CLASS
                || anc == zeo_abi::KERNEL_CLASS
                || anc == zeo_abi::BASIC_OBJECT_CLASS
            {
                break;
            }
            if let Some(f) = crate::builtins::class_table(anc).and_then(|lookup| lookup(name)) {
                return Some(f(&RubyValue::Object(recv.clone()), args, None));
            }
        }
    }
    // A COMPILED `Struct`/`Data` inherits its whole protocol from
    // `Struct`/`Data`'s own table, which the walk above cannot see: those rows
    // are value methods, not registry `methods`. `S.new(1) == S.new(1)`
    // reaches `Struct#==` only here, because `rb_eq` asks this function and
    // nothing else.
    //
    // Exactly those two tables, never a general ancestor walk: probing
    // `String`'s on a `class Tag < String` recurses until the stack dies.
    if crate::builtins::rstruct::meta_of(id).is_some() {
        for root in [zeo_abi::STRUCT_CLASS, zeo_abi::DATA_CLASS] {
            if !ancestors_contain(id, root) {
                continue;
            }
            if let Some(f) = crate::builtins::class_table(root).and_then(|lookup| lookup(name)) {
                return Some(f(&RubyValue::Object(recv.clone()), args, None));
            }
        }
    }
    None
}

/// A registry-optional ancestry probe -- `false` when no
/// registry is installed (this crate's own unit tests), where `is_a`'s
/// hard `registry()` access would panic. Used by `rb_eq`'s Comparable
/// fallback, which must stay callable from anywhere.
pub(crate) fn ancestors_contain(id: ClassId, target: ClassId) -> bool {
    REGISTRY
        .get()
        .is_some_and(|r| r.ancestors_of(id).contains(&target))
}

/// Runs `initialize` on a freshly-constructed instance IF the class
/// defines one -- the shared tail of every `ConstructorFn` (`ruby_class!`'s
/// `__construct`). Deliberately a direct registry lookup, NOT `send`: a
/// class with no `initialize` must not trip the `method_missing` fallback.
/// With no `initialize`, arguments are rejected (real Ruby's
/// `Object#initialize` takes none) with a rescuable `ArgumentError`.
pub fn run_initialize(
    class: ClassId,
    recv: &RObj,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<(), Signal> {
    // With a live overlay, `initialize` resolves like any other instance
    // method -- overlay delta first, per ancestor (`snapshot_instance_method`).
    // A runtime `define_method :initialize` -- or a lazily-loaded unit's
    // `def initialize`, registered but not promised -- lives ONLY there, and
    // the registry walk below would fall through to `BasicObject#initialize`'s
    // zero-arity reject.
    if crate::runtime_meta::is_live()
        && let Some(f) =
            crate::runtime_meta::snapshot_instance_method(class, crate::symbol::wk::initialize())
    {
        f.call(recv, args, block)?;
        return Ok(());
    }
    let init = crate::symbol::wk::initialize();
    // Per ancestor, most-derived first, BOTH channels: the object-channel row
    // (`lookup`) and the VALUE row a builtin REOPEN registers. `lookup_mro`
    // reads only the first, so a namespace-slot builtin whose whole body is
    // Ruby (`WeakRef`, defined in the vendored gem) was walked straight past
    // into its SUPERCLASS's `initialize`.
    for &anc in ancestors_of_value(class) {
        if crate::runtime_meta::is_live() && crate::runtime_meta::overlay_is_undefined(anc, init) {
            break;
        }
        if crate::runtime_meta::is_live() && crate::runtime_meta::overlay_is_removed(anc, init) {
            continue;
        }
        if let Some(f) = registry().lookup(anc, init) {
            f.call(recv, args, block)?;
            return Ok(());
        }
        if let Some(f) = value_method(anc, 0, init) {
            f.call(&RubyValue::Object(recv.clone()), args, block)?;
            return Ok(());
        }
    }
    // A RUNTIME class id (`Class.new(StandardError)`) is in no registry entry at
    // all, so the walk above cannot even reach its ancestors. Its chain lives in
    // the overlay, which is where a caller holding a bare class -- rather than a
    // receiver `send_in` could resolve from -- has to ask.
    if crate::runtime_meta::is_live()
        && let Some(f) =
            crate::runtime_meta::runtime_class_method(class, crate::symbol::wk::initialize())
    {
        f.call(recv, args, block)?;
        return Ok(());
    }
    // No user `initialize` at all, so the inherited `Object#initialize`
    // takes no arguments. A RAISE, not a panic (plan G1): real Ruby resolves
    // arity at runtime and the error is rescuable -- `rescue ArgumentError`
    // around a bad `.new` is a corpus idiom, and a panic is uncatchable.
    // Message shape oracle-verified: CRuby says "wrong number of arguments
    // (given 2, expected 0)" with no method name in it, and the backtrace's
    // innermost row is `'BasicObject#initialize'` at the caller's line.
    if !args.is_empty() {
        let __frame = crate::frames::synthetic_c_frame("BasicObject#initialize");
        return Err(arg_error!(
            "wrong number of arguments (given {}, expected 0)",
            args.len()
        ));
    }
    Ok(())
}

thread_local! {
    /// The method most recently entered via `send_in`/`send_value_in` -- the
    /// diagnostic breadcrumb `ZEO_ARITY_DEBUG` reads to name which method an
    /// "wrong number of arguments" error came from (the message CRuby, and so
    /// we, deliberately leave method-less). Set at dispatch entry, so at the
    /// point a builtin's `arity!` guard raises it still names that builtin.
    static CURRENT_METHOD: std::cell::Cell<Option<Symbol>> = const { std::cell::Cell::new(None) };
}

/// Whether `ZEO_ARITY_DEBUG` is set -- read once, so the cold attribution
/// path (`arity_debug_context`) costs a
/// bool load per dispatch instead of an unconditional thread-local write.
static ARITY_DEBUG: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("ZEO_ARITY_DEBUG").is_some());

/// Record the method being dispatched (for `ZEO_ARITY_DEBUG`). The guard bit
/// rides in the gate byte the dispatch path already loads
/// (`runtime_meta::gates_arity_debug`, armed once at startup) -- the old
/// `LazyLock<bool>` was a second hot-path flag word, the exact shape that
/// once measured 2.6% on dispatch.
#[inline]
pub(super) fn note_dispatch(name: Symbol) {
    note_dispatch_gated(crate::runtime_meta::gates(), name);
}

/// [`note_dispatch`] for a caller that already has the gates byte in a
/// register. The two cached send paths load it once at entry to decide the
/// fast route and then called `note_dispatch`, which loaded it again -- an
/// atomic re-read per hit on the hottest path in every generated program.
#[inline]
pub(super) fn note_dispatch_gated(gates: u16, name: Symbol) {
    if crate::runtime_meta::gates_arity_debug(gates) {
        CURRENT_METHOD.with(|c| c.set(Some(name)));
    }
}

/// Under `ZEO_ARITY_DEBUG`, append ` [method: X]` to an arity error so the
/// conformance triage can attribute the otherwise method-less
/// "wrong number of arguments" pile. A no-op (and no env lookup on the common
/// path) unless the message is an arity error.
pub(super) fn arity_debug_context(msg: String) -> String {
    if !msg.starts_with("wrong number of arguments") || !*ARITY_DEBUG {
        return msg;
    }
    match CURRENT_METHOD.with(|c| c.get()) {
        Some(sym) => format!("{msg} [method: {}]", sym.name_str()),
        None => msg,
    }
}
