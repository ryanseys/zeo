//! `zeo`'s own analog of zeo's `Compiler`/`ClassInfo`/`Scope`
//! (`compiler.h`) -- pure compile-time bookkeeping. It never appears in the
//! generated program; codegen consults it and discards it. See the plan's
//! "Two `ClassId` types, on purpose": this `ClassId` is numerically mirrored
//! into `zeo_rt::ClassId` by codegen, but the two types are otherwise
//! unrelated -- `zeo` never links against `zeo-rt` at all.

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

use crate::hir::{Hir, NodeId, Params, Visibility};
use crate::types::TyKind;

/// The SHARED compiler/runtime class numbering: `ClassId` and
/// every reserved builtin id are re-exported from `zeo-abi`, the
/// zero-dependency leaf crate both `zeo` and `zeo-rt` consume -- the
/// class numbering this file and `zeo_rt::dispatch` share is literally one
/// definition. `Compiler::new` seeds the class arena from
/// `zeo_abi::BUILTINS` (id + Ruby-visible name + module-ness), which is
/// what makes `5.is_a?(Integer)`-style checks work uniformly through the
/// same `classes`/`ancestors` system as user classes.
pub use zeo_abi::{
    ARRAY_CLASS, BASIC_OBJECT_CLASS, CLASS_CLASS, COMPARABLE_CLASS, COMPLEX_CLASS, ClassId,
    DATA_CLASS, ENUMERABLE_CLASS, ENUMERATOR_CLASS, FALSE_CLASS, FFI_STRUCT_CLASS, FIBER_CLASS,
    FLOAT_CLASS, HASH_CLASS, INTEGER_CLASS, KERNEL_CLASS, MATCH_DATA_CLASS, MATH_CLASS,
    MODULE_CLASS, MUTEX_CLASS, NIL_CLASS, NUMERIC_CLASS, OBJECT_CLASS, PROC_CLASS, QUEUE_CLASS,
    RACTOR_CLASS, RANGE_CLASS, RATIONAL_CLASS, REGEXP_CLASS, STRING_CLASS, STRUCT_CLASS,
    SYMBOL_CLASS, THREAD_CLASS, TRUE_CLASS,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScopeId(pub u32);

/// The reserved name a `class << self` body's own class registers under.
///
/// It is the singleton class of the enclosing class, as a real compile-time
/// class: `zeo_rt::register_singleton_surrogate` makes `Foo.singleton_class`
/// answer this class at run time, and constants, `def`s and residual runtime
/// statements from the singleton body all belong to it.
///
/// No Ruby constant can collide with it -- a constant must start with an
/// uppercase letter -- which is what makes it safe to mint under a name the
/// program can also write.
pub const SINGLETON_SURROGATE: &str = "#<Class:self>";

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
    /// first case, while the second is a genuine conflict. MRI rejects both,
    /// but this arises in zeo from wholesale-inlined libraries where the
    /// bare opening and the real declaration are separated -- a deliberate,
    /// documented divergence (see `reopen_split_superclass_dispatch`).
    pub explicit_superclass: bool,
    /// `prepend`ed modules, in source order (see `analyze::mro`'s
    /// linearization -- expanded in REVERSE source order, so the most
    /// recently prepended module ends up closest).
    pub prepends: Vec<ClassId>,
    /// `include`d modules, in source order (same reverse-expansion rule as
    /// `prepends`).
    pub includes: Vec<ClassId>,
    /// The constants this class's body marked with `private_constant`, minus
    /// any a later `public_constant` restored. A qualified `M::A` naming one
    /// of these from OUTSIDE `M`'s lexical scope is a NameError, and
    /// `M.constants` omits it.
    pub private_constants: std::collections::BTreeSet<String>,
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
    pub pending_aliases: Vec<(String, String, bool, u32)>,
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
    pub redef_scopes: Vec<ScopeId>,
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
    pub builtin_aliases: Vec<(String, String)>,
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
    /// no generated Rust struct/`impl RubyObject`/`ClassRegistry` entry --
    /// see `codegen::mod::emit_class`'s docs), used only as a source for
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
    /// The full MRO-resolved set `codegen::mod::emit_class` actually emits
    /// one Rust method per entry for -- own ∪ every name reachable via
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
    method_index: Vec<(NameId, u32)>,
    class_method_index: Vec<(NameId, u32)>,
    /// `@@x` storage ownership, resolved once at analyze time (not per
    /// access, unlike zeo -- see `analyze::mro::resolve_cvars`'s docs):
    /// name -> the class/module that actually OWNS the runtime storage
    /// (nearest ancestor, including self, that ever claimed it first).
    pub cvar_owners: FMap<String, ClassId>,
    /// Bare-constant storage ownership, resolved once at analyze time --
    /// same scheme as `cvar_owners` (nearest ancestor, including self, that
    /// ever claimed the name first), used by `codegen::expr::const_owner_id_opt`.
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
    /// (`Integer`/`Array`/etc.) -- never gets a generated Rust
    /// struct/`impl RubyObject`/`ruby_class!` invocation at all (there's no
    /// concrete struct to generate: `RubyValue::Int`/`Array`/etc. ARE the
    /// runtime representation already -- see `codegen::mod`'s filters), only
    /// a `ClassRegistry` entry (`codegen::mod`'s `builtin_registrations`) so
    /// `is_a?`/`respond_to?` resolve correctly against it. `false` for
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

/// An interned method name. Method tables are the one place in the compiler
/// with a name per (class, name) pair rather than per definition, so this is
/// where string comparison and per-entry `String` allocation actually cost
/// something -- the `FSet<String>::insert` that was 47% of the Rails-scale
/// profile was exactly this.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct NameId(u32);

#[derive(Default)]
pub struct Names {
    ids: FMap<Box<str>, NameId>,
    list: Vec<Box<str>>,
}

impl Names {
    pub fn intern(&mut self, name: &str) -> NameId {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = NameId(self.list.len() as u32);
        let boxed: Box<str> = name.into();
        self.list.push(boxed.clone());
        self.ids.insert(boxed, id);
        id
    }

    /// The id `name` already has, or `None` -- a lookup, which must not mint a
    /// new id for a name nothing ever defined.
    pub fn get(&self, name: &str) -> Option<NameId> {
        self.ids.get(name).copied()
    }

    pub fn str(&self, id: NameId) -> &str {
        &self.list[id.0 as usize]
    }
}

pub struct Scope {
    pub name: String,
    pub class: Option<ClassId>,
    /// Which class/module's HIR body this Scope's `params`/`body` actually
    /// came from -- equal to `class` for an ordinary own-body method, but
    /// set to the true source ancestor for a materialized (inherited or
    /// mixed-in) method. `super` resolution (`codegen::call::emit_super`)
    /// searches `class`'s `ancestors` starting AFTER this position, not
    /// after `class` itself -- see `analyze::mro`'s docs for why these two
    /// need to be distinct once mixins/plain inheritance-without-override
    /// exist.
    pub defining_class: ClassId,
    /// The singleton-class SURROGATE this body was lexically written in, when
    /// the `def` sat in a constant-bearing `class << self` body
    /// (`NodeFlag::SINGLETON_BODY_DEF`). Codegen's method-emission `Ctx` uses it
    /// over `defining_class` for everything lexical -- bare-constant
    /// resolution, `Module.nesting` -- while `class`/`defining_class` keep
    /// owning dispatch, ivars, and `super`. `None` for every other method.
    pub lexical_home: Option<ClassId>,
    /// The `HirNode::DefMethod` this scope was registered from -- its span
    /// gives the `def` keyword's source line, which is what a backtrace
    /// frame shows until the first statement stamps a line (and what an
    /// arity error raised in the prologue reports, CRuby's attribution).
    /// `None` for synthesized scopes (aliases keep their source's node;
    /// the exception prelude has no spans at all).
    pub def_node: Option<NodeId>,
    /// The name this method was born under, when it reached `name` through an
    /// `alias`/`alias_method` -- what `Method#original_name` answers and what
    /// `#inspect` prints in parens. `None` for an ordinary `def`.
    pub alias_of: Option<String>,
    pub params: Params,
    pub body: Vec<NodeId>,
    /// Per-local static type, computed once by `analyze::locals::infer_locals`
    /// (a forward, single-pass walk -- not the deferred whole-program
    /// fixpoint). Lets codegen resolve `x + y` to native `Int` arithmetic
    /// when `x`/`y` are locals, not just literal-on-literal operands.
    pub local_types: FMap<String, TyKind>,
    /// Whether this method's own body (NOT a nested block's) uses a bare
    /// `yield`/`block_given?` -- computed once by `analyze::register_class`'s
    /// `scan_bare_block_use`. Together with `params.block.is_some()`, this
    /// decides whether the method gets an implicit trailing `__blk` Rust
    /// parameter (see `codegen::params`).
    pub uses_bare_block: bool,
    /// As of the `def`'s own position in its class body -- see
    /// `hir::Visibility`'s docs. Enforced at `codegen::call::dispatch`'s
    /// Path 1 site and `zeo_rt::send`'s Path 2 dispatch.
    pub visibility: Visibility,
    /// Whether this scope IS one of the pristine `BUILTIN_EXCEPTIONS_RB` method
    /// bodies (or a materialized copy of one). The native exception hierarchy is
    /// installed once by `zeo-rt`'s `register_exceptions`, so codegen must
    /// emit only the DELTA -- the methods a user reopen/subclass actually
    /// changed (`!native_default`) -- never re-emitting the pristine bodies
    /// `with_core()` already provides. Set true when the bootstrap classes are
    /// registered (`analyze`), propagated onto inherited copies by
    /// `mro::materialize_methods`. `false` for every ordinary user method.
    pub native_default: bool,
    /// This `def` sits in an `if`/`case` branch whose guard zeo cannot decide,
    /// so WHETHER IT RUNS is a runtime fact.
    ///
    /// It is registered all the same -- the name has to be visible to the
    /// compile-time machinery (`extend`, the `instance_methods` fold, the MRO)
    /// that would otherwise miss it entirely. What it must not do is claim a
    /// static method-table row, because that row would answer whether or not
    /// the branch ran. Codegen therefore emits no dispatch entry, no own row
    /// and no reflection row for it, and the name joins
    /// [`Compiler::runtime_patches`] so every call site asks at run time. The
    /// `define_method` codegen emits INSIDE the guard is what installs the
    /// body, and the runtime overlay -- which already outranks both a class's
    /// own table and an inherited one -- is what answers for it.
    ///
    /// See `analyze::register_conditional_defs`.
    pub runtime_conditional: bool,
    /// The `ruby2_keywords` directive marked this `def` -- see
    /// [`crate::hir::NodeFlag::RUBY2_KEYWORDS`].
    pub ruby2_keywords: bool,
    /// `Some` when the whole body is one ivar access and nothing else, so
    /// every caller can replace the call with the access -- see
    /// [`AccessorShape`]. Computed from the body's SHAPE, not from having
    /// been written by `attr_reader`, so a hand-written `def x; @x; end`
    /// (which is what `bm_rbtree` and `bm_inline` contain) devirtualizes
    /// too, and so the property survives `mro::materialize_methods` copying
    /// the body onto a descendant.
    pub accessor: Option<AccessorShape>,
}

/// A method whose entire body is one instance-variable access.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AccessorShape {
    /// WITHOUT its `@` -- exactly the `ClassInfo::ivars` spelling, which is
    /// what `codegen::ident::safe_ident` turns into the struct's field.
    pub ivar: String,
    pub kind: AccessorKind,
    /// Synthesized by `attr_reader`/`attr_writer`/`attr_accessor`/`attr`
    /// rather than written as a `def` (`NodeFlag::ATTR_GENERATED`). CRuby compiles
    /// those to iseq-less methods that fire no `:call`/`:return` `TracePoint`
    /// event, so devirtualizing one is unobservable even while tracing --
    /// where doing the same to a hand-written `def x; @x; end` would swallow
    /// two events CRuby really does produce.
    pub attr_generated: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessorKind {
    /// No parameters, body is exactly `@name`.
    Reader,
    /// One required parameter, body is exactly `@name = that_param`.
    Writer,
}

impl Scope {
    /// Whether this method needs the implicit `__blk: Option<RubyValue>`
    /// trailing parameter -- either it names its block (`&blk`) or uses
    /// bare `yield`/`block_given?`.
    pub fn needs_block_param(&self) -> bool {
        self.params.block.is_some() || self.uses_bare_block
    }
}

pub struct Compiler {
    pub hir: Hir,
    pub classes: Vec<ClassInfo>,
    pub scopes: Vec<Scope>,
    /// Interned method names -- see [`NameId`].
    pub names: Names,
    /// Set when a class/module definition was refused for a reason ruby has an
    /// EXCEPTION for rather than a limitation of zeo's -- `superclass mismatch
    /// for class A`, `C is not a module`. Carries the definition's node, the
    /// exception class, and ruby's own message.
    ///
    /// The refusal is still a compile error wherever nothing would catch it.
    /// But ruby raises these at the definition, so a `begin ... rescue
    /// TypeError` around one CATCHES it and the program continues -- and a
    /// program that does that has to compile. See
    /// `analyze::raise_instead_of_defining`.
    pub pending_ruby_raise: Option<(crate::hir::NodeId, &'static str, String)>,
    /// Each box's TOP-LEVEL SURROGATE: a module-shaped
    /// `ClassInfo` named `#<Ruby::Box:N>` that owns the box's top-level
    /// constants and doubles as the handle's runtime `RubyValue::Class`
    /// payload. Created by `analyze` (one per box id the loader
    /// allocated), looked up by codegen.
    pub box_surrogates: FMap<u32, ClassId>,
    /// One entry per `class`/`module` DEFINITION SITE (reopens included),
    /// in registration order: the site's `ClassDef` marker node (`None`
    /// for the synthetic/pinned registrations, which have no source
    /// position), the class it (re)opens, and ITS OWN body statements to
    /// execute at that position -- real Ruby runs a class body where it
    /// appears in the file, re-running each reopen. `ClassInfo`'s flat
    /// `class_body_stmts` keeps the union (the cvar/const collectors and
    /// `mro` read it); codegen executes per SITE: a marker reachable from
    /// `main_statements` emits inline in document order (`codegen::stmt`'s
    /// `ClassDef` arm), the rest (prelude, `None`) splice at `run_main`'s
    /// head exactly as before. A NESTED `ClassDef` appears as a marker in
    /// its parent's site list, so inner bodies run mid-parent-body.
    pub class_body_sites: Vec<ClassBodySite>,
    /// Definition-hook names the program defined on `Module`/`Class`/
    /// `BasicObject` themselves, which makes them answer for EVERY class.
    /// Codegen hands these to the runtime, whose own per-class owner scan
    /// cannot see one: such a reopen registers an ordinary instance method
    /// whose owner is the very class the no-op default lives on.
    pub global_def_hooks: FSet<String>,
    /// The source extent of every `BEGIN { ... }` block, recorded as the
    /// analyze walk hoists it. Ruby runs these before the main program, so
    /// two definitions written in one file do not run in written order when
    /// one of them sits in here -- which is what
    /// [`crate::analyze::def_hooks::fires`] has to know before comparing
    /// their spans.
    pub pre_exec_spans: Vec<crate::hir::Span>,
    /// Hands out [`SiteDef::seq`].
    pub def_seq: u32,
    /// The same record for TOP-LEVEL definitions, which have no site: a bare
    /// `def foo` is a private instance method of `Object`, and ruby announces
    /// it as `Object.method_added(:foo)`. `at` indexes `main_statements`.
    pub top_level_defs: Vec<SiteDef>,
    /// Whole-program map `(box_id, fully-qualified name) -> is_module`, built
    /// by `analyze` from a read-only scan of EVERY `class`/`module` definition
    /// (all `if` branches included -- it is only a lookup table). Lets
    /// `register_class` resolve a compact-path container that is defined LATER
    /// in the flattened statement list than the definition referencing it
    /// (`class Gem::Security::Policy` reopened by a deferred require before its
    /// `module Gem::Security` forward-declaration registers): the container's
    /// KIND is known, so a shell is created on demand. A container absent from
    /// this map is genuinely undefined and still errors.
    pub shell_kinds: FMap<(u32, String), bool>,
    /// Every constant LEAF name the program assigns anywhere (`NAME = ...`,
    /// `Foo::NAME = ...`), from a read-only scan of the whole node arena --
    /// dead branches and nested scopes included, since it is only ever used to
    /// answer "could this name exist at runtime?" and over-answering `true` is
    /// the safe direction.
    ///
    /// `analyze::resolve_module_target` consults it to tell two failures apart:
    /// a name NOTHING in the program defines (`extend FFI` with no FFI) defers
    /// to the runtime `NameError` Ruby raises there, while a name that IS
    /// assigned but isn't a compile-time module (`M = Module.new; include M`)
    /// stays a compile error -- deferring that one would silently DROP the
    /// mixin, since zeo's compiled classes dispatch off a static MRO.
    pub assigned_const_names: FSet<String>,
    /// `NAME = <value>` written at the TOP LEVEL (never inside a `class`/
    /// `module` body), leaf name -> the assigned value node. First write wins.
    ///
    /// `analyze::const_alias_target` needs the opposite of
    /// `assigned_const_names`' over-approximation: it decides class IDENTITY,
    /// so a write it consults has to be one a bare `class CONST` could really
    /// be reopening, and Ruby only lets a TOP-LEVEL definition reopen a
    /// top-level alias. A write's `scope` field cannot answer this -- it is
    /// `None` for `NAME = ...` at any depth, recording only the explicit
    /// `Foo::NAME = ...` prefix -- so the nesting has to come from the walk.
    pub top_level_const_aliases: FMap<String, crate::hir::NodeId>,
    /// Every `NAME = <value>` write in the program, keyed by `(box, fully
    /// qualified name)` -- the same question the table above answers, asked
    /// from ANY scope rather than only the top level. It exists because a
    /// constant that names a module is how ruby spells a module alias
    /// (oauth2's `OAuth2::FilteredAttributes = OAuth2::AUTH_SANITIZER::
    /// FilteredAttributes`), and an `include` of that name has to reach the
    /// real module. Read through `analyze`'s `resolve_const_alias`, which
    /// searches innermost scope outward.
    pub const_aliases: FMap<(u32, String), crate::hir::NodeId>,
    /// `NAME = <value>` written as a DIRECT statement of a `Program`, mapped to
    /// its value node -- and only when the program writes that name exactly
    /// once. Two writes (log4r's `HAVE_REXML = true` / `= false`, one per branch
    /// of a rescue) have no single initializer to read, so the name is absent.
    ///
    /// Deliberately NOT `top_level_const_aliases`, which answers a similar
    /// question with two differences that matter here: it descends through the
    /// statement wrappers a top-level write can hide behind (`if`, `begin`, a
    /// box scope), and it takes the first write rather than requiring a unique
    /// one. Folding a constant assigned inside a top-level `if` would read an
    /// initializer that may never run.
    pub unique_top_const_inits: FMap<String, crate::hir::NodeId>,
    /// Every `ConstWrite` in the arena, keyed by its LEAF name -- scope
    /// ignored, so both `NAME = v` and `Foo::NAME = v` file under `NAME`.
    ///
    /// This and the two tables below serve the guard predicates
    /// (`analyze::const_defined_outside` and its siblings), which ask whether
    /// a write of some name lies OUTSIDE the branches a guard decides. Each
    /// re-read every node in the program per undecided guard, and then walked
    /// the guard's whole subtree to place the hits. Indexed, a guard tests a
    /// handful of candidate nodes -- and skips the subtree walk entirely when
    /// there are none, which is the common case.
    pub const_write_sites: FMap<String, Vec<crate::hir::NodeId>>,
    /// Every `GlobalWrite`/`AliasGlobal` in the arena, keyed by global name.
    /// Globals have one flat namespace, so the name is the whole question.
    pub global_write_sites: FMap<String, Vec<crate::hir::NodeId>>,
    /// Every `const_set` call in the arena, unkeyed: the name such a call sets
    /// is often computed, so deciding whether one COULD write a given constant
    /// takes the call node itself.
    pub const_set_sites: Vec<crate::hir::NodeId>,
    /// Every method name a RUNTIME site could change out from under a folded
    /// call. Two kinds, because both land in the overlay that only DYNAMIC
    /// dispatch consults:
    ///
    /// - the BODY changes -- a `define_method`/`define_singleton_method`/
    ///   `alias_method` that survived lowering as a real call, or a `def` in
    ///   block position (`C.class_eval { def m; end }`);
    /// - the VISIBILITY changes -- `private`/`protected`/`public` and their
    ///   class-method twins sent as a message. Visibility is a runtime property
    ///   in ruby: `private :m` re-marks a method that already exists, and a
    ///   direct call decided its visibility when it was emitted.
    ///
    /// Collected by a flat arena scan, dead branches included: this only ever
    /// answers "could this name change under us?", and over-answering `true`
    /// costs speed, not correctness.
    pub runtime_patches: FSet<String>,
    /// Method names a runtime-deferred mixin's bodies reach through `super`.
    /// The splice happens when the guard/send runs, so WHICH class's chain
    /// the walk resumes on is a runtime fact -- codegen widens these into
    /// `super_global`, emitting a receiver-generic bridge on every class
    /// that owns the name. Filled by `analyze::defer_mixin_to_runtime`.
    pub runtime_mixin_super_names: FSet<String>,
    /// Whether the analyze walk is currently inside a FEATURE UNIT's body
    /// (`analyze`'s unit loop). A `def` registered under it is registered
    /// but not PROMISED -- see `register_method`'s `runtime_conditional`
    /// marking, which reads this.
    pub unit_walk: bool,
    /// Scopes registered DURING the unit walk -- consulted by
    /// `analyze::add_own_method_at`'s last-def-wins replacement: a unit's
    /// `def` never displaces an EAGER def of the same name. The unit walk
    /// runs after the whole eager stream, but the unit's body EXECUTES at
    /// its (runtime) require -- before any eager statement written below
    /// that require -- so walk order inverts execution order there. The
    /// corpus case: a spec file's `define_singleton_method` stub over a
    /// lazily-required rspec class lost to the gem's own later-walked def.
    pub unit_scopes: FSet<ScopeId>,
    /// Boot-time overlay installs for observable redefinition timelines:
    /// `(class, name, first_scope)`. The static tables carry the FINAL body
    /// (last-`def`-wins, so every compile-time fact -- super inlining,
    /// materialization -- is untouched); the FIRST body is installed into
    /// the runtime overlay before the first statement runs, and each later
    /// redefinition re-installs at its own document position
    /// (`HirNode::MethodRedefine`). Filled by `analyze::redefs`.
    pub positional_redefs: Vec<(ClassId, String, ScopeId)>,
    /// One of those sites names its method with something other than a literal
    /// (`Node.send(:define_method, computed)`, a bare `private`), so NO name is
    /// safe to fold. Kept apart from the set above because it is the expensive
    /// answer: it de-optimizes every direct call in the program.
    pub runtime_patches_any_name: bool,
    /// Whether the program calls `freeze` anywhere. A REOPEN of a frozen class
    /// is a `FrozenError` and its body never runs, so the definitions the
    /// compile-time tables carry for it have to be retractable -- which costs
    /// the name its static call sites. A program that never freezes anything
    /// cannot reach that shape, so it pays neither the check nor the
    /// de-optimization. See `codegen::emit_frozen_reopen_guard`.
    pub program_freezes: bool,
    /// `class_in_scope`'s lazily-drained (box, lexical_parent) -> name -> id
    /// index, replacing its linear whole-`classes` scan (the profiled
    /// hot spot at gem scale: every bare-constant classification paid
    /// O(#classes) string compares). Drained forward from `indexed_upto` on
    /// each query, which is sound because every `add_class` site settles a
    /// class's identity fields (name/box/lexical_parent) before any lookup
    /// can run; `ZEO_VERIFY_CLASS_INDEX=1` shadow-compares every answer
    /// against the original scan.
    class_index: std::cell::RefCell<ClassNameIndex>,
    indexed_upto: std::cell::Cell<usize>,
    /// `cref_of`/`fq_name` answers for every class, precomputed once by
    /// [`Compiler::freeze_identity_caches`] at `mro::materialize`'s head --
    /// the fields both walks read (`name`/`lexical_parent`/`qualified_def`)
    /// are settled by then, and both are otherwise recomputed with a fresh
    /// allocation at every codegen/const-resolution site. `None` during
    /// registration, where the walks still compute live.
    frozen_crefs: Option<Vec<Vec<ClassId>>>,
    frozen_fq_names: Option<Vec<String>>,
    /// Per-class set of statement-level bare `NAME = ...` names, frozen with
    /// the identity caches above so `mro::directly_defines_const` -- probed
    /// once per (lexical scope, name) during constant-ownership resolution
    /// and per ancestor by `codegen::constfold` -- is a set lookup instead of
    /// a rescan of the class body.
    pub(crate) direct_const_defs: Option<Vec<FSet<String>>>,
    /// Where each node sits in the program's EXECUTION order, for the nodes
    /// whose position is a static fact -- a top-level statement, a class-body
    /// statement, and anything nested in a container inside one. A `def`'s
    /// body is deliberately absent: it runs whenever the method is called,
    /// which is not a position. See [`Compiler::doc_position`].
    pub(crate) doc_order: FMap<crate::hir::NodeId, u32>,
    /// The earliest document position at which `(owner, name)` becomes a
    /// defined constant -- a `class`/`module` marker, or a statement-level
    /// `NAME = ...`. Read with [`Compiler::const_defined_before`].
    pub(crate) const_def_order: FMap<(ClassId, String), u32>,
    /// Every `refine Target do ... end` the program wrote, in registration
    /// order. See [`Refinement`].
    pub(crate) refinements: Vec<Refinement>,
    /// Operator names a user reopen redefines on `Integer`'s (fast-path) MRO
    /// (`class Integer; def +` -- or on `Numeric`/`Object`/... above it).
    /// Codegen's `Int` operator fast paths consult this and stand down so
    /// the redefinition is honored at every call site; see
    /// `analyze::register_body_def_method`.
    pub(crate) redefined_int_ops: FSet<String>,
    /// The `Float` lane of [`Compiler::redefined_int_ops`].
    pub(crate) redefined_float_ops: FSet<String>,
    /// Every `using M`, as the LEXICAL byte range it covers. See
    /// [`Activation`] and [`Compiler::refinements_active_at`].
    pub(crate) activations: Vec<Activation>,
    /// Block call sites whose receiver's STATIC type admits a native inline
    /// loop (`analyze::mark_inline_iter_sites`), keyed by the BLOCK node.
    /// Soundness lives in the emitted match GUARD (a mistyped receiver takes
    /// the dynamic-fallback arm), so the map is purely an optimization hint;
    /// the escaping-block scans deliberately ignore it -- a marked site keeps
    /// escaping-style cell captures, correct in both arms.
    pub inline_iter_sites: FMap<crate::hir::NodeId, InlineIterKind>,
    /// A compile-time reopen of `Integer#times` / `Range#each` anywhere in
    /// Integer's/Range's ancestry (`analyze::mark_inline_iter_sites` computes
    /// both): the LITERAL fast paths (`3.times`, `(1..9).each`) must then
    /// dispatch dynamically -- unlike the typed sites they carry no runtime
    /// guard to fall back through.
    pub times_literal_suppressed: bool,
    pub range_each_literal_suppressed: bool,
    /// Memo for [`Hir::uses_call_tracing`] -- a whole-arena scan every Path-1
    /// call site would otherwise repeat. Per-`Compiler`, so it cannot go stale
    /// across the many programs one test process compiles.
    traces_calls: std::cell::OnceCell<bool>,
    /// Memo for [`Compiler::defines_bang`] -- a whole-arena scan every `!x`
    /// fold would otherwise repeat, and `!` is everywhere.
    defines_bang: std::cell::OnceCell<bool>,
    /// Memo for [`Hir::uses_ractor`] -- consulted by every collection
    /// fast-path and inlined-accessor emission site.
    uses_ractor: std::cell::OnceCell<bool>,
}

/// See [`Compiler::inline_iter_sites`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum InlineIterKind {
    /// `n.times { |i| }`, `n` statically `Int`.
    TimesInt,
    /// `n.upto(m) { |i| }`, `n` statically `Int` (the guard also proves `m`).
    UptoInt,
    /// `n.downto(m) { |i| }`, `n` statically `Int`.
    DowntoInt,
    /// `n.step(limit[, by]) { |i| }`, `n` statically `Int`; positional args
    /// only (a keyword form never nominates), all proven `Int` by the guard.
    StepInt,
    /// `r.each { |i| }`, `r` statically `Range`; the guard proves both bounds
    /// are present and `Int` (a String/endless/beginless range falls back).
    RangeEachInt,
    /// `arr.each { |e| }`, `arr` statically `Array`.
    ArrayEach,
    /// `arr.each_with_index { |e, i| }`, `arr` statically `Array`.
    ArrayEachWithIndex,
    /// `arr.map { |e| }` / `collect`, `arr` statically `Array` -- the first
    /// of the VALUE-consuming kinds, where each iteration's block value is
    /// collected rather than discarded (see `Ctx::next_yields_value`).
    ArrayMap,
    /// `arr.select { |e| }` / `filter` / `find_all`, `arr` statically `Array`.
    ArraySelect,
    /// `arr.reject { |e| }`, `arr` statically `Array`.
    ArrayReject,
    /// `arr.sum { |e| }` (block form, no init argument), `arr` statically
    /// `Array`; accumulates through `zeo_rt::SumAcc`, the runtime `sum`'s
    /// own ladder.
    ArraySum,
    /// `arr.find { |e| }` / `detect` (no `ifnone` argument), `arr` statically
    /// `Array`; answers the first element whose block value is truthy, `nil`
    /// if none is. The ORIGINAL element surfaces even if the body reassigns
    /// its param -- the same CRuby rule `select`/`reject` obey.
    ArrayFind,
    /// `arr.all? { |e| }` (no pattern argument), `arr` statically `Array`;
    /// stops at the first falsy block value.
    ArrayAll,
    /// `arr.any? { |e| }`, `arr` statically `Array`; stops at the first
    /// truthy block value.
    ArrayAny,
    /// `arr.none? { |e| }`, `arr` statically `Array`; stops at the first
    /// truthy block value, like `any?`, but answers the other way round.
    ArrayNone,
    /// `arr.count { |e| }` (block form, no argument), `arr` statically
    /// `Array`; counts the truthy block values.
    ArrayCount,
    /// `arr.inject(init) { |acc, e| }` / `reduce`, `arr` statically `Array`.
    /// The initial value must be EXPLICIT: the no-argument form seeds the
    /// accumulator with the first element and does not call the block for it,
    /// and the splice skeleton has no way to skip a body.
    ArrayInject,
    /// `h.each { |k, v| }` / `each_pair`, `h` statically `Hash`; walks the
    /// same pairs snapshot the runtime `Hash#each` takes.
    HashEach,
}

/// The compiler-internal hash policy: fast, not DoS-resistant -- these sets
/// only ever hold program identifiers.
pub(crate) type FSet<T> = std::collections::HashSet<T, foldhash::fast::RandomState>;

/// [`FSet`]'s map twin -- every compiler-internal `HashMap` keyed by program
/// identifiers uses this. Ruby-VISIBLE hashing (`Object#hash`) is a runtime
/// concern and never touches these.
pub(crate) type FMap<K, V> = std::collections::HashMap<K, V, foldhash::fast::RandomState>;
/// `class_index`'s shape: `(box, lexical_parent) -> name -> id`.
type ClassNameIndex = FMap<(u32, Option<ClassId>), FMap<String, ClassId>>;

/// Whether `ZEO_DEBUG=verify-class-index` is set: every `class_in_scope`
/// answer is then shadow-compared against the original linear scan -- the
/// drift detector for the index's settle-before-lookup registration
/// invariant.
fn verify_class_index() -> bool {
    crate::debug_flags::debug(crate::debug_flags::DebugFlag::VerifyClassIndex)
}

/// One `refine Target do ... end`. The refined methods are ordinary
/// instance methods of `holder`, a module the source cannot name; nothing
/// is ever registered ON `target`, which is what keeps the refinement out
/// of `Target.instance_methods` and out of an unrefined call's answer.
pub(crate) struct Refinement {
    /// The module whose body wrote the `refine` -- what a `using` names.
    pub module: ClassId,
    /// The class being refined.
    pub target: ClassId,
    /// The hidden module holding the refined methods.
    pub holder: ClassId,
    /// `refine Target.singleton_class` -- the holder refines Target's CLASS
    /// methods, so a covered site matches a CLASS receiver descending from
    /// `target` where the plain form matches an instance of it.
    pub singleton: bool,
    /// The [`crate::hir::HirNode::Refine`] marker this came from. A
    /// refinement runs nothing where it was written, so codegen drops the
    /// marker -- but only one it can prove reached registration here.
    pub marker: crate::hir::NodeId,
}

/// One `using M`, as the byte range of source it covers: from the `using`
/// itself to the end of the enclosing body (the end of the file at the top
/// level). Real Ruby scopes a refinement lexically, so a byte range IS the
/// rule -- a `def` written after the `using` is covered because its body
/// sits inside the range, and one written above it is not.
pub(crate) struct Activation {
    pub module: ClassId,
    pub file: crate::hir::FileId,
    pub start: u32,
    pub end: u32,
}

/// See [`Compiler::class_body_sites`].
pub struct ClassBodySite {
    pub def_node: Option<crate::hir::NodeId>,
    pub class: ClassId,
    pub stmts: Vec<crate::hir::NodeId>,
    /// Every definition this site's walk CONSUMED -- a `def`, one name of an
    /// `attr_*` expansion, an alias, an `undef` -- in source order. None of
    /// them reaches `stmts`, because zeo compiles a definition into a method
    /// table rather than running it. Ruby still announces each one at its
    /// position, so [`crate::analyze::def_hooks`] keeps them here until it
    /// knows whether any hook body will answer, and splices a
    /// [`crate::hir::HirNode::DefHook`] into `stmts` for the ones that will.
    pub defs: Vec<SiteDef>,
    /// The method names this site's body INSTALLS, kept apart from `defs`
    /// because `def_hooks` takes that list away once it has decided which
    /// definitions get a hook report. Read by codegen's frozen-reopen guard,
    /// which needs to know what a body that never runs would have defined.
    pub installs: Vec<String>,
}

/// One consumed definition, and where its report would go.
pub struct SiteDef {
    /// Where this definition sits in the program's EXECUTION order, counted
    /// across every site and the top level. The analyze walk visits bodies in
    /// the order they run, so a plain counter is exact -- which raw spans are
    /// not, since a spliced `require` puts another file's statements in the
    /// middle of this one. [`crate::analyze::def_hooks`] orders a class's own
    /// definitions by this to work out which are still in the future.
    pub seq: u32,
    /// The index in the site's `stmts` the report belongs BEFORE.
    pub at: usize,
    /// Which STATEMENT STREAM `at` indexes, for a top-level def: `None` is
    /// the main list, `Some(k)` is `feature_units[k]`'s body. The walk runs
    /// the main stream and then every unit through the same
    /// `process_top_stmt`, all pushing into the one `top_level_defs` -- and
    /// splicing a unit's index into the main list panicked the compiler
    /// (autosub: "insertion index (is 58) should be <= len (is 18)").
    /// Class-body defs live in their site's own `defs` list and leave this
    /// `None`.
    pub unit: Option<u32>,
    /// The definition's own node, whose span decides whether a hook installed
    /// later in the same file ever saw it.
    pub node: crate::hir::NodeId,
    pub name: String,
    pub event: DefEvent,
    /// A `def self.x` / `class << self` definition, which reports through
    /// `singleton_method_added` on the class object rather than `method_added`.
    pub singleton: bool,
}

/// Which of Ruby's three definition events a [`SiteDef`] is.
#[derive(Clone, Copy, PartialEq)]
pub enum DefEvent {
    Added,
    Removed,
    Undefined,
}

impl DefEvent {
    /// The hook this event fires, for a definition on a class (`singleton`
    /// false) or on its singleton (`singleton` true).
    pub fn hook(self, singleton: bool) -> &'static str {
        match (self, singleton) {
            (DefEvent::Added, false) => "method_added",
            (DefEvent::Removed, false) => "method_removed",
            (DefEvent::Undefined, false) => "method_undefined",
            (DefEvent::Added, true) => "singleton_method_added",
            (DefEvent::Removed, true) => "singleton_method_removed",
            (DefEvent::Undefined, true) => "singleton_method_undefined",
        }
    }
}

impl Compiler {
    pub fn new(hir: Hir) -> Compiler {
        let mut compiler = Compiler {
            hir,
            classes: vec![ClassInfo {
                name: "Object".to_string(),
                box_id: 0,
                lexical_parent: None,
                cref_parent: None,
                qualified_def: false,
                is_bootstrap: false,
                parent: None,
                prepends: Vec::new(),
                includes: Vec::new(),
                private_constants: Default::default(),
                extends: Vec::new(),
                class_method_prepends: Vec::new(),
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
                is_module: false,
                ivars: Vec::new(),
                hidden_ivars: Vec::new(),
                own_methods: Vec::new(),
                methods: Vec::new(),
                own_method_at: FMap::default(),
                own_class_method_at: FMap::default(),
                explicit_superclass: false,
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
            }],
            scopes: Vec::new(),
            names: Names::default(),
            pending_ruby_raise: None,
            box_surrogates: FMap::default(),
            class_body_sites: Vec::new(),
            global_def_hooks: Default::default(),
            pre_exec_spans: Vec::new(),
            def_seq: 0,
            top_level_defs: Vec::new(),
            shell_kinds: FMap::default(),
            assigned_const_names: FSet::default(),
            top_level_const_aliases: FMap::default(),
            const_aliases: FMap::default(),
            unique_top_const_inits: FMap::default(),
            const_write_sites: FMap::default(),
            global_write_sites: FMap::default(),
            const_set_sites: Vec::new(),
            runtime_patches: FSet::default(),
            runtime_mixin_super_names: FSet::default(),
            unit_walk: false,
            unit_scopes: FSet::default(),
            positional_redefs: Vec::new(),
            runtime_patches_any_name: false,
            program_freezes: false,
            class_index: std::cell::RefCell::new(FMap::default()),
            indexed_upto: std::cell::Cell::new(0),
            frozen_crefs: None,
            frozen_fq_names: None,
            direct_const_defs: None,
            doc_order: FMap::default(),
            const_def_order: FMap::default(),
            refinements: Vec::new(),
            redefined_int_ops: FSet::default(),
            redefined_float_ops: FSet::default(),
            activations: Vec::new(),
            inline_iter_sites: FMap::default(),
            times_literal_suppressed: false,
            range_each_literal_suppressed: false,
            traces_calls: std::cell::OnceCell::new(),
            defines_bang: std::cell::OnceCell::new(),
            uses_ractor: std::cell::OnceCell::new(),
        };
        // The CRuby-exact hierarchy is DECLARED in the ABI table:
        // superclass edges (`Integer < Numeric`, `Class < Module`,
        // `BasicObject` as the parentless root) and real mixins (`Numeric`/
        // `String`/`Symbol` include `Comparable`; `Array`/`Hash`/`Range`/
        // `Struct`/`Enumerator` include `Enumerable`). Forward id refs are
        // fine: `parent`/`includes` are only linearized by
        // `mro::materialize` after every class exists.
        for b in zeo_abi::BUILTINS {
            let id = compiler.add_class(b.name.to_string(), b.superclass, b.is_module);
            debug_assert_eq!(
                id, b.id,
                "zeo_abi::BUILTINS must stay contiguous from ClassId(1)"
            );
            let ci = &mut compiler.classes[id.0 as usize];
            ci.is_builtin = true;
            ci.includes = b.includes.to_vec();
            ci.feature_gate = b.feature;
        }
        // The two edge kinds no `BuiltinClass` field carries, and both are
        // independent of `includes`: `CGI` extends the same module it
        // includes, and `CGI::Escape` prepends one. See their ABI tables.
        for &(id, modules) in zeo_abi::BUILTIN_EXTENDS {
            compiler.classes[id.0 as usize].extends = modules.to_vec();
        }
        for &(id, modules) in zeo_abi::BUILTIN_PREPENDS {
            compiler.classes[id.0 as usize].prepends = modules.to_vec();
        }
        // A nested builtin name (`"Digest::SHA256"`, `"Enumerator::Lazy"`) is
        // stored as its LEAF under a lexical parent, so a constant path
        // (`Digest::SHA256`) descends into it like any user-nested class.
        // Resolved from the ABI names directly, in a second pass, so a parent
        // declared at a LATER id still binds -- `Thread::Backtrace` is exactly
        // that against `Thread::Backtrace::Location`, whose id predates it.
        let by_abi_name: FMap<&str, ClassId> =
            zeo_abi::BUILTINS.iter().map(|b| (b.name, b.id)).collect();
        for b in zeo_abi::BUILTINS {
            let path = crate::constpath::ConstPath::parse(b.name);
            let Some(parent) = path.scope() else { continue };
            let ci = &mut compiler.classes[b.id.0 as usize];
            ci.name = path.base().to_string();
            ci.lexical_parent = by_abi_name.get(parent).copied();
            ci.cref_parent = ci.lexical_parent;
        }
        // Object's own slot in the chain (it isn't a BUILTINS row):
        // `Object < BasicObject`, `include Kernel` -- so EVERY chain ends
        // `..., Object, Kernel, BasicObject`, the real Ruby tail.
        compiler.classes[0].parent = Some(zeo_abi::OBJECT_SUPERCLASS);
        compiler.classes[0].includes = zeo_abi::OBJECT_INCLUDES.to_vec();
        compiler
    }

    /// THE name-resolution primitive -- every "which class does
    /// this name/path mean HERE" question goes through this one function,
    /// keyed by the full resolution context real Ruby uses: the lexical
    /// cref chain (innermost scope LAST -- `cref_of`'s order), and the box
    /// the referencing code is defined in (`0` at the top level).
    ///
    /// `path` may be a multi-segment constant path:
    /// `"Store::Errors::NotFound"` resolves its FIRST segment through the
    /// full unqualified rule below, then descends the remaining segments as
    /// direct namespace children only (no lexical/bootstrap fallback past
    /// the first segment -- real Ruby's own `::` rule). A leading `::`
    /// (`"::Foo"`) anchors the first segment at the top level, skipping the
    /// cref chain.
    ///
    /// Unqualified rule, mirroring CRuby: the lexical chain
    /// innermost-outward, then the box's own top level, then the BOOTSTRAP
    /// set (builtins + the built-in exceptions -- the "defined before any
    /// user program runs" classes every box sees; a box's own definition of
    /// the same name shadows it, exactly like CRuby's per-box constant
    /// overlay). First-registered wins within one scope, same as the old
    /// flat `class_by_name` (reopening semantics attach to that first
    /// registration rather than adding duplicates). One
    /// documented approximation: the cref head's ANCESTORS are not searched
    /// (real Ruby checks them between the lexical chain and the top level
    /// -- a class nested inside a SUPERCLASS referenced by bare name from a
    /// subclass misses here, loudly, rather than resolving wrong).
    pub fn resolve_class(&self, path: &str, cref: &[ClassId], box_id: u32) -> Option<ClassId> {
        let parsed = crate::constpath::ConstPath::parse(path);
        let anchored = parsed.is_top_anchored();
        let mut segments = parsed.segments();
        let first = segments.next()?;
        let mut cur = self.resolve_unqualified(first, if anchored { &[] } else { cref }, box_id)?;
        for seg in segments {
            // Descend within the resolved parent's OWN box (the parent may
            // itself have resolved through the bootstrap fallback into box
            // 0 even when `box_id` differs).
            cur = self.class_in_scope(Some(cur), seg, self.class(cur).box_id)?;
        }
        // A per-box builtin-reopen OVERLAY is a patch container,
        // never a distinct class: as a resolved NAME it collapses to the
        // root builtin (`box::String == String` stays true; instances keep
        // the root's identity). Reopen-merge detection deliberately uses
        // the raw `class_in_scope` instead.
        Some(self.class(cur).builtin_overlay.unwrap_or(cur))
    }

    /// Whether `cid`'s constant is currently VISIBLE to name resolution: a
    /// require-gated builtin (`feature_gate: Some(_)`) is invisible until its
    /// feature has been activated by a `require` (see `ClassInfo::feature_gate`
    /// and `Hir::activated_features`); every ungated class is always visible.
    /// The gate is applied only on the BOOTSTRAP-fallback resolution paths
    /// (`resolve_unqualified`), not on `class_in_scope`'s raw name lookup --
    /// reopen detection must still SEE the gated slot to attach to it.
    /// `pub(crate)` so codegen skips a gated-inactive builtin's runtime
    /// `ClassRegistry` registration (nothing can reference it, so it's dead).
    pub(crate) fn feature_active(&self, cid: ClassId) -> bool {
        match self.class(cid).feature_gate {
            None => true,
            Some(feature) => self.hir.activated_features.contains(feature),
        }
    }

    /// `resolve_class`'s single-segment core -- see its docs for the rule.
    fn resolve_unqualified(&self, name: &str, cref: &[ClassId], box_id: u32) -> Option<ClassId> {
        for &scope in cref.iter().rev() {
            if let Some(cid) = self.class_in_scope(Some(scope), name, box_id) {
                return Some(cid);
            }
        }
        // After the lexical nesting, Ruby consults the ANCESTRY of the
        // innermost lexical module -- a constant nested in an INCLUDED module
        // resolves unqualified. `ancestors` is linearized by mro before
        // codegen (empty during analyze, so this is a no-op there).
        if let Some(&innermost) = cref.last() {
            for &anc in &self.class(innermost).ancestors {
                if anc != innermost
                    && let Some(cid) = self.class_in_scope(Some(anc), name, box_id)
                {
                    return Some(cid);
                }
            }
        }
        if let Some(cid) = self
            .class_in_scope(None, name, box_id)
            .filter(|&c| self.feature_active(c))
        {
            return Some(cid);
        }
        if box_id != 0 {
            // The index's (0, None) row IS the first-registered top-level
            // name; builtins and bootstrap classes register before any user
            // class, so when a builtin/bootstrap of this name exists it is
            // that first entry -- and when the entry is a user class, no
            // builtin of the name exists and the old scan also missed.
            if let Some(cid) = self
                .class_in_scope(None, name, 0)
                .filter(|&c| {
                    let ci = self.class(c);
                    ci.is_builtin || ci.is_bootstrap
                })
                .filter(|&c| self.feature_active(c))
            {
                return Some(cid);
            }
        }
        // A top-level constant alias for a `Thread::`-nested builtin
        // (`::Queue = Thread::Queue`, ...) -- consulted LAST so a user's own
        // top-level `Queue`/`Mutex`/etc. shadows it, as in real Ruby.
        zeo_abi::TOP_LEVEL_ALIASES
            .iter()
            .find(|(alias, _)| *alias == name)
            .map(|&(_, cid)| cid)
            .filter(|&c| self.feature_active(c))
    }

    /// First class/module named `name` defined directly inside
    /// `lexical_parent` (or at the top level for `None`) in `box_id`.
    /// `pub(crate)` because `analyze::register_class`'s
    /// reopening-detection must be SCOPE-EXACT (a nested `Store::Item` must
    /// never be mistaken for a top-level `Item`, or vice versa), which the
    /// lexical-fallback walk `resolve_class` does would get wrong.
    pub(crate) fn class_in_scope(
        &self,
        lexical_parent: Option<ClassId>,
        name: &str,
        box_id: u32,
    ) -> Option<ClassId> {
        let mut index = self.class_index.borrow_mut();
        let upto = self.indexed_upto.get();
        if upto < self.classes.len() {
            // First-registered wins within one scope (`or_insert`), exactly
            // the old scan's `position` semantics.
            for (i, c) in self.classes.iter().enumerate().skip(upto) {
                index
                    .entry((c.box_id, c.lexical_parent))
                    .or_default()
                    .entry(c.name.clone())
                    .or_insert(ClassId(i as u32));
            }
            self.indexed_upto.set(self.classes.len());
        }
        let hit = index
            .get(&(box_id, lexical_parent))
            .and_then(|m| m.get(name))
            .copied();
        if verify_class_index() {
            let scan = self
                .classes
                .iter()
                .position(|c| {
                    c.name == name && c.box_id == box_id && c.lexical_parent == lexical_parent
                })
                .map(|i| ClassId(i as u32));
            assert_eq!(
                hit, scan,
                "class index diverged for {name:?} (box {box_id}, parent {lexical_parent:?})"
            );
        }
        hit
    }

    /// Whether ANY `class`/`module` in `box_id` is spelled `name` or ends in
    /// `::name` -- i.e. whether the name is class-shaped SOMEWHERE, whatever
    /// scope it was written under.
    ///
    /// Both callers ask this to stay honest about a definition they cannot see
    /// yet: `analyze`'s class-body splice folds before the body's own
    /// statements register, and `guard_fold` may be looking at a name written
    /// under a scope the reference spells differently (`Psych::Visitors` from
    /// inside `module Psych`). Neither may answer "not defined" then.
    ///
    /// One implementation because it was two, keyed and suffixed identically,
    /// in files that do not otherwise share code.
    pub fn class_shaped_anywhere(&self, box_id: u32, name: &str) -> bool {
        let suffix = format!("::{name}");
        self.shell_kinds
            .keys()
            .any(|(bx, k)| *bx == box_id && (k == name || k.ends_with(&suffix)))
    }

    /// The `#<Class:self>` surrogate standing in for `owner`'s `class << self`
    /// body, if it has one.
    ///
    /// Both callers used to scan every registered class for it, from inside
    /// per-class loops -- the same scan written twice, quadratic in the class
    /// count. `class_in_scope` answers it with the same first-registered-wins
    /// rule, and the surrogate shares its owner's box because a `class <<
    /// self` body is lexically inside the owner.
    pub(crate) fn singleton_surrogate_of(&self, owner: ClassId) -> Option<ClassId> {
        self.class_in_scope(Some(owner), SINGLETON_SURROGATE, self.class(owner).box_id)
    }

    /// Creates (idempotently) box `box_id`'s top-level surrogate -- see
    /// `box_surrogates`' docs. Registered like any module, so `p box`
    /// prints the surrogate's name through the ordinary Class-value path.
    pub fn ensure_box_surrogate(&mut self, box_id: u32) -> ClassId {
        if let Some(&cid) = self.box_surrogates.get(&box_id) {
            return cid;
        }
        let cid = self.add_class(format!("#<Ruby::Box:{box_id}>"), None, true);
        self.classes[cid.0 as usize].box_id = box_id;
        self.box_surrogates.insert(box_id, cid);
        cid
    }

    pub fn box_surrogate(&self, box_id: u32) -> Option<ClassId> {
        self.box_surrogates.get(&box_id).copied()
    }

    /// Whether `cid` is a singleton-class SURROGATE -- the module a
    /// constant-bearing `class << self` body registers under the reserved
    /// name (see `lower::defs`). Display name and reflection identity come
    /// from `fq_name`'s `#<Class:M>` special case.
    pub fn is_singleton_surrogate(&self, cid: ClassId) -> bool {
        self.class(cid).name == SINGLETON_SURROGATE
    }

    /// The `(target, holder)` pairs a call site at `node` must consult
    /// before ordinary dispatch, MOST RECENTLY activated first -- real
    /// Ruby's own precedence when two `using`s refine the same class.
    ///
    /// Empty for the overwhelming majority of programs, which write no
    /// `using` at all; empty too for a node with no source position (a
    /// synthesized desugaring), where there is no lexical question to ask.
    pub(crate) fn refinements_active_at(
        &self,
        node: crate::hir::NodeId,
    ) -> Vec<(ClassId, ClassId, bool)> {
        if self.activations.is_empty() {
            return Vec::new();
        }
        let Some(span) = self.hir.span(node).and_then(|s| s.known()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for a in self.activations.iter().rev() {
            if a.file != span.file || span.start < a.start || span.start >= a.end {
                continue;
            }
            out.extend(
                self.refinements
                    .iter()
                    .filter(|r| r.module == a.module)
                    .map(|r| (r.target, r.holder, r.singleton)),
            );
        }
        out
    }

    /// `(refining module, refined target)` for a `refine` holder -- what
    /// codegen registers so `Module#refinements` and `Refinement#target` can
    /// answer at run time. `None` for an ordinary module.
    pub(crate) fn refinement_of(&self, holder: ClassId) -> Option<(ClassId, ClassId)> {
        self.refinements
            .iter()
            .find(|r| r.holder == holder)
            .map(|r| (r.module, r.target))
    }

    /// Whether `node` is a `refine` marker registration already consumed --
    /// what lets codegen emit nothing for it. An UNREGISTERED marker stays a
    /// loud rejection: dropping one would lose the refinement silently.
    pub(crate) fn refinement_marker_registered(&self, node: crate::hir::NodeId) -> bool {
        self.refinements.iter().any(|r| r.marker == node)
    }

    /// The class `holder` refines, or `None` when `holder` is an ordinary
    /// module.
    pub(crate) fn refinement_target(&self, holder: ClassId) -> Option<ClassId> {
        self.refinements
            .iter()
            .find(|r| r.holder == holder)
            .map(|r| r.target)
    }

    /// Every `(target, holder, singleton)` written by the SAME module as
    /// `holder`'s own `refine` block, itself included -- what a bare name
    /// inside that block can reach. Empty when `holder` is an ordinary module.
    pub(crate) fn refinements_beside(&self, holder: ClassId) -> Vec<(ClassId, ClassId, bool)> {
        let Some(module) = self
            .refinements
            .iter()
            .find(|r| r.holder == holder)
            .map(|r| r.module)
        else {
            return Vec::new();
        };
        self.refinements
            .iter()
            .filter(|r| r.module == module)
            .map(|r| (r.target, r.holder, r.singleton))
            .collect()
    }

    /// Whether `holder` defines `name` as an instance method of its own --
    /// the question that decides whether a call site routes through the
    /// refinement at all.
    pub(crate) fn refinement_defines(&self, holder: ClassId, name: &str) -> bool {
        let defines = |cid: ClassId| {
            self.class(cid)
                .own_methods
                .iter()
                .any(|&sid| self.scope(sid).name == name)
        };
        defines(holder)
            || self
                .class(holder)
                .imported_modules
                .iter()
                .any(|&m| defines(m))
    }

    /// Precomputes `cref_of`/`fq_name` for every class -- see
    /// [`Compiler::frozen_crefs`]. Called once, at `mro::materialize`'s
    /// head: registration (the only phase that adds classes or writes
    /// their identity fields) is over by then.
    pub fn freeze_identity_caches(&mut self) {
        let crefs = (0..self.classes.len() as u32)
            .map(|i| self.cref_of(Some(ClassId(i))))
            .collect();
        let names = (0..self.classes.len() as u32)
            .map(|i| self.fq_name(ClassId(i)))
            .collect();
        self.frozen_crefs = Some(crefs);
        self.frozen_fq_names = Some(names);
        let defs = self
            .classes
            .iter()
            .map(|ci| {
                ci.class_body_stmts
                    .iter()
                    .filter_map(|&n| match &self.hir[n] {
                        crate::hir::HirNode::ConstWrite {
                            scope: None, name, ..
                        } => Some(name.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        self.direct_const_defs = Some(defs);
    }

    /// Where `node` runs in the program, or `None` when that is not a static
    /// fact -- inside a `def` body, which runs at call time. See
    /// [`Compiler::doc_order`].
    pub(crate) fn doc_position(&self, node: crate::hir::NodeId) -> Option<u32> {
        self.doc_order.get(&node).copied()
    }

    /// Whether `owner::name` is already defined by the time a statically
    /// positioned `at` runs. `None` means "no static answer" -- either the
    /// query has no position, or nothing recorded a position for the
    /// definition (a `const_set`, a name only an ancestor supplies), in which
    /// case the caller keeps whatever whole-program answer it had.
    pub(crate) fn const_defined_before(
        &self,
        owner: ClassId,
        name: &str,
        at: crate::hir::NodeId,
    ) -> Option<bool> {
        let at = self.doc_position(at)?;
        let defined = *self.const_def_order.get(&(owner, name.to_string()))?;
        Some(defined < at)
    }

    /// `cref_of` as a borrowed slice, alloc-free -- valid only after
    /// `freeze_identity_caches` (any post-registration caller).
    pub fn cref_of_ref(&self, cid: ClassId) -> &[ClassId] {
        self.frozen_crefs
            .as_ref()
            .and_then(|c| c.get(cid.0 as usize))
            .expect("cref_of_ref before freeze_identity_caches")
    }

    /// The lexical cref chain enclosing (and including) `defining`,
    /// OUTERMOST FIRST -- exactly the `cref` argument `resolve_class`
    /// takes. Walks `cref_parent` links, which are `lexical_parent` except
    /// where a qualified definition parts the two (see
    /// `ClassInfo::cref_parent`).
    pub fn cref_of(&self, defining: Option<ClassId>) -> Vec<ClassId> {
        if let (Some(cache), Some(cid)) = (&self.frozen_crefs, defining)
            && let Some(chain) = cache.get(cid.0 as usize)
        {
            return chain.clone();
        }
        let mut chain = Vec::new();
        let mut cur = defining;
        while let Some(cid) = cur {
            chain.push(cid);
            // Nesting deeper than this is a `lexical_parent` cycle, not real
            // source. Bounded rather than tracked with a set: this runs on
            // every constant lookup, and a real chain is a handful of links.
            if chain.len() > MAX_NESTING {
                break;
            }
            cur = self.class(cid).cref_parent;
        }
        chain.reverse();
        chain
    }

    /// The fully-qualified display name (`"Store::Errors::NotFound"`) --
    /// joins the `lexical_parent` chain regardless of `qualified_def`
    /// (naming is a namespace property, cref cutting is not). Used for
    /// error messages wherever real Ruby prints the qualified path.
    pub fn fq_name(&self, cid: ClassId) -> String {
        if let Some(cache) = &self.frozen_fq_names
            && let Some(name) = cache.get(cid.0 as usize)
        {
            return name.clone();
        }
        // A singleton-class surrogate (a constant-bearing `class << self`
        // body -- see `lower::defs`) displays as CRuby's `#<Class:M>`, not a
        // `M::...` path: it is not reachable as a constant at all.
        if self.class(cid).name == SINGLETON_SURROGATE
            && let Some(p) = self.class(cid).lexical_parent
        {
            return format!("#<Class:{}>", self.fq_name(p));
        }
        let mut segments = vec![self.class(cid).name.clone()];
        let mut cur = self.class(cid).lexical_parent;
        while let Some(p) = cur {
            segments.push(self.class(p).name.clone());
            if segments.len() > MAX_NESTING {
                break;
            }
            cur = self.class(p).lexical_parent;
        }
        segments.reverse();
        segments.join("::")
    }

    /// Whether a runtime `undef_method` anywhere in `cid`'s ancestry could have
    /// retracted `name` by the time a call runs -- see
    /// [`ClassInfo::runtime_undefs`]. Codegen must not emit a direct call then.
    pub fn may_be_undefined_at_runtime(&self, cid: ClassId, name: &str) -> bool {
        std::iter::once(&cid)
            .chain(self.class(cid).ancestors.iter())
            .any(|&anc| self.class(anc).runtime_undefs.contains(name))
    }

    /// Whether a runtime site could replace `name`'s BODY or change its
    /// VISIBILITY before a call runs -- see [`Compiler::runtime_patches`].
    /// Program-wide rather than per-class: the receiver of a `define_method`
    /// or a `private` is an ordinary expression, and resolving it would be a
    /// second analysis that still could not decide the interesting cases.
    pub fn may_be_patched_at_runtime(&self, name: &str) -> bool {
        self.runtime_patches_any_name || self.runtime_patches.contains(name)
    }

    /// Whether ANY class in this program gives `!` a body. `!` is an
    /// ordinary overridable method (`BasicObject#!`, a real method since
    /// 1.9 precisely so a null-object wrapper can decide its own
    /// truthiness), so folding `!x` to a truthiness test is sound only
    /// while nobody has overridden it.
    ///
    /// The question is the PROGRAM's, not the receiver's: `!x` on a
    /// statically unknown receiver has no class to ask. CRuby decides the
    /// same thing per call through an inline cache -- `vm_opt_not` inlines
    /// only when the resolved entry is literally `rb_obj_not` -- and zeo
    /// decides it once, ahead of time.
    pub fn defines_bang(&self) -> bool {
        *self
            .defines_bang
            .get_or_init(|| self.scopes.iter().any(|s| s.name == "!"))
    }

    /// The LEAF segment of `cid`'s name -- what CRuby puts in a class-body
    /// backtrace frame (`<module:B>`, never `<module:A::B>`), regardless of how
    /// deeply the class is nested or whether it was defined compact
    /// (`module A::B`) or nested. Oracle-verified against ruby 4.0.6.
    pub fn leaf_name(&self, cid: ClassId) -> &str {
        crate::constpath::ConstPath::parse(&self.class(cid).name).base()
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
            prepends: Vec::new(),
            includes: Vec::new(),
            private_constants: Default::default(),
            extends: Vec::new(),
            class_method_prepends: Vec::new(),
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
        });
        ClassId((self.classes.len() - 1) as u32)
    }

    /// A bare arena push -- callers decide which per-class list (`own_methods`/
    /// `methods`/`own_class_methods`/`class_methods`) the returned id belongs
    /// in (see `analyze::register_class` and `analyze::mro::materialize`).
    pub fn push_scope(&mut self, scope: Scope) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(scope);
        id
    }

    pub fn scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.0 as usize]
    }

    /// Whether `cid` is a `Struct` zeo compiled to a real class -- so its
    /// protocol (`deconstruct`, `==`, `to_a`, ...) comes from `Struct`'s own
    /// runtime table and is invisible to [`method_in_chain`](Self::method_in_chain).
    /// A site that concludes "this class cannot have that method" must ask this
    /// first.
    pub fn is_compiled_struct(&self, cid: ClassId) -> bool {
        !self.class(cid).hidden_ivars.is_empty()
    }

    /// See [`Hir::uses_call_tracing`](crate::hir::Hir::uses_call_tracing).
    pub fn traces_calls(&self) -> bool {
        *self
            .traces_calls
            .get_or_init(|| self.hir.uses_call_tracing())
    }

    /// Whether a method prologue must note its `self` for `TracePoint#self`.
    ///
    /// [`Compiler::traces_calls`] plus runtime `eval`, which can name
    /// `TracePoint` in text the arena scan never sees. Over-approximating is
    /// the safe direction: the note is one relaxed load when no hook is
    /// armed, and omitting it where a hook IS armed would answer `#self`
    /// wrongly.
    pub fn notes_frame_self(&self) -> bool {
        self.traces_calls() || self.hir.uses_runtime_eval()
    }

    /// See [`Hir::uses_ractor`](crate::hir::Hir::uses_ractor).
    pub fn uses_ractor(&self) -> bool {
        *self.uses_ractor.get_or_init(|| self.hir.uses_ractor())
    }

    /// `scope`'s [`AccessorShape`] when reaching the field DIRECTLY, in place
    /// of calling it, would be indistinguishable -- the shared precondition of
    /// the dynamic entry (`codegen::params::emit_accessor_trampoline`) and the
    /// static call site (`codegen::call`'s Path 1).
    ///
    /// `owner` is the class whose generated struct will be indexed, which is
    /// not always `scope.class`: a Path-1 site resolves the METHOD through the
    /// receiver's ancestry, and the field it must read belongs to the
    /// receiver's own struct.
    pub fn accessor_shape<'s>(
        &self,
        owner: ClassId,
        scope: &'s Scope,
    ) -> Option<&'s AccessorShape> {
        scope.accessor.as_ref().filter(|a| {
            (a.attr_generated || !self.traces_calls())
                && !scope.needs_block_param()
                // A `Struct` MEMBER devirtualizes exactly like an ivar: it is a
                // real slot, just one `instance_variables` does not report.
                && (self.class(owner).ivars.contains(&a.ivar)
                    || self.class(owner).hidden_ivars.contains(&a.ivar))
        })
    }

    pub fn class(&self, id: ClassId) -> &ClassInfo {
        &self.classes[id.0 as usize]
    }

    /// Whether this class's methods emit as free functions over a boxed
    /// `__self: RubyValue` receiver (the builtin-reopen shape)
    /// rather than `self: Arc<Concrete>` struct methods. True for every
    /// builtin placeholder AND for `Object` itself: top-level `def`s live
    /// on `Object` (real Ruby's private-on-Object rule), whose instances --
    /// the `main` object, and any value at all once dispatch reaches the
    /// MRO tail -- are `RubyValue`s, never a generated struct.
    pub fn value_backed(&self, cid: ClassId) -> bool {
        self.class(cid).is_builtin || cid == OBJECT_CLASS
    }

    /// Whether `cid` has a generated Rust struct in the program, so codegen may
    /// name its type -- `#ident::new_handle(...)`, an unboxed `Arc<Concrete>`, a
    /// static `(recv).method()` call. False for the four kinds that have none,
    /// each of which is instead a boxed `RubyValue` dispatched dynamically:
    /// MODULES (no instances), BUILT-INs (their repr is a `RubyValue` variant),
    /// `Object` (the runtime root, name-keyed ivars), BOOTSTRAP classes (the
    /// built-in exceptions, now in `zeo-rt`), and every
    /// NATIVE-BACKED user subclass: an exception subclass (`class MyErr <
    /// StandardError`, the native `RubyException`) or a value-builtin subclass
    /// (`class Stack < Array`, the native `ValueSubclass`), both constructed via
    /// `construct_by_class_id` rather than a per-class struct. The single source
    /// of truth for "is there a struct here?", which several `TyKind::Object`
    /// and `.new` sites gate on.
    /// Whether any class in the program inherits from `cid` -- a whole-program
    /// fact, and the guard on every fold that treats an instance's RUNTIME
    /// class as its statically-known one. An inherited method compiles to ONE
    /// shared body (`__sh*`) serving the base and every subclass, so `self`
    /// there is typed as the base while its real class may be any descendant.
    pub fn has_subclass(&self, cid: ClassId) -> bool {
        self.classes.iter().any(|c| c.parent == Some(cid))
    }

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
            // `X.new` as `TyKind::Object(X)` bakes `X::new_handle` calls to a
            // struct codegen never emits -- the exact failure the comment
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
    /// `new_handle` calls to structs codegen (correctly) never emits.
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
    fn superclass_chain(&self, cid: ClassId) -> impl Iterator<Item = ClassId> + '_ {
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
    /// `RubyValue`-self free functions (`__exc_<id>`) `define_method`'d onto the
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
    /// for a Proc-typed receiver -- see `codegen::call`'s `.class` arm.
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
    /// two lists must stay disjoint, or `codegen::call::new` and
    /// `super_calls` emit the wrong constructor.
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

    /// A flat lookup into the receiver class's own MATERIALIZED `methods`
    /// list -- no ancestor walk needed at call-resolution time at all,
    /// since `analyze::mro::materialize` already resolved every reachable
    /// name (own, inherited, or mixed-in) onto the class itself. Returns
    /// the ALREADY-RESOLVED `(class, scope)` pair; `super` resolution
    /// (`codegen::call::emit_super`) is the one place that still
    /// needs to walk `ancestors` explicitly, since it must search PAST
    /// wherever the currently-executing method was actually defined, not
    /// just find the winner from scratch.
    /// Whether the program assigns the constant `path` NAMES -- as opposed to
    /// some constant that merely shares its last segment.
    ///
    /// A bare `Template` can be assigned in any lexical scope, so its leaf is
    /// the only question worth asking. `::Tilt::Template` is a different
    /// question: temple writes `class Tilt < ::Tilt::Template` and, with tilt
    /// absent from the require graph, the leaf test found some unrelated
    /// `Template` and called the superclass known -- so the definition was
    /// registered rather than deferred, and failed later as an unknown
    /// superclass instead of raising the `NameError` CRuby raises there.
    pub fn assigns_const_path(&self, path: &str) -> bool {
        let path = crate::constpath::ConstPath::parse(path);
        let key = if path.is_bare() {
            path.base()
        } else {
            path.unanchored()
        };
        self.assigned_const_names.contains(key)
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

    pub fn method_in_chain(&self, class: ClassId, name: &str) -> Option<(ClassId, ScopeId)> {
        self.lookup_method(class, name)
            .map(|e| (e.defined_class(self), e.def))
    }

    /// Where `class`'s body was written, for locating a rejection raised after
    /// the statement walk has finished.
    ///
    /// The passes that run then (`mro`, chiefly) iterate CLASSES rather than
    /// statements, so they have no statement to stamp -- but a class was
    /// written somewhere, and its body site remembers the node. A class with no
    /// site is one nothing declared in Ruby source: a builtin, or a forward
    /// shell minted for a not-yet-seen superclass.
    pub fn class_def_span(&self, class: ClassId) -> Option<crate::hir::Span> {
        let node = self
            .class_body_sites
            .iter()
            .find(|s| s.class == class)
            .and_then(|s| s.def_node)?;
        self.hir.span(node)
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
                .find(|(new, _)| new == name)
                .map(|(_, old)| old.as_str())
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

    /// Same idea as `method_in_chain`, over `class_methods` instead of
    /// `methods` -- used for `ClassName.foo(...)` call sites (see
    /// `HirNode::ClassRef`'s docs).
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
        self.lookup_class_method(class, name)
            .map(|e| (e.defined_class(self), e.def))
    }
}

/// The methods `BasicObject` itself defines -- the ENTIRE surface a blank
/// slate answers before the user adds anything.
///
/// Taken from CRuby's own `Init` functions rather than inferred: `object.c`
/// (`initialize`, `==`, `equal?`, `!`, `!=`, and the three
/// `singleton_method_*` hooks), `vm_eval.c` (`instance_eval`,
/// `instance_exec`, `method_missing`, `__send__`), and `gc.c` (`__id__`).
/// Thirteen in total, and notably NOT `send` or `public_send` -- those live
/// on `Kernel` (`vm_eval.c:2961`), which a BasicObject subclass never sees.
pub fn is_basic_object_method(name: &str) -> bool {
    matches!(
        name,
        "initialize"
            | "=="
            | "equal?"
            | "!"
            | "!="
            | "__id__"
            | "__send__"
            | "instance_eval"
            | "instance_exec"
            | "method_missing"
            | "singleton_method_added"
            | "singleton_method_removed"
            | "singleton_method_undefined"
    )
}

/// The walks over `parent`/`lexical_parent` must terminate on a CYCLE.
///
/// `analyze` rejects the source shapes that build one (see
/// `analyze::resolve_superclass` and the superclass-mismatch guard), so these
/// construct the cycle directly: the point is that the guards hold even if a
/// future path lets one through. Every case is written to FAIL rather than hang
/// -- a bounded walk answers, an unbounded one never returns, and a test that
/// hangs tells no one anything.
#[cfg(test)]
mod cycle_guards {
    use super::*;

    /// `A -> B -> A`, built by hand.
    fn cyclic_pair() -> (Compiler, ClassId, ClassId) {
        let mut compiler = Compiler::new(crate::hir::Hir::default());
        let a = compiler.add_class("A".to_string(), Some(OBJECT_CLASS), false);
        let b = compiler.add_class("B".to_string(), Some(a), false);
        compiler.classes[a.0 as usize].parent = Some(b);
        (compiler, a, b)
    }

    #[test]
    fn the_superclass_chain_is_bounded() {
        let (compiler, a, _) = cyclic_pair();
        assert_eq!(compiler.superclass_chain(a).count(), MAX_NESTING);
    }

    /// The shape of what `is_exception_backed` asks -- a membership question
    /// over the chain. It spun forever here, flat on memory, while `require
    /// "active_record"` looked like it was doing work.
    #[test]
    fn asking_the_chain_a_question_terminates() {
        let (mut compiler, a, b) = cyclic_pair();
        let unrelated = compiler.add_class("Unrelated".to_string(), Some(OBJECT_CLASS), false);
        assert!(compiler.superclass_chain_contains(a, b));
        // The answer a cycle must NOT invent: nothing outside the loop is in it,
        // and asking has to come back to say so.
        assert!(!compiler.superclass_chain_contains(a, unrelated));
    }

    /// The check that keeps the cycle from being built at all.
    #[test]
    fn a_would_be_cycle_is_visible_before_the_link_is_made() {
        let mut compiler = Compiler::new(crate::hir::Hir::default());
        let a = compiler.add_class("A".to_string(), Some(OBJECT_CLASS), false);
        let b = compiler.add_class("B".to_string(), Some(a), false);
        // `class A < B` would close the loop, and this is what says so.
        assert!(compiler.superclass_chain_contains(b, a));
        assert!(!compiler.superclass_chain_contains(a, b));
    }

    #[test]
    fn a_lexical_parent_cycle_still_yields_a_name_and_a_cref() {
        let mut compiler = Compiler::new(crate::hir::Hir::default());
        let outer = compiler.add_class("Outer".to_string(), Some(OBJECT_CLASS), false);
        let inner = compiler.add_class("Inner".to_string(), Some(OBJECT_CLASS), false);
        compiler.classes[inner.0 as usize].lexical_parent = Some(outer);
        compiler.classes[outer.0 as usize].lexical_parent = Some(inner);
        assert!(compiler.fq_name(inner).ends_with("Inner"));
        assert!(compiler.cref_of(Some(inner)).len() <= MAX_NESTING + 1);
    }
}
