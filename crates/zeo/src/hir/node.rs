use super::*;

/// What kind of Ruby scope a node opens, if any.
///
/// This is the ONE place that enumerates every [`HirNode`] for the purpose of
/// "may a walk descend through this?", the way [`HirNode::for_each_child`] is
/// the one place that enumerates them for "what are its children?". A new
/// variant must declare itself here, and the predicate walks that consume it
/// then match on FIVE variants instead of eighty-one -- so each walk's stop
/// policy is two readable lines, and the differences BETWEEN those policies
/// are visible side by side. They were not, when each walk buried its stops in
/// its own copy of the full match, which is how they drifted apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeKind {
    /// An ordinary expression or statement: opens nothing.
    None,
    /// A `class`/`module`/`def` body -- a fresh Ruby scope. A `yield`,
    /// `super` or `return` written inside belongs to it, not to whatever
    /// encloses it.
    Definition,
    /// A lambda literal. It catches its own `Signal::Return`, and has no
    /// implicit block of its own (see [`HirNode::Lambda`]).
    Lambda,
    /// A synthesized `attach_function` wrapper body: contains none of the
    /// constructs these walks look for.
    Ffi,
    /// A block literal. Not a stop for any current walk -- a block has neither
    /// its own implicit block nor its own `super` target, so both refer to the
    /// enclosing method -- but it is reached only through the call that
    /// invokes it, which is where its escaping-ness is decided.
    Block,
}

impl HirNode {
    /// See [`ScopeKind`].
    pub fn scope_kind(&self) -> ScopeKind {
        match self {
            HirNode::Ffi(_) => ScopeKind::Ffi,
            HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => ScopeKind::Definition,
            HirNode::Lambda { .. } => ScopeKind::Lambda,
            HirNode::Block { .. } => ScopeKind::Block,
            HirNode::Program(..)
            | HirNode::IntegerLit(..)
            | HirNode::BigIntegerLit { .. }
            | HirNode::RationalLit { .. }
            | HirNode::ImaginaryLit(..)
            | HirNode::FloatLit(..)
            | HirNode::SymbolLit(..)
            | HirNode::NilLit
            | HirNode::BoolLit(..)
            | HirNode::And(..)
            | HirNode::Or(..)
            | HirNode::Defined(..)
            | HirNode::NotNil(..)
            | HirNode::If { .. }
            | HirNode::CaseWhen { .. }
            | HirNode::ArrayLit(..)
            | HirNode::HashLit(..)
            | HirNode::RangeLit { .. }
            | HirNode::StringLit(..)
            | HirNode::RegexpLit(..)
            | HirNode::LocalRead(..)
            | HirNode::LocalWrite(..)
            | HirNode::IvarRead(..)
            | HirNode::IvarWrite(..)
            | HirNode::ClassVarRead(..)
            | HirNode::ClassVarWrite(..)
            | HirNode::ClassRef(..)
            | HirNode::Call { .. }
            | HirNode::New { .. }
            | HirNode::SuperCall { .. }
            | HirNode::Include(..)
            | HirNode::Extend(..)
            | HirNode::Prepend(..)
            | HirNode::ClassMethodPrepend(..)
            | HirNode::DefHook { .. }
            | HirNode::MethodRedefine { .. }
            | HirNode::MethodReveal(..)
            | HirNode::Refine { .. }
            | HirNode::Using(..)
            | HirNode::While { .. }
            | HirNode::Loop { .. }
            | HirNode::For { .. }
            | HirNode::Break(..)
            | HirNode::Next(..)
            | HirNode::Redo
            | HirNode::MultiWrite { .. }
            | HirNode::BoxScope { .. }
            | HirNode::BoxHandle(..)
            | HirNode::FileEnd(..)
            | HirNode::Return(..)
            | HirNode::Yield(..)
            | HirNode::BlockGiven
            | HirNode::SelfRef
            | HirNode::Raise(..)
            | HirNode::CaseIn { .. }
            | HirNode::MatchPredicate { .. }
            | HirNode::MatchRequired { .. }
            | HirNode::Begin { .. }
            | HirNode::Retry
            | HirNode::GlobalRead(..)
            | HirNode::GlobalWrite(..)
            | HirNode::QualifiedConstRead(..)
            | HirNode::ConstReadOrNil(..)
            | HirNode::ConstWrite { .. }
            | HirNode::DynConstRead { .. }
            | HirNode::DynConstWrite { .. }
            | HirNode::PreExec(..)
            | HirNode::AliasGlobal(..)
            | HirNode::Undef(..)
            | HirNode::FeatureLoaded { .. }
            | HirNode::CExtLoaded { .. }
            | HirNode::ClassMethodUndef(..)
            | HirNode::AliasMethod { .. }
            | HirNode::MethodVisibility { .. }
            | HirNode::ClassMethodVisibility { .. }
            | HirNode::ModuleFunction(..)
            | HirNode::ConstantVisibility { .. }
            | HirNode::LastMatchRef(..)
            | HirNode::Seq(..)
            | HirNode::FlipFlop { .. } => ScopeKind::None,
        }
    }
}

/// The `cause:` keyword on a `raise`, as a genuine THREE-state value.
///
/// CRuby distinguishes these with a `Qundef`/`Qnil`/value sentinel
/// (`rb_f_raise` -> eval.c:740) because the first two mean OPPOSITE things:
/// an omitted `cause:` chains automatically from `$!`, while an explicit
/// `cause: nil` suppresses that chaining. Modeling this as a plain
/// `Option<NodeId>` invites exactly one bug -- lowering `cause: nil` to
/// `None` -- which would silently chain anyway, so the states are named.
#[derive(Debug, Clone, PartialEq)]
pub enum RaiseCause {
    /// No `cause:` written: chain automatically from `$!`.
    Absent,
    /// `cause: <expr>` was written. The expression may still evaluate to
    /// nil, which SUPPRESSES chaining rather than requesting it.
    Explicit(NodeId),
}

/// The `cause:` expression of a `raise`, if one was written.
///
/// Every HIR walker must visit it alongside the positional operands -- it is
/// an ordinary expression that can read locals, capture them into a block, or
/// name constants. Exposed here so no walker has to re-match [`RaiseCause`]
/// and risk quietly forgetting the node.
pub fn raise_cause_node(cause: &RaiseCause) -> Option<NodeId> {
    match cause {
        RaiseCause::Absent => None,
        RaiseCause::Explicit(id) => Some(*id),
    }
}

/// Which end of a spliced file `HirNode::FileEnd` marks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileEdge {
    /// Before the file's first statement: opens the jump target.
    Open,
    /// After its last: the point a top-level `return` jumps to.
    Close,
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
    /// `$+` -- the highest-numbered group that PARTICIPATED in the match
    /// (skipping declared-but-unmatched ones), or nil if only group 0 did.
    LastGroup,
}

/// One `rescue [classes] [=> binding] ... end` clause of a `begin`/an
/// implicit method-body rescue. `classes` empty = a bare `rescue` -- matches
/// `StandardError` and its descendants (real Ruby's own default), NOT
/// literally every `Exception` -- see `clif/control.rs`'s `clause_match`
/// for the matching. `binding`'s name leaks into the enclosing METHOD scope
/// exactly like a `case/in` pattern's bound names do (no new Ruby scope) --
/// the whole-scope local collection (`collect_locals`) needs to see it up
/// front, same treatment as `Pattern::for_each_bound_name`'s callers.
#[derive(Debug, Clone)]
pub struct RescueClause {
    /// The clause's STATIC exception classes -- plain constant references
    /// (`rescue Foo, Bar => e`), resolved to `ClassId`s and matched with a
    /// compile-time `is_a` ancestry check.
    pub classes: Vec<String>,
    /// Splatted exception lists (`rescue *errs => e`): each is a runtime
    /// expression evaluating to an Array of exception classes (or a single
    /// class). Matched at runtime via `zeo_rt::rescue_matches_any`, OR'd in
    /// after `classes`. NOTE: a side-effecting splat expr placed BEFORE a
    /// matching literal class evaluates slightly out of CRuby's strict
    /// left-to-right short-circuit order -- a documented, negligible divergence
    /// (the boolean MATCH result is always identical).
    pub splats: Vec<NodeId>,
    pub binding: Option<String>,
    pub body: Vec<NodeId>,
}

/// One part of a (possibly-interpolated) string literal. A plain `"..."`
/// with no `#{}` lowers to a single `Lit` part. Only a single bare expression
/// is supported inside `#{}` (mirrors `ParenthesesNode`'s single-statement
/// restriction) -- a multi-statement interpolation body is a clean lowering
/// error, not silently truncated to its last statement.
#[derive(Debug, Clone)]
pub enum StrPart {
    Lit(String),
    /// A literal segment whose bytes are NOT valid UTF-8 -- a `"\xNN"`
    /// escape that doesn't form a character (Ruby tags such a literal
    /// ASCII-8BIT). Kept as raw bytes so the encoding engine sees them
    /// verbatim instead of the `String::from_utf8_lossy` U+FFFD mangling.
    Bytes(Vec<u8>),
    Interp(NodeId),
}

/// A `/pattern/flags` / `%r{pattern}flags` literal's option letters --
/// `ruby-prism`'s `RegularExpressionNode`/`InterpolatedRegularExpressionNode`
/// expose several more (`o`/`e`/`n`/`s`/`u`, all encoding/interpolation-once
/// concerns), but zeo is UTF-8-only throughout (see
/// `docs/limitations.md`'s existing posture on strings), so only the three
/// letters that change actual MATCHING semantics are modeled; the rest are
/// silently accepted as no-ops except a genuinely non-UTF-8-forcing encoding
/// flag (`e`/`s`), which is a clean lowering rejection (see
/// `parse/mod.rs`'s recognizer).
#[derive(Debug, Clone, Copy, Default)]
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
    /// `zeo_rt::regexp::regexp_new`'s docs for how this is modeled
    /// (`multi_line(true)` is unconditional, independent of this field).
    pub multiline: bool,
    /// `n`/`e`/`s`/`u` -- the encoding the literal FORCES. Not decoration:
    /// `#options`, `#encoding` and `#fixed_encoding?` each report it. See
    /// [`zeo_abi::RegexpEncoding`].
    pub encoding: zeo_abi::RegexpEncoding,
}

/// A real enum of node kinds; growing it is additive (new variants).
#[derive(Clone, Debug)]
pub enum HirNode {
    Program(Vec<NodeId>),
    IntegerLit(i64),
    /// An Integer literal beyond i64 (the bignum representation) -- carried as
    /// prism's own `(negative, LSB-first u32 digits)` shape so zeo
    /// needs no bigint dependency; codegen emits
    /// `zeo_rt::int_from_u32_digits`. Types as `Int` like `IntegerLit`
    /// (one Ruby Integer class, two payloads).
    BigIntegerLit {
        negative: bool,
        digits: Vec<u32>,
    },
    /// `3r` / `1.5r` -- prism pre-rationalizes the decimal
    /// forms (`1.5r` arrives as numerator 3, denominator 2), so both
    /// components travel as digit strings like `BigIntegerLit`. The
    /// denominator is positive and non-zero by syntax.
    RationalLit {
        negative: bool,
        num_digits: Vec<u32>,
        den_digits: Vec<u32>,
    },
    /// `4i` / `2.0i` / `3ri` -- an imaginary literal wrapping
    /// its lowered inner numeric literal compositionally
    /// (`Complex(0, inner)`).
    ImaginaryLit(NodeId),
    /// A real `f64` payload -- `HirNode` itself derives no `Eq`/`Hash` (see
    /// this enum's own docs), so an un-`Eq`-able float here is no different
    /// from `IntegerLit`'s `i64` in that respect.
    FloatLit(f64),
    SymbolLit(String),
    /// `nil` as a literal. `case/in`'s `Pattern::Value` fallback needs
    /// `in nil` to lower through the ordinary expression path like any
    /// other literal, rather than special-casing pattern lowering around it.
    NilLit,
    /// `true` / `false` -- see `NilLit`'s docs; same reasoning.
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
    /// runtime check. See `clif/expr.rs`'s `lower_defined` for the
    /// scope-cut this approximates.
    Defined(NodeId),
    /// True unless the value is nil -- the TAG test `&.` makes, not a
    /// `nil?` send a program can redefine. It is the condition of the
    /// guard `o&.v ||= 8` and its siblings wrap themselves in; see
    /// `lower::assign`'s `guard_safe_target`.
    NotNil(NodeId),
    /// `if`/`unless`/`elsif`/ternary all normalize to this at lowering time
    /// (`unless` swaps `then_body`/`else_body`; `elsif` is prism's own
    /// `IfNode::subsequent()` recursion, which lowering walks into a nested
    /// `If`; ternary is literally the same `IfNode` shape prism produces for
    /// `a ? b : c`). Ruby's implicit-last-expression-return makes `If`
    /// itself an expression, lowered with a result in tail position.
    If {
        cond: NodeId,
        then_body: Vec<NodeId>,
        else_body: Vec<NodeId>,
    },
    /// `case subject; when v1, v2 then ...; else ...; end` -- value
    /// matching only (no subject means each `when` value is itself the
    /// boolean condition, like a chained `if`/`elsif`). `case/in` pattern
    /// matching is a distinct prism node (`CaseMatchNode`) and isn't
    /// lowered to this.
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
    /// earlier at `zeo` compile time, unlike real Ruby's own parse-time
    /// `SyntaxError` for a static pattern: a documented, narrower-timing
    /// approximation (see `clif/expr.rs`'s `regexp_lit`),
    /// not silent wrongness. Backed by the `regex` crate, not Ruby's own
    /// Onigmo engine -- no backreferences (`\1` inside the PATTERN itself,
    /// as opposed to a `gsub`/`sub` REPLACEMENT string, where they *are*
    /// supported -- see `zeo_rt::regexp`'s docs) and no lookaround
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
    /// a variable, compared, or reflected on at runtime).
    ClassRef(String),
    /// A literal-name-resolvable call: `recv.name(args) { block }`, or an
    /// implicit-self call (`receiver: None`).
    ///
    /// - `send`/`public_send` are ordinary calls named "send", not a separate
    ///   node kind. Codegen reads `name` and, for `send`, `args[0]`, to
    ///   choose static Path 1 or `zeo_rt::send` Path 2.
    /// - `safe: true` is `&.`, a flag on prism's same `CallNode`. The call
    ///   short-circuits to `nil` without evaluating when `receiver` is `nil`.
    /// - `args` reuses `ArrayElem`: a positional value, or a `*expr` splat
    ///   that flattens at runtime.
    /// - `kwargs` is the ordered `KwArg` list. Literal `name: value` pairs
    ///   interleave with `**h` double-splats in source order, and the merge
    ///   is left-to-right last-one-wins, so the order is observable. Any
    ///   `DoubleSplat` routes to the runtime arg-vector path; a pairs-only
    ///   list stays on Path 1. Codegen resolves the names against the
    ///   callee's declared keyword params, which a Path 1 site always knows.
    /// - `block_arg` is `foo(&existing_proc)`, forwarding an already-built
    ///   `Proc`. It is separate from `block`, a literal `{ }`/`do..end` at
    ///   the call site. Ruby rejects both on one call, and `ruby-prism`
    ///   rejects it at parse time, so zeo does not re-validate.
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
    /// parameters bind as keywords.
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
        /// A literal block passed to `.new` (`Foo.new(x) { ... }`), forwarded
        /// to `initialize` so `yield`/`block_given?` inside it see the block.
        /// A block-PASS (`Foo.new(&p)`) still routes through the generic
        /// `Call` lowering instead (this stays `None`).
        block: Option<NodeId>,
    },
    /// A `super` call. Resolved at RUNTIME against the receiver's live
    /// linearized ancestry (`clif/call.rs`'s `lower_super` picks the dispatch
    /// channel).
    ///
    /// `zsuper` distinguishes real Ruby's two zero-written-argument shapes,
    /// which mean OPPOSITE things: bare `super` (prism's
    /// `ForwardingSuperNode`, `zsuper: true`) forwards the current method's
    /// own parameters as currently bound, while `super()` (a `SuperNode`
    /// with no arguments, `zsuper: false`) passes NO arguments at all, so
    /// the parent's optionals take their defaults. `args` is always empty
    /// when `zsuper` is true.
    SuperCall {
        /// Explicit positional args (`super(a, *rest)`) as `ArrayElem`s, so a
        /// splat argument forwards through the runtime arg vector -- the same
        /// shape a `Call`'s args have. Always empty for bare `super`
        /// (`zsuper`), which forwards the current method's own params instead.
        args: Vec<ArrayElem>,
        /// Explicit keyword arguments (`super(x: 1, y: 2)`); bound to the
        /// parent's keyword params by NAME. Always empty for bare
        /// `super` (`zsuper`), which forwards the current method's own
        /// keywords instead.
        kwargs: Vec<KwArg>,
        zsuper: bool,
        /// A literal block written at the `super` site (`super { ... }` /
        /// `super(x) { ... }`) -- a `HirNode::Block`, bound as the spliced
        /// parent body's `__blk` so its `yield` runs this block. `None`
        /// means the parent implicitly sees the CURRENT method's own block
        /// (real Ruby forwards it), which the splice gets for free since
        /// `__blk` is already in scope there.
        block: Option<NodeId>,
        /// A block-PASS argument (`super(x, &blk)`) -- an expression coerced to
        /// a block via `to_proc` at the call. Mutually exclusive with `block`
        /// (Ruby's grammar forbids both), same either/or a `Call` carries.
        block_arg: Option<NodeId>,
    },
    Block {
        params: Box<Params>,
        body: Vec<NodeId>,
    },
    /// `-> (x) { ... }` / `lambda { ... }` -- a STANDALONE expression
    /// producing a real `RubyValue::Proc`, unlike `Block` (only ever reached
    /// via the `Call` that invokes it, see that variant's docs). Reuses
    /// the proc construction machinery (`clif/blocks.rs`'s `build_lambda`
    /// beside `build_proc`) via a shared helper, differing in
    /// exactly two ways real Ruby's own lambda semantics require: STRICT
    /// arity checking (raises `ArgumentError`, not a lenient nil-fill/drop),
    /// and `return`/`break` inside the body terminate the LAMBDA CALL
    /// itself (folded into a normal `Ok` return, like a method boundary)
    /// rather than propagating to the enclosing method/loop.
    Lambda {
        params: Box<Params>,
        body: Vec<NodeId>,
        /// True when this lambda is a METHOD BODY installed at runtime -- the
        /// desugar of a per-object singleton (`def obj.name`, `class << obj`)
        /// or a `def`/`define_method` in expression position. Such a body's
        /// `yield`/`block_given?`/`&blk` targets the block the METHOD is called
        /// with (threaded through `ProcData`'s call-site block slot via
        /// `with_self_and_block`), NOT the lexically enclosing method's block
        /// an ordinary lambda would clone in. CRuby draws the same line --
        /// `invoke_bmethod`'s specval vs the captured env (`vm.c:1786`).
        method_body: bool,
    },
    /// `class Name < Super ... end` / `module Name ... end` -- `is_module`
    /// distinguishes the two: a module has no `superclass` (always `None`)
    /// and is never instantiated (no `Name.new`, no instance
    /// layout). Both share this one node
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
    /// mirroring zeo's `walk_scope`. `is_class_method` is `def self.name`
    /// (`DefNode::receiver()` is `Some(SelfNode)`) -- a genuinely different
    /// registration target (`ClassInfo::class_methods`, not `::methods`; no
    /// `self: Arc<Self>` receiver at codegen time at all, see
    /// `compiler::ClassInfo`'s docs) even though the body shape is
    /// identical. Any OTHER explicit receiver: one that `names_enclosing_
    /// class` (net/smtp's `def SMTP.default_port`) is also a class method,
    /// and the rest desugar to `RECV.define_singleton_method` -- see
    /// `lower/mod.rs`'s def-receiver arms.
    DefMethod {
        name: String,
        params: Box<Params>,
        body: Vec<NodeId>,
        is_class_method: bool,
        visibility: Visibility,
        /// `true` for a real `def` keyword; `false` for a literal
        /// `define_method(:sym){...}` call desugared into this node. Only
        /// matters in EXPRESSION position (inside a block): a real `def`
        /// installs on the runtime default definee (a singleton method under an
        /// `instance_exec` on a plain object), whereas an explicit
        /// `define_method` is an ordinary `Module#define_method` call that
        /// raises `NoMethodError` when `self` isn't a Module/Class.
        is_def: bool,
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
    /// `prepend Mod` written inside `class << self` -- the singleton half of
    /// [`HirNode::Prepend`], exactly as [`HirNode::ClassMethodVisibility`] is
    /// the singleton half of `MethodVisibility`. The module's INSTANCE methods
    /// become the enclosing class's CLASS methods, ahead of its own `def
    /// self.x` (which they may override, `super` reaching the original).
    ///
    /// `ClassInfo::class_method_prepends` -- the same field the equivalent
    /// `C.singleton_class.prepend(M)` CALL form records (see
    /// `analyze::try_prepend_call_edit`), so `mro::materialize_class_methods`
    /// needs nothing new.
    ClassMethodPrepend(String),
    /// A definition REPORT: `Klass.method_added(:name)`, or one of its five
    /// siblings. Ruby announces every definition to the class it landed on, and
    /// the announcement runs at the definition's own position -- but zeo
    /// consumes a class-body `def` at analyze time and emits nothing there. So
    /// `analyze::def_hooks` splices one of these back in at that position, and
    /// only when a hook body will actually answer. Never produced by lowering:
    /// a program that defines no hook carries none of these at all.
    DefHook {
        /// The receiver of the report -- the class for `method_added`, and for
        /// `singleton_method_added` too, since a compiled singleton definition
        /// is always a class method (`def self.x`, `class << self`).
        class: u32,
        /// The hook's name, already resolved for the definition's shape.
        hook: String,
        /// The defined method's name, passed as the hook's one Symbol argument.
        name: String,
        /// Instance methods of `class` that are still in the FUTURE here.
        /// Ruby's hook sees a half-built class; zeo has every table installed
        /// before the first statement runs, so the reflection rows subtract
        /// this set for as long as the hook body is on the stack. Empty for
        /// the last definition in a class, which is the common case.
        pending: Vec<String>,
        /// A PACKAGE build cannot know whether the program it links into
        /// carries a hook body, so it announces through a run-time probe
        /// (`zeo_rt_probe_def_hook`) instead of an unconditional send. The
        /// no-hook answer is one atomic load, and a definition runs once
        /// per require, so the probe is never hot.
        probe: bool,
    },
    /// A method REDEFINITION applied at its document position. Ruby installs
    /// each `def` where it stands, so code running between two same-name
    /// `def`s dispatches to the FIRST body; zeo's static tables carry only
    /// the last-def-wins winner. `analyze::redefs` splices one of these at
    /// each superseded redefinition's position: codegen installs the named
    /// scope's compiled trampoline into the runtime overlay there, and a
    /// boot-time install of the FIRST body covers the window before it.
    /// Never produced by lowering.
    MethodRedefine {
        class: u32,
        name: String,
        /// The `ScopeId` (as raw index) whose body becomes current here.
        scope: u32,
        /// `def self.x` rather than `def x` -- the install goes to the
        /// class-method channel, whose overlay row is a different map.
        singleton: bool,
    },
    /// Lifts one REVEAL GROUP's concealment, spliced at the position of the
    /// `def` whose row it holds back -- `analyze::alias_reveals`. Groups are
    /// numbered past the units', which use the same runtime table. Never
    /// produced by lowering.
    MethodReveal(u32),
    /// `refine Target do ... end` in a module body. The block's `def`s lower
    /// into a HOLDER module (a `ClassDef` pushed immediately before this
    /// marker, named `#refinement:Target` so it claims no Ruby constant);
    /// this node names the class those methods refine. Deliberately not an
    /// `Include`: a refinement edits no ancestry at all -- it is consulted
    /// only at the call sites a `using` scope covers.
    ///
    /// `singleton` is `refine Target.singleton_class do ... end`: the holder's
    /// methods refine Target's CLASS methods (its singleton class), so a
    /// covered `Target.m` consults them where the plain form covers
    /// `instance.m`.
    Refine {
        target: String,
        holder: String,
        singleton: bool,
    },
    /// `using M` -- activates every refinement `M` holds for the code
    /// lexically AFTER this point, to the end of the enclosing body (the
    /// rest of the file at the top level). The position comes from this
    /// node's own span, so a `def` written after it is covered while one
    /// written above it is not, which is exactly real Ruby's rule.
    Using(String),
    /// `while cond ... end` / `until cond ... end` (+ modifier forms
    /// `stmt while cond` / `stmt until cond`) -- `until` folds in here as
    /// `negate: true`, exactly like `unless` folds into `If` by swapping
    /// branches (see `parse/mod.rs`). Always lowers to explicit loop
    /// blocks whose exit lands in expression position, so `break value`
    /// has a target to jump to (see `clif/stmt.rs`'s `loop_value`).
    /// The do-while form (`begin ... end while cond`, body always runs at
    /// least once) is a distinct prism shape wrapping a `BeginNode`, which
    /// isn't lowered -- it already falls through to the generic
    /// "unsupported syntax" error untouched, so it needs no explicit
    /// handling here.
    While {
        cond: NodeId,
        body: Vec<NodeId>,
        negate: bool,
        /// `begin ... end while cond` (prism's begin-modifier flag): a
        /// POST-test loop whose body runs once before the condition is first
        /// checked. `false` for the ordinary pre-test `while`/`until`.
        post: bool,
    },
    /// `loop do ... end` -- NOT a distinct `ruby-prism` node (it's an
    /// ordinary zero-arg, no-receiver `Kernel#loop` call with a block);
    /// `parse/mod.rs` desugars that call shape to this at lowering time,
    /// mirroring `define_method`'s existing call-shape desugar. An
    /// unconditional labeled Rust `loop { }` with no exit test of its own --
    /// only `break` (or an uncaught `raise`) ever ends it.
    Loop {
        body: Vec<NodeId>,
    },
    /// `for var in iterable ... end` / `for a, b in pairs ... end`. Unlike
    /// block-based iteration (`each { |x| ... }`), Ruby's `for` does NOT
    /// introduce a new variable scope: `var` and any locals first assigned
    /// in the body stay visible after the loop ends -- a real semantic
    /// difference, not a shortcut, and one `codegen`'s plain
    /// (non-block-nested) `let` emission already gives for free. `target`
    /// reuses `MultiTarget` (a plain `for x in ...` lowers to
    /// `MultiTarget::Local`; `for a, b in ...` to `MultiTarget::Nested`),
    /// destructured against each iterated element exactly like a
    /// `MultiWrite`'s value. The loop's own expression-position value is
    /// documented as `nil` unless a `break value` fires -- real Ruby returns
    /// the iterated collection itself in the no-break case, a
    /// narrower-than-real-Ruby simplification nothing in the corpus
    /// depends on.
    For {
        target: MultiTarget,
        iterable: NodeId,
        body: Vec<NodeId>,
    },
    /// `break` / `break value` -- unwinds to the end of the nearest *native*
    /// loop construct (`While`/`Loop`/`For`, or the `.times` inlining in
    /// `clif/iter.rs`'s `lower_counted`), which `ruby-prism`
    /// itself already guarantees is the only place these can appear (a bare
    /// `break`/`next`/`redo` outside any loop/block is a parse error, not
    /// something lowering has to re-validate). Compiles to a literal Rust
    /// `break 'label value;` -- no `Signal` involved, per the ABI's stated
    /// scope-cut (see `signal.rs`), since a `break` inside a real escaping
    /// closure is handled separately. A multi-value `break a, b` lowers to a single
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
    /// See `clif/stmt.rs`'s `Redo` arm: a jump back to the body block.
    Redo,
    /// `a, b = 1, 2` / `a, *b, c = arr` / `(a, b), c = ...` / `@x, $y, Z =
    /// ...` -- see `MultiTargetGroup`/`MultiTarget`'s docs for the full
    /// generalized target shape (local/ivar/cvar/global/const/attr/index/
    /// nested-group). See `zeo_rt::multi_assign`'s docs for the exact
    /// leniency rules (missing positions become `nil`; extra values are
    /// silently dropped when there's no splat to catch them).
    MultiWrite {
        targets: MultiTargetGroup,
        value: NodeId,
    },
    /// The body of a synthesized `attach_function` wrapper: marshal args,
    /// call the C symbol, wrap the result. See `FfiCall`.
    Ffi(Box<FfiCall>),
    /// A `Ruby::Box` context switch: the universal wrapper every
    /// box-scoped splice lowers into -- a `box.require`d file's statements,
    /// a `box.eval` body, and a `box::X` external-access expression all
    /// carry their statically-known box id here. Emits exactly like `Eval`
    /// (a brace-wrapped block whose value is the last statement's), except
    /// codegen's `Ctx.box_id` is overridden for the body -- the AOT
    /// translation of CRuby's loading-box/`cme->def->box` context.
    /// `analyze` descends top-level `BoxScope`s to register their
    /// `ClassDef`s under the box.
    BoxScope {
        box_id: u32,
        body: Vec<NodeId>,
    },
    /// The runtime VALUE of a box handle (`box = Ruby::Box.new` binds the
    /// local to this): a `RubyValue::Class` of the box's top-level
    /// surrogate class, so `p box` prints `#<Ruby::Box:N>`, handle equality
    /// works, and storing it is harmless. The four recognized OPERATION
    /// shapes (`box.require`/`box.load`/`box.eval`/`box::X`) resolve
    /// statically through the loader's bindings map, never through this
    /// value.
    BoxHandle(u32),
    /// The two ends of a SPLICED file whose top level writes a `return`.
    /// Ruby's top-level `return` ends the FILE it stands in, and reading
    /// resumes in the requiring file -- so a `return` between these two
    /// markers jumps to the closing one rather than ending `<main>`. They
    /// stand as ORDINARY statements in the enclosing list, so every walk
    /// over the spliced statements sees them exactly as it did before, and
    /// a file that writes no top-level `return` carries neither.
    FileEnd(FileEdge),
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
    /// (already reserved for exactly this in `zeo_rt::Signal`) once it
    /// exists. A multi-value `return a, b` lowers to a single implicit-array
    /// argument (`return [a, b]`), via `lower_single_optional_argument`, so
    /// this stays one optional node (mirroring `Break`/`Next`).
    Return(Option<NodeId>),
    /// `yield` / `yield(args)` -- invokes the enclosing method's implicit
    /// block.
    ///
    /// Recognized only directly within a method's own control flow, not
    /// inside a nested block literal. In real Ruby a `yield` lexically
    /// inside a block passed elsewhere refers to that block's own enclosing
    /// method; zeo does not attempt that case, and
    /// `analyze::register_class`'s `uses_bare_block` scan rejects it
    /// cleanly.
    ///
    /// Compiles to an invocation of the method's implicit `__blk` parameter
    /// (see `clif/expr.rs`'s `Yield` arm), raising real Ruby's `LocalJumpError` message
    /// when the method was called without a block.
    ///
    /// `Vec<ArrayElem>` is the shape `Call`'s positional arguments use, so
    /// `yield(*a)` needs no machinery of its own. Lowering folds a trailing
    /// `yield(k: 1)`/`yield(**h)` into one `HashLit`/merge element, which
    /// the block-parameter binding (`clif/blocks.rs`) binds a block's
    /// keyword params from.
    Yield(Vec<ArrayElem>),
    /// `block_given?` -- a zero-arg, no-receiver call-shape recognized at
    /// lowering time (mirrors `loop`/`define_method`'s desugars), not a
    /// distinct `ruby-prism` node. Same "not inside a nested block" scope-cut
    /// as `Yield`.
    BlockGiven,
    /// A bare `self` used as a VALUE (an explicit receiver, `self.foo`, or
    /// standalone, `puts self`) -- a real `ruby-prism` `SelfNode`, recognized
    /// generically (not a call-shape desugar). Lowered to the current
    /// frame's own `self` (see `clif/expr.rs`'s `SelfRef` arm).
    SelfRef,
    /// `raise`/`fail` (exact synonyms) -- a zero/one/two-arg call-shape
    /// recognized at lowering time, same as `BlockGiven` above (real Ruby:
    /// both are ordinary `Kernel` method calls, not syntax). Codegen
    /// classifies the arg SHAPE itself (`ClassRef` vs. a `Str`-typed
    /// expression vs. an already-constructed exception value) rather than
    /// this node encoding it structurally -- mirrors `Yield`'s "let codegen,
    /// which already has full type-inference machinery, decide" posture.
    /// Bare `raise` (re-raise, zero args) needs a currently-handled
    /// exception context that doesn't exist until `rescue` does --
    /// a clean rejection until then, not a silent no-op.
    Raise(Vec<NodeId>, RaiseCause),
    /// `case subject; in PATTERN [if/unless GUARD] ... [else ...] end` --
    /// see `Pattern`/`PatternArm`'s docs. Arms are tested top to bottom,
    /// first match wins (same "not a native `match`" reasoning as
    /// `CaseWhen`, since a pattern's own class-check/destructure/guard logic
    /// can't be expressed as Rust structural patterns generically).
    /// `else_body: None` with no arm matching raises `NoMatchingPatternError`
    /// (see `clif/patterns.rs`'s `lower_case_in`) -- a real, distinct case from
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
    MatchPredicate {
        subject: NodeId,
        pattern: Pattern,
    },
    /// `expr => pattern` -- the rightward-assignment one-liner: raises
    /// `NoMatchingPatternError` if `pattern` doesn't match (mirrors a
    /// `case/in` with no `else`), otherwise evaluates to `nil` (real Ruby:
    /// this form's value is never used for anything but its binding/raising
    /// side effect).
    MatchRequired {
        subject: NodeId,
        pattern: Pattern,
    },
    /// `begin body rescue ... else ... ensure ... end` -- also the desugared
    /// form of a method body that's implicitly a `BeginNode` (a `def` with a
    /// bare `rescue`/`ensure` and no explicit `begin`/`end`, confirmed
    /// empirically via `Prism.parse`) and of `expr rescue fallback` (the
    /// modifier form, including inside an endless method) -- see
    /// `parse/mod.rs`'s recognizers, all of which produce this same shape.
    /// `rescues` are tested top to bottom, first matching clause wins; an
    /// unmatched raise propagates (the node's own site re-signals,
    /// mirroring every other fallible sub-expression -- see
    /// `clif/control.rs`'s `lower_begin`). `else_body: Some(_)` runs (and its
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
    /// `SyntaxError` otherwise) -- zeo doesn't re-validate that
    /// positional restriction at lowering time; `clif/stmt.rs`'s `Retry`
    /// arm refuses a mis-scoped one with a clear error, not silent
    /// wrongness.
    Retry,
    /// `$foo` read/write -- a flat, genuinely process-wide store (the
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
    /// `ClassVarRead`'s ownership resolution), unlike a bare constant
    /// (lowered as an ordinary `ClassRef`, which falls back to the
    /// LEXICALLY-enclosing class when used as a plain value -- see
    /// `clif/expr.rs`'s `ClassRef` arm).
    /// Unlike an ivar/cvar/global's "never assigned" -> `nil`
    /// convention, an unset constant raises a real `NameError` -- matches
    /// actual Ruby.
    QualifiedConstRead(String, String),
    /// A LENIENT constant read: `None` when never assigned, rather than a
    /// `NameError`. `scope: None` for a bare name, `scope: Some(class_name)`
    /// for `Foo::NAME`.
    ///
    /// Ordinary Ruby source never produces this. It exists only as
    /// `parse::lower_or_write`'s desugar for `CONST ||= value`. Real Ruby
    /// confirms the leniency is that narrow: `CONST ||= v` on a
    /// never-assigned constant quietly defines it, while `CONST += v` and
    /// `CONST &&= v` on the same constant still raise.
    ///
    /// The `Defined` node (`clif/expr.rs`'s `lower_defined`) cannot back this. It
    /// classifies syntax -- whether something was written as a constant read
    /// -- not whether the constant was ever assigned. So this needs its own
    /// `zeo_rt::const_get`-backed codegen.
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
    /// `expr::NAME` where `expr` names no class the compiler can resolve --
    /// `self::OPTION_NAMES` in a `Struct.new` block, `adapter::GitExecuteError`
    /// on a parameter, `self.class::Reason`. The scope is an ordinary
    /// expression, evaluated first, and the constant is found on whatever it
    /// answers.
    ///
    /// `lenient` is `obj::NAME ||= v`'s read half, exactly as
    /// `ConstReadOrNil` is `Foo::NAME ||= v`'s: nil instead of a `NameError`
    /// when the name was never assigned, so the write half can define it. Only
    /// `||=` gets that leniency -- `+=` and `&&=` still raise.
    DynConstRead {
        scope: NodeId,
        name: String,
        lenient: bool,
    },
    /// `expr::NAME = value`. Writes the scope's OWN constant table (assignment
    /// never walks an ancestry) and answers `value`.
    ///
    /// Ruby allows this only where a plain `NAME = value` would also be legal:
    /// inside a method body `obj::NAME = v` is the `dynamic constant
    /// assignment` SyntaxError, and prism reports it before lowering ever runs.
    /// `obj::NAME ||= v` in a method is legal, though -- the check is on the
    /// plain form alone -- which is the shape act_as_attribute is built on.
    DynConstWrite {
        scope: NodeId,
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
    /// resolves the indirection on every access (`zeo_rt::globals`).
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
    /// A `require` that SUCCEEDED, at its own document position -- what
    /// CRuby's `rb_provide_feature` does before it evaluates the file
    /// (`load.c`), and what makes `$LOADED_FEATURES` answer positionally
    /// rather than for the whole program at once.
    ///
    /// `entry` is the `$LOADED_FEATURES` string: a spliced file's canonical
    /// path, or `<zeo-builtin>/<feature>.rb` for a statically linked
    /// extension. `feature` names the canonical `ext/` feature when the
    /// require ACTIVATES one, which is what a require-gated builtin row
    /// (`IO#getch`) and a require-gated constant wait for; `None` for an
    /// ordinary spliced file, which activates nothing.
    ///
    /// It is emitted at the HEAD of a splice, before the file's own
    /// statements, because CRuby records the feature before it runs the
    /// file -- which is what makes a circular require answer `false`
    /// instead of recursing.
    FeatureLoaded {
        entry: String,
        feature: Option<String>,
    },
    /// A gem's compiled C extension is loaded HERE.
    ///
    /// The shared object was built at compile time (`zeo::cext`) and this is
    /// where its `Init_<init>` runs -- which is positional in exactly the way
    /// [`HirNode::FeatureLoaded`] is, because everything the extension
    /// defines becomes answerable from this point and not from line 1.
    ///
    /// `library` is an absolute path. A relative one would resolve against
    /// the working directory, which is the caller's and not the compiler's.
    CExtLoaded {
        library: String,
        init: String,
    },
    /// `undef foo` / `undef_method :foo` written inside `class << self` -- the
    /// singleton half of [`HirNode::Undef`], which makes those names raise
    /// NoMethodError as CLASS methods of the enclosing class, inherited ones
    /// included (oracle-verified against `class << self; undef_method :new`,
    /// the shape optparse and rspec-mocks use to retire an inherited
    /// constructor). `ClassInfo::class_undefined` records them and
    /// `mro::materialize_class_methods` then refuses to materialize them.
    ClassMethodUndef(Vec<String>),
    /// `alias new old` / `alias_method :new, :old` where `old` is NOT defined
    /// earlier in the same class/module body -- an INHERITED method (or one a
    /// later reopen adds). Lowering can't clone the source `DefMethod` because
    /// it isn't in this body, and it can't resolve the ancestor chain either
    /// (ancestors are only linearized in `analyze`). So it records `(new,
    /// old)` here; `analyze::register_class` collects it into the class's
    /// `pending_aliases`, and `mro::resolve_aliases` (after ancestors are
    /// computed, before methods materialize) walks the MRO's `own_methods`,
    /// clones the source scope's params/body under `new`, and registers it --
    /// so it then materializes onto this class AND its subclasses normally.
    /// The same-body case stays a lowering-time `DefMethod` clone (no runtime
    /// target needed), exactly like `alias` on a locally-defined method.
    ///
    /// `is_class_method` marks an `alias` written inside `class << self` (`alias
    /// split shellsplit`): it aliases a SINGLETON method, so it resolves against
    /// the MRO's `own_class_methods` and registers as an own class method.
    AliasMethod {
        new_name: String,
        old_name: String,
        is_class_method: bool,
    },
    /// `private :m` / `public :m` / `protected :m` naming a method NOT defined
    /// earlier in the same class/module body -- an INHERITED method whose
    /// visibility this class re-declares (`class Sub < Base; private :base_pub;
    /// public :base_priv; end`). The same-body case is handled at lowering time
    /// by `set_method_visibility` on the local `DefMethod`; there is no local
    /// def here, so the name+visibility is recorded for `analyze::register_class`
    /// to collect into the class's `visibility_overrides`, applied after
    /// materialization stamps each method with its defining class's visibility.
    MethodVisibility {
        name: String,
        visibility: Visibility,
    },
    /// `private_class_method :m` / `public_class_method :m` naming a class
    /// method NOT defined earlier in the same body -- one inherited from the
    /// superclass, or `new` itself. The same-body case is retagged in place at
    /// lowering time; this variant defers the rest, exactly as
    /// [`HirNode::MethodVisibility`] does for the instance half.
    ClassMethodVisibility {
        name: String,
        visibility: Visibility,
    },
    /// `module_function :name` where `name` is INHERITED rather than defined
    /// in this body -- `erb/util.rb`'s `include ERB::Escape; module_function
    /// :html_escape`. A name the body does define is retagged in place at
    /// lowering time; this variant defers the rest to `mro`, which can only
    /// find the source once ancestors are linearized. The instance copy stays
    /// (the mixin half of `module_function`), and a module method is added
    /// alongside it.
    ModuleFunction(String),
    /// `private_constant :A, :B` / `public_constant :A` in a class body.
    ///
    /// Recognized at LOWERING time rather than left as a runtime call, because
    /// the reference it has to reject -- a qualified `M::A` from outside `M` --
    /// is one zeo resolves statically. Codegen compares the reading scope's
    /// cref against the owner and raises there; the runtime half only has to
    /// keep `Module#constants` and `defined?` honest.
    ConstantVisibility {
        names: Vec<String>,
        private: bool,
    },
    /// The last-match specials: `$~`, `$1`..`$9`, `$&`, `` $` ``, `$'`.
    ///
    /// NOT `GlobalRead`, even though they are spelled like globals: nothing
    /// ever assigns them (a regexp match does, as a side effect), and they
    /// read off a dedicated runtime slot rather than the `$foo` table. See
    /// `zeo_rt::lastmatch`, including the frame-locality divergence.
    LastMatchRef(LastMatch),
    /// A statement sequence evaluated in order, answering its LAST
    /// statement's value. `emit_body` emits it as one tail-value Rust block,
    /// the same shape `Eval` uses.
    ///
    /// Two sources reach here:
    ///   - a parenthesized multi-statement expression in real source
    ///     (`x = (a; b)`, `(puts "hi"; 42)`);
    ///   - lowering, which binds a compound-assignment target's receiver and
    ///     index expressions to a hidden local exactly once before reading
    ///     and writing through them (`obj.attr += 1`, `arr[i] ||= 1`). This
    ///     matches Ruby's evaluate-the-receiver-once rule; see
    ///     `parse::lower_call_operator_write`.
    ///
    /// It introduces no scope of its own. A local assigned inside stays
    /// readable afterwards, which is both what Ruby does with `(a; b)` and
    /// what the compound-assignment lowering needs from its hidden locals.
    ///
    /// Distinct from `Eval`, a literal `eval("...")`'s spliced body, because
    /// the two mean different things to a reader, not because they generate
    /// differently.
    Seq(Vec<NodeId>),
    /// `left..right` / `left...right` used AS a condition -- Ruby's
    /// flip-flop, a two-state latch rather than a Range.
    ///
    /// Off, it evaluates `left` and turns on when that is truthy. On, it
    /// evaluates `right` and turns off when that is truthy. Either way it
    /// answers true whenever it is, or just became, on. The two-dot form
    /// also tests `right` in the evaluation that turned it on, so
    /// `(i == 3)..(i == 3)` is true for one iteration. The three-dot form
    /// (`exclusive`) waits for the next one.
    ///
    /// prism mints this node only in a conditional position -- a `..`
    /// anywhere else is an ordinary `RangeLit` -- so no context flag is
    /// needed. An omitted side is nil, hence falsy: `..(i == 3)` never turns
    /// on and `(i == 2)..` never turns off, both oracle-verified.
    ///
    /// `state` indexes the runtime's latch table, minted per SYNTACTIC
    /// occurrence. That is what Ruby scopes the latch to: two flip-flops in
    /// one loop body keep separate state, and one flip-flop keeps its state
    /// across separate runs of its loop.
    FlipFlop {
        state: u32,
        left: NodeId,
        right: NodeId,
        exclusive: bool,
    },
}

impl HirNode {
    /// Whether ONLY a `class`/`module` body can hold this node.
    ///
    /// These are the class-body directives: the lowerer emits them from the
    /// static class path, and codegen consumes them there and nowhere else. A
    /// class built at RUNTIME (`Class.new { ... }`, a `class_eval` reopen) runs
    /// its body as an ordinary block, so each one has to be rewritten into the
    /// equivalent self-send first -- see
    /// `lower::defs::transform_runtime_class_body`, which refuses to pass an
    /// unrewritten directive through.
    ///
    /// The two halves used to be independent lists and drifted apart silently:
    /// a directive added to the static path but not the runtime one reached
    /// codegen as a "top-level-only node in expression position". Exhaustive
    /// here, with no `_` catch-all, so a new variant has to be classified.
    pub fn is_class_body_directive(&self) -> bool {
        match self {
            HirNode::Include(_)
            | HirNode::Extend(_)
            | HirNode::Prepend(_)
            | HirNode::ClassMethodPrepend(_)
            | HirNode::Refine { .. }
            | HirNode::FeatureLoaded { .. }
            | HirNode::CExtLoaded { .. }
            | HirNode::Undef(_)
            | HirNode::ClassMethodUndef(_)
            | HirNode::AliasMethod { .. }
            | HirNode::MethodVisibility { .. }
            | HirNode::ClassMethodVisibility { .. }
            | HirNode::ConstantVisibility { .. }
            | HirNode::ModuleFunction(_) => true,

            // `ClassDef` and `DefMethod` are class-body shapes too, but both
            // also stand on their own at the top level and inside a method,
            // and codegen emits them in block position -- the runtime path
            // rewrites the former only to give it a runtime superclass.
            HirNode::ClassDef { .. }
            | HirNode::DefMethod { .. }
            | HirNode::Program(_)
            | HirNode::IntegerLit(_)
            | HirNode::BigIntegerLit { .. }
            | HirNode::RationalLit { .. }
            | HirNode::ImaginaryLit(_)
            | HirNode::FloatLit(_)
            | HirNode::SymbolLit(_)
            | HirNode::NilLit
            | HirNode::BoolLit(_)
            | HirNode::And(..)
            | HirNode::Or(..)
            | HirNode::Defined(_)
            | HirNode::NotNil(_)
            | HirNode::If { .. }
            | HirNode::CaseWhen { .. }
            | HirNode::ArrayLit(_)
            | HirNode::HashLit(_)
            | HirNode::RangeLit { .. }
            | HirNode::StringLit(_)
            | HirNode::RegexpLit(..)
            | HirNode::LocalRead(_)
            | HirNode::LocalWrite(..)
            | HirNode::IvarRead(_)
            | HirNode::IvarWrite(..)
            | HirNode::ClassVarRead(_)
            | HirNode::ClassVarWrite(..)
            | HirNode::ClassRef(_)
            | HirNode::Call { .. }
            | HirNode::New { .. }
            | HirNode::SuperCall { .. }
            | HirNode::Block { .. }
            | HirNode::Lambda { .. }
            | HirNode::While { .. }
            | HirNode::Loop { .. }
            | HirNode::For { .. }
            | HirNode::Break(_)
            | HirNode::Next(_)
            | HirNode::Redo
            | HirNode::MultiWrite { .. }
            | HirNode::Ffi(_)
            | HirNode::BoxScope { .. }
            | HirNode::BoxHandle(_)
            | HirNode::FileEnd(_)
            | HirNode::Return(_)
            | HirNode::Yield(_)
            | HirNode::BlockGiven
            | HirNode::SelfRef
            | HirNode::Raise(..)
            | HirNode::CaseIn { .. }
            | HirNode::MatchPredicate { .. }
            | HirNode::MatchRequired { .. }
            | HirNode::Begin { .. }
            | HirNode::Retry
            | HirNode::GlobalRead(_)
            | HirNode::GlobalWrite(..)
            | HirNode::QualifiedConstRead(..)
            | HirNode::ConstReadOrNil(..)
            | HirNode::ConstWrite { .. }
            | HirNode::DynConstRead { .. }
            | HirNode::DynConstWrite { .. }
            | HirNode::PreExec(_)
            | HirNode::AliasGlobal(..)
            | HirNode::LastMatchRef(_)
            | HirNode::Seq(_)
            // `using` stands on its own at the top level far more often than
            // in a class body, and it edits nothing about the class -- it is
            // an ordinary statement whose only effect is compile-time.
            | HirNode::Using(_)
            // A definition REPORT is not a directive: it edits nothing about
            // the class and runs as an ordinary send at its position, which is
            // exactly what the runtime-class-body rewrite wants of it.
            | HirNode::DefHook { .. }
            | HirNode::MethodRedefine { .. }
            | HirNode::MethodReveal(..)
            | HirNode::FlipFlop { .. } => false,
        }
    }

    /// Every child node this one owns, in evaluation order.
    ///
    /// The name-collecting passes (`analyze::collect_ivars`,
    /// `mro::collect_cvars`, `mro::collect_const_refs`) each used to carry
    /// their own copy of this walk, and the copies drifted: one missed a
    /// call's keyword arguments, another a `rescue *errs` splat, a third a
    /// block parameter's default. Every such omission is a silently WRONG
    /// answer, never a crash. One exhaustive match with no `..` rest pattern
    /// is what makes a new variant -- or a new field on an existing one --
    /// a compile error instead.
    ///
    /// `ClassDef`/`DefMethod` bodies are children like any other. A pass that
    /// must stop at a fresh Ruby scope matches those variants ahead of its
    /// fall-through to here.
    pub fn for_each_child(&self, visit: &mut impl FnMut(NodeId)) {
        fn each(ids: impl IntoIterator<Item = NodeId>, visit: &mut impl FnMut(NodeId)) {
            ids.into_iter().for_each(visit);
        }
        fn elems(es: &[ArrayElem], visit: &mut impl FnMut(NodeId)) {
            for e in es {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                visit(*n);
            }
        }
        fn kws(ks: &[KwArg], visit: &mut impl FnMut(NodeId)) {
            ks.iter().flat_map(|kw| kw.node_ids()).for_each(visit);
        }
        fn defaults(p: &Params, visit: &mut impl FnMut(NodeId)) {
            p.default_ids().into_iter().for_each(visit);
        }
        fn parts(ps: &[StrPart], visit: &mut impl FnMut(NodeId)) {
            for p in ps {
                if let StrPart::Interp(n) = p {
                    visit(*n);
                }
            }
        }
        match self {
            HirNode::Program(body)
            | HirNode::PreExec(body)
            | HirNode::Seq(body)
            | HirNode::Loop { body }
            | HirNode::BoxScope { box_id: _, body } => each(body.iter().copied(), visit),
            HirNode::Ffi(call) => call.args.iter().for_each(|(a, _)| visit(*a)),
            HirNode::ImaginaryLit(inner) => visit(*inner),
            HirNode::And(l, r)
            | HirNode::Or(l, r)
            | HirNode::FlipFlop {
                state: _,
                left: l,
                right: r,
                exclusive: _,
            } => {
                visit(*l);
                visit(*r);
            }
            HirNode::Defined(v) | HirNode::NotNil(v) => visit(*v),
            HirNode::If {
                cond,
                then_body,
                else_body,
            } => {
                visit(*cond);
                each(then_body.iter().copied(), visit);
                each(else_body.iter().copied(), visit);
            }
            HirNode::CaseWhen {
                subject,
                arms,
                else_body,
            } => {
                each(subject.iter().copied(), visit);
                for (values, body) in arms {
                    elems(values, visit);
                    each(body.iter().copied(), visit);
                }
                each(else_body.iter().copied(), visit);
            }
            HirNode::ArrayLit(es) | HirNode::Yield(es) => elems(es, visit),
            HirNode::HashLit(pairs) => kws(pairs, visit),
            HirNode::RangeLit {
                start,
                end,
                exclusive: _,
            } => each(start.iter().chain(end).copied(), visit),
            HirNode::StringLit(ps) | HirNode::RegexpLit(ps, _) => parts(ps, visit),
            HirNode::LocalWrite(_, value)
            | HirNode::IvarWrite(_, value)
            | HirNode::ClassVarWrite(_, value)
            | HirNode::GlobalWrite(_, value)
            | HirNode::ConstWrite {
                scope: _,
                name: _,
                value,
            } => visit(*value),
            HirNode::DynConstRead {
                scope,
                name: _,
                lenient: _,
            } => visit(*scope),
            HirNode::DynConstWrite {
                scope,
                name: _,
                value,
            } => {
                visit(*scope);
                visit(*value);
            }
            HirNode::Call {
                receiver,
                name: _,
                args,
                kwargs,
                block,
                block_arg,
                safe: _,
            } => {
                each(receiver.iter().copied(), visit);
                elems(args, visit);
                kws(kwargs, visit);
                each(block.iter().chain(block_arg).copied(), visit);
            }
            HirNode::New {
                class_name: _,
                args,
                kwargs,
                block,
            } => {
                each(args.iter().copied(), visit);
                kws(kwargs, visit);
                each(block.iter().copied(), visit);
            }
            HirNode::SuperCall {
                args,
                kwargs,
                zsuper: _,
                block,
                block_arg,
            } => {
                args.iter().for_each(|a| visit(a.node_id()));
                kws(kwargs, visit);
                each(block.iter().chain(block_arg).copied(), visit);
            }
            HirNode::Block { params: p, body } => {
                defaults(p, visit);
                each(body.iter().copied(), visit);
            }
            HirNode::Lambda {
                params: p,
                body,
                method_body: _,
            } => {
                defaults(p, visit);
                each(body.iter().copied(), visit);
            }
            HirNode::ClassDef {
                name: _,
                superclass: _,
                body,
                is_module: _,
            } => each(body.iter().copied(), visit),
            HirNode::DefMethod {
                name: _,
                params: p,
                body,
                is_class_method: _,
                visibility: _,
                is_def: _,
            } => {
                defaults(p, visit);
                each(body.iter().copied(), visit);
            }
            HirNode::While {
                cond,
                body,
                negate: _,
                post: _,
            } => {
                visit(*cond);
                each(body.iter().copied(), visit);
            }
            HirNode::For {
                target,
                iterable,
                body,
            } => {
                target.for_each_node(visit);
                visit(*iterable);
                each(body.iter().copied(), visit);
            }
            HirNode::MultiWrite { targets, value } => {
                targets.for_each_node(visit);
                visit(*value);
            }
            HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
                each(v.iter().copied(), visit)
            }
            HirNode::Raise(args, cause) => {
                each(args.iter().copied(), visit);
                each(raise_cause_node(cause), visit);
            }
            HirNode::CaseIn {
                subject,
                arms,
                else_body,
            } => {
                visit(*subject);
                for arm in arms {
                    arm.pattern.for_each_node(visit);
                    arm.guard.iter().for_each(|(g, _)| visit(*g));
                    each(arm.body.iter().copied(), visit);
                }
                else_body
                    .iter()
                    .for_each(|b| each(b.iter().copied(), visit));
            }
            HirNode::MatchPredicate { subject, pattern }
            | HirNode::MatchRequired { subject, pattern } => {
                visit(*subject);
                pattern.for_each_node(visit);
            }
            HirNode::Begin {
                body,
                rescues,
                else_body,
                ensure_body,
            } => {
                each(body.iter().copied(), visit);
                for r in rescues {
                    each(r.splats.iter().copied(), visit);
                    each(r.body.iter().copied(), visit);
                }
                else_body
                    .iter()
                    .chain(ensure_body)
                    .for_each(|b| each(b.iter().copied(), visit));
            }
            HirNode::IntegerLit(_)
            | HirNode::BigIntegerLit {
                negative: _,
                digits: _,
            }
            | HirNode::RationalLit {
                negative: _,
                num_digits: _,
                den_digits: _,
            }
            | HirNode::FloatLit(_)
            | HirNode::SymbolLit(_)
            | HirNode::NilLit
            | HirNode::BoolLit(_)
            | HirNode::BoxHandle(_)
            | HirNode::FileEnd(_)
            | HirNode::SelfRef
            | HirNode::BlockGiven
            | HirNode::Redo
            | HirNode::Retry
            | HirNode::LocalRead(_)
            | HirNode::IvarRead(_)
            | HirNode::ClassVarRead(_)
            | HirNode::ClassRef(_)
            | HirNode::GlobalRead(_)
            | HirNode::LastMatchRef(_)
            | HirNode::QualifiedConstRead(_, _)
            | HirNode::ConstReadOrNil(_, _)
            | HirNode::Include(_)
            | HirNode::Extend(_)
            | HirNode::Prepend(_)
            | HirNode::ClassMethodPrepend(_)
            | HirNode::Refine { .. }
            | HirNode::Using(_)
            | HirNode::DefHook { .. }
            | HirNode::FeatureLoaded { .. }
            | HirNode::CExtLoaded { .. }
            | HirNode::MethodRedefine { .. }
            | HirNode::MethodReveal(..)
            | HirNode::Undef(_)
            | HirNode::ClassMethodUndef(_)
            | HirNode::AliasGlobal(_, _)
            | HirNode::AliasMethod {
                new_name: _,
                old_name: _,
                is_class_method: _,
            }
            | HirNode::MethodVisibility {
                name: _,
                visibility: _,
            }
            | HirNode::ClassMethodVisibility {
                name: _,
                visibility: _,
            }
            | HirNode::ModuleFunction(_)
            | HirNode::ConstantVisibility { .. } => {}
        }
    }
}

/// Split a written constant PATH into the `(scope, leaf)` pair a constant
/// read takes: `"M::ALIAS"` -> `(Some("M"), "ALIAS")`.
pub(crate) fn split_const_path(path: &str) -> (Option<&str>, &str) {
    match path.rsplit_once("::") {
        Some((scope, leaf)) => (Some(scope), leaf),
        None => (None, path),
    }
}

/// The Ruby spelling of a definition-level node, for the rejection an emitter
/// raises when one reaches value position.
pub(crate) fn definition_kind(node: &HirNode) -> &'static str {
    match node {
        HirNode::Program(_) => "a program body",
        HirNode::ClassDef {
            is_module: true, ..
        } => "a module definition",
        HirNode::ClassDef { .. } => "a class definition",
        HirNode::DefMethod { .. } => "a method definition",
        HirNode::Refine { .. } => "refine",
        HirNode::Undef(_) => "undef",
        HirNode::ClassMethodUndef(_) => "undef on a singleton class",
        HirNode::AliasMethod { .. } => "alias",
        HirNode::MethodVisibility { .. } => "a visibility directive",
        HirNode::ClassMethodVisibility { .. } => "a class-method visibility directive",
        HirNode::ModuleFunction(_) => "module_function",
        HirNode::MethodRedefine { .. } => "a method redefinition",
        HirNode::ConstantVisibility { .. } => "a constant visibility directive",
        HirNode::Include(_) => "include",
        HirNode::Extend(_) => "extend",
        HirNode::Prepend(_) | HirNode::ClassMethodPrepend(_) => "prepend",
        HirNode::Using(_) => "using",
        HirNode::Call { name, .. } if name.starts_with("attr_") => "an attribute reader/writer",
        HirNode::Call { .. } => "a call the walk consumed",
        _ => "a construct with no value form",
    }
}
