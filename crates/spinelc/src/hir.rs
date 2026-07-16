//! A real typed HIR instead of a text-serialized node table. See the plan's
//! "Compiler internals" section: spinel's `spinel_parse.c` serializes
//! Prism's C AST to a line-oriented text format that `node_table.c`
//! re-parses into a flat, dynamically-typed `SpNode` arena -- a design
//! driven by a historical multi-binary pipeline that no longer applies.
//! Since `ruby-prism` hands us a real, safe, in-process `Node` tree
//! directly, we lower straight from `ruby_prism::Node` into this typed
//! arena: one step instead of two, and no string-keyed dynamic field lookup
//! anywhere.
//!
//! Identifiers (class/method/ivar/local names) are plain `String`s here, not
//! a compact interned id -- this mirrors spinel's own analyze-phase
//! representation (`SpNode`'s string fields are plain C strings too;
//! spinel's `sp_sym_intern` is a *codegen-time*, generated-*program*
//! concern, not a compiler-internal one). Interning spinelc's own
//! identifiers is a straightforward later optimization, not a spike
//! blocker.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(u32);

#[derive(Default)]
pub struct Hir {
    nodes: Vec<HirNode>,
    /// Provenance of every `require`/`require_relative`/`load` SPLICE
    /// INSTANCE grafted into this arena, in splice order (Phase 14.1) --
    /// the main file itself is NOT recorded (matching CRuby, where the main
    /// script never enters `$LOADED_FEATURES`). Deliberately per-instance,
    /// not per-canonical-file: `load` re-splices the same file fresh, and
    /// Phase 14.5's `Ruby::Box` work re-executes a file once per box, so an
    /// instance is the unit provenance must track. Nothing downstream
    /// consumes this yet -- it exists so 14.5's `BoxId` becomes a field
    /// flip on `LoadedFile` plus `(box_id, path)`-keyed dedup instead of a
    /// loader rework (see `parse::loader`).
    pub loaded_files: Vec<LoadedFile>,
    /// Workspace lib crates the compiled program must link (`--extern`),
    /// recorded when a `require` resolves into a package whose `spin.toml`
    /// declares `[native] crate = "..."` (Phase 14.3) -- "only link the
    /// crate when the feature actually fired", the reference project's own
    /// `.o`-linking rule. See `add_native_dep`.
    pub native_deps: Vec<String>,
    /// How many of the root `Program`'s leading statements came from the
    /// built-in exception prelude (`parse::EXCEPTION_PRELUDE`), set by
    /// `parse_and_lower_with` (Phase 15.1). `analyze` marks the classes
    /// those statements register as `is_bootstrap` -- the AOT analogue of
    /// CRuby's "defined before any user program runs" set, which stays
    /// visible inside every `Ruby::Box` (see `Compiler::resolve_class`'s
    /// bootstrap fallback).
    pub prelude_len: usize,
    /// How many `Ruby::Box`es the loader allocated (Phase 18) -- box ids
    /// run 1..=boxes (0 is the root program). `analyze` creates one
    /// top-level surrogate `ClassInfo` per id.
    pub boxes: u32,
}

/// One splice instance -- see `Hir::loaded_files`.
pub struct LoadedFile {
    /// Canonicalized (symlink-resolved) path, mirroring CRuby's separate
    /// realpath dedup layer (`load.c`'s `loaded_features_realpaths`).
    pub canonical: std::path::PathBuf,
    /// Index into `loaded_files` of the file whose `require`/`load`
    /// statement pulled this one in; `None` when required directly by the
    /// main file.
    pub required_from: Option<usize>,
    /// The package (Phase 14.2 `spin.toml` unit) this file belongs to:
    /// `Some(name)` when the file was resolved out of a package's roots, or
    /// pulled in via `require_relative`/`load` FROM a file already
    /// belonging to that package (attribution is inherited -- a package's
    /// internal files are part of the package). `None` for plain `-I`-root
    /// and main-file-relative files. This is the natural box-boundary
    /// candidate for Phase 14.5 (real `Ruby::Box` isolation is per
    /// require-graph subtree, and a package is exactly such a subtree).
    pub package: Option<String>,
    /// Always 0 (the root box) until Phase 14.5 -- see `Hir::loaded_files`.
    pub box_id: u32,
}

impl Hir {
    /// Records that the compiled program must link `crate_name` (a
    /// workspace lib crate named by a required package's `[native]` table,
    /// Phase 14.3) -- deduped; consumed by `compile_to_rust_with` into
    /// `CompileOutput::native_deps` and ultimately `build::
    /// build_binary_with_deps`'s extra `--extern`s.
    pub fn add_native_dep(&mut self, crate_name: &str) {
        if !self.native_deps.iter().any(|d| d == crate_name) {
            self.native_deps.push(crate_name.to_string());
        }
    }
}

impl std::ops::Index<NodeId> for Hir {
    type Output = HirNode;
    fn index(&self, id: NodeId) -> &HirNode {
        &self.nodes[id.0 as usize]
    }
}

impl std::ops::IndexMut<NodeId> for Hir {
    fn index_mut(&mut self, id: NodeId) -> &mut HirNode {
        &mut self.nodes[id.0 as usize]
    }
}

impl Hir {
    pub fn push(&mut self, node: HirNode) -> NodeId {
        self.nodes.push(node);
        NodeId((self.nodes.len() - 1) as u32)
    }

    /// Retroactively overrides an already-lowered `DefMethod`'s visibility --
    /// used by `parse::lower_class_body_statement` for `private`/`public`/
    /// `protected :name` (marking an already-lowered method by name) and the
    /// `private def name; ... end` idiom (the `def` is lowered normally
    /// first, then its visibility corrected). Panics if `id` isn't a
    /// `DefMethod` -- every call site already confirmed the node shape
    /// before calling.
    pub fn set_method_visibility(&mut self, id: NodeId, visibility: Visibility) {
        let HirNode::DefMethod { visibility: v, .. } = &mut self.nodes[id.0 as usize] else {
            panic!("set_method_visibility: node isn't a DefMethod");
        };
        *v = visibility;
    }

    /// Retroactively marks an already-lowered `DefMethod` as a CLASS method
    /// -- used by `parse::lower_class_body_statement`'s `class << self`
    /// recognizer: the nested body is lowered exactly like an ordinary class
    /// body first (so `attr_reader`/`private`/etc. inside it still work),
    /// then every resulting `def` is corrected to `is_class_method: true`
    /// (real Ruby: everything defined inside `class << self` becomes a
    /// method on the class itself, not an instance method). Panics if `id`
    /// isn't a `DefMethod` -- the caller already rejects any other statement
    /// shape appearing inside `class << self` (spike scope).
    pub fn set_method_is_class_method(&mut self, id: NodeId) {
        let HirNode::DefMethod { is_class_method, .. } = &mut self.nodes[id.0 as usize] else {
            panic!("set_method_is_class_method: node isn't a DefMethod");
        };
        *is_class_method = true;
    }

    /// A fresh, arena-wide-unique synthetic identifier, for a compiler-
    /// introduced hidden local that never appears in real Ruby source (e.g.
    /// binding a compound-assignment target's receiver/index expression to a
    /// name exactly once, so `obj.attr += 1` / `arr[i] += 1` don't
    /// double-evaluate a side-effecting receiver -- see `lower_call_operator_write`'s
    /// docs). `self.nodes.len()` strictly increases with every `push`, so
    /// calling this before pushing anything for the current desugar gives a
    /// suffix no earlier OR later desugar in the same file can ever collide
    /// with.
    pub fn gensym(&self, prefix: &str) -> String {
        format!("{prefix}{}", self.nodes.len())
    }
}

/// One element of an `ArrayLit` -- a plain value, or a `*expr` splat whose
/// contents are flattened in at runtime (its length isn't known until then,
/// so this can't just be another plain element).
pub enum ArrayElem {
    Single(NodeId),
    Splat(NodeId),
}

/// A method/block's declared parameter list -- mirrors `ParametersNode`'s own
/// grouping directly (Ruby's grammar already enforces required->optional->
/// rest->post->keyword->keyword_rest->block ordering, so grouping by kind
/// loses nothing, and it maps 1:1 onto what `parse/mod.rs::lower_params`
/// reads off `ruby_prism::ParametersNode`).
///
/// `default_ids()` below is the one place `analyze::collect_ivars`/
/// `analyze::locals::track_extra`/`codegen::hoisting`'s per-method scans walk
/// INTO a default-value expression for `@ivar`/local references -- otherwise
/// a default that reads an ivar/local nowhere else referenced (e.g. `def
/// f(x: @only_here)`) could hit a "no such field" codegen error instead of
/// working.
#[derive(Clone, Default)]
pub struct Params {
    pub required: Vec<String>,
    /// Parenthesized DESTRUCTURING params (`|a, (b, c), d|`, `|(a, *r)|`,
    /// nested `|(a, (b, c))|`). Ruby lets any positional slot be a
    /// parenthesized target list that splits the value bound to it, which is
    /// exactly a multi-assignment of that slot -- so it is lowered as one
    /// rather than given its own binding machinery.
    ///
    /// `lower_params` names each such slot internally (`__destr_<i>`, which
    /// is what lands in `required`/`post`, keeping every arity rule --
    /// counting, auto-splat, `Proc#arity` -- working on plain names), and
    /// records here the `LocalRead` of that slot plus the target group to
    /// split it into. The two param-binding sites (`emit_prologue` for
    /// methods, `emit_proc_param_bindings` for blocks) replay these through
    /// the ordinary `emit_multi_write` immediately after binding.
    pub destructures: Vec<(NodeId, MultiTargetGroup)>,
    /// Each default value expression is evaluated LAZILY -- only when its
    /// argument is actually omitted at the call site -- so codegen emits it
    /// inside the callee's own prologue, never eagerly at every call site.
    pub optional: Vec<(String, NodeId)>,
    /// `None`: no `*` at all. `Some(None)`: an anonymous `*` (collects and
    /// discards the extra positional args). `Some(Some(name))`: `*name`.
    pub rest: Option<Option<String>>,
    /// Required params that appear AFTER a splat (`def f(a, *b, c)` -- `c`
    /// is a `post`; real Ruby allows this, and it's a distinct binding rule
    /// from `required` since its position is anchored from the END of the
    /// argument list, not the start).
    pub post: Vec<String>,
    pub keywords: Vec<KeywordParam>,
    /// Same `None`/`Some(None)`/`Some(Some(name))` shape as `rest`.
    pub keyword_rest: Option<Option<String>>,
    /// `&blk` / anonymous `&` -- same `None`/`Some(None)`/`Some(Some(name))`
    /// shape as `rest`/`keyword_rest` again. Bound to `Nil` when the method
    /// is called with no block (real Ruby: an unyielded `&blk` is `nil`, not
    /// absent) -- see `codegen::params::emit_prologue`. Bare `...`
    /// forwarding (which implies a block too, among other things) is still a
    /// clean lowering error -- see `parse/mod.rs::lower_params`'s docs.
    pub block: Option<Option<String>>,
    /// BLOCK-LOCAL declarations -- the names after the `;` in `|x; sum|`.
    /// Always empty for a method's `Params` (the syntax exists only on a
    /// block).
    ///
    /// Not parameters: nothing is ever bound to them from the argument
    /// list, so they take no signature slot and count toward no arity rule.
    /// They are fresh locals scoped to the block, re-initialized to `nil` on
    /// EVERY invocation -- which is the part that makes them more than a
    /// naming convention, and is oracle-verified:
    ///
    /// ```ruby
    /// total = 42
    /// [1, 2, 3].each { |x; total| total = (total || 0) + x }
    /// total  # => 42 -- never written, AND never accumulated:
    ///        #    `total` is nil again at the top of each call
    /// ```
    ///
    /// They ARE in `bound_names`, though, which is what makes them shadow an
    /// enclosing local correctly: that one enumeration is what
    /// `captures::own_param_names` (capture classification), `Ctx::in_proc`
    /// (static-type/cell shadowing) and hoisting all read.
    pub block_locals: Vec<String>,
}

/// A method's visibility, as of the point in the class body where its `def`
/// was lowered (`private`/`public`/`protected` with no arguments switches the
/// DEFAULT for every subsequent `def` in the same class body -- see
/// `parse::lower_class_body`'s docs) or set retroactively by a same-named
/// `private`/`public`/`protected :name` / `private def name; ... end` form.
/// Enforced at `codegen::call::dispatch`'s Path 1 site and `spinel_rt::send`'s
/// Path 2 dispatch -- see their docs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Visibility {
    #[default]
    Public,
    Private,
    /// Callable with an explicit receiver only from within a method whose
    /// OWN receiver class is ancestor-related to the method's defining
    /// class (real Ruby's actual rule -- e.g. `def ==(other); x == other.x;
    /// end` calling a `protected` `x` on `other`, another instance of the
    /// same class).
    Protected,
}

#[derive(Clone)]
pub enum KeywordParam {
    Required(String),
    /// Same lazy-default-evaluation contract as `Params::optional`.
    Optional(String, NodeId),
}

impl Params {
    /// Every default-value expression this `Params` declares (positional
    /// `optional` + keyword-optional) -- the one place all three per-method
    /// scans (ivar collection, local-type tracking, hoisting's local
    /// collection) need to additionally walk into, alongside the method's
    /// own body, since a default can reference `@ivar`s/locals exactly like
    /// an ordinary statement can (see this struct's docs).
    pub fn default_ids(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self.optional.iter().map(|(_, d)| *d).collect();
        for kw in &self.keywords {
            if let KeywordParam::Optional(_, d) = kw {
                ids.push(*d);
            }
        }
        ids
    }

    /// Every NAME this `Params` binds in the method's scope (required,
    /// optional, named rest/kwrest/block, post, keywords) -- what
    /// `codegen::hoisting` consults so a REASSIGNED parameter is rebound
    /// from its already-bound value (`let mut x = x;`) instead of shadowed
    /// by the nil-defaulted hoisting declaration (Phase 15.2: pre-existing
    /// silent wrongness surfaced by bare-`super` forwarding, where
    /// `def f(name); name = name.upcase; super; end` must forward the
    /// reassigned value).
    pub fn bound_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.required.clone();
        names.extend(self.optional.iter().map(|(n, _)| n.clone()));
        if let Some(Some(n)) = &self.rest {
            names.push(n.clone());
        }
        names.extend(self.post.iter().cloned());
        for kw in &self.keywords {
            match kw {
                KeywordParam::Required(n) | KeywordParam::Optional(n, _) => names.push(n.clone()),
            }
        }
        if let Some(Some(n)) = &self.keyword_rest {
            names.push(n.clone());
        }
        if let Some(Some(n)) = &self.block {
            names.push(n.clone());
        }
        // The names a destructuring param binds are nested inside its target
        // group, not in `required` (which holds only the internal slot name)
        // -- but they are every bit as much parameters of this scope, so
        // hoisting has to declare them and capture analysis has to treat
        // them as the block's OWN names rather than enclosing-scope captures.
        names.extend(self.destructured_names());
        // Block-locals (`|x; sum|`) bind nothing from the argument list, but
        // they are unambiguously this block's OWN names -- which is the
        // question every `bound_names` caller is actually asking. Including
        // them here is what makes them shadow an enclosing `sum` instead of
        // being classified as a capture of it. See the field's docs.
        names.extend(self.block_locals.iter().cloned());
        names
    }

    /// Just the names bound INSIDE destructuring params (`b`/`c` of
    /// `|a, (b, c)|`) -- the part of `bound_names` that the Rust fn signature
    /// does NOT bind, since only the `__destr_<i>` slot has a signature
    /// parameter. Callers that mean "the names arriving as real Rust
    /// parameters" want `bound_names` minus this.
    pub fn destructured_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for (_, group) in &self.destructures {
            group.collect_local_names(&mut names);
        }
        names
    }
}

/// One element of a keyword-argument list / `{ }` literal, in SOURCE ORDER --
/// the keyword-position analogue of [`ArrayElem`]. `Pair` is a literal
/// `k: v` / `k => v`; `DoubleSplat` is a `**expr` whose runtime hash is merged
/// in AT THIS POSITION (its keys aren't known until runtime). Ordering is
/// observable because Ruby's Hash is insertion-ordered, so `f(**a, c: 1)` and
/// `f(c: 1, **a)` build different hashes -- which is why a call's keywords
/// can't be split into a separate `pairs` list and `**` slot. One type serves
/// `Call`, `New`, `HashLit`, and (via a trailing `HashLit`) `Yield`.
pub enum KwArg {
    Pair(NodeId, NodeId),
    DoubleSplat(NodeId),
}

impl KwArg {
    /// The child node ids this element references, for HIR walkers (a `Pair`'s
    /// key+value, or a `DoubleSplat`'s single expression) -- so every pass can
    /// visit a `kwargs` list uniformly without re-matching the variant.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> {
        let (a, b) = match *self {
            KwArg::Pair(k, v) => (k, Some(v)),
            KwArg::DoubleSplat(n) => (n, None),
        };
        std::iter::once(a).chain(b)
    }
}

/// Which last-match special a `LastMatchRef` reads -- see that variant's
/// docs. All of them derive from the one `$~` slot.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LastMatch {
    /// `$~` -- the MatchData itself.
    Data,
    /// `$1`..`$9` (and `$&`, which is group 0).
    Group(usize),
    /// `` $` `` -- the text before the match.
    Pre,
    /// `$'` -- the text after it.
    Post,
}

/// A `case/in` pattern -- a small, directly-recursive tree, deliberately NOT
/// reusing `HirNode`/`NodeId` for every position: a pattern's leaves have
/// fundamentally different semantics than an expression's (a bare
/// identifier BINDS in pattern position, it doesn't READ), so giving it its
/// own type avoids overloading `HirNode::LocalRead`/`LocalWrite` with a
/// meaning they don't otherwise have. Literal sub-expressions (a pinned
/// value, a range endpoint, a plain literal/expression matched via `===`)
/// still lower through the ordinary `NodeId`/`lower_node` path -- only the
/// PATTERN SHAPE itself is bespoke.
pub enum Pattern {
    /// A bare identifier (`x`, `_`, `_foo`) -- always matches, binding the
    /// scrutinee to this name in the enclosing method scope (exactly like an
    /// `if`/`case`-branch local -- no new Ruby scope is introduced).
    Bind(String),
    /// A literal or arbitrary expression, matched via `RubyValue::rb_eq`
    /// against the scrutinee -- the same value-equality escape hatch
    /// `case/when`'s value matching already uses (see `HirNode::CaseWhen`'s
    /// docs), not real Ruby's fully general `#===` protocol. `nil`/`true`/
    /// `false`/`Integer`/`Symbol`/`String` literals and any other plain
    /// expression all fall here.
    Value(NodeId),
    /// `^x` / `^(expr)` -- matched via `RubyValue::rb_eq` against the
    /// ALREADY-BOUND value `x`/`expr` evaluates to (never introduces a new
    /// binding, unlike `Bind`).
    Pin(NodeId),
    /// A bare constant used as a pattern with no capture (`in Integer`,
    /// `in SomeClass`) -- an `is_a?`-style ancestry/tag check with no
    /// binding. See `codegen::patterns::emit_class_check`'s docs for how
    /// this resolves both built-in primitive names (`Integer`/`String`/
    /// `Symbol`/`Array`/`Hash`/`Range`/`Proc`/`NilClass`/`TrueClass`/
    /// `FalseClass`, checked via a runtime tag) and user-defined classes
    /// (checked via the same linearized `ancestors` list `is_a?`/`super`
    /// already consult).
    ClassCheck(String),
    /// `1..10` / `..5` / `1..` as a pattern -- "does the scrutinee fall
    /// inside this range", not `rb_eq`. Scope-cut, matching `codegen::loops`'
    /// existing `for`-in-`Range` restriction: only a statically/dynamically
    /// `Int`-valued scrutinee is supported (checked via `as_int_unchecked`,
    /// not a general `Comparable`-based `#cover?`).
    Range {
        start: Option<NodeId>,
        end: Option<NodeId>,
        exclusive: bool,
    },
    /// `P1 | P2 | ...` -- matches if ANY alternative matches. Real Ruby
    /// forbids any alternative from binding a variable (there's no single
    /// consistent binding to expose otherwise) -- enforced at LOWERING time
    /// (a clean rejection, not silently dropped bindings; see
    /// `parse/mod.rs::lower_pattern`'s validation), so by the time this
    /// reaches `codegen` every nested `Pattern` here is guaranteed
    /// binding-free.
    Or(Vec<Pattern>),
    /// `PAT => name` -- binds `name` to the scrutinee ONLY IF `PAT` itself
    /// matches (unlike a bare `Bind`, which always matches). The common,
    /// load-bearing shape is `Capture(Box::new(ClassCheck(_)), name)`
    /// (`in Integer => n`), which `codegen::patterns::collect_narrowing`
    /// recognizes specially to statically narrow `name`'s inferred type for
    /// the rest of the matching arm's body (see that function's docs for
    /// the deliberate, safety-motivated scope-cut: only built-in, non-
    /// `Object` types are narrowed this way).
    Capture(Box<Pattern>, String),
    /// `[pre.., *rest, post..]` (a `constant` guard, e.g. `Point[x, y]`, is
    /// optional). `rest`: `None` = no `*` at all (exact-length match);
    /// `Some(None)` = an anonymous `*` (discards the middle slice);
    /// `Some(Some(name))` = `*name` binds the middle slice as a new Array.
    Array {
        constant: Option<String>,
        pre: Vec<Pattern>,
        rest: Option<Option<String>>,
        post: Vec<Pattern>,
    },
    /// `[*, mid.., *]` -- a Find pattern: `mid` must match SOME contiguous
    /// window of the scrutinee array (searched left to right, first match
    /// wins), unlike `Array`'s fixed pre/post anchoring. Real Ruby's grammar
    /// requires a splat on BOTH sides (that's the defining shape of a Find
    /// pattern), so `pre_rest`/`post_rest` are always present as a splat,
    /// only their NAME is optional (`None` = anonymous, discards that side's
    /// leftover slice).
    Find {
        constant: Option<String>,
        pre_rest: Option<String>,
        mid: Vec<Pattern>,
        post_rest: Option<String>,
    },
    /// `{key: pattern, ..., **rest}` (a `constant` guard is optional, same
    /// as `Array`). Each pair's value is `None` for the shorthand `{key:}`
    /// form (binds a local named `key` directly -- real Ruby sugar, not a
    /// distinct pattern shape), `Some(pattern)` otherwise. `rest` -- see
    /// `HashPatternRest`'s docs.
    Hash {
        constant: Option<String>,
        pairs: Vec<(String, Option<Pattern>)>,
        rest: HashPatternRest,
    },
}

/// The three shapes a hash pattern's `**` tail can take -- distinct from the
/// `Option<Option<String>>` shape `Array`/`Find`'s splats use because `**nil`
/// (explicit "no other keys allowed") has no positional-splat equivalent.
pub enum HashPatternRest {
    /// No `**` at all -- extra keys in the scrutinee are simply ignored
    /// (real Ruby's default hash-pattern leniency).
    None,
    /// `**rest` / anonymous `**` -- binds the leftover key/value pairs (not
    /// matched by any explicit `pairs` entry) as a new Hash when named.
    Rest(Option<String>),
    /// `**nil` -- the scrutinee must have EXACTLY the declared keys, no more.
    NoMoreKeys,
}

impl Pattern {
    /// Every `NodeId` directly embedded in this pattern (a pinned/value
    /// expression, a range endpoint) -- NOT the pattern's own guard/arm body
    /// (those live on `PatternArm`, a sibling concern). Shared by every
    /// exhaustive-match site that needs to walk sub-expressions nested
    /// inside a pattern (ivar collection, bare-`yield`/`block_given?`
    /// scanning, local-type tracking, capture analysis) so that traversal
    /// logic lives in exactly one place.
    pub fn for_each_node(&self, visit: &mut impl FnMut(NodeId)) {
        match self {
            Pattern::Bind(_) | Pattern::ClassCheck(_) => {}
            Pattern::Value(n) | Pattern::Pin(n) => visit(*n),
            Pattern::Range { start, end, .. } => {
                if let Some(n) = start {
                    visit(*n);
                }
                if let Some(n) = end {
                    visit(*n);
                }
            }
            Pattern::Or(pats) => {
                for p in pats {
                    p.for_each_node(visit);
                }
            }
            Pattern::Capture(inner, _) => inner.for_each_node(visit),
            Pattern::Array { pre, post, .. } => {
                for p in pre.iter().chain(post) {
                    p.for_each_node(visit);
                }
            }
            Pattern::Find { mid, .. } => {
                for p in mid {
                    p.for_each_node(visit);
                }
            }
            Pattern::Hash { pairs, .. } => {
                for (_, p) in pairs {
                    if let Some(p) = p {
                        p.for_each_node(visit);
                    }
                }
            }
        }
    }

    /// Every local-variable name this pattern binds if it matches -- these
    /// leak into the enclosing METHOD scope exactly like an `if`/`case`
    /// branch's locals do (no new Ruby scope), so `codegen::hoisting`'s
    /// whole-scope local collection needs to see every one of them up
    /// front, same as `HirNode::MultiWrite`'s targets. Shared by that
    /// collection pass and `codegen::patterns::collect_narrowing`.
    pub fn for_each_bound_name(&self, visit: &mut impl FnMut(&str)) {
        match self {
            Pattern::Bind(name) => visit(name),
            Pattern::Value(_) | Pattern::Pin(_) | Pattern::ClassCheck(_) | Pattern::Range { .. } => {}
            Pattern::Or(pats) => {
                // No-op in practice -- lowering rejects any binding pattern
                // inside `|` -- but walking is harmless and keeps this
                // function a total, structural traversal rather than
                // silently assuming the invariant holds.
                for p in pats {
                    p.for_each_bound_name(visit);
                }
            }
            Pattern::Capture(inner, name) => {
                inner.for_each_bound_name(visit);
                visit(name);
            }
            Pattern::Array { pre, rest, post, .. } => {
                for p in pre.iter().chain(post) {
                    p.for_each_bound_name(visit);
                }
                if let Some(Some(name)) = rest {
                    visit(name);
                }
            }
            Pattern::Find {
                pre_rest,
                mid,
                post_rest,
                ..
            } => {
                if let Some(name) = pre_rest {
                    visit(name);
                }
                for p in mid {
                    p.for_each_bound_name(visit);
                }
                if let Some(name) = post_rest {
                    visit(name);
                }
            }
            Pattern::Hash { pairs, rest, .. } => {
                for (key, p) in pairs {
                    match p {
                        Some(p) => p.for_each_bound_name(visit),
                        // The `{key:}` shorthand binds a local named `key`
                        // directly -- see `Pattern::Hash`'s docs.
                        None => visit(key),
                    }
                }
                if let HashPatternRest::Rest(Some(name)) = rest {
                    visit(name);
                }
            }
        }
    }
}

/// One `in PATTERN [if/unless GUARD]` arm of a `case/in`.
pub struct PatternArm {
    pub pattern: Pattern,
    /// `(condition, is_unless)` -- `unless` negates the same way `HirNode::While`'s
    /// `negate` flag does, rather than being a separate boolean-inverted
    /// node kind.
    pub guard: Option<(NodeId, bool)>,
    pub body: Vec<NodeId>,
}

/// A single multi-assignment target slot (`a, b = ...`'s `a`/`b`, or a
/// nested `(a, b), c = ...`'s `(a, b)`) -- generalizes the Phase-4-era
/// plain-local-only shape to every real Ruby assignment target kind, mirrored
/// directly from `MultiWriteNode`/`MultiTargetNode`'s own recursive
/// `lefts`/`rest`/`rights` grammar (see `MultiTargetGroup`). `Call` covers
/// BOTH `obj.attr = ...` and `arr[i] = ...` targets uniformly: both are
/// ordinary Ruby method calls (`attr=`/`[]=`), so lowering PRE-BUILDS the
/// actual write `Call` node (`write_call`) with a synthetic hidden local
/// (`tmp_name`) standing in for "the value this target will receive" as its
/// final argument -- codegen only has to bind `tmp_name` to the runtime-
/// destructured value BEFORE emitting `write_call` via the ordinary
/// `emit_expr` path, reusing the EXACT same static/dynamic dispatch
/// `codegen::call::dispatch` already provides for every other call, with no
/// bespoke attr/index-write codegen of its own. See `parse::lower_multi_target`.
// `Clone` because `Params` is `Clone` and now carries destructuring groups
// (`Params::destructures`); the targets themselves are small, owned data.
#[derive(Clone)]
pub enum MultiTarget {
    Local(String),
    Ivar(String),
    ClassVar(String),
    Global(String),
    /// A bare (lexically-scoped) constant target -- see `ConstWrite`'s docs
    /// for the same `scope: None` resolution rule.
    Const(String),
    /// An explicit `Foo::BAR` target -- `scope` resolves like
    /// `ConstWrite`'s `Some(class_name)`.
    ScopedConst { scope: String, name: String },
    /// `obj.attr = tmp_name` / `arr[i] = tmp_name` -- see this enum's own
    /// docs above.
    Call { write_call: NodeId, tmp_name: String },
    /// `(a, b)` -- a nested destructuring group; the value distributed to
    /// this slot is itself further split via `spinel_rt::multi_assign`,
    /// recursively.
    Nested(MultiTargetGroup),
}

/// The `before`/`splat`/`after` shape EVERY multi-assignment target list
/// has, at every nesting level (Ruby's own grammar allows at most one splat
/// per group, anchoring plain targets before/after it) -- shared by the
/// top-level `HirNode::MultiWrite`, a `MultiTarget::Nested` group, and a
/// multi-target `for a, b in ...`'s own index (`HirNode::For`). `splat:
/// None` = no `*` at all; `Some(None)` = an anonymous `*` (discards the
/// middle slice -- still unsupported, matching the pre-existing plain-local
/// restriction, a clean lowering error); `Some(Some(target))` = `*target`.
#[derive(Clone)]
pub struct MultiTargetGroup {
    pub before: Vec<MultiTarget>,
    pub splat: Option<Option<Box<MultiTarget>>>,
    pub after: Vec<MultiTarget>,
}

impl MultiTargetGroup {
    /// Every plain LOCAL name this group binds, at any nesting depth,
    /// appended to `out`. Only locals: an ivar/global/constant/attr-write
    /// target isn't a name the enclosing scope binds, so it is irrelevant to
    /// the callers (`Params::bound_names`, which answers "what does this
    /// parameter list introduce as locals?").
    pub fn collect_local_names(&self, out: &mut Vec<String>) {
        let mut visit = |t: &MultiTarget| match t {
            MultiTarget::Local(n) => out.push(n.clone()),
            MultiTarget::Nested(g) => g.collect_local_names(out),
            _ => {}
        };
        self.before.iter().for_each(&mut visit);
        if let Some(Some(t)) = &self.splat {
            visit(t);
        }
        self.after.iter().for_each(&mut visit);
    }
}

impl MultiTarget {
    /// Every `NodeId` directly embedded in this target (a `Call` target's
    /// pre-built `write_call`, or a nested group's own embedded nodes) --
    /// NOT the multi-assignment's own `value` (a sibling concern) -- mirrors
    /// `Pattern::for_each_node`'s exact shape/purpose, shared by every
    /// traversal that needs to walk sub-expressions nested inside a target
    /// (ivar/cvar collection, local-type tracking, hoisting, capture
    /// analysis).
    pub fn for_each_node(&self, visit: &mut impl FnMut(NodeId)) {
        match self {
            MultiTarget::Local(_)
            | MultiTarget::Ivar(_)
            | MultiTarget::ClassVar(_)
            | MultiTarget::Global(_)
            | MultiTarget::Const(_)
            | MultiTarget::ScopedConst { .. } => {}
            MultiTarget::Call { write_call, .. } => visit(*write_call),
            MultiTarget::Nested(group) => group.for_each_node(visit),
        }
    }

    /// Every LOCAL-like name this target binds, for hoisting/local-type-
    /// tracking purposes: a plain `Local`, or a `Call` target's own hidden
    /// `tmp_name` synthetic local (bound once per multi-assignment, read
    /// back inside `write_call` -- see `MultiTarget::Call`'s docs). Ivar/
    /// cvar/global/const targets have their own, separate storage and don't
    /// participate in the enclosing scope's LOCAL-variable bookkeeping at
    /// all, so they're excluded here (mirrors `Pattern::for_each_bound_name`'s
    /// same "leaks into the enclosing scope" contract, narrowed to only the
    /// target kinds that actually use local-variable storage).
    pub fn for_each_local_name(&self, visit: &mut impl FnMut(&str)) {
        match self {
            MultiTarget::Local(n) => visit(n),
            MultiTarget::Call { tmp_name, .. } => visit(tmp_name),
            MultiTarget::Ivar(_)
            | MultiTarget::ClassVar(_)
            | MultiTarget::Global(_)
            | MultiTarget::Const(_)
            | MultiTarget::ScopedConst { .. } => {}
            MultiTarget::Nested(group) => group.for_each_local_name(visit),
        }
    }
}

impl MultiTargetGroup {
    /// See `MultiTarget::for_each_node`'s docs.
    pub fn for_each_node(&self, visit: &mut impl FnMut(NodeId)) {
        for t in self.before.iter().chain(&self.after) {
            t.for_each_node(visit);
        }
        if let Some(Some(t)) = &self.splat {
            t.for_each_node(visit);
        }
    }

    /// See `MultiTarget::for_each_local_name`'s docs.
    pub fn for_each_local_name(&self, visit: &mut impl FnMut(&str)) {
        for t in self.before.iter().chain(&self.after) {
            t.for_each_local_name(visit);
        }
        if let Some(Some(t)) = &self.splat {
            t.for_each_local_name(visit);
        }
    }
}

/// One `rescue [classes] [=> binding] ... end` clause of a `begin`/an
/// implicit method-body rescue. `classes` empty = a bare `rescue` -- matches
/// `StandardError` and its descendants (real Ruby's own default), NOT
/// literally every `Exception` -- see `codegen::exceptions`'s docs for the
/// matching codegen. `binding`'s name leaks into the enclosing METHOD scope
/// exactly like a `case/in` pattern's bound names do (no new Ruby scope) --
/// `codegen::hoisting`'s whole-scope local collection needs to see it up
/// front, same treatment as `Pattern::for_each_bound_name`'s callers.
pub struct RescueClause {
    pub classes: Vec<String>,
    pub binding: Option<String>,
    pub body: Vec<NodeId>,
}

/// One part of a (possibly-interpolated) string literal. A plain `"..."`
/// with no `#{}` lowers to a single `Lit` part. Only a single bare expression
/// is supported inside `#{}` (mirrors `ParenthesesNode`'s single-statement
/// restriction) -- a multi-statement interpolation body is a clean lowering
/// error, not silently truncated to its last statement.
pub enum StrPart {
    Lit(String),
    Interp(NodeId),
}

/// A `/pattern/flags` / `%r{pattern}flags` literal's option letters --
/// `ruby-prism`'s `RegularExpressionNode`/`InterpolatedRegularExpressionNode`
/// expose several more (`o`/`e`/`n`/`s`/`u`, all encoding/interpolation-once
/// concerns), but this spike is UTF-8-only throughout (see
/// `docs/limitations.md`'s existing posture on strings), so only the three
/// letters that change actual MATCHING semantics are modeled; the rest are
/// silently accepted as no-ops except a genuinely non-UTF-8-forcing encoding
/// flag (`e`/`s`), which is a clean lowering rejection (see
/// `parse/mod.rs`'s recognizer).
#[derive(Clone, Copy, Default)]
pub struct RegexpFlags {
    /// `i` -- case-insensitive matching.
    pub ignore_case: bool,
    /// `x` -- ignore unescaped whitespace and `#` comments in the pattern
    /// source (`regex`'s `ignore_whitespace`).
    pub extended: bool,
    /// `m` -- real Ruby's `/m` makes `.` match a newline too (`regex`'s
    /// `dot_matches_new_line`) -- NOT the same thing as most other regex
    /// flavors' "multi-line mode" (`regex`'s own `multi_line`, which
    /// governs `^`/`$`). Ruby's `^`/`$` ALWAYS match at line boundaries by
    /// default, with no opt-in flag at all -- see
    /// `spinel_rt::regexp::regexp_new`'s docs for how this is modeled
    /// (`multi_line(true)` is unconditional, independent of this field).
    pub multiline: bool,
}

/// A small, real enum instead of spinel's ~115 string-typed `SP_NODE_KINDS`
/// that every pass has to `sp_streq` against. Sized to exactly what the
/// spike's 7 examples need; growing it is additive (new variants), matching
/// spinel's own incremental node-kind coverage.
pub enum HirNode {
    Program(Vec<NodeId>),
    IntegerLit(i64),
    /// An Integer literal beyond i64 (Phase 17.1's bignum) -- carried as
    /// prism's own `(negative, LSB-first u32 digits)` shape so spinelc
    /// needs no bigint dependency; codegen emits
    /// `spinel_rt::int_from_u32_digits`. Types as `Int` like `IntegerLit`
    /// (one Ruby Integer class, two payloads).
    BigIntegerLit { negative: bool, digits: Vec<u32> },
    /// `3r` / `1.5r` (Phase 17.1) -- prism pre-rationalizes the decimal
    /// forms (`1.5r` arrives as numerator 3, denominator 2), so both
    /// components travel as digit strings like `BigIntegerLit`. The
    /// denominator is positive and non-zero by syntax.
    RationalLit {
        negative: bool,
        num_digits: Vec<u32>,
        den_digits: Vec<u32>,
    },
    /// `4i` / `2.0i` / `3ri` (Phase 17.1) -- an imaginary literal wrapping
    /// its lowered inner numeric literal compositionally
    /// (`Complex(0, inner)`).
    ImaginaryLit(NodeId),
    /// A real `f64` payload -- `HirNode` itself derives no `Eq`/`Hash` (see
    /// this enum's own docs), so an un-`Eq`-able float here is no different
    /// from `IntegerLit`'s `i64` in that respect.
    FloatLit(f64),
    SymbolLit(String),
    /// `nil` -- previously unrepresentable (no example needed it before
    /// Phase 8), but `case/in`'s `Pattern::Value` fallback needs `in nil` to
    /// lower through the ordinary expression path like any other literal,
    /// so this closes a genuine, narrow, pre-existing gap rather than
    /// special-casing pattern lowering around it.
    NilLit,
    /// `true` / `false` -- see `NilLit`'s docs; same motivating gap.
    BoolLit(bool),
    /// `a && b` / `a and b` -- prism normalizes both spellings to the same
    /// node (only precedence differs, already resolved by parse time).
    /// Short-circuits like Ruby's real `&&`, not like Rust's bool-typed
    /// `&&`: the *operand itself* is returned (`a` if falsy, else `b`), so
    /// codegen can't emit a literal Rust `&&` here (see `expr.rs`).
    And(NodeId, NodeId),
    /// `a || b` / `a or b` -- see `And`'s docs; same short-circuit-the-
    /// operand-not-a-bool semantics.
    Or(NodeId, NodeId),
    /// `defined?(expr)` -- a compile-time-resolvable classification of
    /// `expr`'s syntactic form (mirrors CRuby's `"expression"`/`"method"`/
    /// `"local-variable"`/`"instance-variable"`/`nil` results), not a
    /// runtime check. See `codegen::expr::emit_defined`'s docs for the
    /// scope-cut this approximates.
    Defined(NodeId),
    /// `if`/`unless`/`elsif`/ternary all normalize to this at lowering time
    /// (`unless` swaps `then_body`/`else_body`; `elsif` is prism's own
    /// `IfNode::subsequent()` recursion, which lowering walks into a nested
    /// `If`; ternary is literally the same `IfNode` shape prism produces for
    /// `a ? b : c`). Ruby's implicit-last-expression-return and Rust's
    /// `if`-as-expression are structurally identical, so this maps directly
    /// onto a Rust `if/else` expression in tail position (see `codegen::expr`).
    If {
        cond: NodeId,
        then_body: Vec<NodeId>,
        else_body: Vec<NodeId>,
    },
    /// `case subject; when v1, v2 then ...; else ...; end` -- value
    /// matching only (no subject means each `when` value is itself the
    /// boolean condition, like a chained `if`/`elsif`). `case/in` pattern
    /// matching is a distinct prism node (`CaseMatchNode`) and isn't
    /// lowered to this -- see the plan's Phase 8.
    CaseWhen {
        subject: Option<NodeId>,
        /// Each arm's `(conditions, body)`. The conditions are
        /// `ArrayElem`s, the same shape a call's arguments use, so
        /// `when *candidates` needs no machinery of its own -- a `Splat`
        /// element tests EVERY element of its array, which is exactly what
        /// the `||` chain over the non-splat ones already does, just with
        /// the arity known only at runtime.
        arms: Vec<(Vec<ArrayElem>, Vec<NodeId>)>,
        else_body: Vec<NodeId>,
    },
    /// `[1, 2, *rest]` -- see `ArrayElem`'s docs for the splat handling.
    ArrayLit(Vec<ArrayElem>),
    /// `{ a: 1, b: 2, **other }` -- an ordered list of pairs and double-splats
    /// (see `KwArg`). Merged left-to-right, last key wins.
    HashLit(Vec<KwArg>),
    /// `a..b` / `a...b` -- either endpoint may be absent (`a..`/`..b`),
    /// matching Ruby's beginless/endless ranges.
    RangeLit {
        start: Option<NodeId>,
        end: Option<NodeId>,
        exclusive: bool,
    },
    /// A (possibly-interpolated) string literal -- see `StrPart`'s docs.
    StringLit(Vec<StrPart>),
    /// `/pattern/flags` / `%r{pattern}flags`, possibly interpolated -- reuses
    /// `StrPart` wholesale (a regex literal's `#{}` interpolation is
    /// structurally identical to a string's, see `parse/mod.rs`'s recognizer,
    /// which shares `lower_string_part`). A regex compile failure (either a
    /// static, unconditionally-invalid pattern, or a runtime-only failure
    /// once an interpolated part is substituted in) raises a real, catchable
    /// `RegexpError` at codegen's construction site -- NOT rejected any
    /// earlier at `spinelc` compile time, unlike real Ruby's own parse-time
    /// `SyntaxError` for a static pattern: a documented, narrower-timing
    /// approximation (see `codegen::collections::emit_regexp_lit`'s docs),
    /// not silent wrongness. Backed by the `regex` crate, not Ruby's own
    /// Onigmo engine -- no backreferences (`\1` inside the PATTERN itself,
    /// as opposed to a `gsub`/`sub` REPLACEMENT string, where they *are*
    /// supported -- see `spinel_rt::regexp`'s docs) and no lookaround
    /// (`(?=...)`/`(?!...)`/`(?<=...)`/`(?<!...)`), a real, documented
    /// semantic gap versus real Ruby, not an oversight.
    RegexpLit(Vec<StrPart>, RegexpFlags),
    LocalRead(String),
    LocalWrite(String, NodeId),
    IvarRead(String),
    IvarWrite(String, NodeId),
    /// `@@x` read/write. Ownership (which class/module's storage this
    /// actually refers to) is resolved once at `analyze` time by walking the
    /// referencing class's `ancestors` for an existing owner -- see
    /// `compiler::ClassInfo::cvar_owners`'s docs -- not re-resolved at
    /// codegen time, so this node just carries the bare name; codegen
    /// consults the already-resolved owner via `Ctx.current_class`.
    ClassVarRead(String),
    ClassVarWrite(String, NodeId),
    /// A bare constant used as a VALUE (currently only meaningful as a call
    /// receiver: `ClassName.foo`/`ModuleName.foo`) -- distinct from
    /// `ClassName.new(...)` (still its own `New` node) and from a
    /// superclass/include/extend/prepend target name (those stay plain
    /// `String`s, resolved directly, never wrapped in this). Resolves to
    /// "the class/module object named X" for dispatch purposes only; there
    /// is no first-class runtime `Class`/`Module` VALUE (can't be stored in
    /// a variable, compared, or reflected on at runtime) -- see the plan's
    /// Part 6 "Explicit scope-cut" on this point.
    ClassRef(String),
    /// A literal-name-resolvable call: `recv.name(args) { block }`, or an
    /// implicit-self call (`receiver: None`). `send`/`public_send` are NOT a
    /// separate node kind (mirroring prism/spinel: they're just calls named
    /// "send") -- codegen inspects `name` and, for `send`, `args[0]` to
    /// decide Path 1 (static) vs. Path 2 (`spinel_rt::send`). `safe: true`
    /// is `&.` (`recv.is_safe_navigation()` in prism -- a flag on the same
    /// `CallNode`, not a separate node kind): the call short-circuits to
    /// `nil` without evaluating at all when `receiver` is `nil` at runtime.
    /// `kwargs` are the call site's own `name: value` pairs (`foo(x: 1)`) --
    /// a distinct `KeywordHashNode` prism peels off the tail of the ordinary
    /// argument list at lowering time (see `parse/mod.rs`), resolved by NAME
    /// against the callee's declared keyword params at codegen time (the
    /// callee's `Params` is always statically known at a Path 1 call site).
    /// `args` reuses `ArrayElem` (a plain positional value, or a `*expr`
    /// splat whose contents are flattened in at runtime -- see
    /// `ArrayElem`'s docs and `codegen::params::emit_call_args`'s Path 1
    /// splat-flattening); `kwargs` is the ordered `KwArg` list (literal
    /// `name: value` pairs INTERLEAVED with `**h` double-splats in source
    /// order -- Ruby's Hash is insertion-ordered and the merge is
    /// left-to-right last-one-wins, so the order is observable). A `kwargs`
    /// containing any `DoubleSplat` routes to the runtime arg-vector path;
    /// a pairs-only `kwargs` stays on the static Path 1 fast path.
    /// `block_arg` is `foo(&existing_proc)` --
    /// forwarding an already-built `Proc` value as the call's block (a
    /// distinct `BlockArgumentNode`), separate from `block` (a literal `{
    /// }`/`do..end` at the call site); real Ruby rejects having both on the
    /// same call, which this spike doesn't separately re-validate (whichever
    /// lowers last silently wins -- harmless, since `ruby-prism` itself
    /// already rejects this at parse time before lowering ever runs).
    Call {
        receiver: Option<NodeId>,
        name: String,
        args: Vec<ArrayElem>,
        kwargs: Vec<KwArg>,
        block: Option<NodeId>,
        block_arg: Option<NodeId>,
        safe: bool,
    },
    /// `ClassName.new(args)` -- a distinct node (not a plain `Call`) because
    /// it's always statically resolvable to a concrete class, and codegen
    /// needs that class name early to route ivar defaults / registration.
    ///
    /// `kwargs` carries a trailing `Foo.new(k: 1)`, kept SEPARATE from
    /// `args` the way `Call`'s own are, so `initialize`'s keyword
    /// parameters bind as keywords. It used to lower to a positional Hash
    /// instead, which bound to the wrong slot (or, more often, made the
    /// call look one argument too long: `def initialize(a, k:)` given
    /// `New.new(1, k: 2)` saw two positionals and raised).
    ///
    /// `args` stays `Vec<NodeId>` rather than `Call`'s `Vec<ArrayElem>`: a
    /// SPLAT at a `.new` site (`Foo.new(*args)`) is still unsupported -- it
    /// needs the runtime arg-vector path, not this static one. See
    /// `parse`'s `.new` lowering. `kwargs` is the ordered `KwArg` list (pairs
    /// plus `**h` double-splats), the same as `Call`'s.
    New {
        class_name: String,
        args: Vec<NodeId>,
        kwargs: Vec<KwArg>,
    },
    /// Mirrors spinel's `emit_super`: always resolved against the *static*
    /// superclass, never through the dynamic dispatch table. See codegen's
    /// handling -- the spike implements this via statement inlining rather
    /// than a cross-type function call (see docs/PORTING_ANALYSIS.md).
    ///
    /// `zsuper` distinguishes real Ruby's two zero-written-argument shapes,
    /// which mean OPPOSITE things: bare `super` (prism's
    /// `ForwardingSuperNode`, `zsuper: true`) forwards the current method's
    /// own parameters as currently bound, while `super()` (a `SuperNode`
    /// with no arguments, `zsuper: false`) passes NO arguments at all, so
    /// the parent's optionals take their defaults. `args` is always empty
    /// when `zsuper` is true.
    SuperCall {
        args: Vec<NodeId>,
        zsuper: bool,
        /// A literal block written at the `super` site (`super { ... }` /
        /// `super(x) { ... }`) -- a `HirNode::Block`, bound as the spliced
        /// parent body's `__blk` so its `yield` runs this block. `None`
        /// means the parent implicitly sees the CURRENT method's own block
        /// (real Ruby forwards it), which the splice gets for free since
        /// `__blk` is already in scope there.
        block: Option<NodeId>,
    },
    Block {
        params: Params,
        body: Vec<NodeId>,
    },
    /// `-> (x) { ... }` / `lambda { ... }` -- a STANDALONE expression
    /// producing a real `RubyValue::Proc`, unlike `Block` (only ever reached
    /// via the `Call` that invokes it, see that variant's docs). Reuses
    /// `codegen::call::emit_proc_value`'s whole construction machinery
    /// (captures, redo-wrapper loop) via a shared helper, differing in
    /// exactly two ways real Ruby's own lambda semantics require: STRICT
    /// arity checking (raises `ArgumentError`, not a lenient nil-fill/drop),
    /// and `return`/`break` inside the body terminate the LAMBDA CALL
    /// itself (folded into a normal `Ok` return, like a method boundary)
    /// rather than propagating to the enclosing method/loop.
    Lambda {
        params: Params,
        body: Vec<NodeId>,
    },
    /// `class Name < Super ... end` / `module Name ... end` -- `is_module`
    /// distinguishes the two: a module has no `superclass` (always `None`)
    /// and is never instantiated (no `Name.new`, no generated Rust struct --
    /// see `codegen::mod::emit_class`'s docs). Both share this one node
    /// since everything about their BODY (methods, nested `include`/
    /// `extend`/`prepend`, class variables) lowers identically; only
    /// `analyze::register_class`'s registration differs.
    ClassDef {
        name: String,
        superclass: Option<String>,
        body: Vec<NodeId>,
        is_module: bool,
    },
    /// Also the desugared form of a literal-name `define_method(:name) { .. }`
    /// -- lowering (`parse/mod.rs`) treats it identically to a plain `def`,
    /// mirroring spinel's `walk_scope`. `is_class_method` is `def self.name`
    /// (`DefNode::receiver()` is `Some(SelfNode)`) -- a genuinely different
    /// registration target (`ClassInfo::class_methods`, not `::methods`; no
    /// `self: Arc<Self>` receiver at codegen time at all, see
    /// `compiler::ClassInfo`'s docs) even though the body shape is
    /// identical. Any OTHER explicit receiver (`def SomeConst.name`) is a
    /// clean lowering rejection (spike scope -- reopening a class from
    /// outside its own body isn't supported).
    DefMethod {
        name: String,
        params: Params,
        body: Vec<NodeId>,
        is_class_method: bool,
        visibility: Visibility,
    },
    /// `include Mod` -- one node per module argument, in left-to-right
    /// source order, when multiple are given (`include A, B` lowers to two
    /// sequential `Include` statements in the class body, matching real
    /// Ruby's "as if each were included one at a time, in order" rule). Only
    /// valid directly in a class/module body -- see
    /// `parse/mod.rs::lower_class_body_statement`.
    Include(String),
    /// `extend Mod` -- see `Include`'s docs; the module's OWN instance
    /// methods are materialized as CLASS methods on the extending class
    /// instead (`ClassInfo::class_methods`), not instance methods.
    Extend(String),
    /// `prepend Mod` -- see `Include`'s docs; the module is inserted BEFORE
    /// the class itself in the linearized `ancestors` list, so its methods
    /// take precedence over the class's own (reachable via `super`).
    Prepend(String),
    /// `native_crate "rust_crate_path"` -- a module-body-only declaration
    /// (Phase 14.3, the native-package DSL) naming the Rust path (crate
    /// name with underscores) whose free functions back this module's
    /// `native_func`s. Recognized by name during class-body lowering,
    /// exactly like `attr_accessor` (see `parse::lower_class_body_statement`);
    /// consumed by `analyze::register_class` into `ClassInfo::native_crate`,
    /// never reaching codegen as an expression. The crate itself is linked
    /// only when the declaring package was actually `require`d -- see
    /// `Hir::native_deps`.
    NativeCrate(String),
    /// `native_func :name, [ArgType, ...], ReturnType` -- declares one
    /// Ruby-visible module function implemented as
    /// `<native_crate>::<name>(RubyValue, ...) -> Result<RubyValue, Signal>`
    /// in the backing Rust crate. Only the ARITY is load-bearing (checked
    /// at each call site at compile time); the type constants are
    /// declaration-file documentation, matching the reference project's
    /// `native_func` DSL shape. See `NativeCrate`.
    NativeFunc { name: String, arity: usize },
    /// `while cond ... end` / `until cond ... end` (+ modifier forms
    /// `stmt while cond` / `stmt until cond`) -- `until` folds in here as
    /// `negate: true`, exactly like `unless` folds into `If` by swapping
    /// branches (see `parse/mod.rs`). Always compiles to a labeled Rust
    /// `loop { }`, never a bare Rust `while` -- even for plain `while` --
    /// so `break value` has an expression-position target to jump to
    /// (Rust's own `while` is never an expression; see `codegen::loops`).
    /// The do-while form (`begin ... end while cond`, body always runs at
    /// least once) is a distinct prism shape wrapping a `BeginNode`, which
    /// isn't lowered until Phase 9's `begin`/`rescue` -- it already falls
    /// through to the generic "unsupported syntax" error untouched, so it
    /// needs no explicit handling here.
    While {
        cond: NodeId,
        body: Vec<NodeId>,
        negate: bool,
    },
    /// `loop do ... end` -- NOT a distinct `ruby-prism` node (it's an
    /// ordinary zero-arg, no-receiver `Kernel#loop` call with a block);
    /// `parse/mod.rs` desugars that call shape to this at lowering time,
    /// mirroring `define_method`'s existing call-shape desugar. An
    /// unconditional labeled Rust `loop { }` with no exit test of its own --
    /// only `break` (or, once Phase 9 exists, an uncaught `raise`) ever ends
    /// it.
    Loop { body: Vec<NodeId> },
    /// `for var in iterable ... end` / `for a, b in pairs ... end`. Unlike
    /// block-based iteration (`each { |x| ... }`), Ruby's `for` does NOT
    /// introduce a new variable scope: `var` and any locals first assigned
    /// in the body stay visible after the loop ends -- a real semantic
    /// difference, not a spike shortcut, and one `codegen`'s plain
    /// (non-block-nested) `let` emission already gives for free. `target`
    /// reuses `MultiTarget` (a plain `for x in ...` lowers to
    /// `MultiTarget::Local`; `for a, b in ...` to `MultiTarget::Nested`),
    /// destructured against each iterated element exactly like a
    /// `MultiWrite`'s value. The loop's own expression-position value is
    /// documented as `nil` unless a `break value` fires -- real Ruby returns
    /// the iterated collection itself in the no-break case, a
    /// narrower-than-real-Ruby simplification nothing in the spike's
    /// examples depends on.
    For {
        target: MultiTarget,
        iterable: NodeId,
        body: Vec<NodeId>,
    },
    /// `break` / `break value` -- unwinds to the end of the nearest *native*
    /// loop construct (`While`/`Loop`/`For`, or the pre-existing `.times`
    /// block-inlining special case in `codegen::call`), which `ruby-prism`
    /// itself already guarantees is the only place these can appear (a bare
    /// `break`/`next`/`redo` outside any loop/block is a parse error, not
    /// something lowering has to re-validate). Compiles to a literal Rust
    /// `break 'label value;` -- no `Signal` involved, per the ABI's stated
    /// scope-cut (see `signal.rs`), since real escaping closures don't exist
    /// until Part 1.3/Phase 6. A multi-value `break a, b` lowers to a single
    /// implicit-array argument (`break [a, b]`), the same as `Return`/`Next`.
    Break(Option<NodeId>),
    /// `next` / `next value` -- ends the current iteration early, jumping to
    /// the loop's own re-test-the-condition point. See `Break`'s docs; the
    /// value is evaluated (for side effects) but otherwise discarded inside
    /// a native loop, matching real Ruby: `next value` only matters as "what
    /// the block call returns", which is meaningless for a bare `while`/
    /// `for`/`loop`.
    Next(Option<NodeId>),
    /// `redo` -- re-runs the current iteration's body from the top WITHOUT
    /// re-testing the loop condition or advancing (the one construct with no
    /// direct native Rust equivalent -- `continue` always re-tests/advances).
    /// See `codegen::loops`' inner-label trick this needs.
    Redo,
    /// `a, b = 1, 2` / `a, *b, c = arr` / `(a, b), c = ...` / `@x, $y, Z =
    /// ...` -- see `MultiTargetGroup`/`MultiTarget`'s docs for the full
    /// generalized target shape (local/ivar/cvar/global/const/attr/index/
    /// nested-group). See `spinel_rt::multi_assign`'s docs for the exact
    /// leniency rules (missing positions become `nil`; extra values are
    /// silently dropped when there's no splat to catch them).
    MultiWrite { targets: MultiTargetGroup, value: NodeId },
    /// `eval("literal ruby source")` -- ONLY the compile-time-constant-string
    /// form (see `parse/mod.rs`'s eval-call-shape recognizer). `body`'s
    /// source was parsed and lowered into THIS SAME arena at lowering time --
    /// by the time `analyze`/`codegen` ever see this node, it's ordinary
    /// already-spliced Hir, indistinguishable from code written inline at the
    /// eval call site (so local/ivar scoping "just works" -- see the
    /// recognizer's docs). Runtime (non-literal) eval, `instance_eval`/
    /// `class_eval` with dynamic content, and `binding` are NOT implemented --
    /// see docs/EVAL_VM.md for the future embedded-interpreter design those
    /// would need.
    Eval(Vec<NodeId>),
    /// A `Ruby::Box` context switch (Phase 18): the universal wrapper every
    /// box-scoped splice lowers into -- a `box.require`d file's statements,
    /// a `box.eval` body, and a `box::X` external-access expression all
    /// carry their statically-known box id here. Emits exactly like `Eval`
    /// (a brace-wrapped block whose value is the last statement's), except
    /// codegen's `Ctx.box_id` is overridden for the body -- the AOT
    /// translation of CRuby's loading-box/`cme->def->box` context.
    /// `analyze` descends top-level `BoxScope`s to register their
    /// `ClassDef`s under the box.
    BoxScope { box_id: u32, body: Vec<NodeId> },
    /// The runtime VALUE of a box handle (`box = Ruby::Box.new` binds the
    /// local to this): a `RubyValue::Class` of the box's top-level
    /// surrogate class, so `p box` prints `#<Ruby::Box:N>`, handle equality
    /// works, and storing it is harmless. The four recognized OPERATION
    /// shapes (`box.require`/`box.load`/`box.eval`/`box::X`) resolve
    /// statically through the loader's bindings map, never through this
    /// value.
    BoxHandle(u32),
    /// `return` / `return value` -- explicit early return from the enclosing
    /// method. Compiles to a literal Rust `return Ok(value);` (Rust's own
    /// early return already exits arbitrarily deep nesting -- an `if`/`case`/
    /// loop body, or a fast-inline-path block like `.times`'s, which is
    /// spliced directly into the SAME enclosing method body, so a literal
    /// Rust `return` there already has real Ruby's exact semantics: `return`
    /// inside a block always exits the enclosing method, not just the
    /// block). This stops being correct only once a block can become a
    /// genuinely separate Rust closure (a real escaping `Proc`, a later
    /// phase) with its own Rust fn boundary a bare `return` would incorrectly
    /// stop at instead of passing through -- that needs `Signal::Return`
    /// (already reserved for exactly this in `spinel_rt::Signal`) once it
    /// exists. A multi-value `return a, b` lowers to a single implicit-array
    /// argument (`return [a, b]`), via `lower_single_optional_argument`, so
    /// this stays one optional node (mirroring `Break`/`Next`).
    Return(Option<NodeId>),
    /// `yield` / `yield(args)` -- invokes the enclosing method's implicit
    /// block. Only recognized directly within a method's own control flow
    /// (if/case/while/etc.), not inside a NESTED block literal -- `yield`
    /// lexically inside a block passed elsewhere refers to a different
    /// thing in real Ruby (the block's own enclosing method, not this one),
    /// a genuinely harder case this spike doesn't attempt; see
    /// `analyze::register_class`'s `uses_bare_block` scan, which enforces
    /// this restriction with a clean rejection. Compiles to invoking the
    /// method's implicit `__blk` parameter (see `codegen::params`), panicking
    /// with a clear "no block given" message (mirroring real Ruby's
    /// `LocalJumpError`) if the method was called without one.
    ///
    /// `Vec<ArrayElem>`, the same shape `Call`'s positional arguments use,
    /// so `yield(*a)` needs no machinery of its own -- a `Splat` element
    /// flattens at runtime exactly as it does at a call site. A trailing
    /// `yield(k: 1)`/`yield(**h)` is folded by lowering into one trailing
    /// `HashLit`/merge element, which is what
    /// `codegen::params::emit_proc_param_bindings` already binds a block's
    /// own keyword params from (real Ruby's auto-conversion of a trailing
    /// Hash into block keywords).
    Yield(Vec<ArrayElem>),
    /// `block_given?` -- a zero-arg, no-receiver call-shape recognized at
    /// lowering time (mirrors `loop`/`define_method`'s desugars), not a
    /// distinct `ruby-prism` node. Same "not inside a nested block" scope-cut
    /// as `Yield`.
    BlockGiven,
    /// A bare `self` used as a VALUE (an explicit receiver, `self.foo`, or
    /// standalone, `puts self`) -- a real `ruby-prism` `SelfNode`, recognized
    /// generically (not a call-shape desugar). Only meaningful inside an
    /// ordinary instance method body (`cx.current_class` is `Some`, see
    /// `codegen::expr::infer`'s special case); a class method/module
    /// function has no backing instance to be (this spike has no first-class
    /// `Class`/`Module` runtime value -- see the plan's Part 6 scope-cut), so
    /// `codegen::expr::emit_expr`'s `SelfRef` arm rejects that case with a
    /// clear error instead of emitting a reference to a Rust `self` that
    /// doesn't exist in that generated function's signature.
    SelfRef,
    /// `raise`/`fail` (exact synonyms) -- a zero/one/two-arg call-shape
    /// recognized at lowering time, same as `BlockGiven` above (real Ruby:
    /// both are ordinary `Kernel` method calls, not syntax). Codegen
    /// classifies the arg SHAPE itself (`ClassRef` vs. a `Str`-typed
    /// expression vs. an already-constructed exception value) rather than
    /// this node encoding it structurally -- mirrors `Yield`'s "let codegen,
    /// which already has full type-inference machinery, decide" posture.
    /// Bare `raise` (re-raise, zero args) needs a currently-handled
    /// exception context that doesn't exist until `rescue` does (Phase 9) --
    /// a clean rejection until then, not a silent no-op. The `cause:`
    /// keyword-argument form isn't lowered (a documented, narrow scope-cut --
    /// automatic cause chaining from an active `rescue` will still work once
    /// Phase 9 lands; only the explicit override is deferred).
    Raise(Vec<NodeId>),
    /// `case subject; in PATTERN [if/unless GUARD] ... [else ...] end` --
    /// see `Pattern`/`PatternArm`'s docs. Arms are tested top to bottom,
    /// first match wins (same "not a native `match`" reasoning as
    /// `CaseWhen`, since a pattern's own class-check/destructure/guard logic
    /// can't be expressed as Rust structural patterns generically).
    /// `else_body: None` with no arm matching raises `NoMatchingPatternError`
    /// (see `codegen::patterns`'s docs) -- a real, distinct case from
    /// `Some(vec![])` (an explicit, empty `else` clause, which just yields
    /// `nil`).
    CaseIn {
        subject: NodeId,
        arms: Vec<PatternArm>,
        else_body: Option<Vec<NodeId>>,
    },
    /// `expr in pattern` -- a boolean one-liner: `true`/`false` for whether
    /// `pattern` matches, never raises on failure. Any binding the pattern
    /// makes on a SUCCESSFUL match still leaks into the enclosing method
    /// scope (matching real Ruby -- pattern variables behave like ordinary
    /// local assignment regardless of which branch of the enclosing code
    /// ends up reading them afterward).
    MatchPredicate { subject: NodeId, pattern: Pattern },
    /// `expr => pattern` -- the rightward-assignment one-liner: raises
    /// `NoMatchingPatternError` if `pattern` doesn't match (mirrors a
    /// `case/in` with no `else`), otherwise evaluates to `nil` (real Ruby:
    /// this form's value is never used for anything but its binding/raising
    /// side effect).
    MatchRequired { subject: NodeId, pattern: Pattern },
    /// `begin body rescue ... else ... ensure ... end` -- also the desugared
    /// form of a method body that's implicitly a `BeginNode` (a `def` with a
    /// bare `rescue`/`ensure` and no explicit `begin`/`end`, confirmed
    /// empirically via `Prism.parse`) and of `expr rescue fallback` (the
    /// modifier form, including inside an endless method) -- see
    /// `parse/mod.rs`'s recognizers, all of which produce this same shape.
    /// `rescues` are tested top to bottom, first matching clause wins; an
    /// unmatched raise propagates (a `?` at this node's own codegen site,
    /// mirroring every other fallible sub-expression -- see
    /// `codegen::exceptions::emit_begin`). `else_body: Some(_)` runs (and its
    /// value REPLACES `body`'s own) only when `body` completed with no
    /// exception, matching real Ruby. `ensure_body`, when present, always
    /// runs exactly once after everything else has settled -- including a
    /// `retry`-driven re-attempt of `body` -- never once per attempt.
    Begin {
        body: Vec<NodeId>,
        rescues: Vec<RescueClause>,
        else_body: Option<Vec<NodeId>>,
        ensure_body: Option<Vec<NodeId>>,
    },
    /// `retry` -- restarts the nearest enclosing `begin`'s own `body` from
    /// the top (an `ensure` that already ran does NOT re-run). Only valid
    /// lexically inside a `rescue` clause in real Ruby (a real, if rare,
    /// `SyntaxError` otherwise) -- this spike doesn't re-validate that
    /// positional restriction at lowering time; see
    /// `codegen::exceptions::emit_retry`'s docs for what happens to a
    /// mis-scoped one instead (an uncaught `Signal`, not silent wrongness).
    Retry,
    /// `$foo` read/write -- a flat, genuinely process-wide store (Part 9's
    /// `LazyLock<Mutex<_>>` pattern, same as `cvars`/the Symbol interner),
    /// needing no ancestor search at all: unlike `@@x`, there's exactly ONE
    /// global namespace, shared by every class and every thread. An unset
    /// global reads as `nil`, matching real Ruby (no `NameError`, unlike an
    /// unset constant -- see `ConstRead`'s docs).
    GlobalRead(String),
    GlobalWrite(String, NodeId),
    /// `Foo::BAR` -- an explicit, namespace-qualified constant READ
    /// (`ConstantPathNode`). The class name must already be a registered
    /// class/module (same rule `constant_name`'s other callers enforce); the
    /// owner search starts there (walking ITS ancestors, the same scheme as
    /// `ClassVarRead`'s ownership resolution -- see
    /// `codegen::expr::const_owner_id`), unlike a bare constant (lowered as
    /// an ordinary `ClassRef`, which falls back to the LEXICALLY-enclosing
    /// class when used as a plain value -- see `codegen::expr`'s `ClassRef`
    /// docs). Unlike an ivar/cvar/global's "never assigned" -> `nil`
    /// convention, an unset constant raises a real `NameError` -- matches
    /// actual Ruby.
    QualifiedConstRead(String, String),
    /// A LENIENT constant read (`None` if never assigned, instead of raising
    /// a `NameError`) -- `scope: None` for a bare name, `scope:
    /// Some(class_name)` for `Foo::NAME`, same shape as `QualifiedConstRead`/
    /// `ConstWrite`. Never produced by ordinary Ruby SOURCE (a real
    /// constant read always raises when unset -- see `QualifiedConstRead`'s
    /// docs); exists ONLY as `parse::lower_or_write`'s internal desugar for
    /// `CONST ||= value` specifically. Confirmed against real Ruby that this
    /// leniency is a genuine, narrow special case: `CONST ||= v` on a
    /// never-before-assigned constant quietly defines it (no `NameError`),
    /// but `CONST += v`/`CONST &&= v` on the same undefined constant DOES
    /// still raise -- Ruby's own `||=`-on-constant sugar treats "never
    /// assigned" as equivalent to a falsy read, `&&=`/other compound ops
    /// don't get that same leniency. `codegen::expr::emit_defined`'s
    /// existing `Defined`-node approximation can't back this (it's a
    /// syntax-only "was this written as a constant read" classifier, not a
    /// real "was this constant ever actually assigned" check -- see that
    /// function's docs), so this needs its own real, `spinel_rt::const_get`-
    /// backed codegen instead of reusing `Defined`.
    ConstReadOrNil(Option<String>, String),
    /// `NAME = value` / `Foo::NAME = value` -- `scope: None` for the bare
    /// form (owned by the LEXICALLY-enclosing class/module body it's written
    /// in, or `Object` at the top level -- exactly `ClassVarWrite`'s
    /// ownership scheme); `scope: Some(class_name)` for an explicit
    /// `Foo::NAME = value` (`ConstantPathWriteNode`), which always targets
    /// that NAMED class directly regardless of lexical position.
    ConstWrite {
        scope: Option<String>,
        name: String,
        value: NodeId,
    },
    /// `BEGIN { ... }` -- its body runs before ANY main statement, and
    /// several run in source order (oracle-verified).
    ///
    /// A marker rather than a construct: `analyze` hoists these bodies to
    /// the front of `main_statements` and drops the node. Nothing downstream
    /// ever sees one, which is why there is no codegen arm for it.
    ///
    /// (`END { ... }` needs no node at all -- it is `at_exit` exactly,
    /// including the reverse-order rule, so lowering rewrites it into that
    /// call.)
    PreExec(Vec<NodeId>),
    /// `alias $new $old` -- makes `$new` name `$old`'s STORAGE. A real,
    /// bidirectional alias, not a copy: oracle-verified that writing either
    /// name is visible through the other, and that a later `$old = 9` shows
    /// up as `$new`. So it can't lower to `$new = $old`; the runtime
    /// resolves the indirection on every access (`spinel_rt::globals`).
    ///
    /// `(new, old)`. Unlike `alias` on a METHOD (which lowering resolves by
    /// cloning the DefMethod), this needs no target to exist yet: aliasing
    /// an unset global is legal and both names then read nil.
    AliasGlobal(String, String),
    /// `undef foo, bar` in a class/module body -- makes those names raise
    /// NoMethodError on this class, INCLUDING names it only inherits
    /// (oracle-verified: `class C < B; undef inherited_m; end` makes
    /// `C.new.inherited_m` a NoMethodError while `B.new.inherited_m` still
    /// works, and `C.new.respond_to?(:inherited_m)` is false).
    ///
    /// That inherited case is why this can't just delete a `DefMethod` from
    /// the class body at lowering time: there is no local def to delete.
    /// `analyze::register_class` records the names, and
    /// `mro::materialize_methods` then refuses to materialize them onto
    /// this class -- which removes them from the one table both dispatch
    /// paths and `respond_to?` consult.
    Undef(Vec<String>),
    /// The last-match specials: `$~`, `$1`..`$9`, `$&`, `` $` ``, `$'`.
    ///
    /// NOT `GlobalRead`, even though they are spelled like globals: nothing
    /// ever assigns them (a regexp match does, as a side effect), and they
    /// read off a dedicated runtime slot rather than the `$foo` table. See
    /// `spinel_rt::lastmatch`, including the frame-locality divergence.
    LastMatchRef(LastMatch),
    /// A statement sequence evaluated in order, answering its LAST
    /// statement's value -- emitted as one tail-value Rust block expression
    /// via `emit_body` (the same codegen shape as `Eval`'s).
    ///
    /// Two sources reach here:
    ///   - a parenthesized multi-statement expression written in real
    ///     source (`x = (a; b)`, `(puts "hi"; 42)`);
    ///   - lowering itself, binding a compound-assignment target's
    ///     receiver/index expression(s) to a hidden local exactly ONCE
    ///     before reading-then-writing through them (`obj.attr += 1`,
    ///     `arr[i] ||= 1`), matching Ruby's "evaluate the receiver once"
    ///     rule; see `parse::lower_call_operator_write`'s docs.
    ///
    /// Introduces NO scope of its own in either case: a local assigned
    /// inside is visible afterwards (`y = (a = 5; a * 2)` leaves `a == 5`
    /// readable), which is both what real Ruby does with `(a; b)` and what
    /// the compound-assignment lowering needs from its hidden locals.
    ///
    /// Distinct from `Eval` (which is specifically a literal
    /// `eval("...")`'s spliced body) because the two mean different things
    /// to a Ruby reader and to tooling, not because they generate
    /// differently.
    Seq(Vec<NodeId>),
}
