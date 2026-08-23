//! `Module` receiver methods (CRuby module.c/object.c), found via the walk on
//! a `RubyValue::Class` receiver whose `class_id()` is `MODULE_CLASS` -- or
//! `CLASS_CLASS`, since `Class`'s ancestry passes through `Module`, so a class
//! value sees this table too. `Module` owns `name`/`ancestors`/`===`; its
//! sibling `Class` (which owns `new`/`allocate`) lives in `rclass.rs`. Both are
//! r-prefixed (like `rproc`/`rstruct`) because `class`/`module` are Rust
//! keywords. The shared `recv_cid` helper is `pub(crate)` for `rclass` to use.

use crate::RubyValue;
use crate::builtins::{inherited_row, name_error, not_impl_error, type_error};
use zeo_macros::ruby_class;

pub(crate) fn recv_cid(recv: &RubyValue) -> crate::ClassId {
    match recv {
        RubyValue::Class(cid) => *cid,
        _ => unreachable!("Class/Module table row dispatched on a non-Class receiver"),
    }
}

/// The shared body of `private_constant`/`public_constant`: validate every
/// name against the receiver's OWN constants -- a value constant in the
/// runtime map, or a nested class/module registered under the receiver's
/// namespace -- record the mark, and answer the module.
///
/// The mark drives reflection (`Module#constants`, `defined?`) and the guard
/// codegen plants on a qualified `M::A`. Codegen already applied every
/// class-body `private_constant` at compile time; this row is what a
/// `public_constant` sent later, or a `send(:private_constant, ...)`, goes
/// through -- so the two halves write the same table.
fn constant_visibility(
    recv: &RubyValue,
    args: &[RubyValue],
    private: bool,
) -> Result<RubyValue, crate::Signal> {
    let cid = recv_cid(recv);
    let owner = crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string());
    for arg in args {
        let name = match arg {
            RubyValue::Symbol(s) => s.name().to_string(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => {
                return Err(type_error!(
                    "{} is not a symbol nor a string",
                    other.inspect_string()
                ));
            }
        };
        let defined = crate::constants::const_get(cid.0, &name).is_some()
            || crate::dispatch::class_id_by_name(&format!("{owner}::{name}")).is_some();
        if !defined {
            return Err(name_error!("constant {owner}::{name} not defined"));
        }
        crate::constants::const_set_private(cid.0, &[&name], private);
    }
    Ok(recv.clone())
}

/// Whether `name_arg` names an instance method of `recv` with exactly `want`
/// visibility -- shared by `public/private/protected_method_defined?`.
fn method_defined_with_vis(
    recv: &RubyValue,
    arg: &RubyValue,
    want: crate::dispatch::MethodVisibility,
) -> Result<bool, crate::Signal> {
    let name = name_arg(arg)?;
    let sym = crate::Symbol::intern(&name);
    // A not-implemented stub is LISTED but not "defined" -- ruby's
    // `rb_method_boundp` rejects it the same way `respond_to?` does, so
    // `private_instance_methods` carries `:syscall` while
    // `private_method_defined?(:syscall)` is false.
    if crate::dispatch::has_notimplement_row(recv, sym) {
        return Ok(false);
    }
    let vis = crate::dispatch::instance_method_visibility(recv_cid(recv), sym);
    Ok(vis == Some(want))
}

/// A Symbol-or-String constant name; TypeError on anything else.
fn const_name_arg(v: &RubyValue) -> Result<String, crate::Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(s.name()),
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            other.inspect_string()
        )),
    }
}

/// Ruby rejects a name that is not a CONSTANT name (`rb_is_const_id`) before it
/// ever looks the constant up, and the `NameError` carries the name -- so
/// `Foo.const_get(:nope)` is `wrong constant name nope` with `#name == :nope`,
/// not an `uninitialized constant` miss with no name at all.
fn check_const_name(name: &str, recv: &RubyValue) -> Result<(), crate::Signal> {
    if is_const_path(name) {
        return Ok(());
    }
    Err(crate::Signal::Raise(crate::dispatch::stamp_backtrace(
        crate::dispatch::make_name_error(format!("wrong constant name {name}"), name, recv.clone()),
    )))
}

/// `rb_is_const_id` over a whole path: every `::`-separated segment starts
/// with an uppercase ASCII letter and carries nothing but identifier
/// characters after it. The first letter alone is not the test -- `Bad.Name`
/// passes that and is still a `wrong constant name`, which is what a
/// `const_get` fed an arbitrary string has to say.
fn is_const_path(name: &str) -> bool {
    let mut segments = name.split("::").peekable();
    // A leading `::` is ruby's top-level spelling, not an empty segment.
    if name.starts_with("::") {
        segments.next();
    }
    let mut any = false;
    for seg in segments {
        any = true;
        let mut chars = seg.chars();
        if !chars.next().is_some_and(|c| c.is_ascii_uppercase()) {
            return false;
        }
        if !chars.all(|c| c.is_alphanumeric() || c == '_') {
            return false;
        }
    }
    any
}

/// A class or module that IS `owner`'s constant `name` but was never written
/// to the constant table: codegen resolves `Zlib::Error` and a bare `Array`
/// statically, so nothing ever `const_set`s either. The registry files both
/// under their qualified name, which is what this reads -- the same source
/// `Module#constants` lists from, so the two cannot disagree.
fn class_constant(owner: crate::ClassId, name: &str) -> Option<RubyValue> {
    // One walk, shared with the constant-read path: a nested class is a
    // constant of its namespace, and a BOX's top-level class is a constant
    // of the box rather than of `Object`. Two copies of that rule
    // disagreed -- `Object.const_defined?` answered for a class a box
    // wrote where the bare read had stopped saying so.
    crate::constants::nested_class_of(owner, name).map(RubyValue::Class)
}

/// Which of ruby's constant searches to run -- the `(recurse, exclude)` pair
/// of `variable.c`'s `rb_const_search`, which is the only thing that varies
/// between them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Search {
    /// `const_get(name, false)` / `const_defined?(name, false)`: the receiver's
    /// OWN binding, no ancestry.
    Own,
    /// `const_get` / `const_defined?`: the receiver and its ancestry, and a
    /// top-level (`Object`-owned) constant answers as well.
    Inherited,
    /// The `::` operator: the receiver and its ancestry, but a constant
    /// `Object` itself owns does NOT answer -- see `const_get_scoped`.
    Scoped,
}

/// `cid`'s constants under one of the three searches. Shared so `const_get`,
/// `const_defined?` and the scope operator cannot disagree about anything but
/// the flag they pass.
fn const_lookup(cid: crate::ClassId, name: &str, how: Search) -> Option<RubyValue> {
    // `const_get` walks the ancestry itself, so the non-inheriting form has to
    // ask for the OWN binding explicitly -- otherwise every class answers for
    // every top-level constant, `Object` being an ancestor of them all.
    if how == Search::Own {
        return crate::constants::const_get_own(cid.0, name).or_else(|| class_constant(cid, name));
    }
    // A constant `Object` owns is out of reach of the scope operator, and the
    // top-level class registry is `Object`'s half of that table.
    let through_object = how == Search::Inherited || cid.0 == 0;
    let scoped = how == Search::Scoped;
    let ancestry = || {
        crate::dispatch::ancestors_of_value(cid)
            .iter()
            .skip(1)
            .filter(|&&anc| through_object || anc.0 != 0)
            .find_map(|&anc| class_constant(anc, name))
    };
    let searched = if scoped {
        crate::constants::const_get_scoped(cid.0, name)
    } else {
        crate::constants::const_get(cid.0, name)
    };
    searched
        .or_else(|| class_constant(cid, name))
        .or_else(|| {
            // A MODULE's ancestry does not pass through `Object`, but the
            // inheriting form consults it anyway -- CRuby's rule, and the only
            // way a module sees a top-level constant.
            let is_module = cid.0 != 0 && crate::dispatch::class_is_module(cid).unwrap_or(false);
            (is_module && through_object).then(|| crate::constants::const_get_own(0, name))?
        })
        // The ancestry walk again, for a class constant an ANCESTOR owns
        // (`StringIO::SEEK_SET` through `IO`). The search above covers the
        // table half; this covers the registered-by-name half.
        .or_else(ancestry)
        // A top-level class is a constant of `Object`, so every class sees it
        // through the ancestry -- but a MODULE's chain never reaches `Object`,
        // and `Object` itself is already covered above.
        .or_else(|| {
            (through_object && cid.0 != 0).then(|| class_constant(crate::ClassId(0), name))?
        })
}

/// `defined?(Scope::NAME)`'s membership test, for a scope codegen resolved but
/// a name it could not: the constant may not exist until a `const_set` runs.
/// The scope OPERATOR's search, not `const_defined?`'s -- `defined?(K::TOP)` is
/// nil for a top-level `TOP` that `K.const_defined?(:TOP)` answers true for.
pub fn const_defined_in(cid: crate::ClassId, name: &str) -> bool {
    const_lookup(cid, name, Search::Scoped).is_some()
}

/// The scope operator's own receiver check, for the DYNAMIC form (`obj::NAME`,
/// where the compiler could not name the scope). CRuby evaluates the left side
/// first and demands a class or module of it, whichever way the constant is
/// then used -- read, write, or `||=`.
fn scope_cid(scope: &RubyValue) -> Result<crate::ClassId, crate::Signal> {
    match scope {
        RubyValue::Class(cid) => Ok(*cid),
        other => Err(type_error!(
            "{} is not a class/module",
            other.inspect_string()
        )),
    }
}

/// `obj::NAME` -- the scope operator on a value. The same search a compile-time
/// `Scope::NAME` runs (`Search::Scoped`, so a constant `Object` owns stays out
/// of reach), the same `private_constant` gate, and the same `const_missing`
/// dispatch on a miss. NOT `const_get`, which reaches through `Object` and past
/// a private mark, and which a class may override.
pub fn scope_const_get(scope: &RubyValue, name: &str) -> Result<RubyValue, crate::Signal> {
    let cid = scope_cid(scope)?;
    if crate::constants::const_is_private(cid.0, name) {
        let owner = crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string());
        return Err(name_error!("private constant {owner}::{name} referenced"));
    }
    match const_lookup(cid, name, Search::Scoped) {
        Some(v) => Ok(v),
        None => crate::dispatch::const_miss(cid, name),
    }
}

/// The lenient half of `obj::NAME ||= value`: nil where `scope_const_get` would
/// raise `NameError`, so the write half defines the constant instead. A
/// non-module scope is still a `TypeError` -- the leniency is about the NAME,
/// not the receiver. See `HirNode::ConstReadOrNil` for the static twin.
pub fn scope_const_get_or_nil(scope: &RubyValue, name: &str) -> Result<RubyValue, crate::Signal> {
    let cid = scope_cid(scope)?;
    if crate::constants::const_is_private(cid.0, name) {
        return Ok(RubyValue::Nil);
    }
    Ok(const_lookup(cid, name, Search::Scoped).unwrap_or(RubyValue::Nil))
}

/// `defined?(obj::NAME)`'s membership test. A scope that is not a module
/// answers false rather than raising -- `defined?` swallows that TypeError, as
/// it swallows every raise from the expression it classifies.
pub fn scope_const_defined(scope: &RubyValue, name: &str) -> bool {
    let RubyValue::Class(cid) = scope else {
        return false;
    };
    !crate::constants::const_is_private(cid.0, name) && const_defined_in(*cid, name)
}

/// `obj::NAME = value` -- defines the constant on `obj` itself (no ancestry:
/// assignment always writes the receiver's own table) and answers the value, as
/// every assignment does. Records the writing line so a rewrite reports
/// "previous definition of NAME was here" against the right one.
pub fn scope_const_set(
    scope: &RubyValue,
    name: &str,
    value: RubyValue,
) -> Result<RubyValue, crate::Signal> {
    let cid = scope_cid(scope)?;
    match crate::frames::current_location() {
        Some((file, line)) => {
            crate::constants::const_set_at(cid.0, name, value.clone(), file, line)
        }
        None => crate::constants::const_set(cid.0, name, value.clone()),
    }
    crate::runtime_meta::fire_const_added(cid, name)?;
    Ok(value)
}

/// The optional `inherit` boolean of `instance_methods`/`methods` (default
/// true) -- only an explicit `false`/`nil` narrows to own methods.
fn inherit_flag(v: Option<&RubyValue>) -> bool {
    !matches!(v, Some(RubyValue::Bool(false)) | Some(RubyValue::Nil))
}

/// The same flag as the search a reflective row (`const_get`,
/// `const_defined?`, `autoload?`) runs.
fn inherit_search(v: Option<&RubyValue>) -> Search {
    if inherit_flag(v) {
        Search::Inherited
    } else {
        Search::Own
    }
}

/// `(owner, constant name) -> feature path` for every `autoload` that reached
/// the RUNTIME row -- see its docs. Read by `autoload?`, by `const_defined?`
/// (ruby announces the constant at declaration, before anything loads) and by
/// `const_missing` (a target that never loads owes a `LoadError` there, not a
/// `NameError`). A program that makes no such `autoload` call never allocates
/// the map.
fn pending_autoloads()
-> &'static parking_lot::Mutex<std::collections::HashMap<(u32, String), String>> {
    static MAP: std::sync::LazyLock<
        parking_lot::Mutex<std::collections::HashMap<(u32, String), String>>,
    > = std::sync::LazyLock::new(Default::default);
    &MAP
}

/// Whether any `autoload` is still unloaded. A constant read consults the
/// map only when this is set, so a program that declares none pays one
/// relaxed load per gated read and nothing else.
pub static ANY_PENDING_AUTOLOAD: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Records `path` as the target of `owner::name`, and arms the gate.
pub fn register_autoload(owner: u32, name: String, path: String) {
    pending_autoloads().lock().insert((owner, name), path);
    ANY_PENDING_AUTOLOAD.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The autoload target `owner::name` still owes, if it has one. Reading the
/// constant is what runs it in ruby, so this is the read's own question.
pub fn take_autoload_target(owner: u32, name: &str) -> Option<String> {
    if !ANY_PENDING_AUTOLOAD.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let mut pending = pending_autoloads().lock();
    let path = pending.remove(&(owner, name.to_string()))?;
    ANY_PENDING_AUTOLOAD.store(!pending.is_empty(), std::sync::atomic::Ordering::Relaxed);
    Some(path)
}

/// Whether `owner::name` still owes an autoload target.
pub fn has_pending_autoload(owner: u32, name: &str) -> bool {
    pending_autoloads()
        .lock()
        .contains_key(&(owner, name.to_string()))
}

/// The target `owner::name` owes, without spending the registration.
fn peek_autoload_target(owner: u32, name: &str) -> Option<String> {
    if !ANY_PENDING_AUTOLOAD.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    pending_autoloads()
        .lock()
        .get(&(owner, name.to_string()))
        .cloned()
}

/// Runs the autoload target `owner::name` owes, if any. This is the point
/// ruby loads at: the constant's first READ.
///
/// A target whose load raises goes back to pending -- ANY class, not just
/// `LoadError`. CRuby runs nothing until the access, so "declared, never
/// loaded" is exactly its state, and `load_feature` leaves the unit
/// retryable so a program that reaches the constant again still sees the
/// error.
pub fn run_pending_autoload(owner: u32, name: &str) -> Result<(), crate::Signal> {
    let Some(path) = peek_autoload_target(owner, name) else {
        return Ok(());
    };
    match crate::features::load_feature(&path) {
        // Loaded: the registration is spent, and `autoload?` answers nil.
        Some(Ok(_)) => {
            take_autoload_target(owner, name);
            Ok(())
        }
        Some(Err(e)) => Err(e),
        // No such feature. The registration STAYS -- reading the constant is
        // what owes the `LoadError`, and `const_miss_signal` raises it from
        // the record left here.
        None => Ok(()),
    }
}

/// What a constant MISS raises. A name registered by an `autoload` whose
/// feature never loaded owes a `LoadError`, because reading the constant is
/// what triggers the load in ruby and so it is what fails; every other miss
/// keeps the `NameError` the call site spelled, whose as-written wording
/// nothing else can reproduce.
///
/// Called from the baked raise codegen emits for a miss, which is the path
/// almost every constant read takes -- `const_missing` itself is dispatched
/// only when a class in the chain defines the hook.
pub fn const_miss_signal(owner: crate::ClassId, name: &str, message: &str) -> crate::Signal {
    let pending = pending_autoloads().lock();
    if !pending.is_empty()
        && let Some(path) = pending.get(&(owner.0, name.to_string()))
    {
        return crate::builtins::kernel::missing_feature_error(path);
    }
    drop(pending);
    crate::Signal::Raise(crate::dispatch::stamp_backtrace(
        crate::dispatch::make_name_error(message.to_string(), name, RubyValue::Class(owner)),
    ))
}

/// A `Vec<Symbol>` as a Ruby Array of Symbols -- reflection's return shape.
fn syms_to_array(names: Vec<crate::Symbol>) -> RubyValue {
    RubyValue::Array(crate::array_new(
        names.into_iter().map(RubyValue::Symbol).collect(),
    ))
}

ruby_class! {
    Module = zeo_abi::MODULE_CLASS < zeo_abi::OBJECT_CLASS;

    // `#name` answers only for a class REACHABLE by a constant path.
    // `Class.new`, `Module.new`, an unassigned `Struct.new`, and every
    // singleton class are nameless, so they answer nil -- while `#to_s`
    // still renders each of them, which is the whole distinction.
    def "name" (recv) {
        Ok(match crate::dispatch::class_real_name(recv_cid(recv)) {
            Some(n) => RubyValue::Str(crate::string_new(n)),
            None => RubyValue::Nil,
        })
    }
    def "to_s" (recv) {
        let cid = recv_cid(recv);
        let n = crate::dispatch::class_name(cid).unwrap_or_else(|| format!("#<Class:{}>", cid.0));
        Ok(RubyValue::Str(crate::string_new(n)))
    }
    // `inspect` is `to_s` PLUS the struct suffix: a `keyword_init: true`
    // Struct class inspects as `K(keyword_init: true)`, where its `name` and
    // `to_s` stay the bare `K`. Only an explicit `true` shows -- the tri-state
    // `nil` (never given) and `false` both print nothing, as in CRuby.
    def "inspect" (recv) {
        let cid = recv_cid(recv);
        let mut n = crate::dispatch::class_name(cid).unwrap_or_else(|| format!("#<Class:{}>", cid.0));
        if crate::builtins::rstruct::keyword_init_suffix(cid) {
            n.push_str("(keyword_init: true)");
        }
        Ok(RubyValue::Str(crate::string_new(n)))
    }
    // The mixin hooks' DEFAULTS. Ruby fires each one on every mixin whether
    // or not the module defines it, so these exist to be the no-op that
    // answers -- and, more to the point, to be what a `def self.included`
    // that ends in `super` reaches.
    private def "included" | "extended" | "prepended" (_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    def "ancestors" (recv) {
        let chain = crate::dispatch::ancestors_of_value(recv_cid(recv))
            .iter()
            .map(|&a| RubyValue::Class(a))
            .collect();
        Ok(RubyValue::Array(crate::array_new(chain)))
    }
    // `Module#included_modules`: the modules in `recv`'s ancestor chain, in MRO
    // order (the classes filtered out). Kernel and any mixed-in module appear;
    // `Object`/`BasicObject` (classes) do not.
    def "included_modules" (recv) {
        // The RECEIVER is never one of its own included modules, even though it
        // heads its own ancestor chain: `Comparable.included_modules` is `[]`,
        // not `[Comparable]`.
        let cid = recv_cid(recv);
        let mods = crate::dispatch::ancestors_of_value(cid)
            .iter()
            .filter(|&&a| a != cid && crate::dispatch::class_is_module(a).unwrap_or(false))
            .map(|&a| RubyValue::Class(a))
            .collect();
        Ok(RubyValue::Array(crate::array_new(mods)))
    }
    // `Module#constants([inherit=true])`: this module's own constant names as
    // Symbols, then -- unless `inherit` is false -- its ancestors' (except
    // `Object`'s, CRuby's rule), own group first. Order within one class is
    // unspecified (an id table in CRuby, a HashMap here).
    // `Module#const_set(name, value)` -- define a constant on this module,
    // answering the value (as CRuby does).
    def "const_set" (recv, arg1, arg2) {
        let cid = recv_cid(recv);
        let name = match arg1 {
            RubyValue::Symbol(s) => s.name(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => {
                return Err(type_error!("{} is not a symbol nor a string", other.inspect_string()))
            }
        };
        // A run-time assignment is located at the line that made it, exactly
        // as a written one is -- and a builtin has no frame of its own, so the
        // top of the stack IS the caller.
        match crate::frames::current_location() {
            Some((file, line)) => {
                crate::constants::const_set_at(cid.0, &name, (*arg2).clone(), file, line)
            }
            None => crate::constants::const_set(cid.0, &name, (*arg2).clone()),
        }
        crate::runtime_meta::fire_const_added(cid, &name)?;
        Ok((*arg2).clone())
    }
    // `Module#private_constant(:A, ...)` / `Module#public_constant(:A, ...)` --
    // argument-validated like CRuby (each name must be an OWN constant of the
    // receiver, else "constant M::A not defined"), answering the module.
    // What the mark HIDES is the scope operator: `M::A` raises while a bare
    // `A` resolved through the cref, and `M.const_get(:A)`, both still answer
    // -- CRuby's rule exactly. See `constant_visibility`.
    def "private_constant" (recv, *args, &_block) {
        constant_visibility(recv, args, true)
    }
    def "public_constant" (recv, *args, &_block) {
        constant_visibility(recv, args, false)
    }
    // `Module#deprecate_constant(:A, ...)` -- same argument validation, same
    // reason for not enforcing: CRuby warns on ACCESS, and zeo binds constant
    // references at compile time, so there is no runtime read to hook. (CRuby's
    // warning is itself off unless `Warning[:deprecated]` is on, which it isn't
    // by default -- so the common case agrees exactly.) net/http deprecates its
    // legacy response-class aliases at load time.
    def "deprecate_constant" (recv, *args, &_block) {
        // Validation only -- `false` leaves the private mark alone, since
        // deprecating a constant does not hide it.
        constant_visibility(recv, args, false)
    }
    def "const_get" cfunc (recv, name, inherit?) {
        let cid = recv_cid(recv);
        let name = const_name_arg(name)?;
        // `const_get("A::B::C")` walks the path, each segment resolved on what
        // the previous one answered (CRuby's own rule). Privacy is deliberately
        // NOT consulted: `const_get` reaches a `private_constant`, and only the
        // scope OPERATOR is gated.
        if let Some((head, rest)) = name.split_once("::") {
            let head_search = inherit_search(inherit);
            // Only the HEAD gets `const_get`'s reach. Every segment past it is
            // a scope operator, and CRuby searches it as one -- so
            // `Object.const_get("K::TOP")` raises where a written `K::TOP`
            // raises, even though `K.const_get(:TOP)` answers.
            let rest_search = match head_search {
                Search::Own => Search::Own,
                _ => Search::Scoped,
            };
            check_const_name(head, recv)?;
            let mut cur = const_lookup(cid, head, head_search)
                .ok_or_else(|| name_error!("uninitialized constant {head}"))?;
            // A miss past the head dispatches `const_missing` on the scope
            // that failed to answer; the default hook reports it qualified
            // by that scope, CRuby's wording.
            for seg in rest.split("::") {
                let RubyValue::Class(scope) = cur else {
                    return Err(type_error!("{} is not a class/module", cur.inspect_string()));
                };
                check_const_name(seg, &cur)?;
                cur = match const_lookup(scope, seg, rest_search) {
                    Some(v) => v,
                    None => crate::dispatch::const_miss(scope, seg)?,
                };
            }
            return Ok(cur);
        }
        // A single-segment miss dispatches `const_missing` on the receiver;
        // the default hook raises qualified by it, which is what tells a
        // miss on `Foo.const_get(:X)` apart from one on a bare `X`.
        check_const_name(&name, recv)?;
        match const_lookup(cid, &name, inherit_search(inherit)) {
            Some(v) => Ok(v),
            None => crate::dispatch::const_miss(cid, &name),
        }
    }
    def "const_defined?" cfunc (recv, name, inherit?) {
        let name = const_name_arg(name)?;
        check_const_name(&name, recv)?;
        let cid = recv_cid(recv);
        // A pending autoload counts: ruby announces the constant when the
        // autoload is DECLARED, long before the feature loads (and whether or
        // not it ever can).
        let found = const_lookup(cid, &name, inherit_search(inherit)).is_some()
            || pending_autoloads().lock().contains_key(&(cid.0, name.clone()));
        Ok(RubyValue::Bool(found))
    }
    // Returns the removed value; NameError when the constant isn't this
    // module's own (an inherited one doesn't count).
    private def "remove_const" (recv, arg) {
        let cid = recv_cid(recv);
        let name = const_name_arg(arg)?;
        crate::constants::const_remove(cid.0, &name)
            .ok_or_else(|| name_error!("constant {name} not defined"))
    }
    def "constants" (recv, inherit?) {
        let cid = recv_cid(recv);
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let mut push_owner = |owner: crate::ClassId, out: &mut Vec<RubyValue>| {
            // Two sources, because a module's constants are stored two ways: an
            // ordinary `FOO = 1` lands in the constant table, while a nested
            // `class Bar` is registered by its qualified NAME and never reaches
            // that table (codegen resolves `Foo::Bar` statically). Both are
            // constants of `Foo` as far as Ruby is concerned.
            let named = crate::constants::const_names_of(owner.0);
            for name in named.into_iter().chain(crate::dispatch::nested_class_names(owner)) {
                // `private_constant` hides the name from the listing without
                // removing the binding -- `const_get` still answers for it,
                // exactly as in CRuby.
                if crate::constants::const_is_private(owner.0, &name) {
                    continue;
                }
                if seen.insert(name.clone()) {
                    out.push(RubyValue::Symbol(crate::Symbol::intern(&name)));
                }
            }
        };
        push_owner(cid, &mut out);
        if inherit_flag(inherit) {
            for anc in crate::dispatch::ancestors_of_value(cid) {
                // `Object`'s constants (every top-level constant) are excluded
                // from a non-Object module's `constants`, matching CRuby.
                if *anc == cid || *anc == zeo_abi::OBJECT_CLASS
                    || *anc == zeo_abi::BASIC_OBJECT_CLASS
                {
                    continue;
                }
                push_owner(*anc, &mut out);
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `Module.constants` is a SINGLETON method, not the instance one above
    // (`rb_mod_s_constants`): it answers the constants visible in the CALLER's
    // lexical scope. Without this row the call fell through to `Module#constants`
    // with `Module` as the receiver, which answered Module's own few.
    //
    // APPROXIMATION: it answers the TOP-LEVEL scope. zeo resolves lexical scope
    // at compile time and a builtin row has no view of its caller's, so a call
    // from inside `module Foo` misses Foo's own constants. Top-level is where
    // this is written (`Module.constants.include?(:Rails)` guards), and
    // answering Object's set is strictly closer than answering Module's.
    def self."constants" (_recv, *args) {
        // Given ANY argument, `rb_mod_s_constants` stops being the lexical-scope
        // query and delegates to `Module#constants` on `Module` itself -- which
        // owns none, so every arity-1 form answers `[]`. `nil` is an ordinary
        // falsy `inherit`, not a missing argument.
        if !args.is_empty() {
            return lookup("constants").expect("Module#constants")(
                &RubyValue::Class(zeo_abi::MODULE_CLASS),
                args,
                None,
            );
        }
        let names = crate::constants::const_names_of(0)
            .into_iter()
            .chain(crate::dispatch::nested_class_names(zeo_abi::OBJECT_CLASS));
        let mut seen = std::collections::HashSet::new();
        Ok(RubyValue::Array(crate::array_new(
            names
                .filter(|n| seen.insert(n.clone()))
                .map(|n| RubyValue::Symbol(crate::Symbol::intern(&n)))
                .collect(),
        )))
    }
    // `Module#include?(mod)`: true when `mod` is a MODULE mixed into `recv` or
    // one of its ancestors (never `recv` itself, and never a superclass --
    // only included/prepended modules count). A non-class/module argument is a
    // TypeError.
    def "include?" (recv, arg) {
        // The argument must be a MODULE. A class (or any non-module) is a
        // TypeError whose type name CRuby reports as `Class` for a class value.
        let type_err = || {
            let name = match arg {
                RubyValue::Class(cid) if !crate::dispatch::class_is_module(*cid).unwrap_or(false) => {
                    "Class".to_string()
                }
                other => crate::builtins::class_name_of(other),
            };
            type_error!("wrong argument type {name} (expected Module)")
        };
        let RubyValue::Class(other) = *arg else {
            return Err(type_err());
        };
        if !crate::dispatch::class_is_module(other).unwrap_or(false) {
            return Err(type_err());
        }
        let me = recv_cid(recv);
        let in_chain =
            other != me && crate::dispatch::ancestors_of_value(me).contains(&other);
        Ok(RubyValue::Bool(in_chain))
    }
    // `Module#===`: instance-of-ancestry, the check `case`/`when` class
    // candidates desugar to.
    def "===" (recv, other) {
        Ok(RubyValue::Bool(crate::dispatch::is_a_value(
            other,
            recv_cid(recv),
        )))
    }
    // `Module`'s ancestry ordering (`rb_class_cmp`): `<`/`<=`/`>`/`>=` answer
    // the subclass relation and `nil` when the two are UNRELATED (neither is
    // an ancestor of the other), while a non-class/module argument is a
    // TypeError. `<=>` is `nil` for both the unrelated and the non-module
    // cases. All five share the one `module_cmp` ordering.
    def "<" (recv, other) {
        module_ordering_op(recv, other, |o| matches!(o, std::cmp::Ordering::Less))
    }
    def "<=" (recv, other) {
        module_ordering_op(recv, other, |o| matches!(o, std::cmp::Ordering::Less | std::cmp::Ordering::Equal))
    }
    def ">" (recv, other) {
        module_ordering_op(recv, other, |o| matches!(o, std::cmp::Ordering::Greater))
    }
    def ">=" (recv, other) {
        module_ordering_op(recv, other, |o| matches!(o, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal))
    }
    def "<=>" (recv, other) {
        match *other {
            RubyValue::Class(other) => Ok(
                crate::dispatch::module_cmp(recv_cid(recv), other)
                    .map_or(RubyValue::Nil, |o| RubyValue::Int(o as i64)),
            ),
            _ => Ok(RubyValue::Nil),
        }
    }
    // Named classes/modules are never singleton (metaclass) classes; zeo
    // doesn't model per-object singleton classes as first-class ids, so this
    // is `false` for every reachable `RubyValue::Class` receiver.
    def "singleton_class?" (recv) {
        let _ = recv_cid(recv);
        Ok(RubyValue::Bool(false))
    }
    // Reflection over a CLASS OBJECT's own ivars -- the `@x` a `def self.x`
    // or a class body writes (see `civars`' docs). Really `Kernel`'s
    // methods, which a class inherits; they live on the Module table
    // because that is the one a `RubyValue::Class` receiver reaches -- so
    // each is marked `inherits` and reflection reports Kernel, which the
    // ancestry reaches and which declares all four itself.
    def "instance_variable_get" inherits (recv, arg) {
        let name = ivar_name_arg(arg)?;
        Ok(crate::civars::class_ivar_get(recv_cid(recv).0, &name))
    }
    def "instance_variable_set" inherits (recv, arg1, arg2) {
        let name = ivar_name_arg(arg1)?;
        crate::civars::class_ivar_set(recv_cid(recv).0, &name, (*arg2).clone())?;
        // Answers the VALUE, not the receiver -- oracle-checked.
        Ok((*arg2).clone())
    }
    def "instance_variable_defined?" inherits (recv, arg) {
        let name = ivar_name_arg(arg)?;
        Ok(RubyValue::Bool(
            crate::civars::class_ivar_names(recv_cid(recv).0).contains(&name),
        ))
    }
    def "instance_variables" inherits (recv) {
        let names = crate::civars::class_ivar_names(recv_cid(recv).0)
            .into_iter()
            .map(|n| RubyValue::Symbol(crate::Symbol::intern(&format!("@{n}"))))
            .collect();
        Ok(RubyValue::Array(crate::array_new(names)))
    }
    // `Module#method_defined?(:name)` -- does the class (or an ancestor)
    // provide `name` as a public OR protected INSTANCE method? (private and
    // nonexistent answer false), so an inherited `object_id`/`frozen?` answers
    // true too.
    def "method_defined?" cfunc (recv, arg1, arg2?) {
        // The optional second `inherit` flag (default true): false restricts the
        // lookup to the receiver's own methods (no ancestor walk).
        let name = name_arg(arg1)?;
        let inherit = arg2.is_none_or(RubyValue::truthy);
        Ok(RubyValue::Bool(crate::dispatch::method_defined_inherit(
            recv_cid(recv),
            crate::Symbol::intern(&name),
            inherit,
        )))
    }
    // `instance_methods(inherit=true)` -- public+protected names of the
    // module/class (and its ancestors unless `inherit` is false). A builtin's
    // list is a subset of CRuby's (this runtime implements a subset), so
    // callers assert membership; a user class's own list is exact.
    def "instance_methods" (recv, inherit?) {
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::NotPrivate,
            inherit_flag(inherit),
        );
        Ok(syms_to_array(names))
    }
    // `public_instance_methods` narrows to public ONLY (protected excluded).
    def "public_instance_methods" (recv, inherit?) {
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::Public,
            inherit_flag(inherit),
        );
        Ok(syms_to_array(names))
    }
    def "private_instance_methods" (recv, inherit?) {
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::Private,
            inherit_flag(inherit),
        );
        Ok(syms_to_array(names))
    }
    def "protected_instance_methods" (recv, inherit?) {
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::Protected,
            inherit_flag(inherit),
        );
        Ok(syms_to_array(names))
    }
    // `private_method_defined?`/`public_method_defined?`/
    // `protected_method_defined?` -- true when `name` is an instance method of
    // this exact visibility. A non-method (or a name of another visibility)
    // answers false.
    def "public_method_defined?" cfunc (recv, arg1, _arg2?) {
        Ok(RubyValue::Bool(method_defined_with_vis(
            recv, arg1, crate::dispatch::MethodVisibility::Public)?))
    }
    def "private_method_defined?" cfunc (recv, arg1, _arg2?) {
        Ok(RubyValue::Bool(method_defined_with_vis(
            recv, arg1, crate::dispatch::MethodVisibility::Private)?))
    }
    def "protected_method_defined?" cfunc (recv, arg1, _arg2?) {
        Ok(RubyValue::Bool(method_defined_with_vis(
            recv, arg1, crate::dispatch::MethodVisibility::Protected)?))
    }
    // `Module#instance_method(:name)` -> an UnboundMethod for the module/class.
    def "instance_method" (recv, arg) {
        crate::builtins::unbound_method::unbound_method_new(recv_cid(recv), arg)
    }
    // `Module#define_method(name) { body }` -- install/override an
    // instance method AT RUNTIME (a computed name, or inside an `each` loop).
    // The literal `define_method(:sym) { ... }` form is desugared to a `def` at
    // compile time in zeo; this row serves everything that isn't literal.
    def "define_method" cfunc (recv, name, body?, &block) {
        let name = crate::runtime_meta::coerce_method_name(Some(name))?;
        // A `Method`/`UnboundMethod` second argument installs that method's
        // own definition under `name` (not a Proc body).
        if let Some(src) = body
            && let Some((owner, src_name)) = crate::builtins::method::method_source(src) {
                return crate::runtime_meta::runtime_define_method_from_method(
                    recv_cid(recv), name, owner, src_name);
            }
        let body = crate::runtime_meta::coerce_method_body(body, &block)?;
        crate::runtime_define_method(recv_cid(recv), name, body)
    }
    // `Module#alias_method(new_name, old_name)` -- the RUNTIME form (a
    // computed name, or inside an `each` loop: ostruct's bulk `!`-alias
    // loop). The literal class-body form resolves at compile time; this row
    // serves everything that isn't literal. Snapshot semantics -- the alias
    // keeps the method `old_name` resolves to NOW -- and returns the new
    // name's Symbol, both per CRuby.
    def "alias_method" (recv, new, old) {
        let new = crate::runtime_meta::coerce_method_name(Some(new))?;
        let old = crate::runtime_meta::coerce_method_name(Some(old))?;
        crate::runtime_meta::runtime_alias_method(recv_cid(recv), new, old)
    }
    // `Module#include(M, ...)` reached at RUNTIME on a Class/Module receiver
    // (`Class.new { include M }`, `mod.class_eval { include Other }`): mix each
    // module's instance methods into the receiver's runtime ancestry. The plain
    // class-body form resolves statically; this serves the runtime shapes.
    def "include" (recv, *args, &_block) {
        crate::runtime_meta::runtime_include(recv, args)
    }
    def "prepend" (recv, *args, &_block) {
        crate::runtime_meta::runtime_prepend(recv, args)
    }
    // The PRIMITIVES `include`/`Object#extend` are defined in terms of, private
    // on Module and overridable -- which is the whole point: a module that
    // wants to police how it is mixed in overrides one of these and calls
    // `super`. The singleton gem does both, defining `append_features` to
    // reject inclusion into a module and undefining `extend_object` so that
    // `obj.extend(Singleton)` cannot work at all -- and `undef_method` needs
    // the name to EXIST before it can take it away.
    // These are the SPLICE itself, not the public verb -- `include` calls this,
    // never the reverse. Routing back through `Module#include` would be an
    // infinite regress the moment a module overrides one and calls `super`.
    private def "append_features" (recv, arg) {
        crate::runtime_meta::splice_mixin(arg, recv, false)?;
        Ok(recv.clone())
    }
    private def "prepend_features" (recv, arg) {
        crate::runtime_meta::splice_mixin(arg, recv, true)?;
        Ok(recv.clone())
    }
    private def "extend_object" (recv, arg) {
        crate::runtime_meta::extend_object_default(arg, recv)?;
        Ok((*arg).clone())
    }
    // Ruby tells a module what was just defined in it. These four are the
    // no-op defaults an override's `super` reaches -- `Class#inherited`'s
    // siblings, and the reason a bare `def self.method_added(n)` needs no
    // `super` guard. What CALLS them is the interesting half: see
    // `runtime_meta::fire_def_hook` for the runtime definitions and
    // `codegen::emit_class_body_site` for the compiled ones.
    //
    // The firing side must NOT dispatch these: a hook whose owner resolves to
    // `Module` is this no-op, and paying a send per definition to reach it
    // would tax every program. `fire_def_hook` checks the owner for exactly
    // that reason.
    private def "method_added" (_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    private def "method_removed" (_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    private def "method_undefined" (_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    private def "const_added" (_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    // The literal class-body forms of these four expand to real `def`s at
    // compile time; these rows serve `Class.new { }` and `class_eval { }`.
    def "attr_reader" (recv, *args, &_block) {
        crate::runtime_meta::runtime_attr(recv_cid(recv), args, crate::runtime_meta::AttrKind::Reader)
    }
    def "attr_writer" (recv, *args, &_block) {
        crate::runtime_meta::runtime_attr(recv_cid(recv), args, crate::runtime_meta::AttrKind::Writer)
    }
    def "attr_accessor" (recv, *args, &_block) {
        crate::runtime_meta::runtime_attr(recv_cid(recv), args, crate::runtime_meta::AttrKind::Accessor)
    }
    // `attr :x` is a reader; the deprecated `attr :x, true` is an accessor.
    def "attr" (recv, *args, &_block) {
        let accessor = matches!(args.last(), Some(RubyValue::Bool(true)));
        let names = if accessor { &args[..args.len() - 1] } else { args };
        let kind = if accessor {
            crate::runtime_meta::AttrKind::Accessor
        } else {
            crate::runtime_meta::AttrKind::Reader
        };
        crate::runtime_meta::runtime_attr(recv_cid(recv), names, kind)
    }
    // A no-op by construction: it flags a method to pass a bare `*args`
    // trailing hash through as keywords, and zeo's keyword arguments are
    // already carried separately from the positionals.
    private def "ruby2_keywords" (_recv, *_args, &_block) {
        Ok(RubyValue::Nil)
    }
    def "undef_method" (recv, *args, &_block) {
        crate::runtime_meta::runtime_undef_method(recv_cid(recv), args)
    }
    def "remove_method" (recv, *args, &_block) {
        crate::runtime_meta::runtime_remove_method(recv_cid(recv), args)
    }
    // `Module#private`/`public`/`protected` reached at RUNTIME (inside a
    // `class_eval` block or a guarded class-body statement -- the plain
    // class-body form resolves at compile time): with names, validate and
    // mark visibility in the runtime overlay; the argument-less
    // default-visibility form is a documented nil no-op (see
    // `runtime_set_visibility`).
    private def "private" (recv, *args, &_block) {
        crate::runtime_meta::runtime_set_visibility(
            recv_cid(recv), args, crate::dispatch::MethodVisibility::Private)
    }
    private def "public" (recv, *args, &_block) {
        crate::runtime_meta::runtime_set_visibility(
            recv_cid(recv), args, crate::dispatch::MethodVisibility::Public)
    }
    private def "protected" (recv, *args, &_block) {
        crate::runtime_meta::runtime_set_visibility(
            recv_cid(recv), args, crate::dispatch::MethodVisibility::Protected)
    }
    // `Module#module_function(name)` reached at RUNTIME (a computed argument --
    // fileutils' `private_module_function` calls `module_function name`). The
    // literal form resolves at compile time in `lower/defs.rs`. Promotes the
    // named instance method to a module method (see `runtime_module_function`).
    private def "module_function" (recv, *args, &_block) {
        crate::runtime_meta::runtime_module_function(recv_cid(recv), args)
    }
    // `private_class_method`/`public_class_method` at RUNTIME. The literal
    // form resolves at compile time in `lower/defs.rs`; this marks the
    // overlay, which outranks whatever the frozen registry baked in.
    def "private_class_method" (recv, *args, &_block) {
        crate::runtime_meta::runtime_class_method_visibility(recv_cid(recv), args, true)?;
        Ok(recv.clone())
    }
    def "public_class_method" (recv, *args, &_block) {
        crate::runtime_meta::runtime_class_method_visibility(recv_cid(recv), args, false)?;
        Ok(recv.clone())
    }
    // `Module#class_eval`/`module_eval` -- run the block with `self` rebound
    // to the module/class value, returning the block's value. A
    // `def`/`define_method` inside installs on the receiver via the dynamic-
    // self path (self is a Class). `module_eval` is an alias.
    def "class_eval" | "module_eval" (recv, *args, &block) {
        if let Some(arg) = args.first() {
            // The string form, compiled with `self` rebound to the
            // class -- the same routing `instance_eval` already uses, and the
            // compiler ALREADY links the eval runtime for it
            // (`Hir::uses_runtime_eval`). Ignoring `args` here meant a string
            // form fell through to the block path and reported the misleading
            // "tried to create Proc object without a block".
            return crate::eval::eval_value_mode(
                arg.clone(),
                recv.clone(),
                0,
                crate::eval::EvalMode::ClassEval,
            );
        }
        let blk = crate::builtins::basic_object::block_proc(block, "class_eval")?;
        crate::runtime_meta::with_body_frame(recv_cid(recv), || blk.call_with_self(recv, &[]))
    }
    // `Module#class_exec`/`module_exec(*args) { |*a| ... }` -- like class_eval
    // but forwards positional args to the block's params.
    def "class_exec" | "module_exec" (recv, *args, &block) {
        let blk = crate::builtins::basic_object::block_proc(block, "class_exec")?;
        crate::runtime_meta::with_body_frame(recv_cid(recv), || blk.call_with_self(recv, args))
    }
    // `Module#initialize`: module_exec the block (on ANY module -- CRuby
    // re-runs it happily, even frozen), answer NIL either way.
    private def "initialize"(recv, &block) {
        if let Some(RubyValue::Proc(_)) = &block {
            let blk = crate::builtins::basic_object::block_proc(block, "initialize")?;
            crate::runtime_meta::with_body_frame(recv_cid(recv), || {
                blk.call_with_self(recv, &[])
            })?;
        }
        Ok(RubyValue::Nil)
    }
    // `Module#initialize_clone(orig, freeze: nil)` -- the copy hook `clone`
    // drives; the freeze keyword is the clone machinery's concern, the copy
    // itself is `initialize_copy`'s.
    private def "initialize_clone" cfunc (recv, *args, &_block) {
        let Some(orig) = args.first() else {
            return Err(crate::builtins::arg_error!(
                "wrong number of arguments (given 0, expected 1)"
            ));
        };
        inherited_row!(kernel, "initialize_copy", recv, std::slice::from_ref(orig), None)?;
        Ok(recv.clone())
    }
    // `Module#class_variable_get/set/defined?` over the linearized ancestry
    // (a `@@x` is owned by the nearest ancestor that first assigned it --
    // see `cvars`' docs). `get` on a never-assigned name is a `NameError`,
    // unlike a plain `@@x` read's nil-on-miss.
    def "class_variable_get" (recv, arg) {
        let name = cvar_name_arg(arg)?;
        let cid = recv_cid(recv);
        for &anc in crate::dispatch::ancestors_of_value(cid) {
            if crate::cvar_defined(anc.0, &name) {
                return Ok(crate::cvar_get(anc.0, &name));
            }
        }
        Err(name_error!("uninitialized class variable @@{name} in {}",
                crate::dispatch::class_name(cid).unwrap_or_default()))
    }
    def "class_variable_set" (recv, arg1, arg2) {
        let name = cvar_name_arg(arg1)?;
        let cid = recv_cid(recv);
        // Assign on the owning ancestor if one already exists, else on the
        // receiver itself (real Ruby's own rule).
        let owner = crate::dispatch::ancestors_of_value(cid)
            .iter()
            .find(|&&anc| crate::cvar_defined(anc.0, &name))
            .map_or(cid, |&anc| anc);
        crate::cvar_set(owner.0, &name, (*arg2).clone())?;
        Ok((*arg2).clone())
    }
    def "class_variable_defined?" (recv, arg) {
        let name = cvar_name_arg(arg)?;
        Ok(RubyValue::Bool(
            crate::dispatch::ancestors_of_value(recv_cid(recv))
                .iter()
                .any(|&anc| crate::cvar_defined(anc.0, &name)),
        ))
    }
    // `Module#class_variables([inherit=true])` -- the `@@name` symbols owned by
    // this class and (unless `inherit` is false) its ancestors, own first.
    // Names store bare (`x`); the reflection re-adds the `@@` prefix.
    def "class_variables" (recv, inherit?) {
        let cid = recv_cid(recv);
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let mut push_owner = |owner: crate::ClassId, out: &mut Vec<RubyValue>| {
            for name in crate::cvar_names_of(owner.0) {
                if seen.insert(name.clone()) {
                    out.push(RubyValue::Symbol(crate::Symbol::intern(&format!("@@{name}"))));
                }
            }
        };
        push_owner(cid, &mut out);
        if inherit_flag(inherit) {
            // No Object/BasicObject exclusion here, unlike `constants` -- a
            // `@@x` can only reach Object through an explicit `class Object`
            // body (a top-level one raises; see `Hir::cvar_is_toplevel`), and
            // CRuby does report that one from every descendant.
            for anc in crate::dispatch::ancestors_of_value(cid) {
                if *anc != cid {
                    push_owner(*anc, &mut out);
                }
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `Module#remove_class_variable(:@@x)` -- drop the slot this exact module
    // owns and answer what it held. An INHERITED `@@x` does not count, which is
    // what makes the NameError's wording name the receiver.
    def "remove_class_variable" (recv, arg) {
        let name = cvar_name_arg(arg)?;
        let cid = recv_cid(recv);
        crate::cvar_remove(cid.0, &name)?.ok_or_else(|| {
            name_error!("class variable @@{name} not defined for {}",
                crate::dispatch::class_name(cid).unwrap_or_default())
        })
    }

    // --- autoload -------------------------------------------------------
    //
    // zeo compiles a literal `autoload :C, "feature"` target in as a LAZY
    // unit (`parse::loader`'s pre-pass) and keeps the call, so this row runs
    // at the declaration's document position and loads the unit right here
    // -- after every statement that precedes the `autoload`, which is what
    // lets a file's earlier definitions be visible to the target's body.
    //
    // Also reached by the residue the structural collector cannot see: an
    // explicit-receiver or computed call, whose feature resolves out of the
    // demanded units the same way.
    def "autoload" (recv, sym, path) {
        let name = const_name_arg(sym)?;
        let path = crate::builtins::convert::to_rstr(path)?
            .lock()
            .to_utf8_lossy()
            .into_owned();
        // Ruby announces an autoload at DECLARATION time, not when the file
        // finally loads (`rb_autoload_str`, `variable.c:2890`) -- and on every
        // declaration, re-declaring the same name included (oracle-verified).
        register_autoload(recv_cid(recv).0, name.clone(), path.clone());
        crate::runtime_meta::fire_const_added(recv_cid(recv), &name)?;
        // A compiled-in unit runs at the constant's first READ, which is
        // ruby's own point. The read cannot MISS -- a unit's classes are in
        // the dispatch tables from startup -- so the emitter gates the read
        // instead (`zeo_rt_autoload_touch`), and this row only records.
        if crate::features::has_feature(&path) {
            return Ok(RubyValue::Nil);
        } else if crate::builtins::kernel::feature_already_loaded(&path) {
            // Spliced at compile time: the constant is already defined, and
            // CRuby answers `nil` from `autoload?` once a feature has loaded.
            take_autoload_target(recv_cid(recv).0, &name);
        }
        // A target that is not on the load path raises NOTHING here. Ruby
        // registers an autoload without touching the file, so a declaration
        // naming a feature that is absent is not an error until the constant
        // is READ -- and most never are: actionpack declares
        // `autoload :Test, "rack/test"` in every program that loads
        // action_dispatch. The record left above is what `const_missing`
        // turns into that read's LoadError.
        Ok(RubyValue::Nil)
    }
    def "autoload?" cfunc (recv, sym, inherit?) {
        let name = const_name_arg(sym)?;
        let cid = recv_cid(recv);
        // A constant that resolved is loaded, and CRuby answers nil for that
        // whatever the registration said.
        if const_lookup(cid, &name, inherit_search(inherit)).is_some() {
            return Ok(RubyValue::Nil);
        }
        Ok(match pending_autoloads().lock().get(&(cid.0, name)) {
            Some(path) => RubyValue::Str(crate::string_new(path.clone())),
            None => RubyValue::Nil,
        })
    }

    // `Module#const_missing(:X)` -- the default hook, which just raises the
    // NameError a missing constant would have raised anyway. A program that
    // OVERRIDES it gets its own definition; this row is what `super` lands on.
    def "const_missing" (recv, name) {
        let name = const_name_arg(name)?;
        let cid = recv_cid(recv);
        // An `autoload` whose feature never loaded: the read is what triggers
        // the load in ruby, so it is what raises the LoadError. Answered here
        // because every const miss funnels through this row.
        if let Some(path) = pending_autoloads().lock().get(&(cid.0, name.clone())) {
            return Err(crate::builtins::kernel::missing_feature_error(path));
        }
        let qualified = match crate::dispatch::class_name(cid) {
            Some(owner) if cid.0 != 0 => format!("uninitialized constant {owner}::{name}"),
            _ => format!("uninitialized constant {name}"),
        };
        // Carries `#name` and `#receiver` exactly as the pre-hook baked
        // raise did -- every const miss now funnels through this row.
        Err(crate::Signal::Raise(crate::dispatch::stamp_backtrace(
            crate::dispatch::make_name_error(qualified, &name, recv.clone()),
        )))
    }
    // `Module#const_source_location` -- `nil` for a constant nobody defines,
    // `[file, line]` for one a Ruby assignment created, and `[]` for one that
    // exists with no source behind it. That last is CRuby's answer for every
    // constant defined in C, and so it is here for the ones the runtime seeds.
    def "const_source_location" cfunc (recv, name, inherit?) {
        let name = const_name_arg(name)?;
        if !is_const_path(&name) {
            return Err(name_error!("wrong constant name {name}"));
        }
        let cid = recv_cid(recv);
        let how = inherit_search(inherit);
        if const_lookup(cid, &name, how).is_none() {
            return Ok(RubyValue::Nil);
        }
        let found = crate::constants::const_location(cid.0, &name, how == Search::Own);
        Ok(RubyValue::Array(crate::array_new(match found {
            Some((file, line)) => vec![
                RubyValue::Str(crate::string_new(file.to_string())),
                RubyValue::Int(line as i64),
            ],
            None => Vec::new(),
        })))
    }

    // `Module#public_instance_method(:name)` -- `instance_method`'s narrowing
    // to a method a caller could reach with an explicit receiver.
    def "public_instance_method" (recv, arg) {
        let cid = recv_cid(recv);
        let name = crate::Symbol::intern(&name_arg(arg)?);
        if crate::dispatch::instance_method_visibility(cid, name)
            == Some(crate::dispatch::MethodVisibility::Private)
        {
            let kind = if crate::dispatch::class_is_module(cid).unwrap_or(false) {
                "module"
            } else {
                "class"
            };
            return Err(name_error!(
                "method '{}' for {kind} '{}' is private",
                name.name(),
                crate::dispatch::class_name(cid).unwrap_or_default()
            ));
        }
        crate::builtins::unbound_method::unbound_method_new(cid, arg)
    }
    // `Module#undefined_instance_methods` -- the names this module's own body
    // `undef`'d. `Complex` is the one core class that uses this, and it is why
    // `Complex(1, 2).positive?` raises where `Rational(1, 2).positive?` is false.
    def "undefined_instance_methods" (recv) {
        Ok(syms_to_array(crate::dispatch::undefined_method_names(recv_cid(recv))))
    }
    // `Module#set_temporary_name` -- only a class with NO permanent name (no
    // constant path to it) may take one, so every compile-time class refuses.
    // `nil` clears a temporary name, making the class anonymous again.
    def "set_temporary_name" (recv, name) {
        let cid = recv_cid(recv);
        let name = match name {
            RubyValue::Nil => None,
            other => {
                let text = crate::builtins::convert::to_rstr(other)?
                    .lock()
                    .to_utf8_lossy()
                    .into_owned();
                if text.is_empty() {
                    return Err(crate::builtins::arg_error!("empty class/module name"));
                }
                if text.contains("::") || text.starts_with(|c: char| c.is_ascii_uppercase()) {
                    return Err(crate::builtins::arg_error!(
                        "the temporary name must not be a constant path to avoid confusion"
                    ));
                }
                Some(text)
            }
        };
        if !crate::runtime_meta::set_temporary_class_name(cid, name) {
            return Err(crate::builtins::runtime_error!("can't change permanent name"));
        }
        Ok(recv.clone())
    }
    // Private `Module#refine` reached at RUNTIME -- `Module.new { refine(C)
    // { ... } }`, the only way to refine a class chosen by the caller. Mints
    // the holder, runs the block with the holder as self/definee, and
    // answers it; DEFINITION only (a `using` is what would activate it).
    private def "refine" (recv, target, &block) {
        let RubyValue::Class(target_id) = target else {
            return Err(type_error!(
                "wrong argument type {} (expected Class or Module)",
                crate::builtins::check_type_name(target)
            ));
        };
        let Some(RubyValue::Proc(b)) = &block else {
            return Err(crate::builtins::arg_error!("no block given"));
        };
        crate::runtime_meta::runtime_refine(recv_cid(recv), *target_id, b)
    }
    // Private `Module#using` reached at RUNTIME. zeo resolves `using` at
    // compile time by rewriting the call sites inside its lexical range; a
    // dynamically dispatched `using` has no lexical range to rewrite, so it
    // refuses loudly rather than activate nothing.
    private def "using" (_recv, _module) {
        Err(not_impl_error!(
            "Module#using cannot be reached through a runtime send: zeo activates refinements at compile time"
        ))
    }
    // `Module#refinements` -- the `Refinement` modules THIS module's `refine`
    // blocks minted, in source order. They are ordinary registered modules
    // carrying the refined methods; what makes each a `Refinement` is the
    // `(module, target)` pair codegen marked it with.
    def "refinements" (recv) {
        Ok(RubyValue::Array(crate::array_new(
            crate::dispatch::refinements_of(recv_cid(recv))
                .into_iter()
                .map(RubyValue::Class)
                .collect(),
        )))
    }

    // `Module.nesting` -- the lexical class/module chain at the CALL SITE,
    // innermost first, which is compile-time knowledge: codegen folds the
    // literal `Module.nesting` into the chain it already tracks. This row
    // serves a computed `Module.send(:nesting)`, where no lexical scope
    // survives to answer with, and top level is `[]` in CRuby too.
    def self."nesting" (_recv) {
        Ok(RubyValue::Array(crate::array_new(Vec::new())))
    }
    // `Module.used_modules`/`.used_refinements` -- the refinements ACTIVATED
    // in the caller's lexical scope. zeo resolves `using` at compile time and
    // keeps no runtime activation set, so both are empty.
    def self."used_modules" | "used_refinements" (_recv) {
        Ok(RubyValue::Array(crate::array_new(Vec::new())))
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "=="(recv, _other) { inherited_row!(basic_object, "==", recv, __args, None) }
    def "freeze"(recv) { inherited_row!(kernel, "freeze", recv, __args, None) }
}

/// Shared body of `Module`'s `<`/`<=`/`>`/`>=`: a non-class/module argument
/// is a TypeError (`compared with non class/module`), an unrelated class is
/// `nil`, and a related one runs `pred` over the `module_cmp` ordering.
fn module_ordering_op(
    recv: &RubyValue,
    arg: &RubyValue,
    pred: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<RubyValue, crate::Signal> {
    let RubyValue::Class(other) = arg else {
        return Err(type_error!("compared with non class/module"));
    };
    Ok(match crate::dispatch::module_cmp(recv_cid(recv), *other) {
        Some(o) => RubyValue::Bool(pred(o)),
        None => RubyValue::Nil,
    })
}

/// A `:name`/`"name"` method-name argument as a bare `String`. Accepts a
/// Symbol or String (real Ruby takes either); anything else is a TypeError.
fn name_arg(v: &RubyValue) -> Result<String, crate::Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(s.name().to_string()),
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        _ => Err(type_error!(
            "{} is not a symbol nor a string",
            v.inspect_string()
        )),
    }
}

/// The `:@@x`/`"@@x"` argument of the `class_variable_*` family, as the bare
/// name (`x`) the `cvars` table is keyed on. A name without the leading
/// `@@` is a NameError, matching real Ruby's shape.
fn cvar_name_arg(v: &RubyValue) -> Result<String, crate::Signal> {
    let raw = name_arg(v)?;
    match raw.strip_prefix("@@") {
        Some(name) => Ok(name.to_string()),
        None => Err(name_error!(
            "'{raw}' is not allowed as a class variable name"
        )),
    }
}

/// The `:@x`/`"@x"` argument of the `instance_variable_*` family, as the
/// BARE name (`x`) the `civars` table is keyed on -- matching what codegen
/// keys a static class-ivar access on, which is `safe_ident`'s output over
/// an already-`@`-less HIR name.
///
/// Both a Symbol and a String are accepted (real Ruby takes either), and a
/// name without the leading `@` is a NameError rather than a silent miss --
/// oracle-verified, message shape included:
/// `K.instance_variable_get(:a)` => `'a' is not allowed as an instance
/// variable name`.
fn ivar_name_arg(v: &RubyValue) -> Result<String, crate::Signal> {
    let raw = match v {
        RubyValue::Symbol(s) => s.name().to_string(),
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        _ => {
            return Err(type_error!(
                "{} is not a symbol nor a string",
                v.inspect_string()
            ));
        }
    };
    match raw.strip_prefix('@') {
        Some(name) => Ok(name.to_string()),
        None => Err(name_error!(
            "'{raw}' is not allowed as an instance variable name"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeo_abi::*;

    /// The `Module` rows are `ruby_class!`-generated (their Rust fn names are
    /// mangled), so reach them the way dispatch does -- through the registered
    /// instance table keyed by `MODULE_CLASS`.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(MODULE_CLASS)
            .expect("Module is a registered builtin table")
            .instance
            .as_ref()
            .expect("Module has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Module#{name} is defined"))
    }

    #[test]
    fn module_case_eq_checks_ancestry_registry_free() {
        // 5.class == Integer; Integer's fallback chain contains Numeric.
        let case_eq = imethod("===");
        let r = case_eq(&RubyValue::Class(NUMERIC_CLASS), &[RubyValue::Int(5)], None).unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let r = case_eq(&RubyValue::Class(STRING_CLASS), &[RubyValue::Int(5)], None).unwrap();
        assert!(matches!(r, RubyValue::Bool(false)));
    }

    #[test]
    fn ancestors_row_reflects_the_fallback_chain() {
        let RubyValue::Array(a) =
            imethod("ancestors")(&RubyValue::Class(INTEGER_CLASS), &[], None).unwrap()
        else {
            panic!()
        };
        assert_eq!(a.lock().len(), 6); // [Integer, Numeric, Comparable, Object, Kernel, BasicObject]
    }
}
