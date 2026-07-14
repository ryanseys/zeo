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
}

impl std::ops::Index<NodeId> for Hir {
    type Output = HirNode;
    fn index(&self, id: NodeId) -> &HirNode {
        &self.nodes[id.0 as usize]
    }
}

impl Hir {
    pub fn push(&mut self, node: HirNode) -> NodeId {
        self.nodes.push(node);
        NodeId((self.nodes.len() - 1) as u32)
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
/// Known, narrow, pre-existing-style gap: `analyze::collect_ivars`/
/// `analyze::locals::track_node`/`codegen::hoisting::collect_locals` don't
/// scan INTO a default-value expression here for `@ivar`/local references --
/// only a body statement or a call-site argument does. A default that reads
/// an ivar/local nowhere else referenced (e.g. `def f(x: @only_here)`) can
/// hit a "no such field" codegen error rather than working; nothing in the
/// spike's examples exercises this, so it's documented rather than fixed.
#[derive(Clone, Default)]
pub struct Params {
    pub required: Vec<String>,
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
}

#[derive(Clone)]
pub enum KeywordParam {
    Required(String),
    /// Same lazy-default-evaluation contract as `Params::optional`.
    Optional(String, NodeId),
}

/// One `key => value` / `key: value` pair inside a `{ }` literal.
/// Double-splat (`**other`) isn't supported yet -- lowering rejects it with
/// a clear error (spike scope), the same posture as `ParenthesesNode`'s other
/// narrowings (see `parse/mod.rs`).
pub struct HashPair(pub NodeId, pub NodeId);

/// One part of a (possibly-interpolated) string literal. A plain `"..."`
/// with no `#{}` lowers to a single `Lit` part. Only a single bare expression
/// is supported inside `#{}` (mirrors `ParenthesesNode`'s single-statement
/// restriction) -- a multi-statement interpolation body is a clean lowering
/// error, not silently truncated to its last statement.
pub enum StrPart {
    Lit(String),
    Interp(NodeId),
}

/// A small, real enum instead of spinel's ~115 string-typed `SP_NODE_KINDS`
/// that every pass has to `sp_streq` against. Sized to exactly what the
/// spike's 7 examples need; growing it is additive (new variants), matching
/// spinel's own incremental node-kind coverage.
pub enum HirNode {
    Program(Vec<NodeId>),
    IntegerLit(i64),
    SymbolLit(String),
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
        arms: Vec<(Vec<NodeId>, Vec<NodeId>)>,
        else_body: Vec<NodeId>,
    },
    /// `[1, 2, *rest]` -- see `ArrayElem`'s docs for the splat handling.
    ArrayLit(Vec<ArrayElem>),
    /// `{ a: 1, b: 2 }` -- see `HashPair`'s docs for the double-splat gap.
    HashLit(Vec<HashPair>),
    /// `a..b` / `a...b` -- either endpoint may be absent (`a..`/`..b`),
    /// matching Ruby's beginless/endless ranges.
    RangeLit {
        start: Option<NodeId>,
        end: Option<NodeId>,
        exclusive: bool,
    },
    /// A (possibly-interpolated) string literal -- see `StrPart`'s docs.
    StringLit(Vec<StrPart>),
    LocalRead(String),
    LocalWrite(String, NodeId),
    IvarRead(String),
    IvarWrite(String, NodeId),
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
    /// A call-site positional splat (`foo(*arr)`) and a call-site double-
    /// splat (`foo(**h)`) aren't lowered yet -- distinct, currently-unhandled
    /// prism nodes, a clean lowering error rather than a panic (spike scope:
    /// a `Params`-declared `rest`/`keyword_rest` can still be exercised by
    /// simply passing enough plain positional/keyword args, no splat syntax
    /// needed at the call site to prove out the parameter-binding side).
    /// `block_arg` is `foo(&existing_proc)` -- forwarding an already-built
    /// `Proc` value as the call's block (a distinct `BlockArgumentNode`),
    /// separate from `block` (a literal `{ }`/`do..end` at the call site);
    /// real Ruby rejects having both on the same call, which this spike
    /// doesn't separately re-validate (whichever lowers last silently wins --
    /// harmless, since `ruby-prism` itself already rejects this at parse
    /// time before lowering ever runs).
    Call {
        receiver: Option<NodeId>,
        name: String,
        args: Vec<NodeId>,
        kwargs: Vec<HashPair>,
        block: Option<NodeId>,
        block_arg: Option<NodeId>,
        safe: bool,
    },
    /// `ClassName.new(args)` -- a distinct node (not a plain `Call`) because
    /// it's always statically resolvable to a concrete class, and codegen
    /// needs that class name early to route ivar defaults / registration.
    New {
        class_name: String,
        args: Vec<NodeId>,
    },
    /// Mirrors spinel's `emit_super`: always resolved against the *static*
    /// superclass, never through the dynamic dispatch table. See codegen's
    /// handling -- the spike implements this via statement inlining rather
    /// than a cross-type function call (see docs/PORTING_ANALYSIS.md).
    SuperCall {
        args: Vec<NodeId>,
    },
    Block {
        params: Params,
        body: Vec<NodeId>,
    },
    ClassDef {
        name: String,
        superclass: Option<String>,
        body: Vec<NodeId>,
    },
    /// Also the desugared form of a literal-name `define_method(:name) { .. }`
    /// -- lowering (`parse/mod.rs`) treats it identically to a plain `def`,
    /// mirroring spinel's `walk_scope`.
    DefMethod {
        name: String,
        params: Params,
        body: Vec<NodeId>,
    },
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
    /// `for var in iterable ... end`. Unlike block-based iteration (`each { |x|
    /// ... }`), Ruby's `for` does NOT introduce a new variable scope: `var`
    /// and any locals first assigned in the body stay visible after the loop
    /// ends -- a real semantic difference, not a spike shortcut, and one
    /// `codegen`'s plain (non-block-nested) `let` emission already gives for
    /// free. Only a single plain local index variable is supported (`for a,
    /// b in ...` multi-target `for` is a clean lowering error, not a panic).
    /// The loop's own expression-position value is documented as `nil`
    /// unless a `break value` fires -- real Ruby returns the iterated
    /// collection itself in the no-break case, a narrower-than-real-Ruby
    /// simplification nothing in the spike's examples depends on.
    For {
        var: String,
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
    /// until Part 1.3/Phase 6. At most one argument is supported (`break a,
    /// b` building an implicit array is a clean lowering error, spike scope).
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
    /// `a, b = 1, 2` / `a, *b, c = arr` -- only plain local-variable targets
    /// are supported on the left (no nested destructuring, ivars, constants,
    /// or `a[i]`/`obj.attr` targets -- each is a distinct `ruby-prism` node
    /// this spike doesn't lower, a clean "unsupported syntax" error rather
    /// than a panic). `splat` is `None` for a plain `a, b = ...` with no `*`
    /// at all; `Some(name)` names the local that captures the
    /// (possibly-empty) middle slice. See `spinel_rt::multi_assign`'s docs
    /// for the exact leniency rules (missing positions become `nil`; extra
    /// values are silently dropped when there's no splat to catch them).
    MultiWrite {
        before: Vec<String>,
        splat: Option<String>,
        after: Vec<String>,
        value: NodeId,
    },
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
    /// exists. At most one value is supported (`return a, b` building an
    /// implicit array is a clean lowering error, spike scope, mirroring
    /// `Break`/`Next`).
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
    Yield(Vec<NodeId>),
    /// `block_given?` -- a zero-arg, no-receiver call-shape recognized at
    /// lowering time (mirrors `loop`/`define_method`'s desugars), not a
    /// distinct `ruby-prism` node. Same "not inside a nested block" scope-cut
    /// as `Yield`.
    BlockGiven,
}
