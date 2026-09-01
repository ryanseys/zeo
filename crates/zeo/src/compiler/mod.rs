//! `zeo`'s own analog of zeo's `Compiler`/`ClassInfo`/`Scope`
//! (`compiler.h`) -- pure compile-time bookkeeping. It never appears in the
//! generated program; codegen consults it and discards it. See the plan's
//! "Two `ClassId` types, on purpose": this `ClassId` is numerically mirrored
//! into `zeo_rt::ClassId` by codegen, but the two types are otherwise
//! unrelated -- `zeo` never links against `zeo-rt` at all.

mod class_info;
mod names;
mod sites;

pub(crate) use class_info::MAX_NESTING;
pub use class_info::{ClassInfo, MethodEntry};
pub use names::{NameId, Names, SINGLETON_SURROGATE};
pub(crate) use sites::{Activation, EvalActivation, Refinement};
pub use sites::{ClassBodySite, DefEvent, MarkerStream, SiteDef};

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

pub struct Scope {
    pub name: String,
    pub class: Option<ClassId>,
    /// Which class/module's HIR body this Scope's `params`/`body` actually
    /// came from -- equal to `class` for an ordinary own-body method, but
    /// set to the true source ancestor for a materialized (inherited or
    /// mixed-in) method. `super` resolution (`clif::call::lower_super`)
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
    /// decides whether the method's body signature carries the trailing
    /// `blk` slot (see `clif::params::body_sig`).
    pub uses_bare_block: bool,
    /// As of the `def`'s own position in its class body -- see
    /// `hir::Visibility`'s docs. Enforced by the caller class each call
    /// site passes (`clif::call::caller_class`) into `zeo_rt`'s dispatch.
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
    /// The compiled-in unit whose file wrote this `def`, if any -- the
    /// method twin of [`ClassInfo::unit`]. A unit is a load-path file
    /// nothing has required yet, so CRuby has no such method until the file
    /// runs. The row registers at startup all the same (the static MRO
    /// needs a shape) and is CONCEALED until the unit's function reveals
    /// it. Materialization clones the scope, so a module method carries the
    /// mark onto every class that mixed it in.
    pub unit: Option<u32>,
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
    /// The exported symbol of a separately compiled
    /// BODY this scope stands for. The scope itself is body-less (a
    /// package interface registered it); codegen must never emit it, and a
    /// typed direct call declares this symbol as an import instead.
    pub extern_symbol: Option<String>,
}

/// A method whose entire body is one instance-variable access.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AccessorShape {
    /// WITHOUT its `@` -- exactly the `ClassInfo::ivars` spelling, which is
    /// the key the emitter's ivar-slot lookup uses.
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

/// One nominated explicit-receiver accessor site: the receiver's static
/// class, the ivar's resolved slot in that class's layout, and whether
/// this is the writer half. See [`Compiler::accessor_sites`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AccessorSite {
    pub cid: ClassId,
    pub slot: u32,
    pub writer: bool,
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
    /// Globals a MERGED PACKAGE writes. Their writes
    /// have no node in this arena, so the site-based guard analysis cannot
    /// see them; the name set is consulted beside it.
    pub external_global_writers: FSet<String>,
    /// The unit-walk blanket de-opt, split out for a
    /// PACKAGE build. A lazily-loaded unit's `def` names go here instead of
    /// [`runtime_patches`](Self::runtime_patches) when compiling a package,
    /// so the manifest's fact vector carries only GENUINE patch sources --
    /// a host would otherwise refuse to devirtualize every packaged method.
    /// [`may_be_patched_at_runtime`](Self::may_be_patched_at_runtime)
    /// unions both, so the package's own compile behaves identically.
    pub unit_blanket_names: FSet<String>,
    /// `(merged-manifest index, package-local class id)`
    /// -> the host id its interface registration minted. The merge writes
    /// each package's id-translation table from this map.
    pub pkg_class_map: FMap<(u32, u32), ClassId>,
    /// `classes.len()` right after the shared bootstrap
    /// (builtins + exception tail) -- the first id a program or package
    /// mints for itself. Recorded by `pin_builtin_exceptions_tail`; a
    /// package manifest carries it, and a host merging packages asserts
    /// its own matches before padding past their bands.
    pub first_program_class_id: u32,
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
    /// `main_statements` emits inline in document order (`clif::stmt::lower_stmt`'s
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
    /// `(extender, module)` -> the `extend`/`include` statement that recorded
    /// the edge. The edge itself ([`ClassInfo::extends`]) is a set membership
    /// with no position, and a hook an extended module supplies is INSTALLED
    /// here, not where the module wrote its `def` -- see
    /// [`Compiler::class_method_install_node`].
    pub extend_sites: FMap<(ClassId, ClassId), crate::hir::NodeId>,
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
    /// The visibility a top-level `def` takes, which a bare `private`/`public`/
    /// `protected` between statements moves. It starts PRIVATE: that is what a
    /// top-level `def` is in ruby before anything says otherwise.
    ///
    /// Keyed by FILE, because that is ruby's scope for it. A required file
    /// that opens with `public` leaves the requiring file's cursor alone, and
    /// a spliced require puts both files' statements in one stream, so the
    /// only thing that separates them is where each statement came from.
    pub top_level_visibility: std::collections::HashMap<crate::hir::FileId, crate::hir::Visibility>,
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
    /// Which STREAM the walk is in: `None` is the eager main file, `Some(k)`
    /// the k-th feature unit.
    ///
    /// `seq` orders definitions WITHIN a stream and says nothing across them:
    /// the walk numbers the whole main file before the first unit, while a
    /// unit's body really runs at its `require`. `resolve_aliases` is the
    /// consumer -- an alias binds the body that existed when it ran, and
    /// comparing seqs from two streams answers a question neither number
    /// asked.
    pub unit_stream: Option<u32>,
    /// The stream each scope was registered in -- [`Self::unit_stream`] at
    /// the moment of registration, kept for the scopes a later pass has to
    /// compare against an alias site.
    pub scope_stream: FMap<ScopeId, u32>,
    /// Boot-time overlay installs for observable redefinition timelines:
    /// `(class, name, first_scope)`. The static tables carry the FINAL body
    /// (last-`def`-wins, so every compile-time fact -- super inlining,
    /// materialization -- is untouched); the FIRST body is installed into
    /// the runtime overlay before the first statement runs, and each later
    /// redefinition re-installs at its own document position
    /// (`HirNode::MethodRedefine`). Filled by `analyze::redefs`.
    pub positional_redefs: Vec<(ClassId, String, ScopeId, bool)>,
    /// `(class, name, class_side, reveal group)` for a `def` a RUNTIME alias
    /// names as its SOURCE. Its row is CONCEALED until the `def`'s own line,
    /// so an alias taken above it copies what the name meant THEN -- CRuby's
    /// `rb_alias`. Groups are numbered past the units', which share the run
    /// time's reveal table. Filled by `analyze::alias_reveals`.
    pub alias_source_reveals: Vec<(ClassId, String, bool, u32)>,
    /// One of those sites names its method with something other than a literal
    /// (`Node.send(:define_method, computed)`, a bare `private`), so NO name is
    /// safe to fold. Kept apart from the set above because it is the expensive
    /// answer: it de-optimizes every direct call in the program.
    pub runtime_patches_any_name: bool,
    /// Whether the program can really compile Ruby at RUN time.
    ///
    /// [`crate::hir::Hir::uses_runtime_eval`] matches a call's NAME and
    /// nothing else, so a program that defines its own `load` -- or a class
    /// with its own `eval` -- reads as an eval site. That decides whether the
    /// binary carries the embedded compiler and all 170 class tables, which
    /// is 14 MB; `OptionParser#load` calling itself was paying it.
    ///
    /// A RECEIVERLESS call resolves the way DISPATCH resolves it: a user
    /// method of that name in the enclosing class's chain is what runs, and
    /// Kernel's is unreachable from there. Every other shape keeps the
    /// conservative answer, so this only ever narrows a site ruby itself
    /// would not send to Kernel.
    ///
    /// Set by `analyze`; `true` until then, which is what keeps a caller that
    /// asks too early safe.
    pub runtime_eval: bool,
    /// The always-on builtin classes this program can reach, or `None` for
    /// "every one of them".
    ///
    /// `None` is the safe answer, and the one a caller sees before `analyze`
    /// fills this in; `analyze::class_reach` computes the narrowed one.
    /// Require-GATED builtins are NOT in here -- their feature gate answers
    /// the same question more precisely.
    pub reachable_builtins: Option<FSet<ClassId>>,
    /// Whether the program calls `freeze` anywhere. A REOPEN of a frozen class
    /// is a `FrozenError` and its body never runs, so the definitions the
    /// compile-time tables carry for it have to be retractable -- which costs
    /// the name its static call sites. A program that never freezes anything
    /// cannot reach that shape, so it pays neither the check nor the
    /// de-optimization. See `clif::params::emit_reopen_guard`.
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
    /// and per ancestor by `analyze::constfold` -- is a set lookup instead of
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
    /// A snippet's `using` sites, in source order -- see [`EvalActivation`].
    pub(crate) eval_activations: Vec<EvalActivation>,
    /// Block call sites whose receiver's STATIC type admits a native inline
    /// loop (`analyze::mark_inline_iter_sites`), keyed by the BLOCK node.
    /// Soundness lives in the emitted match GUARD (a mistyped receiver takes
    /// the dynamic-fallback arm), so the map is purely an optimization hint;
    /// the escaping-block scans deliberately ignore it -- a marked site keeps
    /// escaping-style cell captures, correct in both arms.
    pub inline_iter_sites: FMap<crate::hir::NodeId, InlineIterKind>,
    /// Explicit-receiver accessor CALL sites (`node.nxt`, `obj.attr = v`
    /// on a statically-classed local) the emitter folds through the
    /// guarded runtime attr entries -- see `analyze::mark_accessor_sites`.
    /// Purely a hint, like `inline_iter_sites`: the runtime entry's own
    /// guard decides per call and its slow arm is the full explicit send.
    pub accessor_sites: FMap<crate::hir::NodeId, AccessorSite>,
    /// Explicit-receiver call nodes whose receiver is a local statically
    /// typed `Object(cid)` and whose name resolves in cid's MATERIALIZED
    /// chain to a plain PUBLIC compiled body -- candidates for the typed
    /// direct call (`clif::call::typed_direct_send`). Like the accessor
    /// fold, nomination only: the emitted site re-checks the gate word and
    /// the receiver's exact class per call, so a wrong static type costs a
    /// slow path, never a wrong answer. See `analyze::mark_typed_call_sites`.
    pub typed_call_sites: FMap<crate::hir::NodeId, ClassId>,
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
    /// Memo for [`Compiler::blank_slate_possible`] -- a whole-arena scan
    /// every universal-row fold would otherwise repeat.
    blank_slate_possible: std::cell::OnceCell<bool>,
    /// Memo for [`Compiler::moved_receiver_possible`].
    moved_receiver_possible: std::cell::OnceCell<bool>,
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

impl InlineIterKind {
    /// How many required block parameters the kind's splice can BIND --
    /// the cap `fusable_block` (and its analyze twin) applies per site.
    /// Two only for the kinds that yield two values; every other kind
    /// stays at one, so `3.times { |a, b| }` keeps its dynamic row.
    pub fn max_fused_params(self) -> usize {
        match self {
            InlineIterKind::ArrayEachWithIndex
            | InlineIterKind::ArrayInject
            | InlineIterKind::HashEach => 2,
            _ => 1,
        }
    }
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

impl Compiler {
    pub fn new(hir: Hir) -> Compiler {
        let mut compiler = Compiler {
            hir,
            external_global_writers: FSet::default(),
            first_program_class_id: 0,
            classes: vec![ClassInfo {
                name: "Object".to_string(),
                box_id: 0,
                lexical_parent: None,
                cref_parent: None,
                qualified_def: false,
                is_bootstrap: false,
                parent: None,
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
                is_module: false,
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
                unit: None,
                imported_pkg: None,
            }],
            scopes: Vec::new(),
            names: Names::default(),
            pending_ruby_raise: None,
            box_surrogates: FMap::default(),
            class_body_sites: Vec::new(),
            global_def_hooks: Default::default(),
            extend_sites: FMap::default(),
            pre_exec_spans: Vec::new(),
            def_seq: 0,
            top_level_defs: Vec::new(),
            top_level_visibility: std::collections::HashMap::new(),
            shell_kinds: FMap::default(),
            assigned_const_names: FSet::default(),
            top_level_const_aliases: FMap::default(),
            const_aliases: FMap::default(),
            unique_top_const_inits: FMap::default(),
            const_write_sites: FMap::default(),
            global_write_sites: FMap::default(),
            const_set_sites: Vec::new(),
            runtime_patches: FSet::default(),
            unit_blanket_names: FSet::default(),
            pkg_class_map: FMap::default(),
            runtime_mixin_super_names: FSet::default(),
            unit_walk: false,
            unit_scopes: FSet::default(),
            unit_stream: None,
            scope_stream: FMap::default(),
            positional_redefs: Vec::new(),
            alias_source_reveals: Vec::new(),
            runtime_patches_any_name: false,
            runtime_eval: true,
            reachable_builtins: None,
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
            eval_activations: Vec::new(),
            inline_iter_sites: FMap::default(),
            accessor_sites: FMap::default(),
            typed_call_sites: FMap::default(),
            times_literal_suppressed: false,
            range_each_literal_suppressed: false,
            traces_calls: std::cell::OnceCell::new(),
            defines_bang: std::cell::OnceCell::new(),
            uses_ractor: std::cell::OnceCell::new(),
            blank_slate_possible: std::cell::OnceCell::new(),
            moved_receiver_possible: std::cell::OnceCell::new(),
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
            ci.mixin_order = b.includes.iter().map(|&m| (m, false)).collect();
            ci.feature_gate = b.feature;
        }
        // The two edge kinds no `BuiltinClass` field carries, and both are
        // independent of `includes`: `CGI` extends the same module it
        // includes, and `CGI::Escape` prepends one. See their ABI tables.
        for &(id, modules) in zeo_abi::BUILTIN_EXTENDS {
            compiler.classes[id.0 as usize].extends = modules.to_vec();
        }
        for &(id, modules) in zeo_abi::BUILTIN_PREPENDS {
            compiler.classes[id.0 as usize]
                .mixin_order
                .extend(modules.iter().map(|&m| (m, true)));
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
        compiler.classes[0].mixin_order = zeo_abi::OBJECT_INCLUDES
            .iter()
            .map(|&m| (m, false))
            .collect();
        compiler
    }

    /// The `RubyVM` surfaces whose bodies PARSE at run time, and which no
    /// program can reach without naming `RubyVM`.
    ///
    /// They are the four prism-backed tables plus the iseq one, and together
    /// they root the prism library itself -- the largest saving any single
    /// group of tables carries (`cargo xtask size`). `RubyVM` itself STAYS: ruby
    /// defines it in every program, and these five are namespaced under it,
    /// so only `RubyVM.constants` can see them go and that already names
    /// `RubyVM`.
    pub(crate) fn prism_surface_is_reachable(&self, id: ClassId) -> bool {
        const PRISM_BACKED: &[ClassId] = &[
            zeo_abi::RUBYVM_AST_MODULE,
            zeo_abi::RUBYVM_AST_NODE_CLASS,
            zeo_abi::RUBYVM_AST_LOCATION_CLASS,
            zeo_abi::RUBYVM_ISEQ_CLASS,
            zeo_abi::RUBYVM_YJIT_MODULE,
        ];
        !PRISM_BACKED.contains(&id) || self.needs_prism_runtime()
    }

    /// Whether the binary must carry the prism parser at all.
    ///
    /// Three things reach it, and they are separate questions: a run-time
    /// `eval` compiles Ruby text; `require "prism"` calls the same C
    /// library's serialize entry points; and a `RubyVM` parsing surface
    /// parses in its own body.
    ///
    /// The eval half asks the NARROWED answer ([`Compiler::runtime_eval`]),
    /// never the flat `Hir` scan. A program whose `load` is its own method
    /// is not an eval site, and reading the scan here kept all five
    /// prism-backed tables in a binary that had already dead-stripped the
    /// compiler.
    pub fn needs_prism_runtime(&self) -> bool {
        self.hir.activates_prism() || self.runtime_eval || self.hir.mentions_rubyvm_parser()
    }

    /// Whether an emitted program can reach a BUILTIN class at all -- what
    /// decides if its method table is named in `ProgramDesc::class_tables`
    /// and so kept in the binary. A class the compiler never registered has
    /// no constant and no dispatch path.
    ///
    /// Three gates, in narrowing order: a require-gated extension's feature,
    /// the prism surfaces, and `analyze::class_reach`'s answer for an
    /// always-on class.
    pub(crate) fn builtin_is_reachable(&self, id: zeo_abi::ClassId) -> bool {
        // A builtin's compiler ClassId IS its abi id -- the same identity
        // `classes.rs`'s registration loop relies on. Past the end is a class
        // this compile never saw: keep it, because a missing table is a class
        // that silently loses every method.
        // Both halves of `classes.rs`'s own registration test. `feature_active`
        // alone is not enough: a class can carry a feature gate and still be
        // one `register_builtins` covers, and dropping its table drops its
        // CONSTANTS with it -- `File::RDWR` went missing that way.
        let idx = id.0 as usize;
        if idx >= self.classes.len() {
            return true;
        }
        let cid = ClassId(id.0);
        if zeo_abi::is_gated_builtin(cid) {
            // A gated class's feature gate is the more precise answer, and a
            // gated extension reaches its own nested classes through Rust
            // that no scan of the program can see.
            return self.feature_active(cid) && self.prism_surface_is_reachable(cid);
        }
        // Only a CORE builtin id is narrowed. A per-box copy of one is a
        // fresh id past the exception block -- `builtin_name` is what tells
        // them apart -- and it carries the box's own rows, which no
        // reachability rule about `String` speaks for.
        self.prism_surface_is_reachable(cid)
            && (zeo_abi::builtin_name(zeo_abi::ClassId(id.0)).is_none()
                || self
                    .reachable_builtins
                    .as_ref()
                    .is_none_or(|set| set.contains(&cid)))
    }

    /// Whether `cid`'s constant must WAIT for the file that defines it.
    ///
    /// A compiled-in unit is a load-path file nothing has required yet. Its
    /// classes register for dispatch at startup because the static MRO needs
    /// a shape, but CRuby has no such constant until the file runs -- so
    /// `defined?(PrettyPrint)` before the `require` is nil, not "constant".
    ///
    /// That is exactly the runtime-conditional shape, and it takes the same
    /// two emissions: `conceal_class` in the registration prologue and
    /// `reveal_class` at the head of the class's body site, which lives in
    /// the unit's own function and so runs when the unit does.
    pub(crate) fn class_waits_for_its_unit(&self, cid: ClassId) -> bool {
        self.class(cid).unit.is_some()
    }

    pub(crate) fn feature_active(&self, cid: ClassId) -> bool {
        match self.class(cid).feature_gate {
            None => true,
            // A feature ruby loads before line 1 needs no `require` to make
            // its constant resolve: `Monitor` answers in a program that never
            // mentions `monitor`, oracle-verified.
            Some(feature) => {
                crate::lower::features::is_preloaded_at_boot(feature)
                    || self.hir.activated_features.contains(feature)
            }
        }
    }

    /// Whether `cid`'s CONSTANT exists is a run-time question rather than a
    /// compile-time one -- so `defined?`, a bare reference and every constant
    /// fold must ask instead of answering.
    ///
    /// Two shapes: a class defined under a guard zeo cannot decide
    /// ([`ClassInfo::runtime_conditional`]), and a require-gated builtin,
    /// whose constant exists only from its `require`'s own line
    /// (`HirNode::FeatureLoaded`). A feature ruby has loaded before line 1 is
    /// neither -- it is simply there.
    pub(crate) fn constant_is_positional(&self, cid: ClassId) -> bool {
        // An interface-registered package class installs when its unit runs
        // -- its constant is positional exactly like a lazily-loaded unit's
        // (the merged conceal/reveal rows are what answer the probe).
        if self.class(cid).runtime_conditional
            || self.class(cid).imported_pkg.is_some()
            || self.class_waits_for_its_unit(cid)
        {
            return true;
        }
        // Exactly the classes `clif::classes` conceals: a gated builtin this
        // program REGISTERS (some file requires its feature) and that ruby
        // does not load before line 1. A feature nothing requires registers
        // no builtin at all, so the name belongs to whatever the program
        // defines under it.
        match zeo_abi::builtin_class(cid).and_then(|b| b.feature) {
            Some(feature) => {
                !crate::lower::features::is_preloaded_at_boot(feature)
                    && self.hir.activated_features.contains(feature)
            }
            None => false,
        }
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

    /// Records a surrogate whose id ANOTHER compile decided -- the program a
    /// snippet is evaluated inside, or the run time that minted the box.
    ///
    /// A snippet is its own compile and mints no boxes, so without this its
    /// emitter would find no surrogate for the box the `eval` runs in and
    /// fall back on `Object`, which is MAIN's top level. Class ids are the
    /// one thing both sides always agree on, so the id is taken as given --
    /// and because this compiler's own table is shorter than it, the gap is
    /// filled with placeholder modules. Nothing can name one (no Ruby
    /// source spells `#<zeo:reserved N>`) and a snippet registers nothing,
    /// so none of them reaches an emitted table.
    pub fn adopt_box_surrogate(&mut self, box_id: u32, cid: ClassId) {
        self.box_surrogates.insert(box_id, cid);
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
        self.runtime_patches_any_name
            || self.runtime_patches.contains(name)
            || self.unit_blanket_names.contains(name)
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
    /// A merged package already answered "yes" to one of
    /// the memoized whole-program questions, so the memo is decided before
    /// this arena is ever asked. Only a `true` seeds; `false` stays lazy.
    pub fn seed_world_bits(&mut self, defines_bang: bool, blank_slate: bool, moved: bool) {
        if defines_bang {
            let _ = self.defines_bang.set(true);
        }
        if blank_slate {
            let _ = self.blank_slate_possible.set(true);
        }
        if moved {
            let _ = self.moved_receiver_possible.set(true);
        }
    }

    pub fn defines_bang(&self) -> bool {
        *self
            .defines_bang
            .get_or_init(|| self.scopes.iter().any(|s| s.name == "!"))
    }

    /// Whether ANY receiver in this program could be a Kernel-less
    /// (BasicObject-rooted) instance: a registered blank-slate class, or
    /// the text mentioning `BasicObject` at all -- which is what
    /// `BasicObject.new` and a runtime `Class.new(BasicObject)` both
    /// need. A computed constant read counts as a mention (it could name
    /// anything). Kernel's universal rows (`nil?`, ...) may fold to a
    /// static answer only while this is false: a blank slate must raise
    /// NoMethodError instead. `const_get` with a computed STRING stays
    /// the documented reachability hatch.
    pub fn blank_slate_possible(&self) -> bool {
        *self.blank_slate_possible.get_or_init(|| {
            // `Ractor::MovedObject` is BasicObject-rooted and registered in
            // EVERY program, so counting it here made the answer a constant
            // `true` and no fold ever fired. A husk is only holdable after
            // a Ractor move, which [`Compiler::moved_receiver_possible`]
            // gates separately -- every fold that asks this question must
            // ask that one too.
            (0..self.classes.len() as u32).map(ClassId).any(|cid| {
                cid != BASIC_OBJECT_CLASS
                    && cid != zeo_abi::RACTOR_MOVED_OBJECT_CLASS
                    && self.is_blank_slate(cid)
            }) || self.hir.iter().any(|n| match n {
                crate::hir::HirNode::ClassRef(name) => name == "BasicObject",
                crate::hir::HirNode::QualifiedConstRead(_, name)
                | crate::hir::HirNode::ConstReadOrNil(_, name) => name == "BasicObject",
                crate::hir::HirNode::DynConstRead { .. } => true,
                _ => false,
            })
        })
    }

    /// Whether ANY receiver in this program could be a Ractor-moved husk,
    /// which raises on every method call (its class word is retagged
    /// `Ractor::MovedObject`, its value tag stays `Object`). A move needs
    /// `Ractor`, so the text mentioning it at all is the gate -- the same
    /// mention-scan envelope as [`Compiler::blank_slate_possible`], with
    /// the same documented `const_get` hatch. A universal-row fold whose
    /// static answer would bypass dispatch for an `Object`-tagged receiver
    /// is sound only while this is false: the husk's raise lives in
    /// dispatch.
    pub fn moved_receiver_possible(&self) -> bool {
        *self.moved_receiver_possible.get_or_init(|| {
            self.hir.iter().any(|n| match n {
                crate::hir::HirNode::ClassRef(name) => name == "Ractor",
                crate::hir::HirNode::QualifiedConstRead(_, name)
                | crate::hir::HirNode::ConstReadOrNil(_, name) => name == "Ractor",
                crate::hir::HirNode::DynConstRead { .. } => true,
                _ => false,
            })
        })
    }

    /// A class registered during a unit walk whose unit has not been
    /// accepted yet -- see [`ClassInfo::unit`].
    pub const UNIT_UNRESOLVED: u32 = u32::MAX;

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
        self.traces_calls() || self.runtime_eval
    }

    /// See [`Compiler::runtime_eval`]. The one question every post-analyze
    /// reader asks; the flat `Hir` scan stays for the loader, which runs
    /// before any of this is known.
    pub fn compiles_at_runtime(&self) -> bool {
        self.runtime_eval
    }

    /// See [`Hir::uses_ractor`](crate::hir::Hir::uses_ractor).
    pub fn uses_ractor(&self) -> bool {
        *self.uses_ractor.get_or_init(|| self.hir.uses_ractor())
    }

    /// `scope`'s [`AccessorShape`] when reaching the field DIRECTLY, in place
    /// of calling it, would be indistinguishable -- the shared precondition of
    /// the dynamic entry (`clif::params::define_accessor`) and the
    /// static call site (`clif::expr::inline_accessor`).
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
            // A HAND-written accessor keeps its body wherever instrumentation
            // can observe the call: a TracePoint hook needs the frame, and
            // line coverage needs the body's own line stamped. A GENERATED
            // one stays iseq-less either way, exactly as CRuby compiles it.
            (a.attr_generated
                || !(self.traces_calls() || crate::analyze::coverage::active(self)))
                && !scope.needs_block_param()
                // A `Struct` MEMBER devirtualizes exactly like an ivar: it is a
                // real slot, just one `instance_variables` does not report.
                && (self.class(owner).ivars.contains(&a.ivar)
                    || self.class(owner).hidden_ivars.contains(&a.ivar))
        })
    }
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
