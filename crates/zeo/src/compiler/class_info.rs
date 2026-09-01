//! [`ClassInfo`]/[`MethodEntry`] and [`Compiler`]'s class-shape and
//! method-table queries.

use super::*;

pub struct ClassInfo {
    pub name: String,
    /// Which `Ruby::Box` this class/module is DEFINED in -- `0` is the main
    /// box (where the user's own top-level program runs; builtins and the
    /// built-in exceptions also live at box 0, distinguished by
    /// `is_builtin`/`is_bootstrap`). Set for box-required/box-eval'd
    /// definitions; carried since day one so every consumer is already
    /// box-shaped -- see `Compiler::resolve_class`.
    pub box_id: u32,
    /// The class/module this one is namespace-nested inside (`class Item`
    /// written within `class Store`'s body, or the qualified form `class
    /// Store::Item` -> `Some(Store)`), or `None` for a top-level
    /// definition. Drives both name resolution (`resolve_class`'s
    /// scope-exact lookup) and fully-qualified display names (`fq_name`).
    pub lexical_parent: Option<ClassId>,
    /// `true` when this class/module was defined via the QUALIFIED form
    /// (`class Store::Item ... end`) rather than textual nesting -- real
    /// Ruby gives that form a cref that SKIPS the path's own prefix (its body
    /// does NOT see `Store`'s constants lexically; oracle-verified NameError),
    /// so `cref_of` follows `cref_parent` rather than `lexical_parent` here,
    /// while `fq_name`/resolution keep the `lexical_parent` link. One
    /// documented approximation: the flag is per-CLASS, not
    /// per-body-occurrence, so a nested-form class REOPENED via the qualified
    /// form (or vice versa) keeps its original cref for all bodies.
    pub qualified_def: bool,
    /// The scope this definition was WRITTEN in -- what `cref_of` walks.
    ///
    /// Equal to `lexical_parent` for a textually nested definition, and the
    /// two only part for a qualified one: `module HTTPX; class Connection::
    /// HTTP2` NAMES its class under `HTTPX::Connection` and RESOLVES names in
    /// its body against `[HTTP2, HTTPX]` -- ruby skips the prefix the path
    /// spelled, but keeps every scope the definition is written inside.
    /// Cutting the chain outright instead was a silent wrong answer for every
    /// constant such a body reads, `< Error` among them.
    pub cref_parent: Option<ClassId>,
    /// `true` for classes from the built-in exceptions
    /// (`parse::BUILTIN_EXCEPTIONS_RB`) -- together with `is_builtin`, the
    /// "defined before any user program runs" set that stays visible inside
    /// EVERY box (CRuby's dup-from-master rule).
    /// Marked by `analyze` via `Hir::builtin_exceptions_len`.
    pub is_bootstrap: bool,
    /// `None` for `Object` (the implicit root) and for every MODULE (a
    /// module has no superclass at all, not even implicitly `Object` --
    /// real Ruby's `Module#ancestors` on a standalone module is just
    /// `[M]`). Every ordinary class gets `Some(_)`, defaulting to
    /// `OBJECT_CLASS` when no `< Super` was written.
    pub parent: Option<ClassId>,
    /// Whether a `< Super` clause was ever WRITTEN for this class, as opposed
    /// to `parent` merely defaulting to `OBJECT_CLASS`.
    ///
    /// Needed to tell a class opened bare (`class Sub` -- often just to hold a
    /// nested class) from one explicitly declared `class Sub < Object`. A
    /// later reopen carrying a superclass may ESTABLISH the parent link in the
    /// first case, while the second is a genuine conflict.
    pub explicit_superclass: bool,
    /// Whether a real `class Foo` DEFINITION was written with no `< Super` --
    /// as opposed to the forward SHELL zeo mints for a name a later file
    /// defines (`resolve_or_create_container`, which no ruby program has an
    /// equivalent of).
    ///
    /// The two look identical afterwards: parent `Object`, no explicit clause.
    /// Ruby only knows the first, and rejects a reopen that then declares a
    /// superclass -- `class Sub; end; class Sub < Base; end` is `TypeError:
    /// superclass mismatch for class Sub`. A shell is not a definition and
    /// carries no such history, so the reopen that declares its superclass
    /// establishes it.
    pub bare_definition: bool,
    /// Every compile-time mixin this class's body ran, in DOCUMENT order,
    /// each tagged `true` for a `prepend`.
    ///
    /// ONE list, not an `includes` beside a `prepends`, because the
    /// interleaving decides the chain: the two verbs search different scopes,
    /// so whichever runs first finds an empty one (see
    /// `crate::analyze::mro::compute_ancestors`). `includes()` and
    /// `prepends()` read the halves back out.
    pub mixin_order: Vec<(ClassId, bool)>,
    /// The constants this class's body marked with `private_constant`, minus
    /// any a later `public_constant` restored. A qualified `M::A` naming one
    /// of these from OUTSIDE `M`'s lexical scope is a NameError, and
    /// `M.constants` omits it.
    pub private_constants: std::collections::BTreeSet<String>,
    /// Every constant name this class's body names in a `private_constant`
    /// or `public_constant` directive, whether or not the last one hid it.
    /// Privacy is POSITIONAL -- each directive runs where it is written --
    /// so a read of one of these names asks the run time rather than
    /// folding either way. [`private_constants`] stays the compile-time
    /// view for the folds that can still take one.
    pub const_visibility_names: std::collections::BTreeSet<String>,
    /// `extend`ed modules, in source order -- NOT part of `ancestors()` at
    /// all, which linearizes INSTANCE-method resolution. An extended module
    /// joins this class's SINGLETON chain instead, so it drives
    /// `class_methods` materialization here and is baked into the registry
    /// (`ClassEntry::extends`) for the two things that observe that chain at
    /// run time: `C.is_a?(M)` and `C.singleton_class.ancestors`. An INSTANCE
    /// of the class is unaffected -- `C.new.is_a?(M)` stays false.
    pub extends: Vec<ClassId>,
    /// Modules `prepend`ed onto this class's SINGLETON class
    /// (`C.singleton_class.prepend(M)` / `class << self; prepend M; end`) --
    /// their INSTANCE methods become this class's class methods, at HIGHER
    /// priority than its own `def self.x` (which they may override, with
    /// `super` reaching the original). The class-method analogue of `prepends`;
    /// like `extends` it affects only `class_methods` materialization, never
    /// instance resolution. Source order; most recently prepended wins (closest
    /// to the front of the singleton chain).
    pub class_method_prepends: Vec<ClassId>,
    /// Names this class's body `undef`'d -- see `HirNode::Undef`.
    /// `mro::materialize_methods` refuses to materialize them onto this
    /// class, which is what makes an INHERITED name disappear here while
    /// staying live on the ancestor that defined it.
    pub undefined: FSet<String>,
    /// The CLASS-method half of [`ClassInfo::undefined`] -- names retired by an
    /// `undef`/`undef_method` inside `class << self` (`HirNode::ClassMethodUndef`).
    /// `mro::materialize_class_methods` skips them, so an inherited class method
    /// disappears here while staying live on the ancestor that defined it.
    pub class_undefined: FSet<String>,
    /// Names this class's body may `undef_method` at RUNTIME -- an `undef :m`
    /// under a guard zeo can't decide (`undef :to_a if respond_to?(:to_a)`,
    /// drb), which lowers to a real send rather than the compile-time
    /// `undefined` record above.
    ///
    /// Whether the name survives is a runtime fact, so codegen can't emit the
    /// direct call it otherwise would: `may_be_undefined_at_runtime` sends these
    /// through dynamic dispatch, where the overlay's tombstone is consulted.
    /// Pay-per-use -- a program with no conditional `undef` is untouched.
    pub runtime_undefs: FSet<String>,
    /// The modules a `Refinement#import_methods` in this class's body names.
    /// Only a refinement holder ever has any. The RUNTIME does the copying;
    /// the compiler needs the names so `refinement_defines` nominates the call
    /// sites a `using` covers -- an unnominated site never asks the refinement
    /// at all, however well the copy went.
    pub imported_modules: Vec<ClassId>,
    /// `(new, old, is_class_method, seq)` aliases whose source method is
    /// INHERITED or lives in another body of this class (not defined earlier
    /// in the SAME body -- that form is cloned at lowering, see
    /// `lower::defs::push_alias`) -- recorded by `analyze::register_class`
    /// from a `HirNode::AliasMethod` and resolved by `mro::resolve_aliases`
    /// once the ancestor chain is linearized. `seq` is the alias's
    /// [`SiteDef::seq`] position: an alias binds the body that existed WHEN
    /// IT RAN, so resolution filters `method_history` against it.
    /// `is_class_method` (an `alias` inside `class << self`) resolves against
    /// `own_class_methods`. See `HirNode::AliasMethod`'s docs.
    ///
    /// The last field is the alias site's STREAM (`Compiler::unit_stream`).
    /// `seq` orders definitions within one stream only, so resolution may
    /// compare it against a candidate's seq only when the two share a stream.
    pub pending_aliases: Vec<(String, String, bool, u32, Option<u32>)>,
    /// Every own-method registration in execution order:
    /// `(name, is_class_method, seq, scope)`. Unlike `own_methods`, where a
    /// redefinition REPLACES the earlier row (last-`def`-wins is what dispatch
    /// wants), the history keeps superseded entries -- `compiler.scopes` is
    /// append-only, so the older bodies are still there to clone. This is
    /// what lets `resolve_aliases` bind the body that existed when the alias
    /// ran instead of the final one (the alias-chaining idiom: reopen, alias,
    /// redefine -- resolving against the final table made the alias call
    /// itself and the compiled program abort on a native stack overflow).
    pub method_history: Vec<(String, bool, u32, ScopeId)>,
    /// Superseded instance-method bodies that must still be COMPILED: the
    /// non-final `method_history` rows of a name whose redefinition timeline
    /// is observable (a pre-reopen call, a `method_added` hook). Codegen
    /// emits each as a mangled inherent method beside the live one, so the
    /// positional installs (`HirNode::MethodRedefine`) and the boot install
    /// of the first body ([`Compiler::positional_redefs`]) have a trampoline
    /// target. See `analyze::redefs`.
    /// `(scope, is_class_method)` -- the channel decides which overlay map
    /// the install writes and whether the body's `self` is a class.
    pub redef_scopes: Vec<(ScopeId, bool)>,
    /// Names from a `module_function :m` whose `m` is INHERITED rather than
    /// defined in this body -- recorded by `analyze::register_class` from a
    /// `HirNode::ModuleFunction` and resolved by `mro::resolve_module_functions`
    /// once the ancestor chain is linearized.
    pub pending_module_functions: Vec<String>,
    /// `(new, old)` aliases whose source is a BUILTIN (no user `Scope`
    /// anywhere in the ancestor chain -- Kernel's `raise`, Object's `dup`,
    /// ...): there is no HIR body to clone, so the alias is a NAME
    /// INDIRECTION instead. `old` is always terminal (an alias of an alias
    /// resolves through `builtin_alias_target` when recorded). Codegen
    /// substitutes `old` at statically-resolved call sites and emits a
    /// validated `register_alias` row for dynamic dispatch -- a source that
    /// resolves to nothing raises `NameError` at program start, real Ruby's
    /// timing (a class body executes at runtime). See `mro::resolve_aliases`.
    /// The third field is EAGER: the aliasing class defines `old` itself
    /// LATER, so a live name indirection would follow that later `def` --
    /// which ruby's `rb_alias` does not do. An eager row binds the ancestor's
    /// native body once, at registration. See `mro::resolve_aliases`.
    pub builtin_aliases: Vec<(String, String, bool)>,
    /// [`ClassInfo::builtin_aliases`]'s singleton-side twin: an alias written
    /// inside `class << self` whose source is a builtin CLASS method. `class
    /// << self; alias [] new` -- the `Klass[...]` constructor shorthand rack,
    /// rack-test, pry, coderay, sprockets, warden and omniauth all write -- has
    /// `Class#new` as its source, which lives in the static builtin
    /// class-method table and has no `Scope` to clone. Same terminal rule, and
    /// codegen emits `register_class_alias` rather than `register_alias`, so
    /// the row lands in the table a class-OBJECT receiver consults.
    pub class_aliases: Vec<(String, String)>,
    /// `(name, visibility)` from a `private`/`public`/`protected :m` that
    /// re-declares an INHERITED method's visibility (no local `def` to retag).
    /// Applied by codegen after materialization stamps each method with its
    /// defining class's visibility. See `HirNode::MethodVisibility`.
    pub visibility_overrides: Vec<(String, crate::hir::Visibility)>,
    /// The class-method half of `visibility_overrides`: `(name, visibility)`
    /// from a `private_class_method`/`public_class_method` naming a method this
    /// body does not itself define. See [`crate::hir::HirNode::ClassMethodVisibility`].
    pub class_visibility_overrides: Vec<(String, crate::hir::Visibility)>,
    /// `true` for `module Name ... end`: never instantiated (no `Name.new`,
    /// no instance emission at all), used only as a source for
    /// materialization into whatever includes/prepends/extends it.
    pub is_module: bool,
    /// The full flattened ivar set (own methods' ivars ∪ every
    /// MRO-reachable inherited/mixed-in method's ivars) -- computed once by
    /// `analyze::mro::materialize` from the MATERIALIZED `methods` list, not
    /// `own_methods` (see that module's docs for why this needs no separate
    /// parent-first merge pass the way zeo's C struct layout does).
    pub ivars: Vec<String>,
    /// `Struct`/`Data` MEMBERS -- real slots on the generated struct that are
    /// NOT instance variables -- in `Struct.new`'s own argument order, which is
    /// `members`' order and so `to_a`'s and `[]`'s index order.
    ///
    /// A member costs exactly what an ivar costs to reach, but CRuby answers
    /// `nil` to `S.new(1).instance_variable_get(:@x)` and `[]` to
    /// `instance_variables` (pinned by `tests/spinel/data_struct_ivar_get_nil.rb`),
    /// so it is absent from every by-NAME path. `ruby_class!`'s `hidden` block
    /// is what enforces that. DISJOINT from `ivars`, which `mro::materialize`
    /// guarantees. Empty for every class that is not a compiled `Struct`.
    pub hidden_ivars: Vec<String>,
    /// Instance methods LITERALLY written in this class/module's own body
    /// (`def name`, not `def self.name`) -- the source `materialize` reads
    /// from, and what `super`-resolution searches (see `analyze::mro`'s
    /// docs). Never codegen'd directly except when `methods` reuses one of
    /// these verbatim (this class's own, non-inherited definition).
    pub own_methods: Vec<ScopeId>,
    /// Where each name sits in `own_methods` / `own_class_methods`.
    ///
    /// Registration REPLACES an earlier same-name entry rather than appending
    /// (real Ruby's last-`def`-wins rule), so it has to find that entry first.
    /// Scanning the list and comparing scope names made registering a class's
    /// methods quadratic in their count -- at Rails scale, on classes with
    /// hundreds of them. `analyze::add_own_method_at` is the only writer of
    /// either list, so these stay in step with them.
    pub own_method_at: FMap<String, usize>,
    /// `own_method_at` for `own_class_methods` -- instance and class methods
    /// are separate namespaces, hence the separate index.
    pub own_class_method_at: FMap<String, usize>,
    /// The full MRO-resolved set `clif::classes` registers one method
    /// row per entry for -- own ∪ every name reachable via
    /// `ancestors` (superclass, `include`, `prepend`). Always populated by
    /// `analyze::mro::materialize`, even for a class with no mixins at all
    /// (closes a latent gap: the compiler never generated a Rust method for a
    /// purely-inherited, non-overridden method at all).
    ///
    /// Read it through [`methods_of`](Compiler::methods_of) /
    /// [`lookup_method`](Compiler::lookup_method), never directly: the backing
    /// store is meant to become a memoized ancestor walk, which is what CRuby
    /// does (it flattens nothing -- see [`MethodEntry`]).
    pub methods: Vec<MethodEntry>,
    /// `def self.name` written literally in this class/module's own body.
    pub own_class_methods: Vec<ScopeId>,
    /// MRO-resolved class methods: `own_class_methods` (always wins) ∪
    /// `extends`' own instance methods (materialized as class methods, most
    /// recently `extend`ed module closest -- mirrors `include`'s priority
    /// rule). Codegen emits these as plain associated functions (no `self`
    /// receiver) alongside the class's ordinary `impl` block, or as a
    /// `pub mod` of free functions for a module with no struct of its own.
    /// The singleton-side twin of `methods`, and read the same way.
    pub class_methods: Vec<MethodEntry>,
    /// `(name, index into methods)` and its class-method twin, sorted by name
    /// so [`lookup_method`](Compiler::lookup_method) is a binary search.
    ///
    /// A scan would be O(visible methods), and `method_in_chain` is asked once
    /// per call site: that product is what took spinel's Rails-scale front end
    /// down seven separate times. Built once, at the end of
    /// `analyze::mro::materialize`, after the last thing that rewrites a table.
    pub(super) method_index: Vec<(NameId, u32)>,
    pub(super) class_method_index: Vec<(NameId, u32)>,
    /// `@@x` storage ownership, resolved once at analyze time (not per
    /// access, unlike zeo -- see `analyze::mro::resolve_cvars`'s docs):
    /// name -> the class/module that actually OWNS the runtime storage
    /// (nearest ancestor, including self, that ever claimed it first).
    pub cvar_owners: FMap<String, ClassId>,
    /// Bare-constant storage ownership, resolved once at analyze time --
    /// same scheme as `cvar_owners` (nearest ancestor, including self, that
    /// ever claimed the name first), used by `clif::expr`'s constant lowering.
    /// Only bare (`scope: None`) constant writes register ownership this way
    /// -- an explicit `Foo::NAME` write always targets `Foo` directly,
    /// regardless of lexical position (see `HirNode::ConstWrite`'s docs).
    pub const_owners: FMap<String, ClassId>,
    /// Class-body top-level statements (`@@x = expr`, `CONST = expr`, and
    /// general side-effecting code) across ALL of this class's definition
    /// sites, flat and in registration order -- what the cvar/const
    /// collectors and `mro` read. EXECUTION is per-site and in document
    /// order via `Compiler::class_body_sites` (see its docs); this union
    /// list is analysis-only.
    pub class_body_stmts: Vec<NodeId>,
    /// Every `extend`ed module's method copies registered for THIS class's
    /// singleton-chain `super` resolution -- `(module, materialized scope)`
    /// pairs, winners AND shadowed (the flattened `class_methods` keeps
    /// only winners, which is exactly wrong for `super` -- the same
    /// distinction `own_impls` covers for instance methods). Filled by
    /// `analyze::mro::materialize_class_methods`; emitted + registered as
    /// runtime `define_singleton_super_target` rows by `codegen`.
    pub singleton_super_targets: Vec<(ClassId, ScopeId)>,
    /// The full linearized ancestor chain (this class/module first,
    /// prepends before it, includes/superclass after -- see
    /// `analyze::mro::compute_ancestors`), computed once. Empty until
    /// `analyze::mro::materialize` runs.
    pub ancestors: Vec<ClassId>,
    /// `true` for one of the reserved `BUILTIN_CLASSES` placeholders
    /// (`Integer`/`Array`/etc.) -- gets no emitted class body of its own
    /// (`RubyValue::Int`/`Array`/etc. ARE the runtime representation
    /// already), only a registration (`clif::classes`' `register_builtin`
    /// rows) so `is_a?`/`respond_to?` resolve correctly against it. `false` for
    /// `Object` (index 0, handled by its own pre-existing `idx != 0` checks)
    /// and for every ordinary user-defined class/module.
    pub is_builtin: bool,
    /// `Some(other id)` when this ClassInfo is a NAME rather than a class of
    /// its own: `resolve_class` answers the other id, and codegen registers
    /// nothing for it. Two cases use this. A per-box builtin-reopen OVERLAY
    /// carries a box's patches on the root builtin -- its methods emit as
    /// value methods registered under `(root id, box_id)`, while instances
    /// keep the ROOT's ClassId (`box::String == String`). A core constant
    /// ALIAS is a second spelling of one class (`Errno::EWOULDBLOCK` IS
    /// `Errno::EAGAIN`), which is what makes rescuing by either name catch the
    /// other. `None` for every ordinary class, including root builtins.
    pub builtin_overlay: Option<ClassId>,
    /// `Some(feature)` for a require-gated builtin (`Base64`, gated by
    /// `"base64"`) -- mirrored from `zeo_abi::BuiltinClass::feature`. Its
    /// constant is INVISIBLE to `resolve_class` until `feature` has been
    /// activated by a `require` somewhere in the program
    /// (`Hir::activated_features`), so referencing it un-`require`d NameErrors
    /// exactly as in CRuby (see `Compiler::feature_active`). `None` for every
    /// always-on class (all user classes and all core builtins).
    pub feature_gate: Option<&'static str>,
    /// Registered but not PROMISED -- the class-level lift of
    /// [`Scope::runtime_conditional`]. `true` for a NEW class/module defined
    /// under a top-level guard zeo cannot decide: its shape registers (the
    /// static MRO needs one) and its body still runs at document position
    /// inside the live `if`, but nothing may treat the constant as existing
    /// before that body runs. Codegen emits `zeo_rt::conceal_class` in the
    /// prologue and `zeo_rt::reveal_class` at the head of every body site;
    /// every `defined?`/constant fold answers at run time instead of compile
    /// time; every `def` in the body is `Conditional::Yes`, so method rows
    /// ride the runtime overlay the same way. See
    /// `analyze`'s guarded-top-definition rewrite for what sets it.
    pub runtime_conditional: bool,
    /// The compiled-in load-path unit this class is DEFINED by, if any.
    ///
    /// A unit is a file nothing has required yet -- it runs when a `require`
    /// or an `autoload` reaches it, which is exactly when CRuby defines its
    /// constants. `None` means the class exists before the first statement
    /// runs: a main-file definition, a spliced `require`, or a builtin.
    ///
    /// The class is registered for DISPATCH at startup either way (call
    /// sites bind by id). What waits for the unit is its NAME:
    /// `Compiler::UNIT_UNRESOLVED` until `analyze` accepts the unit, and
    /// still that if the unit is DECLINED -- such a unit never runs, so its
    /// classes never get one.
    pub unit: Option<u32>,
    /// EXPERIMENTAL (M2): `Some(merged-manifest index)` when this class was
    /// registered from a package INTERFACE rather than from this arena. Its
    /// scopes are body-less (`Scope::extern_symbol`); codegen emits no rows
    /// or bodies for it -- the manifest merge provides both -- and a host
    /// definition targeting it (reopen, subclass, mixin) is refused by name
    /// until M4's patch rows land.
    pub imported_pkg: Option<u32>,
}

impl ClassInfo {
    /// The `include`d modules, in source order.
    pub fn includes(&self) -> impl Iterator<Item = ClassId> + '_ {
        self.mixin_order
            .iter()
            .filter(|&&(_, prepend)| !prepend)
            .map(|&(m, _)| m)
    }

    /// The `prepend`ed modules, in source order.
    pub fn prepends(&self) -> impl Iterator<Item = ClassId> + '_ {
        self.mixin_order
            .iter()
            .filter(|&&(_, prepend)| prepend)
            .map(|&(m, _)| m)
    }

    /// Whether this class's body ran any `prepend`.
    pub fn has_prepends(&self) -> bool {
        self.mixin_order.iter().any(|&(_, prepend)| prepend)
    }
}

/// One name, bound on one class -- CRuby's `rb_method_entry_t` (method.h:55).
///
/// The pair it forms with [`Scope`] is the whole point: a `Scope` is the
/// DEFINITION (params, body, inferred local types), written once at one `def`
/// and never copied; a `MethodEntry` is the BINDING of that definition onto a
/// class, and the only thing a class inheriting a method needs. CRuby splits
/// them the same way and for the same reason -- `rb_method_definition_t` is
/// refcounted and shared, and the per-class entry is 5 fields.
///
/// zeo used to fuse the two, so `mro::materialize` minted a fresh `Scope` with
/// a cloned body for every (class, inherited method) pair. That is quadratic in
/// classes x visible methods, which is invisible until a Rails-sized graph and
/// then fatal: `require "active_record"` ran 845s and died past 5GB.
#[derive(Clone, Copy, Debug)]
pub struct MethodEntry {
    /// CRuby's `called_id` -- the name this class answers to, interned. Equal
    /// to the definition's own name (an `alias` gets a real `Scope` of its
    /// own), but kept here so a lookup compares integers.
    pub name: NameId,
    /// The one `Scope` holding params/body/local_types.
    pub def: ScopeId,
    /// The class this entry hangs on -- CRuby's `owner`, what `Method#owner`
    /// answers. NOT where the body was written; that is the definition's
    /// `defining_class`, CRuby's `defined_class`, which is where `super`
    /// resumes from.
    pub owner: ClassId,
    /// Per-CLASS, not per-definition: ruby lets a subclass re-scope a method it
    /// inherited without redefining it (`private :inherited_method`), which
    /// CRuby models as a real entry of its own in the subclass.
    pub visibility: Visibility,
    /// This class RE-DECLARED an inherited method's visibility without
    /// redefining it. CRuby plants a real `VM_METHOD_TYPE_ZSUPER` entry in the
    /// subclass for this (vm_method.c:2304-2341), which is why the name then
    /// answers `private_instance_methods(false)` and `instance_method(:x).owner`
    /// on the SUBCLASS while still running the ancestor's body.
    pub zsuper: bool,
    /// Whether this is one of the pristine `BUILTIN_EXCEPTIONS_RB` bodies that
    /// `zeo-rt`'s `register_exceptions` already installs, so codegen emits
    /// nothing for it. Inherited copies stay pristine; a reopen does not.
    pub native_default: bool,
}

impl MethodEntry {
    /// Where the body was WRITTEN -- CRuby's `defined_class`, the point `super`
    /// resumes after. Equal to `owner` for a class's own `def`.
    pub fn defined_class(&self, compiler: &Compiler) -> ClassId {
        compiler.scope(self.def).defining_class
    }
}

/// How deep a `lexical_parent` chain may go before the walk decides it is a
/// cycle. Real source nests a handful of modules; nothing legitimate is near
/// this. The walks that use it run on every constant lookup, so they bound the
/// depth instead of carrying a visited set.
pub(crate) const MAX_NESTING: usize = 256;

/// `(name, position)` for every entry, sorted by name. Ties keep the EARLIEST
/// position, which is the MRO winner -- a table can hold the same name twice
/// only on the singleton side, where a shadowed copy trails its winner.
fn sorted_index(entries: &[MethodEntry]) -> Vec<(NameId, u32)> {
    let mut index: Vec<(NameId, u32)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name, i as u32))
        .collect();
    index.sort_unstable();
    index.dedup_by_key(|(name, _)| *name);
    index
}

/// The entry `name` resolves to, through a built index; falls back to a scan
/// when the index has not been built yet (every lookup before
/// [`Compiler::index_methods`] runs).
fn find_indexed<'a>(
    index: &[(NameId, u32)],
    entries: &'a [MethodEntry],
    name: NameId,
) -> Option<&'a MethodEntry> {
    if index.is_empty() {
        return entries.iter().find(|e| e.name == name);
    }
    let at = index.binary_search_by_key(&name, |&(n, _)| n).ok()?;
    Some(&entries[index[at].1 as usize])
}

impl Compiler {
    /// [`class`](Self::class) for an id this compiler may have no entry
    /// for. A snippet ADOPTS the surrogate of the box its `eval` runs in
    /// (see [`adopt_box_surrogate`](Self::adopt_box_surrogate)) without
    /// materializing a class for it -- a run-time box's id is above
    /// `RUNTIME_CLASS_ID_BASE` and could not be materialized anyway -- so
    /// every LEXICAL question about it answers "nothing", which is
    /// correct: a snippet has no lexical knowledge of it.
    pub fn class_opt(&self, id: ClassId) -> Option<&ClassInfo> {
        self.classes.get(id.0 as usize)
    }

    /// `parent: None` for a module (no superclass at all) or a fresh root;
    /// `Some(_)` for an ordinary class (`register_class` always resolves a
    /// concrete `Some(OBJECT_CLASS)` default when no `< Super` was
    /// written).
    pub fn add_class(&mut self, name: String, parent: Option<ClassId>, is_module: bool) -> ClassId {
        self.classes.push(ClassInfo {
            name,
            box_id: 0,
            lexical_parent: None,
            cref_parent: None,
            qualified_def: false,
            is_bootstrap: false,
            parent,
            private_constants: Default::default(),
            const_visibility_names: Default::default(),
            extends: Vec::new(),
            class_method_prepends: Vec::new(),
            mixin_order: Vec::new(),
            undefined: FSet::default(),
            class_undefined: FSet::default(),
            runtime_undefs: FSet::default(),
            imported_modules: Vec::new(),
            pending_aliases: Vec::new(),
            method_history: Vec::new(),
            redef_scopes: Vec::new(),
            pending_module_functions: Vec::new(),
            builtin_aliases: Vec::new(),
            class_aliases: Vec::new(),
            visibility_overrides: Vec::new(),
            class_visibility_overrides: Vec::new(),
            is_module,
            ivars: Vec::new(),
            hidden_ivars: Vec::new(),
            own_methods: Vec::new(),
            methods: Vec::new(),
            own_method_at: FMap::default(),
            own_class_method_at: FMap::default(),
            explicit_superclass: false,
            bare_definition: false,
            own_class_methods: Vec::new(),
            class_methods: Vec::new(),
            method_index: Vec::new(),
            class_method_index: Vec::new(),
            cvar_owners: FMap::default(),
            const_owners: FMap::default(),
            class_body_stmts: Vec::new(),
            singleton_super_targets: Vec::new(),
            ancestors: Vec::new(),
            is_builtin: false,
            builtin_overlay: None,
            feature_gate: None,
            runtime_conditional: false,
            // The real index lands when the unit SURVIVES (`analyze_impl`'s
            // unit loop); the sentinel only marks it as not-main until then,
            // and a DECLINED unit keeps it -- that unit never runs, so its
            // classes never get a name.
            unit: self.unit_walk.then_some(Self::UNIT_UNRESOLVED),
            imported_pkg: None,
        });
        ClassId((self.classes.len() - 1) as u32)
    }

    pub fn class(&self, id: ClassId) -> &ClassInfo {
        &self.classes[id.0 as usize]
    }

    /// Whether this class's methods run over a boxed
    /// `RubyValue` receiver (the builtin-reopen shape)
    /// rather than a concrete per-class layout. True for every
    /// builtin placeholder AND for `Object` itself: top-level `def`s live
    /// on `Object` (real Ruby's private-on-Object rule), whose instances --
    /// the `main` object, and any value at all once dispatch reaches the
    /// MRO tail -- are `RubyValue`s, never a compiled layout.
    pub fn value_backed(&self, cid: ClassId) -> bool {
        self.class(cid).is_builtin || cid == OBJECT_CLASS
    }

    /// Whether any class in the program inherits from `cid` -- a whole-program
    /// fact, and the guard on every fold that treats an instance's RUNTIME
    /// class as its statically-known one. An inherited method's body
    /// serves the base and every subclass, so `self`
    /// there is typed as the base while its real class may be any descendant.
    pub fn has_subclass(&self, cid: ClassId) -> bool {
        self.classes.iter().any(|c| c.parent == Some(cid))
    }

    /// Whether `cid` has a compiled instance layout of its own, so the
    /// emitter may treat its instances
    /// statically. False for the kinds that have none,
    /// each of which is instead a boxed `RubyValue` dispatched dynamically:
    /// MODULES (no instances), BUILT-INs (their repr is a `RubyValue` variant),
    /// `Object` (the runtime root, name-keyed ivars), BOOTSTRAP classes (the
    /// built-in exceptions, now in `zeo-rt`), and every
    /// NATIVE-BACKED user subclass: an exception subclass (`class MyErr <
    /// StandardError`, the native `RubyException`) or a value-builtin subclass
    /// (`class Stack < Array`, the native `ValueSubclass`), both constructed via
    /// `construct_by_class_id` rather than a per-class layout. The single source
    /// of truth for "is there a layout here?", which several `TyKind::Object`
    /// and `.new` sites gate on.
    pub fn has_generated_struct(&self, cid: ClassId) -> bool {
        let ci = self.class(cid);
        !ci.is_module
            && !ci.is_builtin
            && !ci.is_bootstrap
            && cid != OBJECT_CLASS
            && !self.is_exception_backed(cid)
            && !self.is_value_subclass(cid)
            && !self.is_immediate_subclass(cid)
            // A `class X < Module` instance is a `RubyValue::Class`. Typing
            // `X.new` as `TyKind::Object(X)` bakes static constructor calls to a
            // layout codegen never emits -- the exact failure the comment
            // below describes, hit while wiring this shape up.
            && !self.is_module_subclass(cid)
            // A WeakMap subclass's instances are the native `WeakMap` RObj,
            // and a WeakRef subclass's the native `WeakRef`.
            && !self.is_weakmap_subclass(cid)
            // A Date subclass's instances are the native `RDate`.
            && !self.is_date_subclass(cid)
            // A Proc subclass's instances are still `RubyValue::Proc`.
            && !self.is_proc_subclass(cid)
    }

    /// Walks the recorded `parent` (superclass) links from `cid` toward the
    /// root, inclusive of `cid` itself. The registration-time-safe form of
    /// "is X in the ancestry": `ancestors` is only linearized later by
    /// `mro::materialize`, but the native-backing predicates below must
    /// answer correctly DURING registration too -- `register_method` runs
    /// local-type inference as each class body is walked, and typing an
    /// exception/value subclass's `.new` as `TyKind::Object` there baked
    /// static constructor calls to layouts codegen (correctly) never emits.
    /// Superclass-chain membership is equivalent to the linearized test for
    /// every predicate here: each targets CLASS ids, which only ever enter
    /// an ancestry through `< Super`, never through a mixin.
    ///
    /// BOUNDED, because `parent` is a graph the front end builds rather than a
    /// verified list: a cycle in it makes an unbounded `successors` spin
    /// forever inside whatever predicate asked, with no allocation to show for
    /// it and nothing in the log. That is a real shape --
    /// `ActiveRecord::ConnectionAdapters::SchemaDumper` became its own parent
    /// (see `analyze::resolve_superclass`) and hung `is_exception_backed`. The
    /// bound costs nothing: a cycle repeats within one lap, so every answer
    /// here is the same one an unbounded walk would give.
    pub(super) fn superclass_chain(&self, cid: ClassId) -> impl Iterator<Item = ClassId> + '_ {
        std::iter::successors(Some(cid), |&c| self.class(c).parent).take(MAX_NESTING)
    }

    /// Whether `needle` is `cid` or one of its superclasses -- asked BEFORE a
    /// parent link is established, so the link cannot close a loop.
    pub fn superclass_chain_contains(&self, cid: ClassId, needle: ClassId) -> bool {
        self.superclass_chain(cid).any(|c| c == needle)
    }

    /// Whether `cid`'s instances are the native `RubyException`: the
    /// bootstrap exception classes themselves, and any user subclass of one
    /// (`class MyErr < StandardError`). Such a class has NO generated struct --
    /// its instances are allocated by `zeo-rt`'s `exception_construct` and
    /// its ivars are name-keyed -- so codegen emits its user methods as
    /// `RubyValue`-self bodies `define_method`'d onto the
    /// class id, over the native defaults `register_exceptions` already installed.
    /// `is_module` guards the `Errno` namespace (a module, never instantiated).
    pub fn is_exception_backed(&self, cid: ClassId) -> bool {
        let ci = self.class(cid);
        !ci.is_module
            && (ci.is_bootstrap
                || self
                    .superclass_chain(cid)
                    .any(|a| a == zeo_abi::EXCEPTION_CLASS))
    }

    /// The instantiable value-builtin a USER subclass wraps as its payload:
    /// the first `Array`/`String`/`Hash` in `cid`'s linearized ancestry,
    /// or `None` for anything that isn't such a subclass. `class Stack < Array`
    /// -> `Some(ARRAY_CLASS)`. The builtin itself (`is_builtin`) is excluded --
    /// only a user subclass has a `ValueSubclass` payload. `Regexp` is
    /// deliberately NOT a root (subclassing it is negligible in practice); a
    /// subclass of one stays an analyze rejection.
    pub fn value_payload_root(&self, cid: ClassId) -> Option<ClassId> {
        let ci = self.class(cid);
        if ci.is_module || ci.is_builtin || ci.is_bootstrap || cid == OBJECT_CLASS {
            return None;
        }
        // Superclass-chain walk, not `ancestors` -- see `superclass_chain`.
        // The rule is `zeo_abi::is_payload_root`, shared with the runtime and
        // analyze's subclassable gate. This function was the THIRD
        // hand-synced copy, and it was the one that drifted: a root the
        // other two knew and this one didn't made codegen emit direct
        // builtin-table calls with the BOXED subclass, panicking the row's
        // receiver downcast at runtime.
        //
        // Several roots can sit on one chain (the IO family); the walk
        // picks the NEAREST -- `class TCP < TCPSocket` seeds its payload
        // from `TCPSocket.new`, not from the `IO.new` four links further up.
        self.superclass_chain(cid)
            .find(|a| zeo_abi::is_payload_root(*a))
    }

    /// Whether `cid`'s instances are the native `ValueSubclass`: a user
    /// subclass of `Array`/`String`/`Hash`. Like `is_exception_backed`, such a
    /// class has NO generated struct -- its user methods emit as dynamic-self
    /// deltas and inherited builtin behavior comes via the payload bridge.
    pub fn is_value_subclass(&self, cid: ClassId) -> bool {
        self.value_payload_root(cid).is_some()
    }

    /// Whether `cid` is a user `class P < Proc`.
    ///
    /// The FIFTH native shape, and it is deliberately not a payload wrapper:
    /// every call-site fast path, `&blk` conversion and `to_proc` matches on
    /// `RubyValue::Proc`, so boxing one inside an object would break all of
    /// them. The class rides in `ProcData` instead, leaving the value shape
    /// untouched. declarative's `Variables::Proc` is the case.
    pub fn is_proc_subclass(&self, cid: ClassId) -> bool {
        let ci = self.class(cid);
        if ci.is_module || ci.is_builtin || ci.is_bootstrap {
            return false;
        }
        self.superclass_chain(cid).any(|a| a == PROC_CLASS)
    }

    /// Whether the program defines ANY `class P < Proc`. A `RubyValue::Proc`
    /// then no longer implies the class `Proc`, so `.class` cannot be folded
    /// for a Proc-typed receiver.
    pub fn has_proc_subclass(&self) -> bool {
        (0..self.classes.len() as u32).any(|i| self.is_proc_subclass(ClassId(i)))
    }

    /// Whether `cid` is a user subclass of `Date`/`DateTime`.
    ///
    /// A THIRD native shape: `Date`'s class-method rows already allocate
    /// through the receiver (`RDate::new(jdn, class_of(recv))`), and an
    /// `RDate` carries its class id directly -- so the subclass needs no
    /// payload wrapper and no re-tagging, only to be passed along. tzinfo's
    /// `DateTimeWithOffset` is the case, and activesupport reaches the ledger
    /// through it.
    pub fn is_date_subclass(&self, cid: ClassId) -> bool {
        let ci = self.class(cid);
        if ci.is_module || ci.is_builtin || ci.is_bootstrap {
            return false;
        }
        self.superclass_chain(cid).any(|a| a == zeo_abi::DATE_CLASS)
    }

    /// Whether `cid` is a user subclass of `ObjectSpace::WeakMap`.
    ///
    /// A FOURTH native shape, and the cleanest of them: `weakmap_construct`
    /// already takes the receiver class and builds `WeakMap::new(class)`, so
    /// the subclass simply IS the native type -- no payload wrapper, no
    /// re-tagging. activesupport's `DescendantsTracker::WeakSet` is the case,
    /// and it gates eleven Rails gems.
    pub fn is_weakmap_subclass(&self, cid: ClassId) -> bool {
        let ci = self.class(cid);
        if ci.is_module || ci.is_builtin || ci.is_bootstrap {
            return false;
        }
        self.superclass_chain(cid)
            .any(|a| a == zeo_abi::WEAKMAP_CLASS)
    }

    /// Whether `cid` is a user `class X < Module` -- a MODULE FACTORY, whose
    /// instances are real runtime module ids tagged as belonging to `X`.
    ///
    /// Deliberately NOT a `value_payload_root`: a `ValueSubclass` wrapper would
    /// produce an object with a module inside it, which is not a module -- it
    /// would fail `include`, `Module#===`, constant lookup and `ancestors`. The
    /// two lists must stay disjoint, or the `.new` and `super`
    /// constructor lowerings emit the wrong constructor.
    ///
    /// Rails' `ActiveSupport::Deprecation::DeprecatedConstantProxy` is the
    /// shape, and 22 of the corpus's `Module` rows reach the ledger through
    /// that one file.
    pub fn is_module_subclass(&self, cid: ClassId) -> bool {
        let ci = self.class(cid);
        if ci.is_module || ci.is_builtin || ci.is_bootstrap {
            return false;
        }
        self.superclass_chain(cid).any(|a| a == MODULE_CLASS)
    }

    /// Whether `cid` is a BLANK SLATE -- a `BasicObject` subclass, which does
    /// NOT inherit the Object/Kernel surface (`class`, `inspect`, `dup`,
    /// `respond_to?`, `send`, ...).
    ///
    /// In CRuby this needs no flag at all: `Kernel` is spliced in as an ICLASS
    /// *between* `Object` and `BasicObject` (`object.c:4550` ->
    /// `class.c:1853`), and since method lookup only walks UP the chain,
    /// anything rooted at BasicObject never sees it. So the blank slate is
    /// purely a consequence of chain position, and the ancestor test below
    /// says exactly that: reaches BasicObject, never passes through Object.
    ///
    /// This compiler still needs the predicate because codegen answers the
    /// universal methods from static fast paths that would otherwise bypass
    /// the ancestor walk entirely.
    pub fn is_blank_slate(&self, cid: ClassId) -> bool {
        let ancestors = &self.class(cid).ancestors;
        ancestors.contains(&BASIC_OBJECT_CLASS) && !ancestors.contains(&OBJECT_CLASS)
    }

    /// Either native-backed shape that has no generated struct and whose user
    /// methods emit as `define_method` deltas over runtime-installed behavior:
    /// an exception subclass (`RubyException`) or a value-builtin subclass
    /// (`ValueSubclass`). The shared gate for the delta/construct/reopen paths.
    pub fn is_native_backed(&self, cid: ClassId) -> bool {
        self.is_exception_backed(cid)
            || self.is_value_subclass(cid)
            // A `class X < Module` instance is a `RubyValue::Class` -- no
            // struct to downcast to, so its methods emit as `RubyValue`-self
            // free functions exactly as the other two shapes' do.
            || self.is_module_subclass(cid)
            || self.is_weakmap_subclass(cid)
            || self.is_date_subclass(cid)
            || self.is_proc_subclass(cid)
    }

    /// Whether `cid` is a user subclass of an INSTANCE-LESS builtin -- an
    /// immediate (`Integer`/`Float`/`Symbol`/`NilClass`/`TrueClass`/
    /// `FalseClass`), or one whose allocator CRuby itself undefines
    /// (`BigDecimal`, `Method`). CRuby allows the class DEFINITION
    /// (`MyInt.superclass == Integer`, `is_a?` queries resolve) but has no
    /// instances: `MyInt.new` raises. So codegen emits a registry entry
    /// ONLY -- no struct, no constructor -- and `.new` dynamically resolves
    /// to a raise (`NoMethodError` here; CRuby raises `TypeError` for the
    /// allocator-undefined pair, a documented message-class divergence).
    pub fn is_immediate_subclass(&self, cid: ClassId) -> bool {
        let ci = self.class(cid);
        if ci.is_module || ci.is_builtin || ci.is_bootstrap || cid == OBJECT_CLASS {
            return false;
        }
        // Superclass-chain walk, not `ancestors` -- see `superclass_chain`.
        self.superclass_chain(cid).any(|a| {
            matches!(
                a,
                INTEGER_CLASS
                    | FLOAT_CLASS
                    | SYMBOL_CLASS
                    | NIL_CLASS
                    | TRUE_CLASS
                    | FALSE_CLASS
                    | zeo_abi::BIGDECIMAL_CLASS
                    | zeo_abi::METHOD_CLASS
                    | zeo_abi::BINDING_CLASS
                    | zeo_abi::ENCODING_CLASS
                    | zeo_abi::RATIONAL_CLASS
                    | zeo_abi::MATCH_DATA_CLASS
            )
        })
    }

    /// Every name `class` answers to, in MRO order -- the reading side of
    /// [`ClassInfo::methods`]. Nothing outside this file should index that
    /// field: the store is meant to become a memoized ancestor walk, and these
    /// two accessors are the whole surface that has to keep working when it
    /// does.
    pub fn methods_of(&self, class: ClassId) -> &[MethodEntry] {
        &self.classes[class.0 as usize].methods
    }

    /// The singleton-side twin of [`methods_of`](Self::methods_of).
    pub fn class_methods_of(&self, class: ClassId) -> &[MethodEntry] {
        &self.classes[class.0 as usize].class_methods
    }

    /// What `class` resolves `name` to -- CRuby's `search_method` answer.
    pub fn lookup_method(&self, class: ClassId, name: &str) -> Option<&MethodEntry> {
        let id = self.names.get(name)?;
        let info = &self.classes[class.0 as usize];
        find_indexed(&info.method_index, &info.methods, id)
    }

    /// The singleton-side twin of [`lookup_method`](Self::lookup_method).
    pub fn lookup_class_method(&self, class: ClassId, name: &str) -> Option<&MethodEntry> {
        let id = self.names.get(name)?;
        let info = &self.classes[class.0 as usize];
        find_indexed(&info.class_method_index, &info.class_methods, id)
    }

    /// Builds the per-class name indexes. Call once, after the last pass that
    /// rewrites a method table -- every lookup before this point is answered by
    /// a scan, and every one after it by a binary search.
    pub fn index_methods(&mut self) {
        for info in &mut self.classes {
            info.method_index = sorted_index(&info.methods);
            info.class_method_index = sorted_index(&info.class_methods);
        }
    }

    /// A flat lookup into the receiver class's own MATERIALIZED `methods`
    /// list -- no ancestor walk needed at call-resolution time at all,
    /// since `analyze::mro::materialize` already resolved every reachable
    /// name (own, inherited, or mixed-in) onto the class itself. Returns
    /// the ALREADY-RESOLVED `(class, scope)` pair; `super` resolution
    /// (`clif::call::lower_super`) is the one place that still
    /// needs to walk `ancestors` explicitly, since it must search PAST
    /// wherever the currently-executing method was actually defined, not
    /// just find the winner from scratch.
    pub fn method_in_chain(&self, class: ClassId, name: &str) -> Option<(ClassId, ScopeId)> {
        self.lookup_method(class, name)
            .map(|e| (e.defined_class(self), e.def))
    }

    /// The terminal source name of a BUILTIN alias visible on `class` --
    /// `Some("raise")` for `raise!` after `alias_method :raise!, :raise`,
    /// on the aliasing class and every subclass (MRO walk, closest wins).
    /// Entries are already terminal (see `ClassInfo::builtin_aliases`), so
    /// no chain-following happens here. `None` for every ordinary name.
    pub fn builtin_alias_target(&self, class: ClassId, name: &str) -> Option<&str> {
        self.class(class).ancestors.iter().find_map(|&anc| {
            self.class(anc)
                .builtin_aliases
                .iter()
                .find(|(new, _, _)| new == name)
                .map(|(_, old, _)| old.as_str())
        })
    }

    /// [`Compiler::builtin_alias_target`]'s singleton-side twin, over
    /// `ClassInfo::class_aliases` -- `Some("new")` for `[]` after
    /// `class << self; alias [] new`.
    pub fn class_alias_target(&self, class: ClassId, name: &str) -> Option<&str> {
        self.class(class).ancestors.iter().find_map(|&anc| {
            self.class(anc)
                .class_aliases
                .iter()
                .find(|(new, _)| new == name)
                .map(|(_, old)| old.as_str())
        })
    }

    /// Whether `private_class_method` marked class method `name` on `class` --
    /// on the resolved definition, or on the nearest ancestor that re-declared
    /// it without one (`private_class_method :new`, where no body defines
    /// `new` at all). The compile-time half of the runtime's own
    /// `class_method_is_private`.
    pub fn class_method_is_private(&self, class: ClassId, name: &str) -> bool {
        for &anc in std::iter::once(&class).chain(&self.classes[class.0 as usize].ancestors) {
            let info = &self.classes[anc.0 as usize];
            if let Some((_, vis)) = info
                .class_visibility_overrides
                .iter()
                .rev()
                .find(|(n, _)| n == name)
            {
                return *vis == crate::hir::Visibility::Private;
            }
            if let Some(&sid) = info
                .own_class_methods
                .iter()
                .find(|&&s| self.scopes[s.0 as usize].name == name)
            {
                return self.scopes[sid.0 as usize].visibility == crate::hir::Visibility::Private;
            }
        }
        false
    }

    /// Whether `module`'s OWN body supplies one of the three mix-in
    /// PRIMITIVES -- `append_features`/`prepend_features`/`extend_object`,
    /// which `include`/`prepend`/`extend` are defined in terms of and which a
    /// module overrides to police how it is mixed in.
    ///
    /// Asked during the analyze walk, before `mro::materialize` has flattened
    /// anything, so it reads `own_class_methods` rather than the inherited
    /// view: an override is written as a `def self.append_features` in the
    /// module's own body, and a module must be defined before it is mixed in.
    /// One inherited from an `extend`ed module is missed, and stays folded.
    pub fn overrides_mixin_primitive(&self, module: ClassId, primitive: &str) -> bool {
        self.classes[module.0 as usize]
            .own_class_methods
            .iter()
            .any(|&s| self.scopes[s.0 as usize].name == primitive)
    }

    /// Same idea as `method_in_chain`, over `class_methods` instead of
    /// `methods` -- used for `ClassName.foo(...)` call sites (see
    /// `HirNode::ClassRef`'s docs).
    pub fn class_method_in_chain(&self, class: ClassId, name: &str) -> Option<(ClassId, ScopeId)> {
        self.class_opt(class)?;
        self.lookup_class_method(class, name)
            .map(|e| (e.defined_class(self), e.def))
    }

    /// The `(class, module)` edges the statement at `stmt` installs -- an
    /// `extend M` written in a class body, and the `base.extend(ClassMethods)`
    /// an `include`'s `included` hook performs.
    ///
    /// Codegen emits one positional mixin per edge here: CRuby seats the
    /// module in the singleton chain WHERE THE STATEMENT STANDS
    /// (`rb_extend_object` is `rb_include_module(rb_singleton_class(obj), m)`),
    /// so the edge cannot be in place from program start. Every compile-time
    /// fact about the edge -- the materialized rows, the hook splices -- still
    /// reads `ClassInfo::extends`; only the timeline moves.
    pub fn extends_installed_at(&self, stmt: crate::hir::NodeId) -> Vec<(ClassId, ClassId)> {
        let mut edges: Vec<(ClassId, ClassId)> = self
            .extend_sites
            .iter()
            .filter(|&(_, at)| *at == stmt)
            .map(|(&pair, _)| pair)
            .collect();
        // The map is unordered and two modules extended by one statement must
        // seat in a stable order, or the winner for a shared name flips
        // between builds.
        edges.sort_by_key(|&(c, m)| (c.0, m.0));
        edges
    }

    /// The class methods `class` carries a MATERIALIZED copy of that only a
    /// positional `extend` seats -- the names whose winner comes from a module
    /// the class body extends.
    ///
    /// zeo flattens an extended module's instance methods onto the class's
    /// class-method table so the steady state costs no walk. CRuby has no such
    /// copy, so each of these is retired at boot and installed at the
    /// statement (`runtime_meta::defer_extended_class_method`).
    ///
    /// A name the class also defines itself is NOT here: its own `def self.x`
    /// won the materialization, and `class_method_install_node` reports that
    /// `def` rather than an `extend`.
    pub fn class_methods_deferred_by_extend(&self, class: ClassId) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for entry in &self.classes[class.0 as usize].class_methods {
            let name = self.scope(entry.def).name.clone();
            let installed_at = self.class_method_install_node(class, &name);
            if installed_at.is_some_and(|at| self.extend_sites.values().any(|&site| site == at)) {
                names.push(name);
            }
        }
        names.sort();
        names.dedup();
        names
    }

    /// WHERE class method `name` became available on `class` -- the node whose
    /// span says when the program installed it.
    ///
    /// Usually the body's own `def`, but `extend M` splits the two: `M` writes
    /// `def method_added` above the class, and the class gains it at the
    /// `extend`. Ruby announces from the `extend` onwards, so a definition
    /// written above it is not reported. `class_method_in_chain` answers WHICH
    /// body wins and cannot answer this, since the edge it resolves through
    /// carries no position of its own.
    ///
    /// `None` when neither end has a span (a native row, an `extend` this
    /// compile never saw as a statement), which leaves the caller's own
    /// span-less rule in charge.
    pub fn class_method_install_node(
        &self,
        class: ClassId,
        name: &str,
    ) -> Option<crate::hir::NodeId> {
        self.class_opt(class)?;
        let entry = self.lookup_class_method(class, name)?;
        let owner = entry.defined_class(self);
        // The edge can sit on the class itself or on any ancestor, and the
        // module it names can be the owner or something that module includes.
        for &c in std::iter::once(&class).chain(&self.classes[class.0 as usize].ancestors) {
            for &m in &self.classes[c.0 as usize].extends {
                if (m == owner || self.classes[m.0 as usize].ancestors.contains(&owner))
                    && let Some(&node) = self.extend_sites.get(&(c, m))
                {
                    return Some(node);
                }
            }
        }
        self.scope(entry.def).def_node
    }
}
