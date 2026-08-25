use super::*;

/// A `case/in` pattern -- a small, directly-recursive tree, deliberately NOT
/// reusing `HirNode`/`NodeId` for every position: a pattern's leaves have
/// fundamentally different semantics than an expression's (a bare
/// identifier BINDS in pattern position, it doesn't READ), so giving it its
/// own type avoids overloading `HirNode::LocalRead`/`LocalWrite` with a
/// meaning they don't otherwise have. Literal sub-expressions (a pinned
/// value, a range endpoint, a plain literal/expression matched via `===`)
/// still lower through the ordinary `NodeId`/`lower_node` path -- only the
/// PATTERN SHAPE itself is bespoke.
#[derive(Debug, Clone)]
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
    /// binding. See `clif/patterns.rs`'s `class_check` for how
    /// this resolves both built-in primitive names (`Integer`/`String`/
    /// `Symbol`/`Array`/`Hash`/`Range`/`Proc`/`NilClass`/`TrueClass`/
    /// `FalseClass`, checked via a runtime tag) and user-defined classes
    /// (checked via the same linearized `ancestors` list `is_a?`/`super`
    /// already consult).
    ClassCheck(String),
    /// `1..10` / `..5` / `1..` as a pattern -- "does the scrutinee fall
    /// inside this range", not `rb_eq`. Lowered as a real `Range` value
    /// matched via `===` -- see `clif/patterns.rs`'s
    /// `range_value`/`case_eq_check`.
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
    /// reaches the emitter every nested `Pattern` here is guaranteed
    /// binding-free.
    Or(Vec<Pattern>),
    /// `PAT => name` -- binds `name` to the scrutinee ONLY IF `PAT` itself
    /// matches (unlike a bare `Bind`, which always matches). The common,
    /// load-bearing shape is `Capture(Box::new(ClassCheck(_)), name)`
    /// (`in Integer => n`).
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
#[derive(Debug, Clone)]
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
    /// branch's locals do (no new Ruby scope), so the whole-scope local
    /// collection (`analyze::local_storage::collect_locals`) needs to see
    /// every one of them up front, same as `HirNode::MultiWrite`'s
    /// targets. Shared by that collection pass and the capture scan.
    pub fn for_each_bound_name(&self, visit: &mut impl FnMut(&str)) {
        match self {
            Pattern::Bind(name) => visit(name),
            Pattern::Value(_)
            | Pattern::Pin(_)
            | Pattern::ClassCheck(_)
            | Pattern::Range { .. } => {}
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
            Pattern::Array {
                pre, rest, post, ..
            } => {
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
#[derive(Debug, Clone)]
pub struct PatternArm {
    pub pattern: Pattern,
    /// `(condition, is_unless)` -- `unless` negates the same way `HirNode::While`'s
    /// `negate` flag does, rather than being a separate boolean-inverted
    /// node kind.
    pub guard: Option<(NodeId, bool)>,
    pub body: Vec<NodeId>,
}
