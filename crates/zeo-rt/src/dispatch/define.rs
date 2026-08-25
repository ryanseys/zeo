//! Runtime definition verbs: the default definee for an expression-position
//! `def`/`alias`, the eval definee, and the builtin-alias rows with their
//! startup validation.

use super::*;

/// A `def` in expression position (inside a block) installs on the "default
/// definee", which in ruby belongs to the FRAME and is NOT `self`. An ordinary
/// block changes it not at all, so it stays the CREF's -- `cref`, which
/// codegen knows outright. Two callers replace it, and only the runtime knows
/// they are on the stack:
///
/// * `instance_eval`/`instance_exec` -- the receiver's singleton class. That
///   is how `SingleForwardable` installs its delegators: it builds a
///   `proc { def name(...) ... end }` and `instance_eval`s it.
/// * `class_eval`/`class_exec`/`Class.new`/`Module.new` -- the module itself,
///   which is why `Class.new { def built; end }` gives the ANONYMOUS class an
///   instance method though the block was written at the top level.
///
/// Deriving the definee from `self` instead answers all three of those
/// correctly and every other block wrongly: `[1].each { def m; end }` and a
/// `def` inside a `def` both came out as singleton methods of whatever object
/// happened to be self, so `Object` never gained them.
pub fn define_in_default_definee(
    cref: &RubyValue,
    slf: &RubyValue,
    name: Symbol,
    body: RubyValue,
    private: bool,
) -> Result<RubyValue, Signal> {
    define_in_default_definee_vis(cref, slf, name, body, private, None)
}

/// Install an eval'd `def` -- see `capi::objects::zeo_rt_eval_define`.
/// `mode` is `eval::EvalMode` as a byte (0 Caller, 1 ClassEval,
/// 2 InstanceEval).
pub fn eval_define(
    mode: u8,
    slf: &RubyValue,
    name: Symbol,
    body: RubyValue,
    vis: u8,
) -> Result<RubyValue, Signal> {
    let (definee, singleton) = eval_definee_parts(mode, slf);
    let installer = if singleton {
        "define_singleton_method"
    } else {
        "define_method"
    };
    let out = send_value(
        &definee,
        Symbol::intern(installer),
        &[RubyValue::Symbol(name), body],
        None,
    )?;
    // The snippet's own running visibility default, applied to whatever
    // definee was resolved -- `class_eval("private; def x; end")` marks
    // `x` private exactly as the same two lines in a class body do.
    if let Some(v) = match vis {
        1 => Some(MethodVisibility::Private),
        2 => Some(MethodVisibility::Protected),
        _ => None,
    } && let RubyValue::Class(id) = &definee
    {
        crate::runtime_meta::runtime_set_visibility(*id, &[RubyValue::Symbol(name)], v)?;
    }
    // Only the TOP level's `def` is private, and the top level is exactly
    // where `self` is the main object -- a `def` inside an eval in a
    // method is an ordinary public method of that method's class.
    if mode == 0
        && crate::builtins::basic_object::value_identity(slf, &crate::main_object())
        && let RubyValue::Class(id) = &definee
    {
        crate::runtime_meta::runtime_set_visibility(
            *id,
            &[RubyValue::Symbol(name)],
            MethodVisibility::Private,
        )?;
    }
    Ok(out)
}

/// Where a `def`, `alias`, `undef` or visibility statement written in a
/// run-time `eval` lands, and whether that definee is a SINGLETON.
///
/// The RUN TIME decides first, and it has to: the statement may be sitting
/// in a proc the eval only BUILT, which something else then runs under an
/// `instance_eval`/`class_eval` of its own (forwardable's
/// `_delegator_method` is exactly that shape). Only when nothing has
/// opened a definee does the eval's own mode answer.
fn eval_definee_parts(mode: u8, slf: &RubyValue) -> (RubyValue, bool) {
    if crate::runtime_meta::singleton_definee(slf) {
        return (slf.clone(), true);
    }
    if let Some(cid) = crate::runtime_meta::module_definee(slf) {
        return (RubyValue::Class(cid), false);
    }
    match (mode, slf) {
        (2, _) => (slf.clone(), true),
        (_, RubyValue::Class(_)) => (slf.clone(), false),
        _ => (RubyValue::Class(slf.class_id()), false),
    }
}

/// The definee itself, as the receiver a definition-level send names --
/// `alias_method`, `undef_method`, `private`, `module_function`. An
/// `instance_eval`'s is the receiver's SINGLETON, which is the class those
/// verbs have to reach.
pub fn eval_definee(mode: u8, slf: &RubyValue) -> Result<RubyValue, Signal> {
    match eval_definee_parts(mode, slf) {
        (v, false) => Ok(v),
        (v, true) => send_value(&v, Symbol::intern("singleton_class"), &[], None),
    }
}

/// [`define_in_default_definee`] carrying the enclosing body's RUNNING
/// visibility default, applied to whatever definee is resolved. The
/// emitter hands it here rather than stamping it itself, because a `def`
/// in a `Class.new` body installs on a class only the run time can name.
pub fn define_in_default_definee_vis(
    cref: &RubyValue,
    slf: &RubyValue,
    name: Symbol,
    body: RubyValue,
    private: bool,
    body_vis: Option<MethodVisibility>,
) -> Result<RubyValue, Signal> {
    // `(definee, installer, the cref's own -- the only one the caller's
    // top-level verdict is about)`.
    let (definee, installer, from_cref) = if crate::runtime_meta::singleton_definee(slf) {
        (slf.clone(), "define_singleton_method", false)
    } else if let Some(cid) = crate::runtime_meta::module_definee(slf) {
        (RubyValue::Class(cid), "define_method", false)
    } else {
        (cref.clone(), "define_method", true)
    };
    let out = send_value(
        &definee,
        Symbol::intern(installer),
        &[RubyValue::Symbol(name), body],
        None,
    )?;
    // A `def` written at the TOP LEVEL is a PRIVATE instance method of Object,
    // however it is spelled -- `p(def m; end)` is the same definition as a
    // bare one, and only the caller reads its `:m`. `define_method` installs
    // public, so the mark is a second step. The caller decides: it is a
    // question about where the `def` was WRITTEN, which is compile-time
    // knowledge, and an `*_eval` can put this same call on any object.
    if private
        && from_cref
        && let RubyValue::Class(id) = &definee
    {
        crate::runtime_meta::runtime_set_visibility(
            *id,
            &[RubyValue::Symbol(name)],
            MethodVisibility::Private,
        )?;
    }
    // The enclosing body's RUNNING visibility default (`private` with no
    // arguments, and ruby's automatic `private` for `initialize`) applies to
    // whatever definee was resolved above -- retagging the CREF instead
    // raised `undefined method 'initialize' for module 'M'` for a `def` in a
    // `Class.new` body, where the definee is the new class.
    if let (Some(vis), RubyValue::Class(id)) = (body_vis, &definee) {
        crate::runtime_meta::runtime_set_visibility(*id, &[RubyValue::Symbol(name)], vis)?;
    }
    Ok(out)
}

/// The `alias` KEYWORD in expression position (inside a block or method
/// body): it aliases on the frame's DEFAULT DEFINEE, resolved exactly as
/// [`define_in_default_definee`] resolves a `def`'s -- the receiver's
/// SINGLETON under `instance_eval`/`instance_exec` (how rspec's top-level DSL
/// pairs `def shared_examples` with `alias shared_context shared_examples`
/// on the `RSpec` module object), the module itself under `class_eval`/
/// `class_exec`/`Class.new`, and the cref's class otherwise. Distinct from
/// `Module#alias_method`, which always operates on its RECEIVER.
pub fn alias_in_default_definee(
    cref: &RubyValue,
    slf: &RubyValue,
    new: RubyValue,
    old: RubyValue,
) -> Result<RubyValue, Signal> {
    let definee = if crate::runtime_meta::singleton_definee(slf) {
        send_value(slf, Symbol::intern("singleton_class"), &[], None)?
    } else if let Some(cid) = crate::runtime_meta::module_definee(slf) {
        RubyValue::Class(cid)
    } else {
        cref.clone()
    };
    send_value(&definee, Symbol::intern("alias_method"), &[new, old], None)
}

/// `recv.class` for the codegen `.class` fast path on a dynamically-typed
/// receiver: normally the receiver's class value, but a Struct/Data instance
/// with a member literally named `class` (`Data.define(:class, :hash)`) has
/// that accessor shadow Kernel#class -- the fold that emits this can't see the
/// runtime-minted member, so the shadow is resolved here. Infallible (a member
/// read), so the fold stays a plain expression.
pub fn value_class(recv: &RubyValue) -> RubyValue {
    if let RubyValue::Object(o) = recv
        && let Some(v) = crate::builtins::rstruct::member_value_named(o, "class")
    {
        return v;
    }
    RubyValue::Class(recv.class_id())
}

/// The builtin row for `name` on `id` itself, ignoring any user definition.
///
/// An alias of a builtin must reach the BODY, not the name: a later `def old`
/// otherwise captures it and the standard wrap idiom recurses. Own class only
/// -- an inherited row can need the subclass payload bridge.
pub(super) fn builtin_row(id: ClassId, name: Symbol) -> Option<crate::builtins::BuiltinMethodFn> {
    crate::builtins::class_table(id)?(name.name_str())
}

/// [`builtin_row`]'s singleton-side twin.
pub(super) fn builtin_class_row(
    id: ClassId,
    name: Symbol,
) -> Option<crate::builtins::BuiltinMethodFn> {
    crate::builtins::class_method_table(id)?(name.name_str())
}

/// The `old` name behind a builtin-alias row visible to instances of `id` --
/// closest ancestor wins, so a subclass resolves an alias its parent (or an
/// included module) declared. `None` for every ordinary name; consulted only
/// on the send MISS paths, so a real method of the same name always beats it.
/// See `ClassEntry::aliases`.
pub(crate) fn alias_target(id: ClassId, name: Symbol) -> Option<Symbol> {
    let r = REGISTRY.get()?;
    for &anc in ancestors_of_value(id) {
        if let Some(e) = r.entries.get(&anc.0)
            && let Some(&old) = e.aliases.get(&name)
        {
            return Some(old);
        }
    }
    None
}

/// [`alias_target`]'s singleton-side twin: the class method `name` on `id`
/// rewrites to, from the closest ancestor that declares the row. Consulted only
/// once every real class-method probe has missed, so a genuine `def self.[]`
/// always beats an `alias [] new`. See `ClassEntry::class_aliases`.
pub(crate) fn class_alias_target(id: ClassId, name: Symbol) -> Option<Symbol> {
    let r = REGISTRY.get()?;
    for &anc in ancestors_of_value(id) {
        if let Some(e) = r.entries.get(&anc.0)
            && let Some(&old) = e.class_aliases.get(&name)
        {
            return Some(old);
        }
    }
    None
}

/// Parse-special Kernel names with NO runtime dispatch row: statically-
/// resolved call sites compile them directly, so an alias of one is valid
/// even though no table can prove it. `validate_aliases` skips them.
pub(crate) const PARSE_SPECIAL_KERNEL: &[&str] = &[
    "block_given?",
    "iterator?",
    "__method__",
    "__callee__",
    "binding",
];

/// Validates every registered builtin-alias row (see `register_alias`) --
/// called once from generated `main`'s fallible closure, right after the
/// registry installs: an alias whose source resolves NOWHERE for instances
/// of its class is `NameError`, raised at program start exactly when real
/// Ruby raises it (the `alias_method` in the class body executing). Walked
/// in id order (superclasses precede subclasses) with sorted names, so the
/// first error is deterministic.
pub fn validate_aliases() -> Result<(), Signal> {
    let Some(r) = REGISTRY.get() else {
        return Ok(());
    };
    let mut ids: Vec<u32> = r.entries.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        validate_class_aliases(ClassId(id))?;
    }
    Ok(())
}

/// One class's builtin-alias sources, checked where CRuby checks them: as that
/// class's BODY runs. A source can be created by the body itself -- bundler's
/// `Runtime` does `definition_method :specs` (a `define_method` wrapper) and
/// then `alias_method :gems, :specs` -- so a single sweep before any body has
/// run answers "undefined" for a method that is about to exist.
pub fn validate_class_aliases(id: ClassId) -> Result<(), Signal> {
    let Some(r) = REGISTRY.get() else {
        return Ok(());
    };
    let Some(entry) = r.entries.get(&id.0) else {
        return Ok(());
    };
    if entry.aliases.is_empty() {
        return Ok(());
    }
    let mut olds: Vec<Symbol> = entry.aliases.values().copied().collect();
    olds.sort_by_key(|s| s.name_str());
    olds.dedup();
    for old in olds {
        let n = old.name_str();
        if PARSE_SPECIAL_KERNEL.contains(&n) {
            continue;
        }
        let resolves = ancestors_of_value(id).iter().any(|&anc| {
            r.lookup(anc, old).is_some()
                || r.lookup_value_method(anc, 0, old).is_some()
                || crate::builtins::class_table(anc).is_some_and(|t| t(n).is_some())
                || crate::runtime_meta::overlay_own_method(anc, old).is_some()
        });
        if !resolves {
            let kind = if entry.is_module { "module" } else { "class" };
            return Err(crate::builtins::name_error!(
                "undefined method '{n}' for {kind} '{}'",
                entry.name
            ));
        }
    }
    Ok(())
}
