//! Lowers a `ruby_prism::Node` tree straight into `Hir` -- the direct analog
//! of `spinel_parse.c`'s `flatten()` + `node_table.c`'s `nt_load_text()`
//! combined into one in-process step (no text-serialization round-trip; see
//! `hir.rs`'s module docs for why spinel needed that step and we don't).
//!
//! Covers exactly the node kinds the spike's 7 examples exercise; anything
//! else is a clean `Err` (mirroring spinel's `unsupported(c, id, "...")`
//! convention), not a panic.

mod loader;
mod rename;

use crate::hir::{
    ArrayElem, HashPair, HashPatternRest, Hir, HirNode, KeywordParam, NodeId, Params, Pattern,
    PatternArm, RegexpFlags, RescueClause, StrPart, Visibility,
};
use ruby_prism::{Node, ParseResult};

type PResult<T> = Result<T, String>;

/// The built-in exception hierarchy (Part 6's minimal "raise/exception
/// foundation", extended to spinel's own ~20-class set in Phase 9) --
/// ordinary Ruby source, spliced into EVERY compiled program ahead of the
/// user's own code via the exact same `parse_and_lower_into` mechanism
/// `eval`'s literal-splice already uses (see that recognizer's docs below).
/// This is the whole point: real classes/inheritance (Phase 7's MRO work)
/// already makes `class X < Y; end` meaningful, so the built-in hierarchy
/// needs ZERO dedicated Rust construction code -- it's just Ruby, using the
/// same machinery a user's own classes do. A custom hierarchy (`class MyError
/// < StandardError; def initialize(x); super(...); @x = x; end; end`) needs
/// NO new machinery either -- it's just ordinary inheritance + materialized
/// `super`, already built in Phase 7. `msg` is a plain REQUIRED param,
/// not a Ruby-level default (`msg = "..."`, which real Ruby's own
/// `Exception.new` supports) -- every construction site this spike
/// generates (`raise`'s codegen -- see `codegen::expr::emit_raise_value`)
/// always supplies a message explicitly (the raising class's own name as a
/// compile-time string literal, when `raise` itself gave none), so this
/// narrower shape avoids `codegen::call::emit_new`'s pre-existing,
/// unrelated gap: it always passes constructor args 1:1 positionally,
/// with no optional-argument `Some(...)`-wrapping smarts (fine for
/// required-only signatures like this one; a separate, unrelated fix if a
/// class's own `initialize` needs real optional-param support via `.new`).
const EXCEPTION_PRELUDE: &str = r#"
class Exception
  def initialize(msg)
    @message = msg
  end
  def message
    @message
  end
  def to_s
    @message
  end
end
class ScriptError < Exception
end
class NotImplementedError < ScriptError
end
class LoadError < ScriptError
end
class StandardError < Exception
end
class ArgumentError < StandardError
end
class EncodingError < StandardError
end
class IOError < StandardError
end
class EOFError < IOError
end
class IndexError < StandardError
end
class KeyError < IndexError
end
class StopIteration < IndexError
end
class NameError < StandardError
end
class NoMethodError < NameError
end
class RangeError < StandardError
end
class RegexpError < StandardError
end
class RuntimeError < StandardError
end
class FrozenError < RuntimeError
end
class NoMatchingPatternError < StandardError
end
class FiberError < StandardError
end
class ThreadError < StandardError
end
class ClosedQueueError < StopIteration
end
class RactorError < StandardError
end
class TypeError < StandardError
end
class ZeroDivisionError < StandardError
end
"#;

/// Returns the built `Hir` plus the id of its `Program` root -- `Hir` itself
/// doesn't track a root (it's just an arena), so lowering hands the root id
/// back explicitly rather than requiring callers to know it's always the
/// last-pushed node.
pub fn parse_and_lower(source: &str) -> PResult<(Hir, NodeId)> {
    parse_and_lower_with(source, None, &[], &[])
}

/// `parse_and_lower` plus the file context Phase 14.1's compile-time
/// `require` resolution needs: `input_path` (the requiring-file directory
/// for the main file's own `require_relative` calls -- `None` means any
/// `require_relative` fails with CRuby's "cannot infer basepath") and the
/// ordered `-I` search roots for plain `require`. The main file's
/// statements go through `loader::lower_main_file` (which recognizes the
/// require/load call shapes at top-level statement position and splices
/// resolved files into this same arena); the exception prelude and `eval`
/// bodies keep going through `parse_and_lower_into`, where those shapes are
/// rejected by `lower_node` instead.
pub fn parse_and_lower_with(
    source: &str,
    input_path: Option<&std::path::Path>,
    load_roots: &[std::path::PathBuf],
    package_dirs: &[std::path::PathBuf],
) -> PResult<(Hir, NodeId)> {
    let mut hir = Hir::default();
    let mut statements = parse_and_lower_into(&mut hir, EXCEPTION_PRELUDE)
        .map_err(|e| format!("internal error in spinelc's built-in exception prelude (this is a spinelc bug): {e}"))?;
    statements.extend(loader::lower_main_file(
        &mut hir,
        source,
        input_path,
        load_roots,
        package_dirs,
    )?);
    let root = hir.push(HirNode::Program(statements));
    Ok((hir, root))
}

/// Parses `source` as a standalone program and lowers it into `hir`, which
/// may already contain other nodes -- the primitive both the top-level entry
/// point above and `eval`'s literal-splice call-shape recognizer (below) need.
/// File resolution/search-path concerns are deliberately NOT part of this --
/// it's purely "parse a string of Ruby into an existing arena".
fn parse_and_lower_into(hir: &mut Hir, source: &str) -> PResult<Vec<NodeId>> {
    let result = ruby_prism::parse(source.as_bytes());
    if let Some(err) = result.errors().next() {
        return Err(format!("parse error: {}", err.message()));
    }
    let program = result
        .node()
        .as_program_node()
        .ok_or("expected a top-level ProgramNode")?;
    lower_statement_list(&result, hir, program.statements().body())
}

fn lower_statement_list(
    result: &ParseResult,
    hir: &mut Hir,
    body: ruby_prism::NodeList<'_>,
) -> PResult<Vec<NodeId>> {
    body.iter().map(|n| lower_node(result, hir, &n)).collect()
}

/// `body` from a `def`/`class`/`if` -- may be `None` (empty body), a single
/// bare statement (prism doesn't wrap a one-statement body in
/// `StatementsNode`), or a real `StatementsNode`.
fn lower_body(result: &ParseResult, hir: &mut Hir, body: Option<Node<'_>>) -> PResult<Vec<NodeId>> {
    match body {
        None => Ok(Vec::new()),
        Some(n) => {
            if let Some(stmts) = n.as_statements_node() {
                lower_statement_list(result, hir, stmts.body())
            } else {
                Ok(vec![lower_node(result, hir, &n)?])
            }
        }
    }
}

/// `if`/`elsif`/`elsif`.../`else` is one `IfNode` per level, chained through
/// `subsequent()`: `None` (no further clauses), another `IfNode` (an
/// `elsif`), or an `ElseNode` (the final `else`). Recursing here builds the
/// same nesting `HirNode::If`'s `else_body` already expects -- an `elsif`
/// becomes a single-statement `else_body` containing the nested `If`.
fn lower_if_chain(
    result: &ParseResult,
    hir: &mut Hir,
    predicate: &Node<'_>,
    then_stmts: Option<ruby_prism::StatementsNode<'_>>,
    subsequent: Option<Node<'_>>,
) -> PResult<NodeId> {
    let cond = lower_node(result, hir, predicate)?;
    let then_body = lower_body(result, hir, then_stmts.map(|s| s.as_node()))?;
    let else_body = match subsequent {
        None => Vec::new(),
        Some(n) => {
            if let Some(elsif) = n.as_if_node() {
                vec![lower_if_chain(
                    result,
                    hir,
                    &elsif.predicate(),
                    elsif.statements(),
                    elsif.subsequent(),
                )?]
            } else if let Some(else_node) = n.as_else_node() {
                lower_body(result, hir, else_node.statements().map(|s| s.as_node()))?
            } else {
                return Err("expected `elsif` or `else` after `if` (spike scope)".to_string());
            }
        }
    };
    Ok(hir.push(HirNode::If {
        cond,
        then_body,
        else_body,
    }))
}

fn constant_name(node: &Node<'_>) -> PResult<String> {
    let cr = node
        .as_constant_read_node()
        .ok_or("expected a plain constant name (e.g. `Foo`, not `Foo::Bar`)")?;
    Ok(String::from_utf8_lossy(cr.name().as_slice()).into_owned())
}

/// `AliasMethodNode`'s `new_name`/`old_name` -- always a `SymbolNode` in
/// practice (confirmed via `Prism.parse`: both the bareword `alias new old`
/// and symbol `alias :new :old` spellings produce the identical node shape),
/// but checked defensively (a clean `Err`, not a panic) rather than assumed.
fn alias_target_name(node: &Node<'_>) -> PResult<String> {
    let sym = node
        .as_symbol_node()
        .ok_or("`alias`'s target must be a plain method name (spike scope)")?;
    Ok(String::from_utf8_lossy(sym.unescaped()).into_owned())
}

/// `Foo::BAR` (`ConstantPathNode`) -- resolves to `(scope, name)` for a
/// `HirNode::ConstWrite`/`QualifiedConstRead`'s fields. Only a single level
/// of explicit namespacing is supported (`parent`, if present, must itself
/// be a plain `Foo` -- matching this spike's flat, non-nested class/module
/// model, same restriction `constant_name` already enforces elsewhere);
/// `::Foo` (no `parent` at all -- an explicit top-level anchor) resolves
/// against `Object` directly, mirroring real Ruby's own representation of
/// top-level constants as living on `Object`.
fn constant_path_scope_and_name(node: &ruby_prism::ConstantPathNode<'_>) -> PResult<(String, String)> {
    let name = node
        .name()
        .ok_or("a `::` constant path with a dynamic/computed name isn't supported (spike scope)")?;
    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
    let scope = match node.parent() {
        None => "Object".to_string(),
        Some(p) => constant_name(&p)?,
    };
    Ok((scope, name))
}

/// The lvalue "storage kind" a compound-assignment (`+=`)/`||=`/`&&=`
/// operator can target -- factors their identical read-then-write desugar
/// (see the call sites in `lower_node` above) into one place instead of
/// five near-identical repetitions, one per underlying `HirNode` read/write
/// pair.
enum Storage {
    Local(String),
    Ivar(String),
    ClassVar(String),
    Global(String),
    /// `scope: None` = a bare, lexically-resolved name; `scope:
    /// Some(class_name)` = an explicit `Foo::NAME` -- see
    /// `HirNode::ConstWrite`'s docs.
    Const { scope: Option<String>, name: String },
}

impl Storage {
    fn read(&self, hir: &mut Hir) -> NodeId {
        match self {
            Storage::Local(n) => hir.push(HirNode::LocalRead(n.clone())),
            Storage::Ivar(n) => hir.push(HirNode::IvarRead(n.clone())),
            Storage::ClassVar(n) => hir.push(HirNode::ClassVarRead(n.clone())),
            Storage::Global(n) => hir.push(HirNode::GlobalRead(n.clone())),
            Storage::Const { scope: None, name } => hir.push(HirNode::ClassRef(name.clone())),
            Storage::Const {
                scope: Some(scope),
                name,
            } => hir.push(HirNode::QualifiedConstRead(scope.clone(), name.clone())),
        }
    }

    fn write(&self, hir: &mut Hir, value: NodeId) -> NodeId {
        match self {
            Storage::Local(n) => hir.push(HirNode::LocalWrite(n.clone(), value)),
            Storage::Ivar(n) => hir.push(HirNode::IvarWrite(n.clone(), value)),
            Storage::ClassVar(n) => hir.push(HirNode::ClassVarWrite(n.clone(), value)),
            Storage::Global(n) => hir.push(HirNode::GlobalWrite(n.clone(), value)),
            Storage::Const { scope, name } => hir.push(HirNode::ConstWrite {
                scope: scope.clone(),
                name: name.clone(),
                value,
            }),
        }
    }
}

/// `target op= rhs` -- e.g. `x += 1`, desugared to `x = x + 1` (evaluating
/// `rhs` unconditionally, unlike `||=`/`&&=` below).
fn lower_compound_op_write(hir: &mut Hir, target: Storage, op: String, rhs: NodeId) -> NodeId {
    let read = target.read(hir);
    let call = hir.push(HirNode::Call {
        receiver: Some(read),
        name: op,
        args: vec![ArrayElem::Single(rhs)],
        kwargs: Vec::new(),
        kwargs_splat: None,
        block: None,
        block_arg: None,
        safe: false,
    });
    target.write(hir, call)
}

/// `target ||= rhs` -- `target || (target = rhs)`, NOT `target = target ||
/// rhs`: `rhs` (and the write itself) must only be evaluated when `target`
/// is falsy, which `HirNode::Or`'s existing short-circuit codegen gives for
/// free.
///
/// A `Const` target is a genuine, narrow exception to reusing `Storage::read`
/// as-is (confirmed against real Ruby, not assumed): `CONST ||= v` on a
/// constant that was NEVER assigned quietly defines it, treating "never
/// assigned" as equivalent to a falsy read -- unlike an ordinary constant
/// read (`Storage::read`'s `ClassRef`/`QualifiedConstRead`), which always
/// raises `NameError` for that case, and unlike `CONST += v`/`CONST &&= v`
/// on the same undefined constant, which still DO raise (Ruby doesn't
/// extend this leniency to any other compound-assignment operator on a
/// constant). So only THIS function substitutes the lenient
/// `HirNode::ConstReadOrNil` for a `Const` target's read half --
/// `lower_and_write`/`lower_compound_op_write` deliberately keep using
/// `Storage::read` unchanged.
fn lower_or_write(hir: &mut Hir, target: Storage, rhs: NodeId) -> NodeId {
    let read = match &target {
        Storage::Const { scope, name } => hir.push(HirNode::ConstReadOrNil(scope.clone(), name.clone())),
        _ => target.read(hir),
    };
    let write = target.write(hir, rhs);
    hir.push(HirNode::Or(read, write))
}

/// `target &&= rhs` -- `target && (target = rhs)`; see `lower_or_write`'s
/// docs for the same "don't evaluate/write unless needed" reasoning.
fn lower_and_write(hir: &mut Hir, target: Storage, rhs: NodeId) -> NodeId {
    let read = target.read(hir);
    let write = target.write(hir, rhs);
    hir.push(HirNode::And(read, write))
}

/// Required-parameter-only helper for a `posts`/`requireds` entry -- both
/// only ever contain `RequiredParameterNode`s (Ruby's grammar guarantees a
/// splat's "post" params are always plain required names, same as the
/// params before it).
fn required_param_name(node: &Node<'_>, where_: &str) -> PResult<String> {
    let p = node
        .as_required_parameter_node()
        .ok_or_else(|| format!("only plain required parameters are supported {where_} (spike scope)"))?;
    Ok(String::from_utf8_lossy(p.name().as_slice()).into_owned())
}

/// Full `ParametersNode` lowering: required -> optional (default evaluated
/// LAZILY by the callee -- see `Params::optional`'s docs, so its expression
/// is only lowered here, never eagerly evaluated at every call site) ->
/// rest (`*`/`*name`) -> post (required params after a splat) -> keyword
/// (required/optional) -> keyword_rest (`**`/`**name`/explicit `**nil`) ->
/// `&block`/anonymous `&` (same `None`/`Some(None)`/`Some(Some(name))` shape
/// as `rest`/`keyword_rest` -- see `hir::Params::block`'s docs). Bare `...`
/// forwarding (positional + keyword + block all at once) is a separate,
/// still-unsupported call-site construct -- see the `keyword_rest` match arm
/// below, which gives it a dedicated rejection message.
fn lower_params(
    result: &ParseResult,
    hir: &mut Hir,
    params: Option<ruby_prism::ParametersNode<'_>>,
) -> PResult<Params> {
    let Some(params) = params else {
        return Ok(Params::default());
    };
    let block = params
        .block()
        .map(|b| b.name().map(|name| String::from_utf8_lossy(name.as_slice()).into_owned()));

    let required = params
        .requireds()
        .iter()
        .map(|n| required_param_name(&n, "before a `*rest`"))
        .collect::<PResult<Vec<_>>>()?;

    let optional = params
        .optionals()
        .iter()
        .map(|n| {
            let p = n
                .as_optional_parameter_node()
                .ok_or("expected an optional parameter (spike scope)")?;
            let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
            let default = lower_node(result, hir, &p.value())?;
            Ok((name, default))
        })
        .collect::<PResult<Vec<_>>>()?;

    let rest = match params.rest() {
        None => None,
        Some(n) => {
            let r = n.as_rest_parameter_node().ok_or(
                "`...` forwarding isn't supported yet (spike scope) -- needs a real Proc runtime, a later phase",
            )?;
            Some(r.name().map(|name| String::from_utf8_lossy(name.as_slice()).into_owned()))
        }
    };

    let post = params
        .posts()
        .iter()
        .map(|n| required_param_name(&n, "after a `*rest`"))
        .collect::<PResult<Vec<_>>>()?;

    let keywords = params
        .keywords()
        .iter()
        .map(|n| {
            if let Some(p) = n.as_required_keyword_parameter_node() {
                Ok(KeywordParam::Required(
                    String::from_utf8_lossy(p.name().as_slice()).into_owned(),
                ))
            } else if let Some(p) = n.as_optional_keyword_parameter_node() {
                let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
                let default = lower_node(result, hir, &p.value())?;
                Ok(KeywordParam::Optional(name, default))
            } else {
                Err("unsupported keyword parameter form (spike scope)".to_string())
            }
        })
        .collect::<PResult<Vec<_>>>()?;

    let keyword_rest = match params.keyword_rest() {
        None => None,
        // Bare `...` forwarding surfaces here as a `ForwardingParameterNode`
        // occupying the `keyword_rest` slot (confirmed empirically: `.rest()`
        // and `.block()` both come back `None` for it) -- not a real
        // `**`/`**name`, so give the dedicated forwarding message, not the
        // generic "unsupported keyword-rest parameter form" one below. Still
        // unsupported even though a real Proc runtime exists now (Phase 6):
        // `...` needs one call-site construct forwarding rest+keyword_rest+
        // block all at once, which hasn't been built -- a narrower gap than
        // before, not the same one.
        Some(n) if n.as_forwarding_parameter_node().is_some() => {
            return Err("`...` forwarding isn't supported yet (spike scope) -- it needs a dedicated call-site construct forwarding positional/keyword/block args all at once".to_string());
        }
        Some(n) if n.as_no_keywords_parameter_node().is_some() => {
            // `**nil` -- explicit "no extra keywords accepted". Treated the
            // same as "no keyword_rest at all": real Ruby raises
            // `ArgumentError` for an unexpected kwarg only when `**nil` is
            // present, which needs exceptions to matter (spike scope).
            None
        }
        Some(n) => {
            let r = n
                .as_keyword_rest_parameter_node()
                .ok_or("unsupported keyword-rest parameter form (spike scope)")?;
            Some(r.name().map(|name| String::from_utf8_lossy(name.as_slice()).into_owned()))
        }
    };

    Ok(Params {
        required,
        optional,
        rest,
        post,
        keywords,
        keyword_rest,
        block,
    })
}

/// Shared by `lower_block` and lambda lowering (`-> (x) { }`/`lambda { }`):
/// both a `BlockNode` and a `LambdaNode` expose their own `.parameters()` as
/// the identical `Option<Node>` shape (a `BlockParametersNode`, or the
/// `_1`/`it` sugar nodes -- confirmed via `Prism.parse` directly, not just
/// inferred from the bindings).
fn lower_block_like_params(result: &ParseResult, hir: &mut Hir, params: Option<Node<'_>>) -> PResult<Params> {
    match params {
        None => Ok(Params::default()),
        // `_1`/`_2`/... -- `NumberedParametersNode { maximum }` reports the
        // highest `_N` referenced in the body; synthesize that many plain
        // required params (pure lowering-time sugar, no new HIR).
        Some(p) if p.as_numbered_parameters_node().is_some() => {
            let n = p.as_numbered_parameters_node().unwrap().maximum();
            Ok(Params {
                required: (1..=n).map(|i| format!("_{i}")).collect(),
                ..Params::default()
            })
        }
        // `it` -- `ItParametersNode` carries no fields (the body just
        // references bare `it`); synthesize a single required param.
        Some(p) if p.as_it_parameters_node().is_some() => Ok(Params {
            required: vec!["it".to_string()],
            ..Params::default()
        }),
        Some(p) => {
            let bp = p
                .as_block_parameters_node()
                .ok_or("unsupported block parameter form (spike scope)")?;
            lower_params(result, hir, bp.parameters())
        }
    }
}

fn lower_block(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<NodeId> {
    let block = node
        .as_block_node()
        .ok_or("expected a block (`{ }` or `do..end`)")?;
    let params = lower_block_like_params(result, hir, block.parameters())?;
    let body = lower_body(result, hir, block.body())?;
    Ok(hir.push(HirNode::Block { params, body }))
}

fn lower_node(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<NodeId> {
    if let Some(int) = node.as_integer_node() {
        let text = std::str::from_utf8(result.as_slice(&int.location()))
            .map_err(|_| "integer literal is not valid UTF-8")?;
        let value: i64 = text.parse().map_err(|_| {
            format!(
                "unsupported integer literal `{text}` (spike only handles values that fit in i64)"
            )
        })?;
        return Ok(hir.push(HirNode::IntegerLit(value)));
    }

    if let Some(float) = node.as_float_node() {
        return Ok(hir.push(HirNode::FloatLit(float.value())));
    }

    // `-> (x) { ... }` -- a real `ruby-prism` node (unlike `lambda { }`
    // below, which is an ordinary method call). See `hir::HirNode::Lambda`'s
    // docs.
    if let Some(lambda) = node.as_lambda_node() {
        let params = lower_block_like_params(result, hir, lambda.parameters())?;
        let body = lower_body(result, hir, lambda.body())?;
        return Ok(hir.push(HirNode::Lambda { params, body }));
    }

    if let Some(sym) = node.as_symbol_node() {
        let name = String::from_utf8_lossy(sym.unescaped()).into_owned();
        return Ok(hir.push(HirNode::SymbolLit(name)));
    }

    if node.as_nil_node().is_some() {
        return Ok(hir.push(HirNode::NilLit));
    }
    if node.as_true_node().is_some() {
        return Ok(hir.push(HirNode::BoolLit(true)));
    }
    if node.as_false_node().is_some() {
        return Ok(hir.push(HirNode::BoolLit(false)));
    }
    if node.as_self_node().is_some() {
        return Ok(hir.push(HirNode::SelfRef));
    }

    // `(expr)` -- prism wraps a parenthesized expression in its own node
    // (not transparently folded away), distinct from the identically-shaped
    // `body: Option<Node>` on a `def`/`class`/`if` (see `lower_body`).
    // Multiple-statement parens (`(a; b)`) would need a first-class
    // "sequence of statements as one expression" HIR shape this spike
    // doesn't have yet -- narrower than real Ruby, a clean error rather
    // than silently dropping all but the last statement.
    if let Some(paren) = node.as_parentheses_node() {
        return match paren.body() {
            None => Err("empty parentheses `()` aren't supported yet (spike scope)".to_string()),
            Some(n) => match n.as_statements_node() {
                Some(stmts) => {
                    let body: Vec<_> = stmts.body().iter().collect();
                    match body.len() {
                        1 => lower_node(result, hir, &body[0]),
                        _ => Err(
                            "parenthesized multi-statement expressions aren't supported yet (spike scope)"
                                .to_string(),
                        ),
                    }
                }
                None => lower_node(result, hir, &n),
            },
        };
    }

    if let Some(lvr) = node.as_local_variable_read_node() {
        let name = String::from_utf8_lossy(lvr.name().as_slice()).into_owned();
        return Ok(hir.push(HirNode::LocalRead(name)));
    }

    // A bare `it` inside a block body (implicit-parameter sugar, distinct
    // from an ordinary local read at the `ruby-prism` level) -- matches the
    // synthesized `it` required-param name `lower_block` binds for
    // `ItParametersNode` blocks.
    if node.as_it_local_variable_read_node().is_some() {
        return Ok(hir.push(HirNode::LocalRead("it".to_string())));
    }

    if let Some(lvw) = node.as_local_variable_write_node() {
        let name = String::from_utf8_lossy(lvw.name().as_slice()).into_owned();
        let value = lower_node(result, hir, &lvw.value())?;
        return Ok(hir.push(HirNode::LocalWrite(name, value)));
    }

    if let Some(ivr) = node.as_instance_variable_read_node() {
        let name = String::from_utf8_lossy(ivr.name().as_slice()).into_owned();
        return Ok(hir.push(HirNode::IvarRead(name.trim_start_matches('@').to_string())));
    }

    if let Some(ivw) = node.as_instance_variable_write_node() {
        let name = String::from_utf8_lossy(ivw.name().as_slice()).into_owned();
        let value = lower_node(result, hir, &ivw.value())?;
        return Ok(hir.push(HirNode::IvarWrite(
            name.trim_start_matches('@').to_string(),
            value,
        )));
    }

    // `x += 1` / `@x += 1` / `@@x += 1` / `$x += 1` / `X += 1` -- desugars to
    // a plain read-operator-write, e.g. `x = x + 1`, reusing the existing
    // `*Read`/`*Write` + operator `Call` dispatch infrastructure entirely (no
    // new HIR node needed for the operator form itself, exactly like
    // `unless`/ternary reuse `If`). `||=`/`&&=` desugar to `Or`/`And` over the
    // same read/write pair (`a ||= b` is `a || (a = b)`, NOT `a = a || b` --
    // the RHS/assignment must not even be EVALUATED when `a` is already
    // truthy, which `HirNode::Or`'s existing short-circuit codegen already
    // gives for free). See `Storage`'s docs for why every one of these five
    // storage kinds shares this one desugar instead of five near-identical
    // repetitions.
    if let Some(op) = node.as_local_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_compound_op_write(hir, Storage::Local(name), op_name, rhs));
    }
    if let Some(op) = node.as_local_variable_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_and_write(hir, Storage::Local(name), rhs));
    }
    if let Some(op) = node.as_local_variable_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_or_write(hir, Storage::Local(name), rhs));
    }
    if let Some(op) = node.as_instance_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_compound_op_write(hir, Storage::Ivar(name), op_name, rhs));
    }
    if let Some(op) = node.as_instance_variable_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_and_write(hir, Storage::Ivar(name), rhs));
    }
    if let Some(op) = node.as_instance_variable_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_or_write(hir, Storage::Ivar(name), rhs));
    }
    if let Some(op) = node.as_class_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_compound_op_write(hir, Storage::ClassVar(name), op_name, rhs));
    }
    if let Some(op) = node.as_class_variable_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_and_write(hir, Storage::ClassVar(name), rhs));
    }
    if let Some(op) = node.as_class_variable_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_or_write(hir, Storage::ClassVar(name), rhs));
    }
    if let Some(op) = node.as_global_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_compound_op_write(hir, Storage::Global(name), op_name, rhs));
    }
    if let Some(op) = node.as_global_variable_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_and_write(hir, Storage::Global(name), rhs));
    }
    if let Some(op) = node.as_global_variable_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_or_write(hir, Storage::Global(name), rhs));
    }
    if let Some(gvr) = node.as_global_variable_read_node() {
        let name = String::from_utf8_lossy(gvr.name().as_slice()).into_owned();
        return Ok(hir.push(HirNode::GlobalRead(name)));
    }
    if let Some(gvw) = node.as_global_variable_write_node() {
        let name = String::from_utf8_lossy(gvw.name().as_slice()).into_owned();
        let value = lower_node(result, hir, &gvw.value())?;
        return Ok(hir.push(HirNode::GlobalWrite(name, value)));
    }
    if let Some(op) = node.as_constant_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_compound_op_write(hir, Storage::Const { scope: None, name }, op_name, rhs));
    }
    if let Some(op) = node.as_constant_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_and_write(hir, Storage::Const { scope: None, name }, rhs));
    }
    if let Some(op) = node.as_constant_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_or_write(hir, Storage::Const { scope: None, name }, rhs));
    }
    if let Some(cw) = node.as_constant_write_node() {
        let name = String::from_utf8_lossy(cw.name().as_slice()).into_owned();
        let value = lower_node(result, hir, &cw.value())?;
        return Ok(hir.push(HirNode::ConstWrite { scope: None, name, value }));
    }
    // `Foo::BAR` / `Foo::BAR = v` / `Foo::BAR += v` / `Foo::BAR ||= v` /
    // `Foo::BAR &&= v` -- an explicitly namespace-qualified constant
    // (`ConstantPathNode` and its write/operator-write/and-write/or-write
    // relatives). See `constant_path_scope_and_name`'s docs.
    if let Some(op) = node.as_constant_path_operator_write_node() {
        let (scope, name) = constant_path_scope_and_name(&op.target())?;
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_compound_op_write(hir, Storage::Const { scope: Some(scope), name }, op_name, rhs));
    }
    if let Some(op) = node.as_constant_path_and_write_node() {
        let (scope, name) = constant_path_scope_and_name(&op.target())?;
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_and_write(hir, Storage::Const { scope: Some(scope), name }, rhs));
    }
    if let Some(op) = node.as_constant_path_or_write_node() {
        let (scope, name) = constant_path_scope_and_name(&op.target())?;
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_or_write(hir, Storage::Const { scope: Some(scope), name }, rhs));
    }
    if let Some(cpw) = node.as_constant_path_write_node() {
        let (scope, name) = constant_path_scope_and_name(&cpw.target())?;
        let value = lower_node(result, hir, &cpw.value())?;
        return Ok(hir.push(HirNode::ConstWrite { scope: Some(scope), name, value }));
    }
    if let Some(cp) = node.as_constant_path_node() {
        let (scope, name) = constant_path_scope_and_name(&cp)?;
        return Ok(hir.push(HirNode::QualifiedConstRead(scope, name)));
    }

    // `obj.attr += rhs` / `obj.attr ||= rhs` / `obj.attr &&= rhs` -- evaluates
    // `obj` exactly ONCE (bound to a hidden local via `HirNode::Seq`), since
    // a receiver expression may have side effects (e.g. `get_obj().attr +=
    // 1`) -- naively re-lowering the SAME prism receiver node twice (once
    // for the getter call, once for the setter call) would silently
    // double-evaluate it, a real correctness bug real Ruby doesn't have. See
    // `bind_call_target_once`'s docs.
    if let Some(op) = node.as_call_operator_write_node() {
        let recv = op
            .receiver()
            .ok_or("`+=` on a method call with no receiver isn't supported (spike scope)")?;
        let read_name = String::from_utf8_lossy(op.read_name().as_slice()).into_owned();
        let write_name = String::from_utf8_lossy(op.write_name().as_slice()).into_owned();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        let (bind, read_call, tmp) = bind_call_target_once(result, hir, &recv, &read_name)?;
        let combined = hir.push(HirNode::Call {
            receiver: Some(read_call),
            name: op_name,
            args: vec![ArrayElem::Single(rhs)],
            kwargs: Vec::new(),
            kwargs_splat: None,
            block: None,
            block_arg: None,
            safe: false,
        });
        let write_call = build_call_target_write(hir, &tmp, &write_name, combined);
        return Ok(hir.push(HirNode::Seq(vec![bind, write_call])));
    }
    if let Some(op) = node.as_call_and_write_node() {
        let recv = op
            .receiver()
            .ok_or("`&&=` on a method call with no receiver isn't supported (spike scope)")?;
        let read_name = String::from_utf8_lossy(op.read_name().as_slice()).into_owned();
        let write_name = String::from_utf8_lossy(op.write_name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        let (bind, read_call, tmp) = bind_call_target_once(result, hir, &recv, &read_name)?;
        let write_call = build_call_target_write(hir, &tmp, &write_name, rhs);
        let and_node = hir.push(HirNode::And(read_call, write_call));
        return Ok(hir.push(HirNode::Seq(vec![bind, and_node])));
    }
    if let Some(op) = node.as_call_or_write_node() {
        let recv = op
            .receiver()
            .ok_or("`||=` on a method call with no receiver isn't supported (spike scope)")?;
        let read_name = String::from_utf8_lossy(op.read_name().as_slice()).into_owned();
        let write_name = String::from_utf8_lossy(op.write_name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        let (bind, read_call, tmp) = bind_call_target_once(result, hir, &recv, &read_name)?;
        let write_call = build_call_target_write(hir, &tmp, &write_name, rhs);
        let or_node = hir.push(HirNode::Or(read_call, write_call));
        return Ok(hir.push(HirNode::Seq(vec![bind, or_node])));
    }

    // `arr[i] += rhs` / `arr[i] ||= rhs` / `arr[i] &&= rhs` -- same
    // evaluate-once reasoning as the `obj.attr` forms above, extended to
    // BOTH the receiver and the (single) index argument (`arr[compute_idx()]
    // += 1` must call `compute_idx()` exactly once too). See
    // `bind_index_target_once`'s docs.
    if let Some(op) = node.as_index_operator_write_node() {
        let recv = op
            .receiver()
            .ok_or("`+=` on an indexing expression with no receiver isn't supported (spike scope)")?;
        let idx = single_index_argument(op.arguments())?;
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        let (binds, read_call, recv_tmp, idx_tmp) = bind_index_target_once(result, hir, &recv, &idx)?;
        let combined = hir.push(HirNode::Call {
            receiver: Some(read_call),
            name: op_name,
            args: vec![ArrayElem::Single(rhs)],
            kwargs: Vec::new(),
            kwargs_splat: None,
            block: None,
            block_arg: None,
            safe: false,
        });
        let write_call = build_index_target_write(hir, &recv_tmp, &idx_tmp, combined);
        let mut stmts = binds;
        stmts.push(write_call);
        return Ok(hir.push(HirNode::Seq(stmts)));
    }
    if let Some(op) = node.as_index_and_write_node() {
        let recv = op
            .receiver()
            .ok_or("`&&=` on an indexing expression with no receiver isn't supported (spike scope)")?;
        let idx = single_index_argument(op.arguments())?;
        let rhs = lower_node(result, hir, &op.value())?;
        let (binds, read_call, recv_tmp, idx_tmp) = bind_index_target_once(result, hir, &recv, &idx)?;
        let write_call = build_index_target_write(hir, &recv_tmp, &idx_tmp, rhs);
        let and_node = hir.push(HirNode::And(read_call, write_call));
        let mut stmts = binds;
        stmts.push(and_node);
        return Ok(hir.push(HirNode::Seq(stmts)));
    }
    if let Some(op) = node.as_index_or_write_node() {
        let recv = op
            .receiver()
            .ok_or("`||=` on an indexing expression with no receiver isn't supported (spike scope)")?;
        let idx = single_index_argument(op.arguments())?;
        let rhs = lower_node(result, hir, &op.value())?;
        let (binds, read_call, recv_tmp, idx_tmp) = bind_index_target_once(result, hir, &recv, &idx)?;
        let write_call = build_index_target_write(hir, &recv_tmp, &idx_tmp, rhs);
        let or_node = hir.push(HirNode::Or(read_call, write_call));
        let mut stmts = binds;
        stmts.push(or_node);
        return Ok(hir.push(HirNode::Seq(stmts)));
    }

    if let Some(and) = node.as_and_node() {
        let left = lower_node(result, hir, &and.left())?;
        let right = lower_node(result, hir, &and.right())?;
        return Ok(hir.push(HirNode::And(left, right)));
    }

    if let Some(or) = node.as_or_node() {
        let left = lower_node(result, hir, &or.left())?;
        let right = lower_node(result, hir, &or.right())?;
        return Ok(hir.push(HirNode::Or(left, right)));
    }

    if let Some(defined) = node.as_defined_node() {
        let value = lower_node(result, hir, &defined.value())?;
        return Ok(hir.push(HirNode::Defined(value)));
    }

    // `yield` / `yield(args)` -- a real, distinct `ruby-prism` node (not an
    // ordinary call), unlike `block_given?` below. Reuses `lower_call_args`
    // (not a bare per-argument `lower_node` map) so a trailing keyword hash
    // (`yield x: 1, y: 2`) is recognized the same way an ordinary call's
    // is -- `codegen::params::emit_proc_param_bindings` binds a block's own
    // keyword params from the LAST *positional* yielded value, matching
    // real Ruby's auto-conversion of a trailing Hash into block keywords,
    // so the peeled kwargs are folded back into one trailing `HashLit`.
    if let Some(yield_node) = node.as_yield_node() {
        let (arg_elems, kwargs, kwargs_splat) = lower_call_args(result, hir, yield_node.arguments())?;
        if kwargs_splat.is_some() {
            return Err("a `**h` double-splat argument isn't supported in `yield` (spike scope)".to_string());
        }
        let mut args = arg_elems
            .into_iter()
            .map(|e| match e {
                ArrayElem::Single(n) => Ok(n),
                ArrayElem::Splat(_) => {
                    Err("a `*expr` splat argument isn't supported in `yield` (spike scope)".to_string())
                }
            })
            .collect::<PResult<Vec<_>>>()?;
        if !kwargs.is_empty() {
            args.push(hir.push(HirNode::HashLit(kwargs)));
        }
        return Ok(hir.push(HirNode::Yield(args)));
    }

    if let Some(if_node) = node.as_if_node() {
        return lower_if_chain(
            result,
            hir,
            &if_node.predicate(),
            if_node.statements(),
            if_node.subsequent(),
        );
    }

    // `unless` has no `elsif` chain (only an optional `else`), and swaps
    // which body is which relative to `HirNode::If`: Ruby runs `unless`'s
    // primary statements when the predicate is FALSY, its `else` (if any)
    // when truthy -- the opposite of `if`.
    if let Some(unless_node) = node.as_unless_node() {
        let cond = lower_node(result, hir, &unless_node.predicate())?;
        let falsy_body = lower_body(result, hir, unless_node.statements().map(|s| s.as_node()))?;
        let truthy_body = match unless_node.else_clause() {
            None => Vec::new(),
            Some(e) => lower_body(result, hir, e.statements().map(|s| s.as_node()))?,
        };
        return Ok(hir.push(HirNode::If {
            cond,
            then_body: truthy_body,
            else_body: falsy_body,
        }));
    }

    // `case subject; when ...; else ...; end` -- value matching only.
    // `case/in` pattern matching (`CaseMatchNode`) is a distinct prism node,
    // not handled here (see the plan's Phase 8).
    if let Some(case_node) = node.as_case_node() {
        let subject = match case_node.predicate() {
            None => None,
            Some(p) => Some(lower_node(result, hir, &p)?),
        };
        let mut arms = Vec::new();
        for cond in case_node.conditions().iter() {
            let when = cond
                .as_when_node()
                .ok_or("expected a `when` clause inside `case` (spike scope)")?;
            let values = when
                .conditions()
                .iter()
                .map(|n| lower_node(result, hir, &n))
                .collect::<PResult<Vec<_>>>()?;
            let body = lower_body(result, hir, when.statements().map(|s| s.as_node()))?;
            arms.push((values, body));
        }
        let else_body = match case_node.else_clause() {
            None => Vec::new(),
            Some(e) => lower_body(result, hir, e.statements().map(|s| s.as_node()))?,
        };
        return Ok(hir.push(HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        }));
    }

    // `case subject; in PATTERN ... end` -- real pattern matching, a
    // distinct prism node (`CaseMatchNode`) from value-matching `case/when`
    // above. See `Pattern`/`PatternArm`'s docs.
    if let Some(case_match) = node.as_case_match_node() {
        let subject = case_match
            .predicate()
            .ok_or("`case/in` requires a subject (spike scope)")?;
        let subject = lower_node(result, hir, &subject)?;
        let mut arms = Vec::new();
        for cond in case_match.conditions().iter() {
            let in_node = cond
                .as_in_node()
                .ok_or("expected an `in` clause inside `case/in` (spike scope)")?;
            let (pattern, guard) = lower_in_pattern_and_guard(result, hir, &in_node.pattern())?;
            let body = lower_body(result, hir, in_node.statements().map(|s| s.as_node()))?;
            arms.push(PatternArm { pattern, guard, body });
        }
        let else_body = match case_match.else_clause() {
            None => None,
            Some(e) => Some(lower_body(result, hir, e.statements().map(|s| s.as_node()))?),
        };
        return Ok(hir.push(HirNode::CaseIn {
            subject,
            arms,
            else_body,
        }));
    }

    // `expr in pattern` -- boolean one-liner, never raises.
    if let Some(mp) = node.as_match_predicate_node() {
        let subject = lower_node(result, hir, &mp.value())?;
        let pattern = lower_pattern(result, hir, &mp.pattern())?;
        return Ok(hir.push(HirNode::MatchPredicate { subject, pattern }));
    }

    // `expr => pattern` -- rightward assignment, raises `NoMatchingPatternError`
    // on failure.
    if let Some(mr) = node.as_match_required_node() {
        let subject = lower_node(result, hir, &mr.value())?;
        let pattern = lower_pattern(result, hir, &mr.pattern())?;
        return Ok(hir.push(HirNode::MatchRequired { subject, pattern }));
    }

    if let Some(sup) = node.as_super_node() {
        let args = match sup.arguments() {
            None => Vec::new(),
            Some(a) => a
                .arguments()
                .iter()
                .map(|n| lower_node(result, hir, &n))
                .collect::<PResult<Vec<_>>>()?,
        };
        return Ok(hir.push(HirNode::SuperCall { args }));
    }

    // Bare `super` (no parens) -- a distinct prism node from `super(...)`
    // since it forwards the enclosing method's arguments implicitly. None of
    // the spike's examples pass args through a bare `super`, so it lowers to
    // the same `SuperCall { args: [] }` shape; forwarding real arguments is
    // deferred (see docs/PORTING_ANALYSIS.md).
    if node.as_forwarding_super_node().is_some() {
        return Ok(hir.push(HirNode::SuperCall { args: Vec::new() }));
    }

    if let Some(class) = node.as_class_node() {
        let name = constant_name(&class.constant_path())?;
        let superclass = match class.superclass() {
            None => None,
            Some(sc) => Some(constant_name(&sc)?),
        };
        let body = lower_class_body(result, hir, class.body())?;
        return Ok(hir.push(HirNode::ClassDef {
            name,
            superclass,
            body,
            is_module: false,
        }));
    }

    // `module Name ... end` -- see `HirNode::ClassDef`'s docs for why this
    // shares the same node as `class`. Nested modules/namespaced constant
    // paths (`module Foo::Bar`) aren't supported yet (spike scope, matching
    // today's existing top-level-only class restriction) -- `constant_name`
    // already rejects anything but a plain `ConstantReadNode`.
    if let Some(module) = node.as_module_node() {
        let name = constant_name(&module.constant_path())?;
        let body = lower_class_body(result, hir, module.body())?;
        return Ok(hir.push(HirNode::ClassDef {
            name,
            superclass: None,
            body,
            is_module: true,
        }));
    }

    if let Some(def) = node.as_def_node() {
        let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        // `def self.name` (`DefNode::receiver()` is `Some(SelfNode)`) is a
        // class method; any OTHER explicit receiver (`def SomeConst.name`,
        // reopening a class from outside its own body) is a clean rejection
        // -- see `HirNode::DefMethod`'s docs.
        let is_class_method = match def.receiver() {
            None => false,
            Some(r) if r.as_self_node().is_some() => true,
            Some(_) => {
                return Err("`def` with an explicit non-`self` receiver isn't supported yet (spike scope) -- only `def self.name` inside the class/module's own body".to_string());
            }
        };
        let params = lower_params(result, hir, def.parameters())?;
        let body = lower_body(result, hir, def.body())?;
        return Ok(hir.push(HirNode::DefMethod {
            name,
            params,
            body,
            is_class_method,
            // Only `lower_class_body_statement`'s own class-body-scoped
            // default-visibility tracking ever produces non-`Public` --
            // this generic path is reached for a top-level/nested `def`, or
            // one appearing as an ARGUMENT expression (`private def foo;
            // end` lowers its inner `def` through here, then
            // `lower_class_body_statement` retroactively mutates this same
            // node's `visibility` field once it sees the enclosing call).
            visibility: Visibility::Public,
        }));
    }

    // A bare constant used as a VALUE -- currently only meaningful as a call
    // receiver (`ClassName.foo`); see `HirNode::ClassRef`'s docs. Falls
    // through generically via the ordinary `Call` receiver-lowering path
    // below, so no change is needed there.
    if let Some(c) = node.as_constant_read_node() {
        let name = String::from_utf8_lossy(c.name().as_slice()).into_owned();
        return Ok(hir.push(HirNode::ClassRef(name)));
    }

    if let Some(cvar) = node.as_class_variable_read_node() {
        let name = String::from_utf8_lossy(cvar.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        return Ok(hir.push(HirNode::ClassVarRead(name)));
    }

    if let Some(cvar) = node.as_class_variable_write_node() {
        let name = String::from_utf8_lossy(cvar.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let value = lower_node(result, hir, &cvar.value())?;
        return Ok(hir.push(HirNode::ClassVarWrite(name, value)));
    }

    if let Some(call) = node.as_call_node() {
        let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();

        // `ClassName.new(args)` -- a distinct node; see hir.rs. The
        // concurrency builtins (`Fiber.new { }`, `Thread.new { }`,
        // `Mutex.new`, `Queue.new`) are deliberately NOT this shape:
        // `Fiber`/`Thread` must keep their BLOCK (the body), which
        // `HirNode::New` has no slot for, so all four fall through to the
        // generic `Call` lowering below (receiver becomes an ordinary
        // `ClassRef(name)`) and are intercepted by
        // `codegen::call::emit_call`'s builtin-constructor dispatch.
        if name == "new" {
            if let Some(recv) = call.receiver() {
                let class_name = constant_name(&recv)?;
                if !matches!(class_name.as_str(), "Fiber" | "Thread" | "Mutex" | "Queue" | "Ractor") {
                    let args = match call.arguments() {
                        None => Vec::new(),
                        Some(a) => a
                            .arguments()
                            .iter()
                            .map(|n| lower_node(result, hir, &n))
                            .collect::<PResult<Vec<_>>>()?,
                    };
                    return Ok(hir.push(HirNode::New { class_name, args }));
                }
            }
        }

        // `define_method(:literal) { block }` -- desugars to a plain
        // `DefMethod`, identical treatment to `def`, mirroring spinel's
        // `walk_scope`. Only reachable here with a literal symbol name and a
        // block; anything else (computed name, no block) falls through to
        // the generic `Call` case below and is a compile-time rejection --
        // the spike has no runtime "define a method on any class from
        // arbitrary code" path, only the two forms spinel itself supports
        // plus the literal-and-desugared one.
        if name == "define_method" && call.receiver().is_none() {
            if let (Some(args), Some(block_node)) = (call.arguments(), call.block()) {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if arg_list.len() == 1 {
                    if let Some(sym) = arg_list[0].as_symbol_node() {
                        let method_name = String::from_utf8_lossy(sym.unescaped()).into_owned();
                        let block = block_node
                            .as_block_node()
                            .ok_or("define_method's second argument must be a block")?;
                        let params = match block.parameters() {
                            None => Params::default(),
                            Some(p) => {
                                let bp = p
                                    .as_block_parameters_node()
                                    .ok_or("unsupported block parameter form")?;
                                lower_params(result, hir, bp.parameters())?
                            }
                        };
                        let body = lower_body(result, hir, block.body())?;
                        return Ok(hir.push(HirNode::DefMethod {
                            name: method_name,
                            params,
                            body,
                            is_class_method: false,
                            visibility: Visibility::Public,
                        }));
                    }
                }
            }
        }

        // `loop do ... end` -- `Kernel#loop` is an ordinary method call, not
        // syntax, so this is a lowering-time call-shape desugar exactly like
        // `define_method` above, not a distinct `ruby-prism` node. Only a
        // zero-arg, no-param-block `loop` desugars here; anything else (an
        // explicit receiver, arguments, or declared block params -- which
        // `Kernel#loop` never yields anyway) falls through to the generic
        // `Call` case and is handled as an ordinary (currently unsupported)
        // implicit-self call.
        if name == "loop" && call.receiver().is_none() {
            let no_args = call.arguments().is_none_or(|a| a.arguments().iter().next().is_none());
            if no_args {
                if let Some(block_node) = call.block() {
                    if let Some(block) = block_node.as_block_node() {
                        let has_params = block
                            .parameters()
                            .is_some_and(|p| p.as_block_parameters_node().is_some());
                        if !has_params {
                            let body = lower_body(result, hir, block.body())?;
                            return Ok(hir.push(HirNode::Loop { body }));
                        }
                    }
                }
            }
        }

        // `block_given?` -- an ordinary zero-arg, no-receiver `Kernel`
        // method call at the `ruby-prism` level (not a distinct node, unlike
        // `yield` above), so this is a lowering-time call-shape desugar
        // exactly like `loop`/`define_method`.
        if name == "block_given?" && call.receiver().is_none() {
            let no_args = call.arguments().is_none_or(|a| a.arguments().iter().next().is_none());
            if no_args && call.block().is_none() {
                return Ok(hir.push(HirNode::BlockGiven));
            }
        }

        // `lambda { ... }` / `lambda do ... end` -- an alternate spelling of
        // `-> { ... }` (an ordinary `Kernel` method call with a block, not a
        // distinct node, unlike `LambdaNode` above) -- same call-shape
        // desugar posture as `loop`/`block_given?`. Only a zero-arg,
        // literal-block call desugars here; anything else (an explicit
        // receiver, arguments, or a forwarded `&block`) falls through to an
        // ordinary `Call`, a clean rejection at codegen if `lambda` itself
        // isn't otherwise defined (matching `loop`'s identical posture).
        if name == "lambda" && call.receiver().is_none() {
            let no_args = call.arguments().is_none_or(|a| a.arguments().iter().next().is_none());
            if no_args {
                if let Some(block_node) = call.block() {
                    if let Some(block) = block_node.as_block_node() {
                        let params = lower_block_like_params(result, hir, block.parameters())?;
                        let body = lower_body(result, hir, block.body())?;
                        return Ok(hir.push(HirNode::Lambda { params, body }));
                    }
                }
            }
        }

        // `raise`/`fail` (exact synonyms) -- a zero/one/two positional-arg
        // call-shape desugar, same posture as `block_given?` above. The
        // `cause:` keyword form isn't lowered yet (see `HirNode::Raise`'s
        // docs) -- rejected here rather than silently dropped, matching
        // this project's "clean rejection over silent wrongness" rule.
        if (name == "raise" || name == "fail") && call.receiver().is_none() {
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if arg_list.iter().any(|n| n.as_keyword_hash_node().is_some()) {
                return Err("`raise`/`fail` with a `cause:` keyword argument isn't supported yet (spike scope) -- automatic cause chaining from an active `rescue` works once that lands (Phase 9); only the explicit override is deferred".to_string());
            }
            if arg_list.len() > 2 {
                return Err("`raise`/`fail` with more than 2 positional arguments isn't supported yet (spike scope)".to_string());
            }
            let args = arg_list
                .iter()
                .map(|n| lower_node(result, hir, n))
                .collect::<PResult<Vec<_>>>()?;
            return Ok(hir.push(HirNode::Raise(args)));
        }

        // `eval("literal string")` -- ONLY the compile-time-constant-string
        // form. Unlike `define_method`/`loop` above, this is intercepted
        // UNCONDITIONALLY: those two have a genuine second runtime path for
        // their non-desugared shape (an ordinary implicit-self `Call`), but
        // `eval` doesn't -- this spike has no runtime parser/interpreter (see
        // docs/EVAL_VM.md), so letting a non-literal `eval(...)` fall through
        // as a plain `Call` would compile cleanly and only fail at RUNTIME
        // with a confusing `NoMethodError`, strictly worse than a clear
        // compile-time rejection.
        // `require`/`require_relative`/`load` reaching THIS function means
        // the statement was NOT in direct top-level statement position (the
        // one place `parse::loader`'s file-level loop recognizes and
        // resolves them at compile time) -- a method body, a `begin` block,
        // a conditional, an `eval` body, a class body. Rejected
        // unconditionally, same reasoning as `eval` below: there is no
        // runtime loader, so falling through as a plain `Call` would
        // compile cleanly and only fail at RUNTIME with a confusing
        // `NoMethodError`. Notably this makes the `begin; require "x";
        // rescue LoadError; end` optional-dependency idiom a LOUD compile
        // error -- a documented divergence (a compile-time resolver has no
        // runtime LoadError to rescue).
        if call.receiver().is_none()
            && matches!(name.as_str(), "require" | "require_relative" | "load")
        {
            return Err(format!(
                "`{name}` is only supported as a top-level statement with a single string-literal argument (spike scope) -- it's resolved at compile time, so it can't appear inside a method, block, conditional, `begin`, or `eval` body"
            ));
        }
        // `autoload` registers a constant-triggered LAZY require -- the
        // trigger point (first constant ACCESS, from anywhere, at runtime)
        // has no faithful compile-time equivalent (eager splicing changes
        // top-level side-effect ordering; `defined?` doesn't trigger it) --
        // a clean rejection, not an approximation.
        if name == "autoload" && call.receiver().is_none() {
            return Err(
                "`autoload` isn't supported (spike scope) -- its lazy, first-constant-access trigger has no compile-time equivalent; use an explicit `require`"
                    .to_string(),
            );
        }

        if name == "eval" && call.receiver().is_none() {
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if arg_list.len() != 1 {
                return Err("`eval` is only supported with exactly one string-literal argument (spike scope) -- the `binding`/`filename`/`lineno` forms need `binding` support, which doesn't exist yet".to_string());
            }
            let arg_id = lower_node(result, hir, &arg_list[0])?;
            let Some(src) = literal_string_text(hir, arg_id) else {
                return Err("`eval` with a non-literal argument isn't supported yet (spike scope) -- only a plain string literal, e.g. `eval(\"1 + 2\")`, is currently accepted; dynamic `eval` needs a runtime parser/interpreter (see docs/EVAL_VM.md)".to_string());
            };
            let body = parse_and_lower_into(hir, &src).map_err(|e| format!("eval(\"...\"): {e}"))?;
            reject_top_level_defs(hir, &body)?;
            return Ok(hir.push(HirNode::Eval(body)));
        }

        let receiver = match call.receiver() {
            None => None,
            Some(r) => Some(lower_node(result, hir, &r)?),
        };
        let (args, kwargs, kwargs_splat) = lower_call_args(result, hir, call.arguments())?;
        // A call's `block()` slot is one of two distinct shapes: a literal
        // `{ }`/`do..end` (`BlockNode`), or `&existing_proc` forwarding an
        // already-built Proc value onward (`BlockArgumentNode`) -- real Ruby
        // syntax forbids a call from having both, so this is a clean
        // either/or, not a "prefer one" choice.
        let (block, block_arg) = match call.block() {
            None => (None, None),
            Some(b) => {
                if let Some(barg) = b.as_block_argument_node() {
                    let expr = match barg.expression() {
                        Some(e) => lower_node(result, hir, &e)?,
                        None => return Err(
                            "an anonymous `&` block-forwarding argument (forwarding the enclosing method's own `&block` onward without naming it) isn't supported yet (spike scope)".to_string(),
                        ),
                    };
                    (None, Some(expr))
                } else {
                    (Some(lower_block(result, hir, &b)?), None)
                }
            }
        };
        return Ok(hir.push(HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            kwargs_splat,
            block,
            block_arg,
            safe: call.is_safe_navigation(),
        }));
    }

    if let Some(s) = node.as_string_node() {
        let content = String::from_utf8_lossy(s.unescaped()).into_owned();
        return Ok(hir.push(HirNode::StringLit(vec![StrPart::Lit(content)])));
    }

    if let Some(istr) = node.as_interpolated_string_node() {
        let parts = istr
            .parts()
            .iter()
            .map(|part| lower_string_part(result, hir, &part))
            .collect::<PResult<Vec<_>>>()?;
        return Ok(hir.push(HirNode::StringLit(parts)));
    }

    // `/pattern/flags` / `%r{pattern}flags` (`RegularExpressionNode` covers
    // BOTH delimiter spellings -- prism only distinguishes opening/closing
    // `Location`s, not a separate node kind). `e`/`s` (EUC-JP/Windows-31J)
    // are a clean rejection: this spike is UTF-8-only throughout (see
    // `docs/limitations.md`), unlike `o`/`n`/`u`, which are harmless no-ops
    // here (`o`'s "only interpolate once" has no effect when every regex
    // literal is freshly constructed anyway; `n`/`u` just reassert the
    // encoding this spike already assumes).
    if let Some(re) = node.as_regular_expression_node() {
        if re.is_euc_jp() || re.is_windows_31j() {
            return Err(
                "a Regexp literal forcing a non-UTF-8 encoding (`/e`/`/s`) isn't supported yet (spike scope, UTF-8-only)".to_string(),
            );
        }
        let content = String::from_utf8_lossy(re.unescaped()).into_owned();
        return Ok(hir.push(HirNode::RegexpLit(
            vec![StrPart::Lit(content)],
            RegexpFlags {
                ignore_case: re.is_ignore_case(),
                extended: re.is_extended(),
                multiline: re.is_multi_line(),
            },
        )));
    }

    if let Some(re) = node.as_interpolated_regular_expression_node() {
        if re.is_euc_jp() || re.is_windows_31j() {
            return Err(
                "a Regexp literal forcing a non-UTF-8 encoding (`/e`/`/s`) isn't supported yet (spike scope, UTF-8-only)".to_string(),
            );
        }
        let parts = re
            .parts()
            .iter()
            .map(|part| lower_string_part(result, hir, &part))
            .collect::<PResult<Vec<_>>>()?;
        return Ok(hir.push(HirNode::RegexpLit(
            parts,
            RegexpFlags {
                ignore_case: re.is_ignore_case(),
                extended: re.is_extended(),
                multiline: re.is_multi_line(),
            },
        )));
    }

    // A bare regex literal used directly as an implicit condition against
    // `$_` (`if /foo/` -- `MatchLastLineNode`/its interpolated counterpart)
    // -- `$_`/the "last read line" concept isn't modeled at all, a clean
    // rejection rather than silently matching against an always-empty
    // string.
    if node.as_match_last_line_node().is_some() || node.as_interpolated_match_last_line_node().is_some() {
        return Err(
            "a bare Regexp literal used as an implicit condition (`if /foo/`, matching against `$_`) isn't supported yet (spike scope) -- write an explicit `=~`/`match?` against a real receiver instead".to_string(),
        );
    }

    // `/(?<name>...)/  =~ str` -- the named-capture auto-binding sugar
    // (`MatchWriteNode`), which synthesizes local-variable writes for every
    // named group. An ordinary `=~` with NO named captures is just a plain
    // `CallNode` (falls through to the generic `Call` handling further below,
    // dispatched by `codegen::call`'s Regexp/String arms) -- only this
    // auto-binding sugar itself is out of scope.
    if node.as_match_write_node().is_some() {
        return Err(
            "`=~`'s named-capture auto-binding sugar (synthesizing a local per named group) isn't supported yet (spike scope) -- bind the `MatchData` explicitly via `#match`/`#[]` instead".to_string(),
        );
    }

    if let Some(arr) = node.as_array_node() {
        let elements = arr
            .elements()
            .iter()
            .map(|el| lower_array_elem(result, hir, &el))
            .collect::<PResult<Vec<_>>>()?;
        return Ok(hir.push(HirNode::ArrayLit(elements)));
    }

    if let Some(h) = node.as_hash_node() {
        let pairs = h
            .elements()
            .iter()
            .map(|el| {
                let assoc = el.as_assoc_node().ok_or(
                    "double-splat (`**expr`) in a hash literal isn't supported yet (spike scope)",
                )?;
                let key = lower_node(result, hir, &assoc.key())?;
                let value = lower_node(result, hir, &assoc.value())?;
                Ok(HashPair(key, value))
            })
            .collect::<PResult<Vec<_>>>()?;
        return Ok(hir.push(HirNode::HashLit(pairs)));
    }

    if let Some(range) = node.as_range_node() {
        let start = match range.left() {
            None => None,
            Some(n) => Some(lower_node(result, hir, &n)?),
        };
        let end = match range.right() {
            None => None,
            Some(n) => Some(lower_node(result, hir, &n)?),
        };
        return Ok(hir.push(HirNode::RangeLit {
            start,
            end,
            exclusive: range.is_exclude_end(),
        }));
    }

    // `while`/`until`, both statement and modifier form -- `until` is `While`
    // with `negate: true`, exactly like `unless` swaps `If`'s branches above.
    // The do-while form (`begin...end while cond`) wraps an unhandled
    // `BeginNode` in its `statements`, so it already surfaces as a clean
    // "unsupported syntax" error from the recursive `lower_body` call below,
    // with no special detection needed here.
    if let Some(while_node) = node.as_while_node() {
        let cond = lower_node(result, hir, &while_node.predicate())?;
        let body = lower_body(result, hir, while_node.statements().map(|s| s.as_node()))?;
        return Ok(hir.push(HirNode::While {
            cond,
            body,
            negate: false,
        }));
    }
    if let Some(until_node) = node.as_until_node() {
        let cond = lower_node(result, hir, &until_node.predicate())?;
        let body = lower_body(result, hir, until_node.statements().map(|s| s.as_node()))?;
        return Ok(hir.push(HirNode::While {
            cond,
            body,
            negate: true,
        }));
    }

    // `for var in iterable ... end` / `for a, b in pairs ... end` -- see
    // `lower_multi_target`'s docs for the full generalized target shape.
    if let Some(for_node) = node.as_for_node() {
        let target = lower_multi_target(result, hir, &for_node.index())?;
        let iterable = lower_node(result, hir, &for_node.collection())?;
        let body = lower_body(result, hir, for_node.statements().map(|s| s.as_node()))?;
        return Ok(hir.push(HirNode::For { target, iterable, body }));
    }

    // `break`/`next` (with an optional single value) and `redo` -- `ruby-prism`
    // itself already guarantees these only ever appear inside a loop or block
    // (a bare one anywhere else is a parse error caught before lowering even
    // starts), so there's no context to re-validate here; `codegen::loops`
    // is what actually resolves which native loop they target.
    if let Some(brk) = node.as_break_node() {
        let value = lower_single_optional_argument(result, hir, brk.arguments(), "break")?;
        return Ok(hir.push(HirNode::Break(value)));
    }
    if let Some(nxt) = node.as_next_node() {
        let value = lower_single_optional_argument(result, hir, nxt.arguments(), "next")?;
        return Ok(hir.push(HirNode::Next(value)));
    }
    if node.as_redo_node().is_some() {
        return Ok(hir.push(HirNode::Redo));
    }

    // `return` / `return value` -- see `HirNode::Return`'s docs.
    if let Some(ret) = node.as_return_node() {
        let value = lower_single_optional_argument(result, hir, ret.arguments(), "return")?;
        return Ok(hir.push(HirNode::Return(value)));
    }

    // `begin ... rescue ... else ... ensure ... end` -- also reached for a
    // method body that's implicitly a `BeginNode` (no explicit `begin`/`end`,
    // just a bare `rescue`/`ensure` directly inside `def`), since
    // `DefNode::body()` is that SAME node shape in that case (confirmed
    // empirically via `Prism.parse`) and flows through this same `lower_node`
    // call from `lower_body`.
    if let Some(begin) = node.as_begin_node() {
        return lower_begin(result, hir, &begin);
    }

    // `expr rescue fallback` -- the modifier form (also how an endless
    // method's `def foo = risky rescue 1` and an assignment's `x = risky
    // rescue 1` both surface: `RescueModifierNode` sits directly in the
    // value/body position). Desugars to the same `HirNode::Begin` shape as
    // an explicit `begin/rescue` with one bare (`classes: []`, matching
    // `StandardError` and below) rescue clause and no `else`/`ensure`.
    if let Some(rm) = node.as_rescue_modifier_node() {
        let body = vec![lower_node(result, hir, &rm.expression())?];
        let fallback = vec![lower_node(result, hir, &rm.rescue_expression())?];
        return Ok(hir.push(HirNode::Begin {
            body,
            rescues: vec![RescueClause {
                classes: Vec::new(),
                binding: None,
                body: fallback,
            }],
            else_body: None,
            ensure_body: None,
        }));
    }

    // `retry` -- see `HirNode::Retry`'s docs.
    if node.as_retry_node().is_some() {
        return Ok(hir.push(HirNode::Retry));
    }

    // `a, b = 1, 2` / `a, *b, c = arr` / `(a, b), @x, $y, Z, obj.attr, arr[i]
    // = ...` -- see `MultiTargetGroup`/`lower_multi_target`'s docs for the
    // full generalized target shape.
    if let Some(mw) = node.as_multi_write_node() {
        let targets = lower_multi_target_group(result, hir, mw.lefts(), mw.rest(), mw.rights())?;
        let value = lower_node(result, hir, &mw.value())?;
        return Ok(hir.push(HirNode::MultiWrite { targets, value }));
    }

    Err(format!(
        "unsupported syntax at {:?} (spike handles only what the 7 example programs need)",
        node.location()
    ))
}

/// One `elements()` entry of an `ArrayNode` -- either a plain value or a
/// `*expr` splat (`SplatNode`). A bare `*` with no expression is only valid
/// in a parameter/pattern position, never inside an array literal, so
/// `SplatNode::expression()` returning `None` here is unreachable from real
/// source and treated as a clean error rather than a panic.
fn lower_array_elem(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<ArrayElem> {
    if let Some(splat) = node.as_splat_node() {
        let expr = splat
            .expression()
            .ok_or("a bare `*` isn't supported inside an array literal (spike scope)")?;
        return Ok(ArrayElem::Splat(lower_node(result, hir, &expr)?));
    }
    Ok(ArrayElem::Single(lower_node(result, hir, node)?))
}

/// Splits a call's raw argument list into (positional `ArrayElem`s, literal
/// keyword `HashPair`s, an optional trailing `**h` double-splat) -- a
/// trailing `KeywordHashNode` (`foo(x: 1, y: 2, **h)`) is the only prism
/// shape recognized as keyword arguments; every other entry lowers as an
/// ordinary positional argument via `lower_array_elem` (reusing the exact
/// same plain-value-or-`*expr`-splat recognizer an array literal's own
/// elements already use -- `foo(*arr)` and `[*arr]` are structurally the
/// same `SplatNode` shape at the `ruby-prism` level). A `KeywordHashNode`'s
/// own elements are a mix of plain `key: value` `AssocNode`s and, at most
/// one trailing `**h` `AssocSplatNode` (real Ruby only allows one double-
/// splat per call, always last) -- `kwargs_splat` carries that one
/// separately since it's merged into the callee's keyword args at RUNTIME
/// (its keys aren't known at compile time), unlike every literal `key:
/// value` pair.
fn lower_call_args(
    result: &ParseResult,
    hir: &mut Hir,
    arguments: Option<ruby_prism::ArgumentsNode<'_>>,
) -> PResult<(Vec<ArrayElem>, Vec<HashPair>, Option<NodeId>)> {
    let Some(arguments) = arguments else {
        return Ok((Vec::new(), Vec::new(), None));
    };
    let mut list: Vec<_> = arguments.arguments().iter().collect();
    let mut kwargs_splat = None;
    let kwargs = match list.last().and_then(|n| n.as_keyword_hash_node()) {
        Some(kw) => {
            list.pop();
            let mut pairs = Vec::new();
            for el in kw.elements().iter() {
                if let Some(splat) = el.as_assoc_splat_node() {
                    if kwargs_splat.is_some() {
                        return Err("at most one `**h` double-splat is supported per call (spike scope)".to_string());
                    }
                    let expr = splat.value().ok_or(
                        "an anonymous `**` keyword-forwarding argument isn't supported yet (spike scope)",
                    )?;
                    kwargs_splat = Some(lower_node(result, hir, &expr)?);
                    continue;
                }
                let assoc = el
                    .as_assoc_node()
                    .ok_or("unsupported keyword-argument shape (spike scope)")?;
                let key = lower_node(result, hir, &assoc.key())?;
                let value = lower_node(result, hir, &assoc.value())?;
                pairs.push(HashPair(key, value));
            }
            pairs
        }
        None => Vec::new(),
    };
    let args = list
        .iter()
        .map(|n| lower_array_elem(result, hir, n))
        .collect::<PResult<Vec<_>>>()?;
    Ok((args, kwargs, kwargs_splat))
}

/// A class body's statement list -- like `lower_statement_list`, but
/// recognizes a handful of zero-receiver call shapes at this exact position
/// (mirroring `lower_node`'s own `define_method`/`loop` desugars) that a
/// strict 1-statement-to-1-node map can't express: `attr_reader`/
/// `attr_writer`/`attr_accessor` each expand into MULTIPLE synthesized
/// `DefMethod`s from one statement, and `private`/`public`/`protected` expand
/// into NONE.
fn lower_class_body(
    result: &ParseResult,
    hir: &mut Hir,
    body: Option<Node<'_>>,
) -> PResult<Vec<NodeId>> {
    let stmts: Vec<Node<'_>> = match body {
        None => return Ok(Vec::new()),
        Some(n) => match n.as_statements_node() {
            Some(stmts) => stmts.body().iter().collect(),
            None => vec![n],
        },
    };
    let mut out = Vec::new();
    // The DEFAULT visibility for every subsequent `def` in this class body,
    // switched by a bare `private`/`public`/`protected` (no arguments) --
    // see `lower_class_body_statement`'s docs.
    let mut visibility = Visibility::Public;
    for stmt in &stmts {
        lower_class_body_statement(result, hir, stmt, &mut visibility, &mut out)?;
    }
    Ok(out)
}

/// `attr_reader :a, :b` -> a `DefMethod` getter per name (`body: [IvarRead]`).
/// `attr_writer :a, :b` -> a `DefMethod` setter per name (`name=`, one
/// required param, `body: [IvarWrite]`). `attr_accessor` emits both. Only
/// literal symbol arguments are recognized (matching `define_method`'s own
/// literal-name restriction elsewhere in this file); anything else falls
/// through to an ordinary `Call` (which real Ruby would resolve dynamically,
/// e.g. `attr_reader(*names)` -- outside spike scope, a clean rejection at
/// codegen if `attr_reader` itself isn't otherwise defined). Every
/// synthesized getter/setter gets the CURRENT default `visibility`, exactly
/// like an ordinary `def` would.
///
/// `private`/`public`/`protected` recognize three real Ruby forms, appending
/// nothing to `out` themselves (they're never a standalone HIR node): (1) a
/// bare call with no arguments switches the DEFAULT `visibility` for every
/// `def` for the REST of this class body; (2) `private def name; ... end`
/// (the `def`-as-sole-argument idiom) lowers the `def` normally through the
/// generic `lower_node` path, then retroactively overrides ITS OWN
/// visibility; (3) `private :name1, :name2, ...` retroactively overrides
/// the visibility of already-lowered method(s) of those names (searched in
/// `out`, everything lowered so far in this same class body -- real Ruby
/// requires the target already be defined earlier in the same body, so no
/// forward search is needed). Anything else (a dynamic/computed argument)
/// falls through to an ordinary `Call` -- a clean rejection at codegen time
/// if `private`/`public`/`protected` themselves aren't otherwise defined,
/// matching this function's own posture elsewhere.
fn lower_class_body_statement(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    visibility: &mut Visibility,
    out: &mut Vec<NodeId>,
) -> PResult<()> {
    // `alias new_name old_name` / `alias :new_name :old_name` (`AliasMethodNode`
    // -- a real Ruby KEYWORD, not a method call, so this is checked before the
    // `as_call_node()` cascade below). `old_name` must already be defined
    // EARLIER in this SAME class/module body (searched in `out`, exactly the
    // same "no forward search, no ancestor walk" restriction `private
    // :name1, :name2` already enforces above) -- aliasing an INHERITED
    // method is a clean rejection, a documented, narrow scope-cut. Resolved
    // entirely at LOWERING time: since the found `DefMethod`'s `params`/
    // `body`/`is_class_method`/`visibility` are all cheaply `Clone`-able,
    // the alias is just a second `DefMethod` node under a different name --
    // no new analyze-phase machinery, no shared-body indirection to keep in
    // sync with `super`/materialization.
    if let Some(alias) = node.as_alias_method_node() {
        let new_name = alias_target_name(&alias.new_name())?;
        let old_name = alias_target_name(&alias.old_name())?;
        let Some(&old_id) = out.iter().rev().find(
            |&&id| matches!(&hir[id], HirNode::DefMethod { name, .. } if *name == old_name),
        ) else {
            return Err(format!(
                "`alias {new_name} {old_name}`: `{old_name}` must already be defined earlier in the same class/module body (spike scope) -- aliasing an inherited method isn't supported yet"
            ));
        };
        let HirNode::DefMethod {
            params,
            body,
            is_class_method,
            visibility: old_vis,
            ..
        } = &hir[old_id]
        else {
            unreachable!("guarded by the `find` above")
        };
        let (params, body, is_class_method, old_vis) =
            (params.clone(), body.clone(), *is_class_method, *old_vis);
        out.push(hir.push(HirNode::DefMethod {
            name: new_name,
            params,
            body,
            is_class_method,
            visibility: old_vis,
        }));
        return Ok(());
    }

    // `class << self ... end` (`SingletonClassNode`) -- reopens the class's
    // OWN singleton class, the idiomatic way to define several class
    // methods at once without repeating `def self.` on each one. `class <<
    // obj` on any expression OTHER than a bare `self` is a per-instance
    // singleton class -- a materially bigger feature (a dynamically-
    // growable per-instance vtable) this spike doesn't support, matching
    // the plan's existing scope-cut on `define_singleton_method`; a clean
    // rejection, not silently ignored. The nested body is lowered through
    // the ORDINARY class-body path (so `attr_reader`/`private`/nested
    // `def`s all work exactly as they would directly in the class body),
    // then every resulting `def` is retroactively corrected to a class
    // method -- see `Hir::set_method_is_class_method`'s docs. Anything else
    // in the body (`include`/`extend`/`prepend`/a nested `class << self`)
    // is a clean rejection: those would need to affect the ENCLOSING
    // class's `class_methods` materialization in a way plain
    // `is_class_method` retagging can't express, a separate, unattempted
    // feature.
    if let Some(singleton) = node.as_singleton_class_node() {
        if singleton.expression().as_self_node().is_none() {
            return Err(
                "`class << obj` (a per-instance singleton class, `obj` other than a bare `self`) isn't supported yet (spike scope)".to_string(),
            );
        }
        let inner = lower_class_body(result, hir, singleton.body())?;
        for &id in &inner {
            if !matches!(&hir[id], HirNode::DefMethod { .. }) {
                return Err(
                    "`class << self` may only contain `def`s (spike scope) -- `include`/`extend`/`prepend`/a nested `class << self` aren't supported inside it yet".to_string(),
                );
            }
            hir.set_method_is_class_method(id);
        }
        out.extend(inner);
        return Ok(());
    }

    if let Some(call) = node.as_call_node() {
        if call.receiver().is_none() {
            let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
            if matches!(name.as_str(), "private" | "public" | "protected") {
                let new_vis = match name.as_str() {
                    "private" => Visibility::Private,
                    "protected" => Visibility::Protected,
                    _ => Visibility::Public,
                };
                let arg_list: Vec<_> = call
                    .arguments()
                    .map(|a| a.arguments().iter().collect())
                    .unwrap_or_default();
                if arg_list.is_empty() {
                    *visibility = new_vis;
                    return Ok(());
                }
                if arg_list.len() == 1 && arg_list[0].as_def_node().is_some() {
                    let id = lower_node(result, hir, &arg_list[0])?;
                    hir.set_method_visibility(id, new_vis);
                    out.push(id);
                    return Ok(());
                }
                if arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
                    for n in &arg_list {
                        let target = String::from_utf8_lossy(
                            n.as_symbol_node().expect("checked above").unescaped(),
                        )
                        .into_owned();
                        if let Some(&id) = out.iter().find(|&&id| {
                            matches!(&hir[id], HirNode::DefMethod { name: existing, .. } if *existing == target)
                        }) {
                            hir.set_method_visibility(id, new_vis);
                        }
                    }
                    return Ok(());
                }
                // Falls through to the generic `Call` lowering below --
                // a dynamic/computed argument (e.g. `private(*names)`).
            }
            // `include Mod`/`extend Mod`/`prepend Mod` -- one or more bare
            // constant arguments, applied left-to-right (see `HirNode::
            // Include`'s docs for the multi-arg ordering rule). Anything
            // else (a non-constant argument, e.g. a computed module
            // expression) falls through to an ordinary `Call`, a clean
            // rejection at codegen time (spike scope: only a literal module
            // name is resolvable to a `ClassId` at compile time anyway).
            if matches!(name.as_str(), "include" | "extend" | "prepend") {
                if let Some(args) = call.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    if !arg_list.is_empty() {
                        let names = arg_list
                            .iter()
                            .map(constant_name)
                            .collect::<PResult<Vec<_>>>()?;
                        out.extend(names.into_iter().map(|n| {
                            hir.push(match name.as_str() {
                                "include" => HirNode::Include(n),
                                "extend" => HirNode::Extend(n),
                                _ => HirNode::Prepend(n),
                            })
                        }));
                        return Ok(());
                    }
                }
            }
            // The native-package DSL (Phase 14.3) -- `native_crate "path"`
            // and `native_func :name, [ArgTypes...], ReturnType`, recognized
            // by name at module-body top level exactly like `attr_accessor`
            // above. UNCONDITIONAL (an error, never a fall-through to a
            // generic `Call`): there is no runtime definition of either, so
            // falling through would only fail later and further from the
            // cause. Meaningful only inside a package whose `spin.toml`
            // declares the matching `[native]` crate -- using the DSL
            // outside one leaves the generated `crate_path::fn` reference
            // unresolvable, a loud (if Rust-level) build error.
            if name == "native_crate" {
                let arg_list: Vec<_> = call
                    .arguments()
                    .map(|a| a.arguments().iter().collect())
                    .unwrap_or_default();
                if arg_list.len() != 1 || arg_list[0].as_string_node().is_none() {
                    return Err(
                        "`native_crate` takes exactly one string literal (the backing Rust crate's path, e.g. `native_crate \"spinelc_base64\"`)"
                            .to_string(),
                    );
                }
                let crate_path = String::from_utf8_lossy(
                    arg_list[0].as_string_node().expect("checked above").unescaped(),
                )
                .into_owned();
                out.push(hir.push(HirNode::NativeCrate(crate_path)));
                return Ok(());
            }
            if name == "native_func" {
                let arg_list: Vec<_> = call
                    .arguments()
                    .map(|a| a.arguments().iter().collect())
                    .unwrap_or_default();
                let shape_err = || {
                    "`native_func` takes a symbol, an array of argument type constants, and a return type constant, e.g. `native_func :encode64, [String], String`"
                        .to_string()
                };
                if arg_list.len() != 3 {
                    return Err(shape_err());
                }
                let Some(sym) = arg_list[0].as_symbol_node() else {
                    return Err(shape_err());
                };
                let Some(arg_types) = arg_list[1].as_array_node() else {
                    return Err(shape_err());
                };
                if arg_list[2].as_constant_read_node().is_none() {
                    return Err(shape_err());
                }
                out.push(hir.push(HirNode::NativeFunc {
                    name: String::from_utf8_lossy(sym.unescaped()).into_owned(),
                    arity: arg_types.elements().iter().count(),
                }));
                return Ok(());
            }
            if matches!(name.as_str(), "attr_reader" | "attr_writer" | "attr_accessor") {
                if let Some(args) = call.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    if !arg_list.is_empty() && arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
                        for n in &arg_list {
                            let ivar = String::from_utf8_lossy(
                                n.as_symbol_node().expect("checked above").unescaped(),
                            )
                            .into_owned();
                            if name != "attr_writer" {
                                let read = hir.push(HirNode::IvarRead(ivar.clone()));
                                out.push(hir.push(HirNode::DefMethod {
                                    name: ivar.clone(),
                                    params: Params::default(),
                                    body: vec![read],
                                    is_class_method: false,
                                    visibility: *visibility,
                                }));
                            }
                            if name != "attr_reader" {
                                let param = "value".to_string();
                                let read_param = hir.push(HirNode::LocalRead(param.clone()));
                                let write = hir.push(HirNode::IvarWrite(ivar.clone(), read_param));
                                out.push(hir.push(HirNode::DefMethod {
                                    name: format!("{ivar}="),
                                    params: Params {
                                        required: vec![param],
                                        ..Params::default()
                                    },
                                    body: vec![write],
                                    is_class_method: false,
                                    visibility: *visibility,
                                }));
                            }
                        }
                        return Ok(());
                    }
                }
            }
        }
    }
    // An ordinary `def` gets the CURRENT default visibility -- the generic
    // `lower_node` path (reached below) always sets `Public` (it has no
    // notion of a class body's running default; see its own docs), so this
    // corrects it retroactively when the current default isn't `Public`.
    if node.as_def_node().is_some() {
        let id = lower_node(result, hir, node)?;
        if *visibility != Visibility::Public {
            hir.set_method_visibility(id, *visibility);
        }
        out.push(id);
        return Ok(());
    }
    out.push(lower_node(result, hir, node)?);
    Ok(())
}

/// A `break`/`next`'s optional value -- at most one argument is supported
/// (`break a, b`, which real Ruby builds into an implicit array, is a clean
/// lowering error rather than a panic; spike scope).
fn lower_single_optional_argument(
    result: &ParseResult,
    hir: &mut Hir,
    args: Option<ruby_prism::ArgumentsNode<'_>>,
    keyword: &str,
) -> PResult<Option<NodeId>> {
    let Some(args) = args else { return Ok(None) };
    let list: Vec<_> = args.arguments().iter().collect();
    match list.len() {
        0 => Ok(None),
        1 => Ok(Some(lower_node(result, hir, &list[0])?)),
        _ => Err(format!(
            "`{keyword}` with more than one value isn't supported yet (spike scope)"
        )),
    }
}

/// Exactly one index argument (`arr[i]`, not `arr[i, j]`) -- the same
/// single-index restriction `codegen::call::try_collection_dispatch`'s
/// `[]`/`[]=` fast path already enforces, extended to the operator-write
/// forms.
fn single_index_argument(
    result_args: Option<ruby_prism::ArgumentsNode<'_>>,
) -> PResult<ruby_prism::ArgumentsNode<'_>> {
    let args = result_args.ok_or("`[]`-style compound assignment requires exactly one index argument (spike scope)")?;
    if args.arguments().iter().count() != 1 {
        return Err("`[]`-style compound assignment only supports a single index argument (spike scope)".to_string());
    }
    Ok(args)
}

/// Binds `receiver` to a hidden local (a `HirNode::LocalWrite` statement)
/// exactly ONCE, then builds `tmp.read_name` against that same binding --
/// shared by every `obj.attr op= rhs`/`||=`/`&&=` desugar (see their own
/// call sites in `lower_node`): a receiver expression may have side effects
/// (`get_obj().attr += 1`), so re-lowering the SAME prism node twice (once
/// per read/write call) would silently double-evaluate it. Returns `(bind
/// statement, read call, hidden local's name)` -- the caller combines the
/// read call with `rhs` however its own operator requires (see
/// `build_call_target_write`'s docs for the matching write half).
fn bind_call_target_once(
    result: &ParseResult,
    hir: &mut Hir,
    receiver: &Node<'_>,
    read_name: &str,
) -> PResult<(NodeId, NodeId, String)> {
    let recv_expr = lower_node(result, hir, receiver)?;
    let tmp = hir.gensym("__recv");
    let bind = hir.push(HirNode::LocalWrite(tmp.clone(), recv_expr));
    let read_recv = hir.push(HirNode::LocalRead(tmp.clone()));
    let read_call = hir.push(HirNode::Call {
        receiver: Some(read_recv),
        name: read_name.to_string(),
        args: Vec::new(),
        kwargs: Vec::new(),
        kwargs_splat: None,
        block: None,
        block_arg: None,
        safe: false,
    });
    Ok((bind, read_call, tmp))
}

/// The write half of `bind_call_target_once` -- `tmp.write_name(value)`,
/// reading the SAME hidden receiver binding.
fn build_call_target_write(hir: &mut Hir, tmp: &str, write_name: &str, value: NodeId) -> NodeId {
    let write_recv = hir.push(HirNode::LocalRead(tmp.to_string()));
    hir.push(HirNode::Call {
        receiver: Some(write_recv),
        name: write_name.to_string(),
        args: vec![ArrayElem::Single(value)],
        kwargs: Vec::new(),
        kwargs_splat: None,
        block: None,
        block_arg: None,
        safe: false,
    })
}

/// Same reasoning as `bind_call_target_once`, extended to BOTH the receiver
/// AND the (single) index argument of `arr[i] op= rhs`/`||=`/`&&=`
/// (`arr[compute_idx()] += 1` must call `compute_idx()` exactly once too,
/// not once per read/write `[]`/`[]=` call). Returns `(bind statements,
/// read call, receiver's hidden local name, index's hidden local name)`.
fn bind_index_target_once(
    result: &ParseResult,
    hir: &mut Hir,
    receiver: &Node<'_>,
    index_args: &ruby_prism::ArgumentsNode<'_>,
) -> PResult<(Vec<NodeId>, NodeId, String, String)> {
    let index_node = index_args.arguments().iter().next().expect("checked by single_index_argument");
    let recv_expr = lower_node(result, hir, receiver)?;
    let idx_expr = lower_node(result, hir, &index_node)?;
    let recv_tmp = hir.gensym("__recv");
    let bind_recv = hir.push(HirNode::LocalWrite(recv_tmp.clone(), recv_expr));
    let idx_tmp = hir.gensym("__idx");
    let bind_idx = hir.push(HirNode::LocalWrite(idx_tmp.clone(), idx_expr));
    let read_recv = hir.push(HirNode::LocalRead(recv_tmp.clone()));
    let read_idx = hir.push(HirNode::LocalRead(idx_tmp.clone()));
    let read_call = hir.push(HirNode::Call {
        receiver: Some(read_recv),
        name: "[]".to_string(),
        args: vec![ArrayElem::Single(read_idx)],
        kwargs: Vec::new(),
        kwargs_splat: None,
        block: None,
        block_arg: None,
        safe: false,
    });
    Ok((vec![bind_recv, bind_idx], read_call, recv_tmp, idx_tmp))
}

/// The write half of `bind_index_target_once` -- `recv_tmp[idx_tmp] =
/// value`, reading the SAME hidden receiver/index bindings.
fn build_index_target_write(hir: &mut Hir, recv_tmp: &str, idx_tmp: &str, value: NodeId) -> NodeId {
    let write_recv = hir.push(HirNode::LocalRead(recv_tmp.to_string()));
    let write_idx = hir.push(HirNode::LocalRead(idx_tmp.to_string()));
    hir.push(HirNode::Call {
        receiver: Some(write_recv),
        name: "[]=".to_string(),
        args: vec![ArrayElem::Single(write_idx), ArrayElem::Single(value)],
        kwargs: Vec::new(),
        kwargs_splat: None,
        block: None,
        block_arg: None,
        safe: false,
    })
}

/// A `rescue ... => e` binding -- always a plain local-variable target in
/// real Ruby's own grammar for this one position (unlike a general
/// multi-assignment target, which additionally allows ivars/cvars/globals/
/// constants/`obj.attr`/`arr[i]`/nested groups -- see `lower_multi_target`).
fn local_target_name(node: &Node<'_>) -> PResult<String> {
    let target = node
        .as_local_variable_target_node()
        .ok_or("`rescue => name` only supports a plain local variable binding (spike scope)")?;
    Ok(String::from_utf8_lossy(target.name().as_slice()).into_owned())
}

/// One `MultiTarget` -- a `MultiWriteNode`/nested `MultiTargetNode`'s own
/// `lefts`/`rest`/`rights` entry, or a `for`-loop's `index()`. See
/// `MultiTarget`'s docs for the full generalized shape this now covers
/// (beyond the original plain-local-only restriction): local/ivar/cvar/
/// global/bare-constant/`obj.attr`/`arr[i]`/nested-group targets.
fn lower_multi_target(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<crate::hir::MultiTarget> {
    use crate::hir::MultiTarget;

    if let Some(t) = node.as_local_variable_target_node() {
        return Ok(MultiTarget::Local(String::from_utf8_lossy(t.name().as_slice()).into_owned()));
    }
    if let Some(t) = node.as_instance_variable_target_node() {
        let name = String::from_utf8_lossy(t.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        return Ok(MultiTarget::Ivar(name));
    }
    if let Some(t) = node.as_class_variable_target_node() {
        let name = String::from_utf8_lossy(t.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        return Ok(MultiTarget::ClassVar(name));
    }
    if let Some(t) = node.as_global_variable_target_node() {
        return Ok(MultiTarget::Global(String::from_utf8_lossy(t.name().as_slice()).into_owned()));
    }
    if let Some(t) = node.as_constant_target_node() {
        return Ok(MultiTarget::Const(String::from_utf8_lossy(t.name().as_slice()).into_owned()));
    }
    if node.as_constant_path_target_node().is_some() {
        return Err(
            "an explicit `Foo::BAR` multi-assignment target isn't supported yet (spike scope) -- only a bare, lexically-scoped constant name is".to_string(),
        );
    }
    // `obj.attr, ... = ...` -- pre-builds the `attr=` write `Call` right now,
    // with a synthetic hidden local (`tmp_name`) standing in for "the value
    // this target receives" -- see `MultiTarget::Call`'s docs for why this
    // lets codegen reuse the ordinary static/dynamic dispatch machinery with
    // no bespoke attr-write codegen of its own.
    if let Some(t) = node.as_call_target_node() {
        let receiver = lower_node(result, hir, &t.receiver())?;
        // `CallTargetNode::name()` is ALREADY the setter name (`:x=`, not
        // `:x`) -- confirmed via `Prism.parse("b.x, b.y = ...")`; appending
        // another `=` here (a real bug, found via this session's own
        // testing) produced a double-equals method name (`x==`) that could
        // never resolve, silently breaking every multi-assignment into an
        // attr target (`b.x, b.y = b.y, b.x`) with a confusing "unsupported
        // call" panic instead of the correct swap.
        let setter_name = String::from_utf8_lossy(t.name().as_slice()).into_owned();
        let tmp_name = hir.gensym("__mval");
        let tmp_read = hir.push(HirNode::LocalRead(tmp_name.clone()));
        let write_call = hir.push(HirNode::Call {
            receiver: Some(receiver),
            name: setter_name,
            args: vec![ArrayElem::Single(tmp_read)],
            kwargs: Vec::new(),
            kwargs_splat: None,
            block: None,
            block_arg: None,
            safe: false,
        });
        return Ok(MultiTarget::Call { write_call, tmp_name });
    }
    // `arr[i], ... = ...` -- see `MultiTarget::Call`'s docs; same synthetic-
    // hidden-local trick, targeting `[]=` instead of `attr=`.
    if let Some(t) = node.as_index_target_node() {
        let receiver = lower_node(result, hir, &t.receiver())?;
        let arg_list: Vec<_> = t.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        if arg_list.len() != 1 {
            return Err("`arr[i] = ...` as a multi-assignment target only supports a single index argument (spike scope)".to_string());
        }
        let index = lower_node(result, hir, &arg_list[0])?;
        let tmp_name = hir.gensym("__mval");
        let tmp_read = hir.push(HirNode::LocalRead(tmp_name.clone()));
        let write_call = hir.push(HirNode::Call {
            receiver: Some(receiver),
            name: "[]=".to_string(),
            args: vec![ArrayElem::Single(index), ArrayElem::Single(tmp_read)],
            kwargs: Vec::new(),
            kwargs_splat: None,
            block: None,
            block_arg: None,
            safe: false,
        });
        return Ok(MultiTarget::Call { write_call, tmp_name });
    }
    // `(a, b), c = ...` -- a nested destructuring group; see
    // `lower_multi_target_group`'s docs.
    if let Some(t) = node.as_multi_target_node() {
        let group = lower_multi_target_group(result, hir, t.lefts(), t.rest(), t.rights())?;
        return Ok(MultiTarget::Nested(group));
    }
    Err("unsupported multi-assignment/`for`-loop target shape (spike scope)".to_string())
}

/// The `before`/`splat`/`after` shape shared by `MultiWriteNode` and a
/// nested `MultiTargetNode` (both expose the identical `lefts()`/`rest()`/
/// `rights()` grammar) -- see `MultiTargetGroup`'s docs. An anonymous `*`
/// splat target (no name at all) is still a clean lowering error, unchanged
/// from the pre-existing plain-local-only restriction.
fn lower_multi_target_group(
    result: &ParseResult,
    hir: &mut Hir,
    lefts: ruby_prism::NodeList<'_>,
    rest: Option<Node<'_>>,
    rights: ruby_prism::NodeList<'_>,
) -> PResult<crate::hir::MultiTargetGroup> {
    let before = lefts
        .iter()
        .map(|n| lower_multi_target(result, hir, &n))
        .collect::<PResult<Vec<_>>>()?;
    let splat = match rest {
        None => None,
        Some(n) => {
            let splat = n
                .as_splat_node()
                .ok_or("expected `*name` as a multi-assignment's splat target")?;
            let expr = splat.expression().ok_or(
                "an anonymous `*` target in a multi-assignment isn't supported yet (spike scope)",
            )?;
            Some(Some(Box::new(lower_multi_target(result, hir, &expr)?)))
        }
    };
    let after = rights
        .iter()
        .map(|n| lower_multi_target(result, hir, &n))
        .collect::<PResult<Vec<_>>>()?;
    Ok(crate::hir::MultiTargetGroup { before, splat, after })
}

/// `begin body rescue R1 rescue R2 ... else ... ensure ... end` -- `rescue`
/// clauses arrive as a singly-linked chain (`RescueNode::subsequent()`), not
/// a list, mirroring `if`/`elsif`'s own `subsequent()` chaining. `exceptions()`
/// entries are expected to be plain constants (`rescue Foo, Bar => e`) --
/// anything else (a splatted exception list, `rescue *errs`) falls through
/// to `constant_name`'s existing "expected a plain constant name" rejection,
/// same posture as `superclass`/`include`/`extend`/`prepend` resolution
/// elsewhere in this file. `reference()` (the `=> e` binding) is always a
/// plain local-variable target in real Ruby's own grammar for this position.
fn lower_begin(result: &ParseResult, hir: &mut Hir, begin: &ruby_prism::BeginNode<'_>) -> PResult<NodeId> {
    let body = lower_body(result, hir, begin.statements().map(|s| s.as_node()))?;

    let mut rescues = Vec::new();
    let mut next = begin.rescue_clause();
    while let Some(r) = next {
        let classes = r
            .exceptions()
            .iter()
            .map(|n| constant_name(&n))
            .collect::<PResult<Vec<_>>>()?;
        let binding = match r.reference() {
            None => None,
            Some(n) => Some(local_target_name(&n)?),
        };
        let rescue_body = lower_body(result, hir, r.statements().map(|s| s.as_node()))?;
        rescues.push(RescueClause {
            classes,
            binding,
            body: rescue_body,
        });
        next = r.subsequent();
    }

    let else_body = match begin.else_clause() {
        None => None,
        Some(e) => Some(lower_body(result, hir, e.statements().map(|s| s.as_node()))?),
    };
    let ensure_body = match begin.ensure_clause() {
        None => None,
        Some(e) => Some(lower_body(result, hir, e.statements().map(|s| s.as_node()))?),
    };

    Ok(hir.push(HirNode::Begin {
        body,
        rescues,
        else_body,
        ensure_body,
    }))
}

/// An `in` clause's pattern slot -- either the bare pattern, or (for a
/// guarded arm) prism's own encoding of `PATTERN if/unless COND`: the
/// pattern wrapped in an `IfNode`/`UnlessNode` whose `predicate` is the
/// guard condition and whose single statement is the real pattern
/// (confirmed empirically against `Prism.parse` -- there is no separate
/// "guard" field on `InNode` itself). Returns `(pattern, guard)` where
/// `guard` is `(condition, is_unless)`, mirroring `HirNode::While`'s
/// `negate`-flag convention rather than a separate boolean-inverted shape.
fn lower_in_pattern_and_guard(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<(Pattern, Option<(NodeId, bool)>)> {
    if let Some(if_node) = node.as_if_node() {
        let cond = lower_node(result, hir, &if_node.predicate())?;
        let pattern = lower_single_wrapped_pattern(result, hir, if_node.statements(), "if")?;
        return Ok((pattern, Some((cond, false))));
    }
    if let Some(unless_node) = node.as_unless_node() {
        let cond = lower_node(result, hir, &unless_node.predicate())?;
        let pattern = lower_single_wrapped_pattern(result, hir, unless_node.statements(), "unless")?;
        return Ok((pattern, Some((cond, true))));
    }
    Ok((lower_pattern(result, hir, node)?, None))
}

fn lower_single_wrapped_pattern(
    result: &ParseResult,
    hir: &mut Hir,
    stmts: Option<ruby_prism::StatementsNode<'_>>,
    guard_kind: &str,
) -> PResult<Pattern> {
    let stmts = stmts.ok_or_else(|| format!("expected a pattern inside an `{guard_kind}`-guarded `in` clause"))?;
    let body: Vec<_> = stmts.body().iter().collect();
    if body.len() != 1 {
        return Err(format!(
            "expected exactly one pattern inside an `{guard_kind}`-guarded `in` clause (spike scope)"
        ));
    }
    lower_pattern(result, hir, &body[0])
}

/// Lowers one `case/in`/`in pattern`/`=> pattern` PATTERN node (as opposed to
/// an ordinary expression -- see `Pattern`'s docs for why this is a separate
/// tree from `HirNode`). Dispatches on the pattern's own `ruby-prism` node
/// shape; anything not specifically recognized falls through to the general
/// `Value` case (an ordinary expression, matched via `rb_eq`), which is what
/// makes a bare literal (`in 1`, `in nil`, `in "x"`) work for free.
fn lower_pattern(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<Pattern> {
    if let Some(t) = node.as_local_variable_target_node() {
        let name = String::from_utf8_lossy(t.name().as_slice()).into_owned();
        return Ok(Pattern::Bind(name));
    }
    if let Some(cap) = node.as_capture_pattern_node() {
        let inner = lower_pattern(result, hir, &cap.value())?;
        let name = String::from_utf8_lossy(cap.target().name().as_slice()).into_owned();
        return Ok(Pattern::Capture(Box::new(inner), name));
    }
    if node.as_alternation_pattern_node().is_some() {
        let mut parts = Vec::new();
        flatten_alternation(result, hir, node, &mut parts)?;
        if parts.iter().any(pattern_may_bind) {
            return Err(
                "a pattern can't bind a variable inside a `|` alternation (spike scope, matches real Ruby)"
                    .to_string(),
            );
        }
        return Ok(Pattern::Or(parts));
    }
    if let Some(pin) = node.as_pinned_variable_node() {
        let expr = lower_node(result, hir, &pin.variable())?;
        return Ok(Pattern::Pin(expr));
    }
    if let Some(pin) = node.as_pinned_expression_node() {
        let expr = lower_node(result, hir, &pin.expression())?;
        return Ok(Pattern::Pin(expr));
    }
    if let Some(arr) = node.as_array_pattern_node() {
        let constant = arr.constant().map(|c| constant_name(&c)).transpose()?;
        let pre = arr
            .requireds()
            .iter()
            .map(|n| lower_pattern(result, hir, &n))
            .collect::<PResult<Vec<_>>>()?;
        let rest = arr.rest().map(|n| array_or_find_rest_name(&n)).transpose()?;
        let post = arr
            .posts()
            .iter()
            .map(|n| lower_pattern(result, hir, &n))
            .collect::<PResult<Vec<_>>>()?;
        return Ok(Pattern::Array { constant, pre, rest, post });
    }
    if let Some(find) = node.as_find_pattern_node() {
        let constant = find.constant().map(|c| constant_name(&c)).transpose()?;
        // `left()` is already typed as `SplatNode` by `ruby-prism`; `right()`
        // (asymmetrically) comes back as a generic `Node` that must still be
        // cast -- confirmed against the actual generated bindings, not
        // assumed from the grammar's apparent symmetry.
        let pre_rest = splat_target_name(&find.left())?;
        let mid = find
            .requireds()
            .iter()
            .map(|n| lower_pattern(result, hir, &n))
            .collect::<PResult<Vec<_>>>()?;
        let post_rest = array_or_find_rest_name(&find.right())?;
        return Ok(Pattern::Find {
            constant,
            pre_rest,
            mid,
            post_rest,
        });
    }
    if let Some(hp) = node.as_hash_pattern_node() {
        let constant = hp.constant().map(|c| constant_name(&c)).transpose()?;
        let mut pairs = Vec::new();
        for el in hp.elements().iter() {
            let assoc = el
                .as_assoc_node()
                .ok_or("expected `key: pattern` inside a hash pattern (spike scope)")?;
            let key = hash_pattern_key_name(&assoc.key())?;
            // The `{key:}` shorthand -- prism synthesizes the value as an
            // `ImplicitNode` wrapping a `LocalVariableTargetNode` of the
            // same name (confirmed empirically against `Prism.parse`), so
            // `None` here means "bind a local named `key` directly", not
            // "no value at all".
            let value_pattern = if assoc.value().as_implicit_node().is_some() {
                None
            } else {
                Some(lower_pattern(result, hir, &assoc.value())?)
            };
            pairs.push((key, value_pattern));
        }
        let rest = hash_pattern_rest(hp.rest())?;
        return Ok(Pattern::Hash { constant, pairs, rest });
    }
    if let Some(range) = node.as_range_node() {
        let start = match range.left() {
            None => None,
            Some(n) => Some(lower_node(result, hir, &n)?),
        };
        let end = match range.right() {
            None => None,
            Some(n) => Some(lower_node(result, hir, &n)?),
        };
        return Ok(Pattern::Range {
            start,
            end,
            exclusive: range.is_exclude_end(),
        });
    }
    // A bare constant with no capture (`in Integer`, `in SomeClass`) -- an
    // `is_a?`-style check, resolved (built-in tag vs. user-class ancestry)
    // entirely in `codegen::patterns::emit_class_check`.
    if let Some(c) = node.as_constant_read_node() {
        let name = String::from_utf8_lossy(c.name().as_slice()).into_owned();
        return Ok(Pattern::ClassCheck(name));
    }
    // Fallback: an ordinary expression (literal or otherwise), matched via
    // `rb_eq` -- see `Pattern::Value`'s docs. Lowering this through the
    // generic `lower_node` path is what makes `nil`/`true`/`false`/
    // `Integer`/`String`/`Symbol` literals, and even an arbitrary method
    // call, work as a pattern for free.
    let value = lower_node(result, hir, node)?;
    Ok(Pattern::Value(value))
}

/// Recursively flattens `P1 | P2 | ... | Pn` (parsed as left-associative
/// nested `AlternationPatternNode`s) into a flat list -- validation that no
/// alternative binds a variable happens at the call site in `lower_pattern`.
fn flatten_alternation(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    out: &mut Vec<Pattern>,
) -> PResult<()> {
    if let Some(alt) = node.as_alternation_pattern_node() {
        flatten_alternation(result, hir, &alt.left(), out)?;
        flatten_alternation(result, hir, &alt.right(), out)?;
    } else {
        out.push(lower_pattern(result, hir, node)?);
    }
    Ok(())
}

fn pattern_may_bind(p: &Pattern) -> bool {
    let mut found = false;
    p.for_each_bound_name(&mut |_| found = true);
    found
}

/// A generic `*`/`**` splat target's name, given the already-cast `SplatNode`
/// -- `None` for an anonymous `*`/`**` (discards its slice), `Some(name)` for
/// a named one. Shared by `Find`'s always-present `left`/`right` splats.
fn splat_target_name(splat: &ruby_prism::SplatNode<'_>) -> PResult<Option<String>> {
    match splat.expression() {
        None => Ok(None),
        Some(e) => {
            let t = e.as_local_variable_target_node().ok_or(
                "a pattern's `*` splat may only bind a plain local variable name (spike scope)",
            )?;
            Ok(Some(String::from_utf8_lossy(t.name().as_slice()).into_owned()))
        }
    }
}

/// `ArrayPatternNode::rest()`'s payload -- a generic `Node` that must itself
/// be a `SplatNode` (unlike `Find`'s `left`/`right`, which prism already
/// types as `SplatNode` directly).
fn array_or_find_rest_name(node: &Node<'_>) -> PResult<Option<String>> {
    let splat = node
        .as_splat_node()
        .ok_or("expected a `*name` splat in this array pattern (spike scope)")?;
    splat_target_name(&splat)
}

/// A hash pattern key -- only a literal `key:` symbol is supported (spike
/// scope, matching this project's existing symbol-key-only restriction on
/// hash pattern -- a computed/string key needs a distinct `AssocNode` shape
/// this doesn't lower).
fn hash_pattern_key_name(node: &Node<'_>) -> PResult<String> {
    let sym = node
        .as_symbol_node()
        .ok_or("only symbol keys (`key:`) are supported in a hash pattern (spike scope)")?;
    Ok(String::from_utf8_lossy(sym.unescaped()).into_owned())
}

/// `HashPatternNode::rest()`'s payload -- see `HashPatternRest`'s docs for
/// the three shapes (`**rest`/anonymous `**`, `**nil`, or absent entirely).
fn hash_pattern_rest(node: Option<Node<'_>>) -> PResult<HashPatternRest> {
    let Some(n) = node else {
        return Ok(HashPatternRest::None);
    };
    if n.as_no_keywords_parameter_node().is_some() {
        return Ok(HashPatternRest::NoMoreKeys);
    }
    let sp = n
        .as_assoc_splat_node()
        .ok_or("expected `**rest`/`**nil` in this hash pattern (spike scope)")?;
    match sp.value() {
        None => Ok(HashPatternRest::Rest(None)),
        Some(v) => {
            let t = v.as_local_variable_target_node().ok_or(
                "a hash pattern's `**` may only bind a plain local variable name (spike scope)",
            )?;
            Ok(HashPatternRest::Rest(Some(
                String::from_utf8_lossy(t.name().as_slice()).into_owned(),
            )))
        }
    }
}

/// If `id` is a `StringLit` HIR node with no interpolation, its concatenated
/// literal text -- the exact structural check `eval`'s literal-splice path
/// needs (a `StringLit` is compile-time-constant iff every `StrPart` is
/// `Lit`, never `Interp`). Reusable for any future "must be a literal"
/// construct.
fn literal_string_text(hir: &Hir, id: NodeId) -> Option<String> {
    let HirNode::StringLit(parts) = &hir[id] else {
        return None;
    };
    let mut out = String::new();
    for p in parts {
        match p {
            StrPart::Lit(s) => out.push_str(s),
            StrPart::Interp(_) => return None,
        }
    }
    Some(out)
}

/// `analyze::register_class`'s registration walk only ever scans the
/// LITERAL top level of `Program`'s (or a class body's) own statement list
/// for `ClassDef`/`DefMethod` -- an `Eval`'d body's top-level statements are
/// nested inside its `Eval(body)` node, which that walk never unwraps. A
/// top-level `class`/`def` inside an eval'd literal would otherwise flow
/// straight to `codegen::expr::emit_expr`'s "unexpected top-level-only node
/// in expression position" panic -- this rejects that case with a clean
/// compile error instead of letting spinelc itself panic (spike scope: the
/// same gap already exists today for any non-eval code that nests a
/// `class`/`def` inside e.g. an `if`, so this isn't a new hole, just a new
/// way to trigger an old one).
fn reject_top_level_defs(hir: &Hir, body: &[NodeId]) -> PResult<()> {
    for &id in body {
        if matches!(hir[id], HirNode::ClassDef { .. } | HirNode::DefMethod { .. }) {
            return Err(
                "`eval` containing a top-level `class`/`def` isn't supported yet (spike scope)"
                    .to_string(),
            );
        }
    }
    Ok(())
}

/// One `parts()` entry of an `InterpolatedStringNode` -- either a literal
/// chunk (`StringNode`) or an `#{ }` (`EmbeddedStatementsNode`, exactly one
/// statement supported -- see `StrPart`'s docs). `EmbeddedVariableNode`
/// (bare `#@ivar`/`#$global` interpolation, no braces) isn't handled, a
/// narrower-than-real-Ruby spike scope-cut.
fn lower_string_part(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<StrPart> {
    if let Some(s) = node.as_string_node() {
        return Ok(StrPart::Lit(String::from_utf8_lossy(s.unescaped()).into_owned()));
    }
    if let Some(embedded) = node.as_embedded_statements_node() {
        let stmts: Vec<_> = embedded
            .statements()
            .map(|s| s.body().iter().collect())
            .unwrap_or_default();
        return match stmts.len() {
            1 => Ok(StrPart::Interp(lower_node(result, hir, &stmts[0])?)),
            _ => Err(
                "string interpolation only supports a single expression inside `#{ }` (spike scope)"
                    .to_string(),
            ),
        };
    }
    Err("unsupported string interpolation part (spike scope)".to_string())
}
