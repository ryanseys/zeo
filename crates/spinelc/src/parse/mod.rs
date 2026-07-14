//! Lowers a `ruby_prism::Node` tree straight into `Hir` -- the direct analog
//! of `spinel_parse.c`'s `flatten()` + `node_table.c`'s `nt_load_text()`
//! combined into one in-process step (no text-serialization round-trip; see
//! `hir.rs`'s module docs for why spinel needed that step and we don't).
//!
//! Covers exactly the node kinds the spike's 7 examples exercise; anything
//! else is a clean `Err` (mirroring spinel's `unsupported(c, id, "...")`
//! convention), not a panic.

use crate::hir::{ArrayElem, HashPair, Hir, HirNode, NodeId, StrPart};
use ruby_prism::{Node, ParseResult};

type PResult<T> = Result<T, String>;

/// Returns the built `Hir` plus the id of its `Program` root -- `Hir` itself
/// doesn't track a root (it's just an arena), so lowering hands the root id
/// back explicitly rather than requiring callers to know it's always the
/// last-pushed node.
pub fn parse_and_lower(source: &str) -> PResult<(Hir, NodeId)> {
    let mut hir = Hir::default();
    let statements = parse_and_lower_into(&mut hir, source)?;
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

    // `x += 1` / `@x += 1` -- desugars to a plain read-operator-write, e.g.
    // `x = x + 1`, reusing the existing `LocalWrite`/`IvarWrite` + operator
    // `Call` dispatch infrastructure entirely (no new HIR node needed, exactly
    // like `unless`/ternary reuse `If`). Only the plain binary-operator form
    // is handled -- `||=`/`&&=` are distinct prism nodes with short-circuit-
    // don't-evaluate-the-value semantics (mirroring `And`/`Or`) rather than
    // an unconditional read-op-write, and aren't supported yet (spike scope,
    // falls through to the generic "unsupported syntax" error).
    if let Some(op) = node.as_local_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let read = hir.push(HirNode::LocalRead(name.clone()));
        let rhs = lower_node(result, hir, &op.value())?;
        let call = hir.push(HirNode::Call {
            receiver: Some(read),
            name: op_name,
            args: vec![rhs],
            block: None,
            safe: false,
        });
        return Ok(hir.push(HirNode::LocalWrite(name, call)));
    }
    if let Some(op) = node.as_instance_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let read = hir.push(HirNode::IvarRead(name.clone()));
        let rhs = lower_node(result, hir, &op.value())?;
        let call = hir.push(HirNode::Call {
            receiver: Some(read),
            name: op_name,
            args: vec![rhs],
            block: None,
            safe: false,
        });
        return Ok(hir.push(HirNode::IvarWrite(name, call)));
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

        // `eval("literal string")` -- ONLY the compile-time-constant-string
        // form. Unlike `define_method`/`loop` above, this is intercepted
        // UNCONDITIONALLY: those two have a genuine second runtime path for
        // their non-desugared shape (an ordinary implicit-self `Call`), but
        // `eval` doesn't -- this spike has no runtime parser/interpreter (see
        // docs/EVAL_VM.md), so letting a non-literal `eval(...)` fall through
        // as a plain `Call` would compile cleanly and only fail at RUNTIME
        // with a confusing `NoMethodError`, strictly worse than a clear
        // compile-time rejection.
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

    // `for var in iterable ... end` -- only a single plain local index
    // variable is supported (`for a, b in pairs` destructuring is a distinct
    // `MultiTargetNode` index, a clean lowering error rather than a panic).
    if let Some(for_node) = node.as_for_node() {
        let var = for_node
            .index()
            .as_local_variable_target_node()
            .ok_or("`for` only supports a single plain local variable index (spike scope)")?;
        let var = String::from_utf8_lossy(var.name().as_slice()).into_owned();
        let iterable = lower_node(result, hir, &for_node.collection())?;
        let body = lower_body(result, hir, for_node.statements().map(|s| s.as_node()))?;
        return Ok(hir.push(HirNode::For { var, iterable, body }));
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

    // `a, b = 1, 2` / `a, *b, c = arr` -- see `HirNode::MultiWrite`'s docs for
    // the supported target shape.
    if let Some(mw) = node.as_multi_write_node() {
        let before = mw
            .lefts()
            .iter()
            .map(|n| local_target_name(&n))
            .collect::<PResult<Vec<_>>>()?;
        let splat = match mw.rest() {
            None => None,
            Some(n) => {
                let splat = n
                    .as_splat_node()
                    .ok_or("expected `*name` as a multi-assignment's splat target")?;
                let expr = splat.expression().ok_or(
                    "an anonymous `*` target in a multi-assignment isn't supported yet (spike scope)",
                )?;
                Some(local_target_name(&expr)?)
            }
        };
        let after = mw
            .rights()
            .iter()
            .map(|n| local_target_name(&n))
            .collect::<PResult<Vec<_>>>()?;
        let value = lower_node(result, hir, &mw.value())?;
        return Ok(hir.push(HirNode::MultiWrite {
            before,
            splat,
            after,
            value,
        }));
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

/// A multi-assignment target (`MultiWriteNode`'s `lefts`/`rights` entries, or
/// a splat's inner expression) -- only a plain local variable is supported;
/// nested destructuring, ivars, constants, and `a[i]`/`obj.attr` targets are
/// each a distinct `ruby-prism` node this spike doesn't lower.
fn local_target_name(node: &Node<'_>) -> PResult<String> {
    let target = node.as_local_variable_target_node().ok_or(
        "multi-assignment only supports plain local variable targets (spike scope)",
    )?;
    Ok(String::from_utf8_lossy(target.name().as_slice()).into_owned())
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
