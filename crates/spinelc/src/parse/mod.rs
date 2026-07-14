//! Lowers a `ruby_prism::Node` tree straight into `Hir` -- the direct analog
//! of `spinel_parse.c`'s `flatten()` + `node_table.c`'s `nt_load_text()`
//! combined into one in-process step (no text-serialization round-trip; see
//! `hir.rs`'s module docs for why spinel needed that step and we don't).
//!
//! Covers exactly the node kinds the spike's 7 examples exercise; anything
//! else is a clean `Err` (mirroring spinel's `unsupported(c, id, "...")`
//! convention), not a panic.

use crate::hir::{Hir, HirNode, NodeId};
use ruby_prism::{Node, ParseResult};

type PResult<T> = Result<T, String>;

/// Returns the built `Hir` plus the id of its `Program` root -- `Hir` itself
/// doesn't track a root (it's just an arena), so lowering hands the root id
/// back explicitly rather than requiring callers to know it's always the
/// last-pushed node.
pub fn parse_and_lower(source: &str) -> PResult<(Hir, NodeId)> {
    let result = ruby_prism::parse(source.as_bytes());
    if let Some(err) = result.errors().next() {
        return Err(format!("parse error: {}", err.message()));
    }
    let program = result
        .node()
        .as_program_node()
        .ok_or("expected a top-level ProgramNode")?;

    let mut hir = Hir::default();
    let statements = lower_statement_list(&result, &mut hir, program.statements().body())?;
    let root = hir.push(HirNode::Program(statements));
    Ok((hir, root))
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

fn required_param_names(params: Option<ruby_prism::ParametersNode<'_>>) -> PResult<Vec<String>> {
    let Some(params) = params else {
        return Ok(Vec::new());
    };
    params
        .requireds()
        .iter()
        .map(|n| {
            let p = n
                .as_required_parameter_node()
                .ok_or("only plain required parameters are supported")?;
            Ok(String::from_utf8_lossy(p.name().as_slice()).into_owned())
        })
        .collect()
}

fn lower_block(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<NodeId> {
    let block = node
        .as_block_node()
        .ok_or("expected a block (`{ }` or `do..end`)")?;
    let params = match block.parameters() {
        None => Vec::new(),
        Some(p) => {
            let bp = p
                .as_block_parameters_node()
                .ok_or("unsupported block parameter form")?;
            required_param_names(bp.parameters())?
        }
    };
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

    if let Some(sym) = node.as_symbol_node() {
        let name = String::from_utf8_lossy(sym.unescaped()).into_owned();
        return Ok(hir.push(HirNode::SymbolLit(name)));
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
        let body = lower_body(result, hir, class.body())?;
        return Ok(hir.push(HirNode::ClassDef {
            name,
            superclass,
            body,
        }));
    }

    if let Some(def) = node.as_def_node() {
        let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        let params = required_param_names(def.parameters())?;
        let body = lower_body(result, hir, def.body())?;
        return Ok(hir.push(HirNode::DefMethod { name, params, body }));
    }

    if let Some(call) = node.as_call_node() {
        let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();

        // `ClassName.new(args)` -- a distinct node; see hir.rs.
        if name == "new" {
            if let Some(recv) = call.receiver() {
                let class_name = constant_name(&recv)?;
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
                            None => Vec::new(),
                            Some(p) => {
                                let bp = p
                                    .as_block_parameters_node()
                                    .ok_or("unsupported block parameter form")?;
                                required_param_names(bp.parameters())?
                            }
                        };
                        let body = lower_body(result, hir, block.body())?;
                        return Ok(hir.push(HirNode::DefMethod {
                            name: method_name,
                            params,
                            body,
                        }));
                    }
                }
            }
        }

        let receiver = match call.receiver() {
            None => None,
            Some(r) => Some(lower_node(result, hir, &r)?),
        };
        let args = match call.arguments() {
            None => Vec::new(),
            Some(a) => a
                .arguments()
                .iter()
                .map(|n| lower_node(result, hir, &n))
                .collect::<PResult<Vec<_>>>()?,
        };
        let block = match call.block() {
            None => None,
            Some(b) => Some(lower_block(result, hir, &b)?),
        };
        return Ok(hir.push(HirNode::Call {
            receiver,
            name,
            args,
            block,
            safe: call.is_safe_navigation(),
        }));
    }

    Err(format!(
        "unsupported syntax at {:?} (spike handles only what the 7 example programs need)",
        node.location()
    ))
}
