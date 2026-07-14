//! `spinelc`'s own analog of spinel's `Compiler`/`ClassInfo`/`Scope`
//! (`compiler.h`) -- pure compile-time bookkeeping. It never appears in the
//! generated program; codegen consults it and discards it. See the plan's
//! "Two `ClassId` types, on purpose": this `ClassId` is numerically mirrored
//! into `spinel_rt::ClassId` by codegen, but the two types are otherwise
//! unrelated -- `spinelc` never links against `spinel-rt` at all.

use crate::hir::{Hir, NodeId, Params, Visibility};
use crate::types::TyKind;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClassId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScopeId(pub u32);

/// `ClassId(0)`, always present, no ivars, no methods, no superclass -- the
/// root every user class ultimately chains up to. Mirrors spinel reserving
/// class index 0 as the implicit root.
pub const OBJECT_CLASS: ClassId = ClassId(0);

pub struct ClassInfo {
    pub name: String,
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
}

impl Compiler {
    pub fn new(hir: Hir) -> Compiler {
        Compiler {
            hir,
            classes: vec![ClassInfo {
                name: "Object".to_string(),
                parent: None,
                prepends: Vec::new(),
                includes: Vec::new(),
                extends: Vec::new(),
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
            }],
            scopes: Vec::new(),
        }
    }

    pub fn class_by_name(&self, name: &str) -> Option<ClassId> {
        self.classes
            .iter()
            .position(|c| c.name == name)
            .map(|i| ClassId(i as u32))
    }

    /// `parent: None` for a module (no superclass at all) or a fresh root;
    /// `Some(_)` for an ordinary class (`register_class` always resolves a
    /// concrete `Some(OBJECT_CLASS)` default when no `< Super` was
    /// written).
    pub fn add_class(&mut self, name: String, parent: Option<ClassId>, is_module: bool) -> ClassId {
        self.classes.push(ClassInfo {
            name,
            parent,
            prepends: Vec::new(),
            includes: Vec::new(),
            extends: Vec::new(),
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
