//! `if`/`unless` chain lowering and the class-body static-`if`/`unless`
//! fold (`static_bool`), `begin`/`rescue`/`ensure`, and the shared
//! `return`/`break`/`next` optional-value helper. Split out of
//! `parse/mod.rs`.

use super::assign::lower_multi_target;
use super::consts::constant_path_name;
use super::pattern::{lower_in_pattern_and_guard, lower_pattern};
use super::{PResult, lower_array_elem, lower_body, lower_node};
use crate::hir::{Hir, HirNode, NodeId, PatternArm, RescueClause};
use ruby_prism::{Node, ParseResult};

/// `if`/`elsif`/`elsif`.../`else` is one `IfNode` per level, chained through
/// `subsequent()`: `None` (no further clauses), another `IfNode` (an
/// `elsif`), or an `ElseNode` (the final `else`). Recursing here builds the
/// same nesting `HirNode::If`'s `else_body` already expects -- an `elsif`
/// becomes a single-statement `else_body` containing the nested `If`.
pub(crate) fn lower_if_chain(
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
                return Err("expected `elsif` or `else` after `if` (zeo limitation)"
                    .to_string()
                    .into());
            }
        }
    };
    Ok(hir.push(HirNode::If {
        cond,
        then_body,
        else_body,
    }))
}

/// A statically-known boolean value for a class-body `if`/`unless` guard --
/// only the literal forms real programs use to compile-time-select a `def`/
/// `alias` (`... if true`, `... unless false`, `... if (true)`). `nil` counts
/// as false (Ruby's own truthiness). Anything else (a method call, a
/// constant, a comparison) is `None`, leaving the `if` to lower as an
/// ordinary runtime conditional.
pub(crate) fn static_bool(node: &Node<'_>) -> Option<bool> {
    if node.as_true_node().is_some() {
        return Some(true);
    }
    if node.as_false_node().is_some() || node.as_nil_node().is_some() {
        return Some(false);
    }
    if let Some(paren) = node.as_parentheses_node() {
        let stmts = paren.body()?.as_statements_node()?;
        let body: Vec<_> = stmts.body().iter().collect();
        if let [only] = body.as_slice() {
            return static_bool(only);
        }
    }
    None
}

/// `begin body rescue R1 rescue R2 ... else ... ensure ... end` -- `rescue`
/// clauses arrive as a singly-linked chain (`RescueNode::subsequent()`), not
/// a list, mirroring `if`/`elsif`'s own `subsequent()` chaining. An
/// `exceptions()` entry that is a plain constant (`rescue Foo, Bar => e`)
/// resolves to a compile-time class; a splat (`rescue *errs`) or any other
/// COMPUTED expression (`rescue defined?(X) ? A : B`) is lowered and matched at
/// runtime instead (`zeo_rt::rescue_matches_any`). `reference()` (the `=> e`
/// binding) is always a plain local-variable target in real Ruby's own grammar
/// for this position.
pub(crate) fn lower_begin(
    result: &ParseResult,
    hir: &mut Hir,
    begin: &ruby_prism::BeginNode<'_>,
) -> PResult<NodeId> {
    let body = lower_body(result, hir, begin.statements().map(|s| s.as_node()))?;
    let rescues = lower_rescue_clauses(result, hir, begin)?;

    let else_body = match begin.else_clause() {
        None => None,
        Some(e) => Some(lower_body(
            result,
            hir,
            e.statements().map(|s| s.as_node()),
        )?),
    };
    let ensure_body = match begin.ensure_clause() {
        None => None,
        Some(e) => Some(lower_body(
            result,
            hir,
            e.statements().map(|s| s.as_node()),
        )?),
    };

    Ok(hir.push(HirNode::Begin {
        body,
        rescues,
        else_body,
        ensure_body,
    }))
}

/// The lowered rescue clauses of a `begin`, on their own -- [`lower_begin`]'s
/// middle third. Also used by the loader when it wraps a rescued require's
/// SPLICED statements in a synthesized `Begin` carrying the same clauses, so
/// a raise at the required file's own top level (power_assert's TracePoint
/// probe) is caught by the handler the source wrote, as CRuby's `require`
/// timing would.
pub(crate) fn lower_rescue_clauses(
    result: &ParseResult,
    hir: &mut Hir,
    begin: &ruby_prism::BeginNode<'_>,
) -> PResult<Vec<RescueClause>> {
    let mut rescues = Vec::new();
    let mut next = begin.rescue_clause();
    while let Some(r) = next {
        // Partition the clause's exception list into STATIC constant references
        // (`rescue Foo, Bar`) and SPLATS (`rescue *errs`). A splat has no
        // compile-time class to resolve, so its expression is lowered and
        // matched at runtime (`zeo_rt::rescue_matches_any`).
        let mut classes = Vec::new();
        let mut splats = Vec::new();
        for n in r.exceptions().iter() {
            if let Some(splat) = n.as_splat_node() {
                let expr = splat
                    .expression()
                    .ok_or("`rescue *` needs an expression after the `*`")?;
                splats.push(lower_node(result, hir, &expr)?);
            } else if let Ok(name) = constant_path_name(&n) {
                classes.push(name);
            } else {
                // A COMPUTED exception class -- e.g. net/http's `rescue
                // defined?(OpenSSL::SSL) ? OpenSSL::SSL::SSLError : IOError`,
                // or a constant path rooted at one (1hdoc's `rescue
                // adapter::GitExecuteError`, whose `adapter` is a parameter).
                // There is no compile-time class to resolve, so lower the
                // expression and match it at runtime exactly like a splat
                // (`rescue_matches_any` matches a single class as well as an
                // array).
                splats.push(lower_node(result, hir, &n)?);
            }
        }
        let (binding, copy_out) = match r.reference() {
            None => (None, None),
            Some(n) => {
                let (name, copy) = rescue_binding_target(result, hir, &n)?;
                (Some(name), copy)
            }
        };
        let mut rescue_body = lower_body(result, hir, r.statements().map(|s| s.as_node()))?;
        // `rescue => @e` binds the hidden local, then copies it into the real
        // target before anything else in the clause runs.
        if let Some(copy) = copy_out {
            rescue_body.insert(0, copy);
        }
        rescues.push(RescueClause {
            classes,
            splats,
            binding,
            body: rescue_body,
        });
        next = r.subsequent();
    }
    Ok(rescues)
}

/// A `return`/`break`/`next`'s optional value.
///
/// More than one value (`return 1, 2`) builds an implicit ARRAY -- the same
/// array a `[1, 2]` literal would, splats included, which is why this
/// delegates to `lower_array_elem` rather than re-deriving the shape. Real
/// Ruby, oracle-verified:
///
/// ```text
/// def two = (return 1, 2)      # => [1, 2]
/// def m(a) = (return 1, *a)    # m([2, 3]) => [1, 2, 3]
/// [1].each { break 1, 2 }      # => [1, 2]
/// ```
///
/// A SINGLE splat is an array too, and that is the case a plain
/// "len == 1 ? lower it : error" rule gets wrong: `return *a` with `a ==
/// [1]` is `[1]`, not `1` -- the splat expands into a fresh array rather
/// than passing its operand through. So one argument only takes the
/// scalar path when it isn't a splat.
pub(crate) fn lower_single_optional_argument(
    result: &ParseResult,
    hir: &mut Hir,
    args: Option<ruby_prism::ArgumentsNode<'_>>,
    _keyword: &str,
) -> PResult<Option<NodeId>> {
    let Some(args) = args else { return Ok(None) };
    let list: Vec<_> = args.arguments().iter().collect();
    match list.as_slice() {
        [] => Ok(None),
        [only] if only.as_splat_node().is_none() => Ok(Some(lower_node(result, hir, only)?)),
        _ => {
            let elems = list
                .iter()
                .map(|n| lower_array_elem(result, hir, n))
                .collect::<PResult<Vec<_>>>()?;
            Ok(Some(hir.push(HirNode::ArrayLit(elems))))
        }
    }
}

/// A `rescue ... => e` binding -- always a plain local-variable target in
/// real Ruby's own grammar for this one position (unlike a general
/// multi-assignment target, which additionally allows ivars/cvars/globals/
/// constants/`obj.attr`/`arr[i]`/nested groups -- see `lower_multi_target`).
/// Where a `rescue => target` puts the exception. Ruby takes any assignable
/// target here, not just a local: activesupport writes `rescue => @setup_exception`
/// and rspec-rails `rescue *exceptions => @rescued_exception`.
///
/// A plain local is the binding itself. Anything else binds a HIDDEN local and
/// gets a copy statement, which the caller runs as the clause's first
/// statement -- so the rescue machinery keeps its one shape (a local name) and
/// the target sees the exception before any of the clause's own code.
///
/// Returns `(the local the machinery binds, the copy-out statement)`.
fn rescue_binding_target(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<(String, Option<NodeId>)> {
    if let Some(t) = node.as_local_variable_target_node() {
        return Ok((
            String::from_utf8_lossy(t.name().as_slice()).into_owned(),
            None,
        ));
    }
    let tmp = hir.gensym("__resc");
    let value = hir.push(HirNode::LocalRead(tmp.clone()));
    let strip_at = |raw: &[u8]| {
        String::from_utf8_lossy(raw)
            .trim_start_matches('@')
            .to_string()
    };
    let write = if let Some(t) = node.as_instance_variable_target_node() {
        hir.push(HirNode::IvarWrite(strip_at(t.name().as_slice()), value))
    } else if let Some(t) = node.as_class_variable_target_node() {
        crate::lower::cvar_write(hir, strip_at(t.name().as_slice()), value)
    } else if let Some(t) = node.as_global_variable_target_node() {
        hir.push(HirNode::GlobalWrite(
            String::from_utf8_lossy(t.name().as_slice()).into_owned(),
            value,
        ))
    } else if let Some(t) = node.as_constant_target_node() {
        hir.push(HirNode::ConstWrite {
            scope: None,
            name: String::from_utf8_lossy(t.name().as_slice()).into_owned(),
            value,
        })
    } else if let Some(t) = node.as_call_target_node() {
        // `rescue => obj.attr` -- an ordinary setter send, receiver evaluated
        // when the clause fires (CRuby's timing; the copy-out runs first in
        // the rescue body). `CallTargetNode::name()` is ALREADY the setter
        // name (`:attr=`) -- same fact `lower_multi_target` records.
        let receiver = lower_node(result, hir, &t.receiver())?;
        let setter_name = String::from_utf8_lossy(t.name().as_slice()).into_owned();
        hir.push(HirNode::Call {
            receiver: Some(receiver),
            name: setter_name,
            args: vec![crate::hir::ArrayElem::Single(value)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        })
    } else if let Some(t) = node.as_index_target_node() {
        // `rescue => h[k]` -- `[]=` with the exception as the trailing
        // argument; any index count, splats riding along, exactly as a
        // multi-assignment index target.
        let receiver = lower_node(result, hir, &t.receiver())?;
        let mut args: Vec<crate::hir::ArrayElem> = Vec::new();
        for a in t
            .arguments()
            .map(|a| a.arguments().iter().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            args.push(match a.as_splat_node() {
                Some(s) => {
                    let inner = s
                        .expression()
                        .ok_or("a bare `*` has no index expression to bind")?;
                    crate::hir::ArrayElem::Splat(lower_node(result, hir, &inner)?)
                }
                None => crate::hir::ArrayElem::Single(lower_node(result, hir, &a)?),
            });
        }
        args.push(crate::hir::ArrayElem::Single(value));
        hir.push(HirNode::Call {
            receiver: Some(receiver),
            name: "[]=".to_string(),
            args,
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        })
    } else {
        return Err(
            "`rescue => target` supports a local, `@ivar`, `@@cvar`, `$global`, a constant, \
             an `obj.attr` setter, or an `obj[key]` index (zeo limitation)"
                .to_string()
                .into(),
        );
    };
    Ok((tmp, Some(write)))
}

/// The control-flow family of [`super::lower_node_inner`]'s recognizer
/// chain: `&&`/`||`, `defined?`, branches (`if`/`unless`/`case`/`case-in`
/// and the pattern one-liners `in`/`=>`), loops (`while`/`until`/`for`),
/// `break`/`next`/`redo`/`return`/`retry`, and `begin`/rescue (modifier form
/// included). `Ok(None)` = not this family's node.
pub(crate) fn try_lower(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<Option<NodeId>> {
    // `obj.attr += rhs` / `obj.attr ||= rhs` / `obj.attr &&= rhs` -- evaluates
    // `obj` exactly ONCE (bound to a hidden local via `HirNode::Seq`), since
    // a receiver expression may have side effects (e.g. `get_obj().attr +=
    // 1`) -- naively re-lowering the SAME prism receiver node twice (once
    // for the getter call, once for the setter call) would silently
    // double-evaluate it, a real correctness bug real Ruby doesn't have. See
    // `bind_call_target_once`'s docs.
    if let Some(and) = node.as_and_node() {
        let left = lower_node(result, hir, &and.left())?;
        let right = lower_node(result, hir, &and.right())?;
        return Ok(Some(hir.push(HirNode::And(left, right))));
    }

    if let Some(or) = node.as_or_node() {
        let left = lower_node(result, hir, &or.left())?;
        let right = lower_node(result, hir, &or.right())?;
        return Ok(Some(hir.push(HirNode::Or(left, right))));
    }

    if let Some(defined) = node.as_defined_node() {
        // `defined?(@@x)` outside any class body answers nil rather than
        // raising -- `defined?` never evaluates its operand, so the raise
        // `cvar_read` would otherwise put there must not be lowered at all.
        if defined.value().as_class_variable_read_node().is_some() && hir.cvar_is_toplevel() {
            return Ok(Some(hir.push(HirNode::NilLit)));
        }
        let value = lower_node(result, hir, &defined.value())?;
        return Ok(Some(hir.push(HirNode::Defined(value))));
    }

    if let Some(if_node) = node.as_if_node() {
        return lower_if_chain(
            result,
            hir,
            &if_node.predicate(),
            if_node.statements(),
            if_node.subsequent(),
        )
        .map(Some);
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
        return Ok(Some(hir.push(HirNode::If {
            cond,
            then_body: truthy_body,
            else_body: falsy_body,
        })));
    }

    // `case subject; when ...; else ...; end` -- value matching only.
    // `case/in` pattern matching (`CaseMatchNode`) is a distinct prism node,
    // not handled here (see the `as_case_match_node` arm below).
    if let Some(case_node) = node.as_case_node() {
        let subject = match case_node.predicate() {
            None => None,
            Some(p) => Some(lower_node(result, hir, &p)?),
        };
        let mut arms = Vec::new();
        for cond in case_node.conditions().iter() {
            let when = cond
                .as_when_node()
                .ok_or("expected a `when` clause inside `case` (zeo limitation)")?;
            // `lower_array_elem`, not a bare `lower_node`: `when *a` is a
            // SplatNode, structurally identical to `[*a]`'s element.
            let values = when
                .conditions()
                .iter()
                .map(|n| lower_array_elem(result, hir, &n))
                .collect::<PResult<Vec<_>>>()?;
            let body = lower_body(result, hir, when.statements().map(|s| s.as_node()))?;
            arms.push((values, body));
        }
        let else_body = match case_node.else_clause() {
            None => Vec::new(),
            Some(e) => lower_body(result, hir, e.statements().map(|s| s.as_node()))?,
        };
        return Ok(Some(hir.push(HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        })));
    }

    // `case subject; in PATTERN ... end` -- real pattern matching, a
    // distinct prism node (`CaseMatchNode`) from value-matching `case/when`
    // above. See `Pattern`/`PatternArm`'s docs.
    if let Some(case_match) = node.as_case_match_node() {
        let subject = case_match
            .predicate()
            .ok_or("`case/in` requires a subject (zeo limitation)")?;
        let subject = lower_node(result, hir, &subject)?;
        let mut arms = Vec::new();
        for cond in case_match.conditions().iter() {
            let in_node = cond
                .as_in_node()
                .ok_or("expected an `in` clause inside `case/in` (zeo limitation)")?;
            let (pattern, guard) = lower_in_pattern_and_guard(result, hir, &in_node.pattern())?;
            let body = lower_body(result, hir, in_node.statements().map(|s| s.as_node()))?;
            arms.push(PatternArm {
                pattern,
                guard,
                body,
            });
        }
        let else_body = match case_match.else_clause() {
            None => None,
            Some(e) => Some(lower_body(
                result,
                hir,
                e.statements().map(|s| s.as_node()),
            )?),
        };
        return Ok(Some(hir.push(HirNode::CaseIn {
            subject,
            arms,
            else_body,
        })));
    }

    // `expr in pattern` -- boolean one-liner, never raises.
    if let Some(mp) = node.as_match_predicate_node() {
        let subject = lower_node(result, hir, &mp.value())?;
        let pattern = lower_pattern(result, hir, &mp.pattern())?;
        return Ok(Some(hir.push(HirNode::MatchPredicate { subject, pattern })));
    }

    // `expr => pattern` -- rightward assignment, raises `NoMatchingPatternError`
    // on failure.
    if let Some(mr) = node.as_match_required_node() {
        let subject = lower_node(result, hir, &mr.value())?;
        let pattern = lower_pattern(result, hir, &mr.pattern())?;
        return Ok(Some(hir.push(HirNode::MatchRequired { subject, pattern })));
    }

    // `while`/`until`, both statement and modifier form -- `until` is `While`
    // with `negate: true`, exactly like `unless` swaps `If`'s branches above.
    // The do-while form (`begin...end while cond`) is prism's begin-modifier
    // flag on the same node -- carried through as `post` so codegen runs the
    // body once before the first condition test.
    if let Some(while_node) = node.as_while_node() {
        let cond = lower_node(result, hir, &while_node.predicate())?;
        let body = lower_body(result, hir, while_node.statements().map(|s| s.as_node()))?;
        return Ok(Some(hir.push(HirNode::While {
            cond,
            body,
            negate: false,
            post: while_node.is_begin_modifier(),
        })));
    }
    if let Some(until_node) = node.as_until_node() {
        let cond = lower_node(result, hir, &until_node.predicate())?;
        let body = lower_body(result, hir, until_node.statements().map(|s| s.as_node()))?;
        return Ok(Some(hir.push(HirNode::While {
            cond,
            body,
            negate: true,
            post: until_node.is_begin_modifier(),
        })));
    }

    // `for var in iterable ... end` / `for a, b in pairs ... end` -- see
    // `lower_multi_target`'s docs for the full generalized target shape.
    if let Some(for_node) = node.as_for_node() {
        let target = lower_multi_target(result, hir, &for_node.index())?;
        let iterable = lower_node(result, hir, &for_node.collection())?;
        let body = lower_body(result, hir, for_node.statements().map(|s| s.as_node()))?;
        return Ok(Some(hir.push(HirNode::For {
            target,
            iterable,
            body,
        })));
    }

    // `break`/`next` (with an optional single value) and `redo` -- `ruby-prism`
    // itself already guarantees these only ever appear inside a loop or block
    // (a bare one anywhere else is a parse error caught before lowering even
    // starts), so there's no context to re-validate here; the emitter's
    // loop lowering (`clif::stmt`) resolves which loop they target.
    if let Some(brk) = node.as_break_node() {
        let value = lower_single_optional_argument(result, hir, brk.arguments(), "break")?;
        return Ok(Some(hir.push(HirNode::Break(value))));
    }
    if let Some(nxt) = node.as_next_node() {
        let value = lower_single_optional_argument(result, hir, nxt.arguments(), "next")?;
        return Ok(Some(hir.push(HirNode::Next(value))));
    }
    if node.as_redo_node().is_some() {
        return Ok(Some(hir.push(HirNode::Redo)));
    }

    // `return` / `return value` -- see `HirNode::Return`'s docs.
    if let Some(ret) = node.as_return_node() {
        let value = lower_single_optional_argument(result, hir, ret.arguments(), "return")?;
        return Ok(Some(hir.push(HirNode::Return(value))));
    }

    // `begin ... rescue ... else ... ensure ... end` -- also reached for a
    // method body that's implicitly a `BeginNode` (no explicit `begin`/`end`,
    // just a bare `rescue`/`ensure` directly inside `def`), since
    // `DefNode::body()` is that SAME node shape in that case (confirmed
    // empirically via `Prism.parse`) and flows through this same `lower_node`
    // call from `lower_body`.
    if let Some(begin) = node.as_begin_node() {
        return lower_begin(result, hir, &begin).map(Some);
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
        return Ok(Some(hir.push(HirNode::Begin {
            body,
            rescues: vec![RescueClause {
                classes: Vec::new(),
                splats: Vec::new(),
                binding: None,
                body: fallback,
            }],
            else_body: None,
            ensure_body: None,
        })));
    }

    // `retry` -- see `HirNode::Retry`'s docs.
    if node.as_retry_node().is_some() {
        return Ok(Some(hir.push(HirNode::Retry)));
    }

    Ok(None)
}
