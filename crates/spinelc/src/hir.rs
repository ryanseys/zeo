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
    Call {
        receiver: Option<NodeId>,
        name: String,
        args: Vec<NodeId>,
        block: Option<NodeId>,
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
        params: Vec<String>,
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
        params: Vec<String>,
        body: Vec<NodeId>,
    },
}
