use super::*;

/// One element of an `ArrayLit` -- a plain value, or a `*expr` splat whose
/// contents are flattened in at runtime (its length isn't known until then,
/// so this can't just be another plain element).
#[derive(Debug, Clone)]
pub enum ArrayElem {
    Single(NodeId),
    Splat(NodeId),
}

impl ArrayElem {
    /// The single child node this element wraps (`Single`'s value or `Splat`'s
    /// operand) -- lets HIR walkers visit an arg list without re-matching the
    /// variant. The `Single`/`Splat` distinction only matters at codegen.
    pub fn node_id(&self) -> NodeId {
        let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = self;
        *n
    }
}

/// A method/block's declared parameter list -- mirrors `ParametersNode`'s own
/// grouping directly (Ruby's grammar already enforces required->optional->
/// rest->post->keyword->keyword_rest->block ordering, so grouping by kind
/// loses nothing, and it maps 1:1 onto what `parse/mod.rs::lower_params`
/// reads off `ruby_prism::ParametersNode`).
///
/// `default_ids()` below is the one place `analyze::collect_ivars`/
/// `analyze::locals::track_extra`/`analyze::local_storage::collect_locals`'s
/// per-method scans walk INTO a default-value expression for `@ivar`/local
/// references -- otherwise
/// a default that reads an ivar/local nowhere else referenced (e.g. `def
/// f(x: @only_here)`) could hit a "no such field" codegen error instead of
/// working.
#[derive(Debug, Clone, Default)]
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
    /// The `rest` above came from a TRAILING COMMA (`|a,|`), not from a written
    /// `*`. It turns on auto-splat and discards what follows the named
    /// parameters -- but it is not part of the SIGNATURE: `proc { |x,| }.arity`
    /// is 1, `#parameters` reports only `x`, and `lambda { |a,| }` still
    /// refuses two arguments.
    pub implicit_rest: bool,
    /// Required params that appear AFTER a splat (`def f(a, *b, c)` -- `c`
    /// is a `post`; real Ruby allows this, and it's a distinct binding rule
    /// from `required` since its position is anchored from the END of the
    /// argument list, not the start).
    pub post: Vec<String>,
    pub keywords: Vec<KeywordParam>,
    /// Same `None`/`Some(None)`/`Some(Some(name))` shape as `rest`.
    pub keyword_rest: Option<Option<String>>,
    /// `**nil` -- the method accepts NO keywords at all. Distinct from
    /// "no `keyword_rest`": without a `**kwrest` a callee that declares no
    /// keywords silently takes trailing ones as a positional options Hash, and
    /// `**nil` (ruby 3.0) exists to forbid exactly that conversion. Passing
    /// keywords anyway is `ArgumentError: no keywords accepted`, raised BEFORE
    /// the arity check -- `nokw(1, 2, 3, b: 4)` reports the keywords, not the
    /// count. An EMPTY `**{}` splat passes nothing, so it does not raise.
    pub no_keywords: bool,
    /// `&blk` / anonymous `&` -- same `None`/`Some(None)`/`Some(Some(name))`
    /// shape as `rest`/`keyword_rest` again. Bound to `Nil` when the method
    /// is called with no block (real Ruby: an unyielded `&blk` is `nil`, not
    /// absent). Bare `...`
    /// forwarding (which implies a block too, among other things) is still a
    /// clean lowering error -- see `parse/mod.rs::lower_params`'s docs.
    pub block: Option<Option<String>>,
    /// BLOCK-LOCAL declarations -- the names after the `;` in `|x; sum|`.
    /// Always empty for a method's `Params`; the syntax exists only on a
    /// block.
    ///
    /// These are not parameters. Nothing binds to them from the argument
    /// list, so they take no signature slot and count toward no arity rule.
    /// They are fresh locals, re-initialized to `nil` on EVERY invocation.
    /// That is what makes them more than a naming convention:
    ///
    /// ```ruby
    /// total = 42
    /// [1, 2, 3].each { |x; total| total = (total || 0) + x }
    /// total  # => 42 -- never written, and never accumulated
    /// ```
    ///
    /// They do appear in `bound_names`, which is what shadows an enclosing
    /// local correctly. `captures::own_param_names`, `Ctx::in_proc` and
    /// hoisting all read that one enumeration.
    pub block_locals: Vec<String>,
    /// The block's IMPLICIT block-locals: names in prism's block-scope local
    /// table that are first-assigned inside the body (so not parameters and not
    /// the explicit `;`-block-locals above). Ruby re-initializes them to nil on
    /// EVERY invocation, so a conditional first-assignment (`x = v if cond`)
    /// must not leak into the next iteration. The inline `.times`/range-each
    /// splice (which shares the enclosing Rust scope rather than allocating a
    /// closure) resets them per iteration; escaping blocks reset via their
    /// own-locals prelude instead. Empty for methods.
    pub implicit_block_locals: Vec<String>,
}

/// A method's visibility, as of the point in the class body where its `def`
/// was lowered (`private`/`public`/`protected` with no arguments switches the
/// DEFAULT for every subsequent `def` in the same class body -- see
/// `parse::lower_class_body`'s docs) or set retroactively by a same-named
/// `private`/`public`/`protected :name` / `private def name; ... end` form.
/// Enforced through the caller-class channel every send carries -- see
/// `clif/call.rs`'s `caller_class` and the runtime dispatch.
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

#[derive(Debug, Clone)]
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
    /// optional, named rest/kwrest/block, post, keywords) -- what the
    /// local collection consults so a REASSIGNED parameter keeps its
    /// already-bound value instead of being shadowed by a fresh
    /// nil-defaulted local (a pre-existing
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
#[derive(Debug, Clone)]
pub enum KwArg {
    Pair(NodeId, NodeId),
    DoubleSplat(NodeId),
}

impl KwArg {
    /// The child node ids this element references, for HIR walkers (a `Pair`'s
    /// key+value, or a `DoubleSplat`'s single expression) -- so every pass can
    /// visit a `kwargs` list uniformly without re-matching the variant.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + use<> {
        let (a, b) = match *self {
            KwArg::Pair(k, v) => (k, Some(v)),
            KwArg::DoubleSplat(n) => (n, None),
        };
        std::iter::once(a).chain(b)
    }
}
