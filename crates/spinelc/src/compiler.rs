//! `spinelc`'s own analog of spinel's `Compiler`/`ClassInfo`/`Scope`
//! (`compiler.h`) -- pure compile-time bookkeeping. It never appears in the
//! generated program; codegen consults it and discards it. See the plan's
//! "Two `ClassId` types, on purpose": this `ClassId` is numerically mirrored
//! into `spinel_rt::ClassId` by codegen, but the two types are otherwise
//! unrelated -- `spinelc` never links against `spinel-rt` at all.

use crate::hir::{Hir, NodeId, Params, Visibility};
use crate::types::TyKind;
use std::collections::HashMap;

/// The SHARED compiler/runtime class numbering (Phase 15.1): `ClassId` and
/// every reserved builtin id are re-exported from `spinel-abi`, the
/// zero-dependency leaf crate both `spinelc` and `spinel-rt` consume -- the
/// two numbering schemes this file and `spinel_rt::dispatch` used to
/// maintain in parallel (synced by a `debug_assert`) are now literally one
/// definition. `Compiler::new` seeds the class arena from
/// `spinel_abi::BUILTINS` (id + Ruby-visible name + module-ness), which is
/// what makes `5.is_a?(Integer)`-style checks work uniformly through the
/// same `classes`/`ancestors` system as user classes.
pub use spinel_abi::{
    ClassId, ARRAY_CLASS, BASIC_OBJECT_CLASS, CLASS_CLASS, COMPARABLE_CLASS, COMPLEX_CLASS,
    DATA_CLASS, ENUMERABLE_CLASS, ENUMERATOR_CLASS, FIBER_CLASS, FLOAT_CLASS, HASH_CLASS,
    INTEGER_CLASS, KERNEL_CLASS, MATCH_DATA_CLASS, MATH_CLASS, MODULE_CLASS, MUTEX_CLASS,
    NUMERIC_CLASS, OBJECT_CLASS, PROC_CLASS, QUEUE_CLASS, RACTOR_CLASS, RANGE_CLASS,
    RATIONAL_CLASS, REGEXP_CLASS, STRING_CLASS, STRUCT_CLASS, SYMBOL_CLASS, THREAD_CLASS,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScopeId(pub u32);

pub struct ClassInfo {
    pub name: String,
    /// Which `Ruby::Box` this class/module is DEFINED in -- `0` is the main
    /// box (where the user's own top-level program runs; builtins and the
    /// exception prelude also live at box 0, distinguished by
    /// `is_builtin`/`is_bootstrap`). Always `0` until Phase 18 populates it
    /// for box-required/box-eval'd definitions; carried from day one (Phase
    /// 15.1) so every consumer is already box-shaped -- see
    /// `Compiler::resolve_class`.
    pub box_id: u32,
    /// The class/module this one is namespace-nested inside (`class Item`
    /// written within `class Store`'s body, or the qualified form `class
    /// Store::Item` -> `Some(Store)`), or `None` for a top-level
    /// definition. Drives both name resolution (`resolve_class`'s
    /// scope-exact lookup) and fully-qualified display names (`fq_name`).
    pub lexical_parent: Option<ClassId>,
    /// `true` when this class/module was defined via the QUALIFIED form
    /// (`class Store::Item ... end`) rather than textual nesting -- real
    /// Ruby gives that form a cref of just `[Item]` (its body does NOT see
    /// `Store`'s constants lexically; oracle-verified NameError), so
    /// `cref_of` cuts the chain here while `fq_name`/resolution keep the
    /// `lexical_parent` link. One documented approximation: the flag is
    /// per-CLASS, not per-body-occurrence, so a nested-form class REOPENED
    /// via the qualified form (or vice versa) keeps its original cref for
    /// all bodies.
    pub qualified_def: bool,
    /// `true` for classes from the built-in exception prelude
    /// (`parse::EXCEPTION_PRELUDE`) -- together with `is_builtin`, the
    /// "defined before any user program runs" set that stays visible inside
    /// EVERY box (CRuby's dup-from-master rule; see the plan's Part 14).
    /// Marked by `analyze` via `Hir::prelude_len`.
    pub is_bootstrap: bool,
    /// `None` for `Object` (the implicit root) and for every MODULE (a
    /// module has no superclass at all, not even implicitly `Object` --
    /// real Ruby's `Module#ancestors` on a standalone module is just
    /// `[M]`). Every ordinary class gets `Some(_)`, defaulting to
    /// `OBJECT_CLASS` when no `< Super` was written.
    pub parent: Option<ClassId>,
    /// `prepend`ed modules, in source order (see `analyze::mro`'s
    /// linearization -- expanded in REVERSE source order, so the most
    /// recently prepended module ends up closest).
    pub prepends: Vec<ClassId>,
    /// `include`d modules, in source order (same reverse-expansion rule as
    /// `prepends`).
    pub includes: Vec<ClassId>,
    /// `extend`ed modules, in source order -- NOT part of `ancestors()` at
    /// all (extend only affects this class's own `class_methods`
    /// materialization, never instance-method resolution or `is_a?`).
    pub extends: Vec<ClassId>,
    /// Names this class's body `undef`'d -- see `HirNode::Undef`.
    /// `mro::materialize_methods` refuses to materialize them onto this
    /// class, which is what makes an INHERITED name disappear here while
    /// staying live on the ancestor that defined it.
    pub undefined: std::collections::HashSet<String>,
    /// `(new, old)` aliases whose source method is INHERITED (not defined in
    /// this class's own body) -- recorded by `analyze::register_class` from a
    /// `HirNode::AliasMethod` and resolved by `mro::resolve_aliases` once the
    /// ancestor chain is linearized. See `HirNode::AliasMethod`'s docs.
    pub pending_aliases: Vec<(String, String)>,
    /// `true` for `module Name ... end`: never instantiated (no `Name.new`,
    /// no generated Rust struct/`impl RubyObject`/`ClassRegistry` entry --
    /// see `codegen::mod::emit_class`'s docs), used only as a source for
    /// materialization into whatever includes/prepends/extends it.
    pub is_module: bool,
    /// The full flattened ivar set (own methods' ivars ∪ every
    /// MRO-reachable inherited/mixed-in method's ivars) -- computed once by
    /// `analyze::mro::materialize` from the MATERIALIZED `methods` list, not
    /// `own_methods` (see that module's docs for why this needs no separate
    /// parent-first merge pass the way spinel's C struct layout does).
    pub ivars: Vec<String>,
    /// Instance methods LITERALLY written in this class/module's own body
    /// (`def name`, not `def self.name`) -- the source `materialize` reads
    /// from, and what `super`-resolution searches (see `analyze::mro`'s
    /// docs). Never codegen'd directly except when `methods` reuses one of
    /// these verbatim (this class's own, non-inherited definition).
    pub own_methods: Vec<ScopeId>,
    /// The full MRO-resolved set `codegen::mod::emit_class` actually emits
    /// one Rust method per entry for -- own ∪ every name reachable via
    /// `ancestors` (superclass, `include`, `prepend`), each carrying its own
    /// `Scope::defining_class` for `super` to search from. Always populated
    /// by `analyze::mro::materialize`, even for a class with no mixins at
    /// all (closes a latent gap: today's spike never generated a Rust
    /// method for a purely-inherited, non-overridden method at all).
    pub methods: Vec<ScopeId>,
    /// `def self.name` written literally in this class/module's own body.
    pub own_class_methods: Vec<ScopeId>,
    /// MRO-resolved class methods: `own_class_methods` (always wins) ∪
    /// `extends`' own instance methods (materialized as class methods, most
    /// recently `extend`ed module closest -- mirrors `include`'s priority
    /// rule). Codegen emits these as plain associated functions (no `self`
    /// receiver) alongside the class's ordinary `impl` block, or as a
    /// `pub mod` of free functions for a module with no struct of its own.
    pub class_methods: Vec<ScopeId>,
    /// `@@x` storage ownership, resolved once at analyze time (not per
    /// access, unlike spinel -- see `analyze::mro::resolve_cvars`'s docs):
    /// name -> the class/module that actually OWNS the runtime storage
    /// (nearest ancestor, including self, that ever claimed it first).
    pub cvar_owners: HashMap<String, ClassId>,
    /// Bare-constant storage ownership, resolved once at analyze time --
    /// same scheme as `cvar_owners` (nearest ancestor, including self, that
    /// ever claimed the name first), used by `codegen::expr::const_owner_id`.
    /// Only bare (`scope: None`) constant writes register ownership this way
    /// -- an explicit `Foo::NAME` write always targets `Foo` directly,
    /// regardless of lexical position (see `HirNode::ConstWrite`'s docs).
    pub const_owners: HashMap<String, ClassId>,
    /// Class-body TOP-LEVEL `@@x = expr` statements (`@@count = 0` written
    /// directly inside `class Foo; ... end`, not inside any method) -- real
    /// Ruby executes a class body immediately, top to bottom, as part of
    /// loading the class. This spike doesn't model general class-body
    /// statement execution (arbitrary side-effecting code interleaved with
    /// other top-level code) -- only this one common, narrow shape, run
    /// once from generated `main()` right after this class's own
    /// `__register()` call (see `codegen::mod::codegen`).
    pub class_body_stmts: Vec<NodeId>,
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
    /// `Some(root builtin id)` for a PER-BOX builtin-reopen OVERLAY (Phase
    /// 18): this ClassInfo carries a box's patches on the root builtin --
    /// its methods emit as value methods registered under `(root id,
    /// box_id)`, while instances keep the ROOT's ClassId (`box::String ==
    /// String`). `None` for every ordinary class, including root builtins.
    pub builtin_overlay: Option<ClassId>,
    /// `Some(feature)` for a require-gated builtin (`Base64`, gated by
    /// `"base64"`) -- mirrored from `spinel_abi::BuiltinClass::feature`. Its
    /// constant is INVISIBLE to `resolve_class` until `feature` has been
    /// activated by a `require` somewhere in the program
    /// (`Hir::activated_features`), so referencing it un-`require`d NameErrors
    /// exactly as in CRuby (see `Compiler::feature_active`). `None` for every
    /// always-on class (all user classes and all core builtins).
    pub feature_gate: Option<&'static str>,
}

pub struct Scope {
    pub name: String,
    pub class: Option<ClassId>,
    /// Which class/module's HIR body this Scope's `params`/`body` actually
    /// came from -- equal to `class` for an ordinary own-body method, but
    /// set to the true source ancestor for a materialized (inherited or
    /// mixed-in) method. `super` resolution (`codegen::call::emit_super_inline`)
    /// searches `class`'s `ancestors` starting AFTER this position, not
    /// after `class` itself -- see `analyze::mro`'s docs for why these two
    /// need to be distinct once mixins/plain inheritance-without-override
    /// exist.
    pub defining_class: ClassId,
    pub params: Params,
    pub body: Vec<NodeId>,
    /// Per-local static type, computed once by `analyze::locals::infer_locals`
    /// (a forward, single-pass walk -- not the deferred whole-program
    /// fixpoint). Lets codegen resolve `x + y` to native `Int` arithmetic
    /// when `x`/`y` are locals, not just literal-on-literal operands.
    pub local_types: HashMap<String, TyKind>,
    /// Whether this method's own body (NOT a nested block's) uses a bare
    /// `yield`/`block_given?` -- computed once by `analyze::register_class`'s
    /// `scan_bare_block_use`. Together with `params.block.is_some()`, this
    /// decides whether the method gets an implicit trailing `__blk` Rust
    /// parameter (see `codegen::params`).
    pub uses_bare_block: bool,
    /// As of the `def`'s own position in its class body -- see
    /// `hir::Visibility`'s docs. Enforced at `codegen::call::dispatch`'s
    /// Path 1 site and `spinel_rt::send`'s Path 2 dispatch.
    pub visibility: Visibility,
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
    /// Each box's TOP-LEVEL SURROGATE (Phase 18): a module-shaped
    /// `ClassInfo` named `#<Ruby::Box:N>` that owns the box's top-level
    /// constants and doubles as the handle's runtime `RubyValue::Class`
    /// payload. Created by `analyze` (one per box id the loader
    /// allocated), looked up by codegen.
    pub box_surrogates: HashMap<u32, ClassId>,
}

impl Compiler {
    pub fn new(hir: Hir) -> Compiler {
        let mut compiler = Compiler {
            hir,
            classes: vec![ClassInfo {
                name: "Object".to_string(),
                box_id: 0,
                lexical_parent: None,
                qualified_def: false,
                is_bootstrap: false,
                parent: None,
                prepends: Vec::new(),
                includes: Vec::new(),
                extends: Vec::new(),
            undefined: std::collections::HashSet::new(),
            pending_aliases: Vec::new(),
                is_module: false,
                ivars: Vec::new(),
                own_methods: Vec::new(),
                methods: Vec::new(),
                own_class_methods: Vec::new(),
                class_methods: Vec::new(),
                cvar_owners: HashMap::new(),
                const_owners: HashMap::new(),
                class_body_stmts: Vec::new(),
                ancestors: Vec::new(),
                is_builtin: false,
                builtin_overlay: None,
                feature_gate: None,
            }],
            scopes: Vec::new(),
            box_surrogates: HashMap::new(),
        };
        // The CRuby-exact hierarchy is DECLARED in the ABI table (Phase
        // 17.1): superclass edges (`Integer < Numeric`, `Class < Module`,
        // `BasicObject` as the parentless root) and real mixins (`Numeric`/
        // `String`/`Symbol` include `Comparable`; `Array`/`Hash`/`Range`/
        // `Struct`/`Enumerator` include `Enumerable`). Forward id refs are
        // fine: `parent`/`includes` are only linearized by
        // `mro::materialize` after every class exists.
        for b in spinel_abi::BUILTINS {
            let id = compiler.add_class(b.name.to_string(), b.superclass, b.is_module);
            debug_assert_eq!(
                id, b.id,
                "spinel_abi::BUILTINS must stay contiguous from ClassId(1)"
            );
            let ci = &mut compiler.classes[id.0 as usize];
            ci.is_builtin = true;
            ci.includes = b.includes.to_vec();
            ci.feature_gate = b.feature;
        }
        // Object's own slot in the chain (it isn't a BUILTINS row):
        // `Object < BasicObject`, `include Kernel` -- so EVERY chain ends
        // `..., Object, Kernel, BasicObject`, the real Ruby tail.
        compiler.classes[0].parent = Some(spinel_abi::OBJECT_SUPERCLASS);
        compiler.classes[0].includes = spinel_abi::OBJECT_INCLUDES.to_vec();
        compiler
    }

    /// THE name-resolution primitive (Phase 15.1) -- every "which class does
    /// this name/path mean HERE" question goes through this one function,
    /// keyed by the full resolution context real Ruby uses: the lexical
    /// cref chain (innermost scope LAST -- `cref_of`'s order), and the box
    /// the referencing code is defined in (always `0` until Phase 18).
    ///
    /// `path` may be a multi-segment constant path (Phase 15.3):
    /// `"Store::Errors::NotFound"` resolves its FIRST segment through the
    /// full unqualified rule below, then descends the remaining segments as
    /// direct namespace children only (no lexical/bootstrap fallback past
    /// the first segment -- real Ruby's own `::` rule). A leading `::`
    /// (`"::Foo"`) anchors the first segment at the top level, skipping the
    /// cref chain.
    ///
    /// Unqualified rule, mirroring CRuby: the lexical chain
    /// innermost-outward, then the box's own top level, then the BOOTSTRAP
    /// set (builtins + the exception prelude -- the "defined before any
    /// user program runs" classes every box sees; a box's own definition of
    /// the same name shadows it, exactly like CRuby's per-box constant
    /// overlay). First-registered wins within one scope, same as the old
    /// flat `class_by_name` (reopening semantics -- Phase 15.2 -- attach to
    /// that first registration rather than adding duplicates). One
    /// documented approximation: the cref head's ANCESTORS are not searched
    /// (real Ruby checks them between the lexical chain and the top level
    /// -- a class nested inside a SUPERCLASS referenced by bare name from a
    /// subclass misses here, loudly, rather than resolving wrong).
    pub fn resolve_class(&self, path: &str, cref: &[ClassId], box_id: u32) -> Option<ClassId> {
        let (path, anchored) = match path.strip_prefix("::") {
            Some(rest) => (rest, true),
            None => (path, false),
        };
        let mut segments = path.split("::");
        let first = segments.next()?;
        let mut cur = self.resolve_unqualified(first, if anchored { &[] } else { cref }, box_id)?;
        for seg in segments {
            // Descend within the resolved parent's OWN box (the parent may
            // itself have resolved through the bootstrap fallback into box
            // 0 even when `box_id` differs).
            cur = self.class_in_scope(Some(cur), seg, self.class(cur).box_id)?;
        }
        // A per-box builtin-reopen OVERLAY (Phase 18) is a patch container,
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
        if let Some(cid) = self
            .class_in_scope(None, name, box_id)
            .filter(|&c| self.feature_active(c))
        {
            return Some(cid);
        }
        if box_id != 0 {
            return self
                .classes
                .iter()
                .position(|c| {
                    c.name == name
                        && c.box_id == 0
                        && c.lexical_parent.is_none()
                        && (c.is_builtin || c.is_bootstrap)
                })
                .map(|i| ClassId(i as u32))
                .filter(|&c| self.feature_active(c));
        }
        None
    }

    /// First class/module named `name` defined directly inside
    /// `lexical_parent` (or at the top level for `None`) in `box_id`.
    /// `pub(crate)` since Phase 15.3: `analyze::register_class`'s
    /// reopening-detection must be SCOPE-EXACT (a nested `Store::Item` must
    /// never be mistaken for a top-level `Item`, or vice versa), which the
    /// lexical-fallback walk `resolve_class` does would get wrong.
    pub(crate) fn class_in_scope(
        &self,
        lexical_parent: Option<ClassId>,
        name: &str,
        box_id: u32,
    ) -> Option<ClassId> {
        self.classes
            .iter()
            .position(|c| c.name == name && c.box_id == box_id && c.lexical_parent == lexical_parent)
            .map(|i| ClassId(i as u32))
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

    /// The lexical cref chain enclosing (and including) `defining`,
    /// OUTERMOST FIRST -- exactly the `cref` argument `resolve_class`
    /// takes. Walks `lexical_parent` links, stopping above a
    /// `qualified_def` class (the `class Store::Item` form's body does not
    /// see `Store` lexically -- see `ClassInfo::qualified_def`).
    pub fn cref_of(&self, defining: Option<ClassId>) -> Vec<ClassId> {
        let mut chain = Vec::new();
        let mut cur = defining;
        while let Some(cid) = cur {
            chain.push(cid);
            let ci = self.class(cid);
            cur = if ci.qualified_def { None } else { ci.lexical_parent };
        }
        chain.reverse();
        chain
    }

    /// The fully-qualified display name (`"Store::Errors::NotFound"`) --
    /// joins the `lexical_parent` chain regardless of `qualified_def`
    /// (naming is a namespace property, cref cutting is not). Used for
    /// error messages wherever real Ruby prints the qualified path.
    pub fn fq_name(&self, cid: ClassId) -> String {
        let mut segments = vec![self.class(cid).name.clone()];
        let mut cur = self.class(cid).lexical_parent;
        while let Some(p) = cur {
            segments.push(self.class(p).name.clone());
            cur = self.class(p).lexical_parent;
        }
        segments.reverse();
        segments.join("::")
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
            qualified_def: false,
            is_bootstrap: false,
            parent,
            prepends: Vec::new(),
            includes: Vec::new(),
            extends: Vec::new(),
            undefined: std::collections::HashSet::new(),
            pending_aliases: Vec::new(),
            is_module,
            ivars: Vec::new(),
            own_methods: Vec::new(),
            methods: Vec::new(),
            own_class_methods: Vec::new(),
            class_methods: Vec::new(),
            cvar_owners: HashMap::new(),
            const_owners: HashMap::new(),
            class_body_stmts: Vec::new(),
            ancestors: Vec::new(),
            is_builtin: false,
            builtin_overlay: None,
            feature_gate: None,
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

    pub fn class(&self, id: ClassId) -> &ClassInfo {
        &self.classes[id.0 as usize]
    }

    /// Whether this class's methods emit as free functions over a boxed
    /// `__self: RubyValue` receiver (the Phase 16.3 builtin-reopen shape)
    /// rather than `self: Arc<Concrete>` struct methods. True for every
    /// builtin placeholder AND for `Object` itself: top-level `def`s live
    /// on `Object` (real Ruby's private-on-Object rule), whose instances --
    /// the `main` object, and any value at all once dispatch reaches the
    /// MRO tail -- are `RubyValue`s, never a generated struct.
    pub fn value_backed(&self, cid: ClassId) -> bool {
        self.class(cid).is_builtin || cid == OBJECT_CLASS
    }

    /// A flat lookup into the receiver class's own MATERIALIZED `methods`
    /// list -- no ancestor walk needed at call-resolution time at all,
    /// since `analyze::mro::materialize` already resolved every reachable
    /// name (own, inherited, or mixed-in) onto the class itself. Returns
    /// the ALREADY-RESOLVED `(class, scope)` pair; `super` resolution
    /// (`codegen::call::emit_super_inline`) is the one place that still
    /// needs to walk `ancestors` explicitly, since it must search PAST
    /// wherever the currently-executing method was actually defined, not
    /// just find the winner from scratch.
    pub fn method_in_chain(&self, class: ClassId, name: &str) -> Option<(ClassId, ScopeId)> {
        let info = &self.classes[class.0 as usize];
        info.methods
            .iter()
            .find(|&&s| self.scopes[s.0 as usize].name == name)
            .map(|&sid| (self.scopes[sid.0 as usize].defining_class, sid))
    }

    /// Same idea as `method_in_chain`, over `class_methods` instead of
    /// `methods` -- used for `ClassName.foo(...)` call sites (see
    /// `HirNode::ClassRef`'s docs).
    pub fn class_method_in_chain(&self, class: ClassId, name: &str) -> Option<(ClassId, ScopeId)> {
        let info = &self.classes[class.0 as usize];
        info.class_methods
            .iter()
            .find(|&&s| self.scopes[s.0 as usize].name == name)
            .map(|&sid| (self.scopes[sid.0 as usize].defining_class, sid))
    }
}
