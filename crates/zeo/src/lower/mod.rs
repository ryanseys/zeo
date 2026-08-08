//! Lowers a `ruby_prism::Node` tree straight into `Hir` -- the direct analog
//! of `zeo_parse.c`'s `flatten()` + `node_table.c`'s `nt_load_text()`
//! combined into one in-process step (no text-serialization round-trip; see
//! `hir.rs`'s module docs for why zeo needed that step and we don't).
//!
//! Covers the node kinds the corpus exercises; anything
//! else is a clean `Err` (mirroring zeo's `unsupported(c, id, "...")`
//! convention), not a panic.

mod assign;
mod calls;
pub mod consts;
pub mod context;
mod control;
pub mod defs;
pub mod eval_splice;
pub mod features;
mod ffi;
mod literals;
mod pattern;

use crate::hir::{
    ArrayElem, Hir, HirNode, KwArg, LastMatch, NodeId, Params, PatternArm, RaiseCause, RegexpFlags,
    RescueClause, Span, StrPart, Visibility,
};
use crate::lower_error::LowerError;
use ruby_prism::{CallNode, Node, ParseResult};

use assign::{
    Storage, bind_call_target_once, bind_dynamic_const_scope, bind_index_target_once,
    build_call_target_write, build_index_target_write, index_arguments, lower_and_write,
    lower_compound_op_write, lower_multi_target, lower_multi_target_group, lower_or_write,
};
use calls::{lower_block, lower_block_like_params, lower_call_args};
use consts::{
    box_rooted_path, constant_path_name, constant_path_scope_and_name, dynamic_const_scope,
};
use control::{lower_begin, lower_if_chain, lower_single_optional_argument};
use defs::{
    const_holds_runtime_class, const_is_assigned, const_is_class_def, desugar_singleton_class_defs,
    lower_class_body, lower_params, lower_runtime_class, lower_runtime_class_reopen,
    runtime_class_body_is_expressible,
};
use eval_splice::{lower_box_eval, reject_top_level_defs, single_literal_string_arg};
pub use literals::encoding_const_name;
use literals::{line_of, lower_string_parts, string_literal_part};
use pattern::{lower_in_pattern_and_guard, lower_pattern};

pub type PResult<T> = Result<T, crate::lower_error::LowerError>;

/// Parses `source` as a standalone program and lowers it into `hir`, which
/// may already contain other nodes -- the primitive the compiler's top-level
/// drivers (`zeo::parse`), the loader's file splicing, and `eval`'s
/// literal-splice call-shape recognizer (below) all share.
/// File resolution/search-path concerns are deliberately NOT part of this --
/// it's purely "parse a string of Ruby into an existing arena".
pub fn parse_and_lower_into(hir: &mut Hir, source: &str) -> PResult<Vec<NodeId>> {
    let result = ruby_prism::parse(source.as_bytes());
    if let Some(err) = result.errors().next() {
        return Err(LowerError::syntax(format!(
            "parse error: {}",
            err.message()
        )));
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

/// Assembles prism's `(negative, LSB-first u32 digits)` integer shape into
/// an `i64` when it fits (`None` = a bignum literal).
fn assemble_i64(negative: bool, digits: &[u32]) -> Option<i64> {
    let mut magnitude: u64 = 0;
    for (i, &d) in digits.iter().enumerate() {
        if i >= 2 {
            if d != 0 {
                return None;
            }
            continue;
        }
        magnitude |= u64::from(d) << (32 * i);
    }
    if negative {
        // i64::MIN's magnitude is representable; anything larger isn't.
        if magnitude > (i64::MAX as u64) + 1 {
            return None;
        }
        Some((magnitude as i128).wrapping_neg() as i64)
    } else {
        i64::try_from(magnitude).ok()
    }
}

/// The span-stamping wrapper around the big lowering match: every prism node
/// entering lowering pushes its byte range (in the file currently being
/// lowered -- `Hir::lowering_file`) onto the arena's span stack, so each
/// `hir.push` during that node's lowering is stamped with ITS provenance,
/// and an error propagating out picks up the innermost frame's span
/// (`LowerError::with_span_if_missing`). `HirNode` itself carries no span
/// field -- the parallel `Hir::spans` table is the whole design.
/// A `@@name` READ, or the `raise` that stands in for one written outside any
/// class body -- see [`Hir::cvar_is_toplevel`].
pub(crate) fn cvar_read(hir: &mut Hir, name: String) -> NodeId {
    match hir.cvar_is_toplevel() {
        false => hir.push(HirNode::ClassVarRead(name)),
        true => cvar_toplevel_raise(hir),
    }
}

/// A `@@name = value` WRITE, or -- outside any class body -- `value` evaluated
/// for its side effects followed by the raise. Ruby's `setclassvariable`
/// instruction is what raises, so the right-hand side has already run by then.
pub(crate) fn cvar_write(hir: &mut Hir, name: String, value: NodeId) -> NodeId {
    if !hir.cvar_is_toplevel() {
        return hir.push(HirNode::ClassVarWrite(name, value));
    }
    let raise = cvar_toplevel_raise(hir);
    hir.push(HirNode::Seq(vec![value, raise]))
}

fn cvar_toplevel_raise(hir: &mut Hir) -> NodeId {
    let class = hir.push(HirNode::ClassRef("RuntimeError".to_string()));
    let message = hir.push(HirNode::StringLit(vec![StrPart::Lit(
        "class variable access from toplevel".to_string(),
    )]));
    hir.push(HirNode::Call {
        receiver: None,
        name: "raise".to_string(),
        args: vec![ArrayElem::Single(class), ArrayElem::Single(message)],
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    })
}

pub fn lower_node(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<NodeId> {
    let span = span_of(hir, node);
    hir.push_span(span);
    let out = lower_node_inner(result, hir, node);
    hir.pop_span();
    out.map_err(|e| e.with_span_if_missing(span))
}

/// Whether `recv` is the constant naming the `class`/`module` body being
/// lowered, spelled the same way that body's own header spelled it.
///
/// Comparing the WRITTEN names is what CRuby's cref rule comes to: the body of
/// `class SMTP` nested in `module Net` sees a bare `SMTP`, but the body of the
/// compact `class Pkg::Inner` does NOT -- its cref is just `[Pkg::Inner]`, so a
/// bare `Inner` there is `uninitialized constant Pkg::Inner::Inner`
/// (oracle-verified). Anything else is a genuine per-object singleton def and
/// keeps the runtime `define_singleton_method` desugar.
pub(crate) fn names_enclosing_class(hir: &Hir, recv: &Node<'_>) -> bool {
    let Some(enclosing) = hir.enclosing_class() else {
        return false;
    };
    consts::constant_path_name(recv).is_ok_and(|name| name == enclosing)
}

/// Which encoding a Regexp literal's flag letter forces, from prism's four
/// mutually exclusive flags. See [`zeo_abi::RegexpEncoding`] for what the answer
/// is observable through.
///
/// `/n`, `/e` and `/s` name an encoding that is not the source's, which is only
/// answerable for an ASCII-only pattern: every byte then means the same thing in
/// ASCII-8BIT, EUC-JP, Windows-31J and UTF-8 alike, so the flag changes nothing
/// but what the regexp REPORTS about itself. One non-ASCII byte and the readings
/// genuinely differ -- and ruby refuses to guess which was meant, in the PARSER
/// (`regexp encoding option 'e' differs from source encoding 'UTF-8'`, a
/// `SyntaxError`). prism is that parser, so `parse_and_lower_into` has already
/// turned those away by the time this runs; there is nothing left here to
/// reject, and re-deriving the rule would only be a second, worse copy of it.
///
/// ruby2ruby and ruby_parser both open with `ENC_EUC = /x/e.options` -- a
/// throwaway ASCII pattern whose only purpose is the flag bits.
fn forced_regexp_encoding(
    ascii_8bit: bool,
    euc_jp: bool,
    windows_31j: bool,
    utf_8: bool,
) -> zeo_abi::RegexpEncoding {
    use zeo_abi::RegexpEncoding;
    match (ascii_8bit, euc_jp, windows_31j, utf_8) {
        (true, ..) => RegexpEncoding::None,
        (_, true, ..) => RegexpEncoding::EucJp,
        (_, _, true, _) => RegexpEncoding::Windows31j,
        (.., true) => RegexpEncoding::Utf8,
        _ => RegexpEncoding::Source,
    }
}

/// The class a `class Sub < ... end` header names as its superclass.
///
/// `< self` inside a class body is the ENCLOSING class -- a compile-time fact,
/// and the shape optparse gives every argument style (`class NoArgument <
/// self` inside `class Switch`). Read as a dynamic superclass instead, the
/// subclass is minted at runtime over a compiled parent, and its instances are
/// name-keyed `DynObject`s the parent's own methods cannot run against.
fn superclass_name(hir: &Hir, sc: &Node<'_>) -> PResult<String> {
    if sc.as_self_node().is_some() {
        return hir
            .enclosing_class()
            .map(str::to_string)
            .ok_or_else(|| "`< self` names the enclosing class, and there isn't one here".into());
    }
    consts::constant_path_name(sc)
}

/// One prism node's provenance in the file currently lowering, or
/// `Span::SYNTH` when there is none.
///
/// Split out of [`lower_node`] for lowerings that EXPAND a node into several
/// `HirNode`s without descending through it -- `attr_accessor` becomes a pair
/// of `DefMethod`s -- because those must stamp the span themselves.
pub(crate) fn span_of(hir: &Hir, node: &Node<'_>) -> Span {
    let loc = node.location();
    match hir.lowering_file {
        Some(file) => Span {
            file,
            start: loc.start_offset() as u32,
            end: loc.end_offset() as u32,
        },
        None => Span::SYNTH,
    }
}

fn lower_node_inner(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<NodeId> {
    if let Some(int) = node.as_integer_node() {
        // prism's own arbitrary-precision value (LSB-first u32 digits) --
        // which also handles `0xff`/`0b101`/`1_000` uniformly, unlike the
        // old source-text `parse::<i64>()`. Values that fit stay the
        // ordinary `IntegerLit(i64)`; anything bigger is a bignum literal
        //.
        let value = int.value();
        let (negative, digits) = value.to_u32_digits();
        return Ok(hir.push(match assemble_i64(negative, digits) {
            Some(v) => HirNode::IntegerLit(v),
            None => HirNode::BigIntegerLit {
                negative,
                digits: digits.to_vec(),
            },
        }));
    }

    if let Some(float) = node.as_float_node() {
        return Ok(hir.push(HirNode::FloatLit(float.value())));
    }

    if let Some(rat) = node.as_rational_node() {
        // prism pre-rationalizes: `1.5r` arrives numerator 3, denominator
        // 2. The numerator carries the sign; the denominator is positive
        // and non-zero by syntax.
        let numerator = rat.numerator();
        let denominator = rat.denominator();
        let (negative, num_digits) = numerator.to_u32_digits();
        let (_, den_digits) = denominator.to_u32_digits();
        return Ok(hir.push(HirNode::RationalLit {
            negative,
            num_digits: num_digits.to_vec(),
            den_digits: den_digits.to_vec(),
        }));
    }

    if let Some(im) = node.as_imaginary_node() {
        let inner = lower_node(result, hir, &im.numeric())?;
        return Ok(hir.push(HirNode::ImaginaryLit(inner)));
    }

    // `-> (x) { ... }` -- a real `ruby-prism` node (unlike `lambda { }`
    // below, which is an ordinary method call). See `hir::HirNode::Lambda`'s
    // docs.
    if let Some(lambda) = node.as_lambda_node() {
        let params = lower_block_like_params(result, hir, lambda.parameters())?;
        let body = lower_body(result, hir, lambda.body())?;
        return Ok(hir.push(HirNode::Lambda {
            params,
            body,
            method_body: false,
        }));
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
    //
    // Multiple statements (`(a; b)`) lower to a `Seq`: evaluate each in
    // order, answer the last. That is exactly `Seq`'s existing codegen (one
    // tail-value Rust block expression), and it needs no scope of its own --
    // a local assigned inside leaks out, oracle-verified: `y = (a = 5; a *
    // 2)` leaves `a == 5` visible afterwards, so these are ordinary
    // statements in the enclosing scope, not a nested one.
    //
    // `()` is `nil` -- valid Ruby in expression position (`p(())` prints
    // `nil`), falsy as a condition (`while () ; end` never enters, matching
    // CRuby), and the falsy operand of a `&&`/`||`.
    if let Some(paren) = node.as_parentheses_node() {
        return match paren.body() {
            None => Ok(hir.push(HirNode::NilLit)),
            Some(n) => match n.as_statements_node() {
                Some(stmts) => {
                    let body: Vec<_> = stmts.body().iter().collect();
                    match body.as_slice() {
                        // Not wrapped in a `Seq`: `(x)` IS `x`, and the extra
                        // node would only cost a block expression around it.
                        [only] => lower_node(result, hir, only),
                        _ => {
                            let ids = body
                                .iter()
                                .map(|s| lower_node(result, hir, s))
                                .collect::<PResult<Vec<_>>>()?;
                            Ok(hir.push(HirNode::Seq(ids)))
                        }
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
        return Ok(lower_compound_op_write(
            hir,
            Storage::Local(name),
            op_name,
            rhs,
        ));
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
        return Ok(lower_compound_op_write(
            hir,
            Storage::Ivar(name),
            op_name,
            rhs,
        ));
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
        return Ok(lower_compound_op_write(
            hir,
            Storage::ClassVar(name),
            op_name,
            rhs,
        ));
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
        return Ok(lower_compound_op_write(
            hir,
            Storage::Global(name),
            op_name,
            rhs,
        ));
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
    // `BEGIN { ... }` -- hoisted by `analyze`; see `HirNode::PreExec`.
    if let Some(pre) = node.as_pre_execution_node() {
        let body = lower_body(result, hir, pre.statements().map(|s| s.as_node()))?;
        return Ok(hir.push(HirNode::PreExec(body)));
    }

    // `END { ... }` -- `at_exit { ... }` exactly, down to the reverse-order
    // rule (oracle-verified: two ENDs run last-written-first, identical to
    // two at_exits). Rewritten into that call rather than given a node of
    // its own, so it inherits the registration and the exit-time driver
    // already behind `at_exit`.
    if let Some(post) = node.as_post_execution_node() {
        let body = lower_body(result, hir, post.statements().map(|s| s.as_node()))?;
        let block = hir.push(HirNode::Block {
            params: Params::default(),
            body,
        });
        return Ok(hir.push(HirNode::Call {
            receiver: None,
            name: "at_exit".to_string(),
            args: Vec::new(),
            kwargs: Vec::new(),
            block: Some(block),
            block_arg: None,
            safe: false,
        }));
    }

    // `alias $new $old` -- an expression, not a class-body-only statement
    // (unlike `alias` on a method), so it lowers here. prism gives both
    // names as GlobalVariableReadNodes.
    if let Some(alias) = node.as_alias_global_variable_node() {
        // The SOURCE may also be a special: `alias $MATCH $&`, which is all
        // `require "English"` does. prism gives those their own node types
        // (`$&`/`` $` ``/`$'`/`$+`/`$~` are back-references, `$1`.. numbered),
        // so normalize back to the `$`-spelling the alias table keys on --
        // `globals::global_get` knows where each one really reads from. The
        // TARGET is always a plain name: `alias $& $x` is a SyntaxError.
        let special = |n: &Node<'_>| -> Option<String> {
            let loc = n
                .as_back_reference_read_node()
                .map(|b| b.location())
                .or_else(|| n.as_numbered_reference_read_node().map(|b| b.location()))?;
            Some(String::from_utf8_lossy(loc.as_slice()).into_owned())
        };
        let plain = |n: &Node<'_>| -> Option<String> {
            let g = n.as_global_variable_read_node()?;
            Some(String::from_utf8_lossy(g.name().as_slice()).into_owned())
        };
        let new_name = plain(&alias.new_name())
            .ok_or("`alias`'s new global name must be a plain `$name` global")?;
        let old = alias.old_name();
        let old_name = plain(&old)
            .or_else(|| special(&old))
            .ok_or("`alias`'s source must be a `$name` global or a match special")?;
        return Ok(hir.push(HirNode::AliasGlobal(new_name, old_name)));
    }

    // Hash shorthand -- `{x:, name:}`, the value-omitted form. prism wraps
    // the value it filled in (a local read, or a method call when no such
    // local exists) in an `ImplicitNode`; unwrapping it here means the
    // shorthand works everywhere a hash does -- literals, keyword
    // arguments, pattern matching -- rather than needing each site to know
    // about it.
    if let Some(implicit) = node.as_implicit_node() {
        return lower_node(result, hir, &implicit.value());
    }

    // `__FILE__` / `__LINE__` / `__ENCODING__` -- resolved HERE, at lowering
    // time, into ordinary literals. That is not a shortcut: they are
    // compile-time constants in real Ruby too, fixed by where the code was
    // WRITTEN. Deferring them to codegen would be strictly worse, since
    // `require` merges every file's statements into one `Program` and by
    // then nothing distinguishes them (see `loader`'s SOURCE_FILE stack).
    if node.as_source_file_node().is_some() {
        return Ok(hir.push(HirNode::StringLit(vec![StrPart::Lit(current_file_str()?)])));
    }
    if node.as_source_line_node().is_some() {
        let line = line_of(result, node.location().start_offset());
        return Ok(hir.push(HirNode::IntegerLit(line)));
    }
    // `__ENCODING__` would be `Encoding::UTF_8` (this compiler is UTF-8-only
    // throughout), but no `Encoding` CLASS exists yet to answer with -- it
    // is the encoding work's own deliverable: an RObj wrapping an EncodingId,
    // with `Encoding::UTF_8` and friends as real constants.
    // Rejected rather than stubbed: inventing a placeholder Encoding now
    // would pre-empt that design, and `__ENCODING__` is only useful if the
    // object it answers actually behaves like one.
    // `__ENCODING__` answers the script's own encoding -- UTF-8 by default,
    // or whatever a `# encoding:` magic comment set. Lowered to the ordinary
    // `Encoding::<NAME>` constant read, which resolves to the seeded singleton.
    if node.as_source_encoding_node().is_some() {
        let const_name = hir
            .script_encoding
            .clone()
            .unwrap_or_else(|| "UTF_8".to_string());
        return Ok(hir.push(HirNode::QualifiedConstRead(
            "Encoding".to_string(),
            const_name,
        )));
    }

    // `$1`..`$9` -- prism gives these their own node kind, not a global
    // read, because nothing can assign them.
    if let Some(nref) = node.as_numbered_reference_read_node() {
        return Ok(hir.push(HirNode::LastMatchRef(LastMatch::Group(
            nref.number() as usize
        ))));
    }
    // `` $` ``, `$&`, `$'` -- one node kind for all three, told apart by
    // name. (`$~` itself arrives as an ordinary global read, handled below.)
    if let Some(bref) = node.as_back_reference_read_node() {
        let name = String::from_utf8_lossy(bref.name().as_slice()).into_owned();
        let which = match name.as_str() {
            "$&" => LastMatch::Group(0),
            "$`" => LastMatch::Pre,
            "$'" => LastMatch::Post,
            "$+" => LastMatch::LastGroup,
            other => {
                return Err(format!(
                    "the `{other}` back-reference global isn't supported yet (zeo limitation)"
                )
                .into());
            }
        };
        return Ok(hir.push(HirNode::LastMatchRef(which)));
    }
    if let Some(gvr) = node.as_global_variable_read_node() {
        let name = String::from_utf8_lossy(gvr.name().as_slice()).into_owned();
        // `$~` reads the last-match slot, not the `$foo` table -- see
        // `HirNode::LastMatchRef`.
        if name == "$~" {
            return Ok(hir.push(HirNode::LastMatchRef(LastMatch::Data)));
        }
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
        return Ok(lower_compound_op_write(
            hir,
            Storage::Const { scope: None, name },
            op_name,
            rhs,
        ));
    }
    if let Some(op) = node.as_constant_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_and_write(
            hir,
            Storage::Const { scope: None, name },
            rhs,
        ));
    }
    // A `# shareable_constant_value:` magic comment makes prism wrap the
    // constant write in a `ShareableConstantNode`. Zeo enforces no Ractor
    // sharing, so unwrap to the inner write and lower it verbatim.
    if let Some(sc) = node.as_shareable_constant_node() {
        return lower_node(result, hir, &sc.write());
    }
    if let Some(op) = node.as_constant_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_or_write(
            hir,
            Storage::Const { scope: None, name },
            rhs,
        ));
    }
    if let Some(cw) = node.as_constant_write_node() {
        let name = String::from_utf8_lossy(cw.name().as_slice()).into_owned();
        // `Name = Struct.new(:a, :b)` with a literal member list compiles to a
        // REAL class, so a member is a struct field an accessor reaches
        // directly instead of an overlay slot reached through a dynamic send.
        if let Some(members) = defs::as_compiled_struct(&cw.value()) {
            return defs::synthesize_struct_class(hir, &name, &members);
        }
        // Everything else stays the RUNTIME path: the `Struct.new`/`Data.define`
        // call MINTS a class (`rstruct::struct_new`), the write binds it to the
        // constant, and `const_set` names the freshly anonymous class (Ruby's
        // "assigning an anonymous class to a constant names it").
        let value = lower_node(result, hir, &cw.value())?;
        return Ok(hir.push(HirNode::ConstWrite {
            scope: None,
            name,
            value,
        }));
    }
    // `Foo::BAR` / `Foo::BAR = v` / `Foo::BAR += v` / `Foo::BAR ||= v` /
    // `Foo::BAR &&= v` -- an explicitly namespace-qualified constant
    // (`ConstantPathNode` and its write/operator-write/and-write/or-write
    // relatives). See `constant_path_scope_and_name`'s docs.
    if let Some(op) = node.as_constant_path_operator_write_node() {
        let target = op.target();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        if let Some((parent, name)) = dynamic_const_scope(&target)? {
            let (bind, storage) = bind_dynamic_const_scope(result, hir, &parent, name)?;
            let rhs = lower_node(result, hir, &op.value())?;
            let write = lower_compound_op_write(hir, storage, op_name, rhs);
            return Ok(hir.push(HirNode::Seq(vec![bind, write])));
        }
        let (scope, name) = constant_path_scope_and_name(&target)?;
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_compound_op_write(
            hir,
            Storage::Const {
                scope: Some(scope),
                name,
            },
            op_name,
            rhs,
        ));
    }
    if let Some(op) = node.as_constant_path_and_write_node() {
        let target = op.target();
        if let Some((parent, name)) = dynamic_const_scope(&target)? {
            let (bind, storage) = bind_dynamic_const_scope(result, hir, &parent, name)?;
            let rhs = lower_node(result, hir, &op.value())?;
            let write = lower_and_write(hir, storage, rhs);
            return Ok(hir.push(HirNode::Seq(vec![bind, write])));
        }
        let (scope, name) = constant_path_scope_and_name(&target)?;
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_and_write(
            hir,
            Storage::Const {
                scope: Some(scope),
                name,
            },
            rhs,
        ));
    }
    if let Some(op) = node.as_constant_path_or_write_node() {
        let target = op.target();
        if let Some((parent, name)) = dynamic_const_scope(&target)? {
            let (bind, storage) = bind_dynamic_const_scope(result, hir, &parent, name)?;
            let rhs = lower_node(result, hir, &op.value())?;
            let write = lower_or_write(hir, storage, rhs);
            return Ok(hir.push(HirNode::Seq(vec![bind, write])));
        }
        let (scope, name) = constant_path_scope_and_name(&target)?;
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_or_write(
            hir,
            Storage::Const {
                scope: Some(scope),
                name,
            },
            rhs,
        ));
    }
    if let Some(cpw) = node.as_constant_path_write_node() {
        let target = cpw.target();
        // A dynamic scope needs no hidden binding here -- a plain write reads
        // the scope exactly once -- but it must still be evaluated BEFORE the
        // right-hand side, which is ruby's order and the node's own.
        if let Some((parent, name)) = dynamic_const_scope(&target)? {
            let scope = lower_node(result, hir, &parent)?;
            let value = lower_node(result, hir, &cpw.value())?;
            return Ok(hir.push(HirNode::DynConstWrite { scope, name, value }));
        }
        let (scope, name) = constant_path_scope_and_name(&target)?;
        let value = lower_node(result, hir, &cpw.value())?;
        return Ok(hir.push(HirNode::ConstWrite {
            scope: Some(scope),
            name,
            value,
        }));
    }
    if let Some(cp) = node.as_constant_path_node() {
        // `box::X`: an external access into the box -- the
        // ordinary bare-name lowering, wrapped in the box's scope.
        // A single segment lowers as a bare `ClassRef` (codegen's class-
        // or-constant rule under the box); deeper paths as the qualified
        // read they'd be inside the box.
        if let Some((bx, path)) = box_rooted_path(node) {
            let parsed = crate::constpath::ConstPath::parse(&path);
            let inner = match parsed.scope() {
                Some(scope) => hir.push(HirNode::QualifiedConstRead(
                    scope.to_string(),
                    parsed.base().to_string(),
                )),
                None => hir.push(HirNode::ClassRef(path.clone())),
            };
            return Ok(hir.push(HirNode::BoxScope {
                box_id: bx,
                body: vec![inner],
            }));
        }
        // A DYNAMIC scope (`self.class::Reason`, `@rbconfig::CONFIG`): no
        // segment of the path's parent names a static owner, so ask whether
        // the WHOLE parent spells a constant path rather than just its outer
        // node -- `self::Readline::HISTORY` (irb's input-method.rb) has a
        // parent that is itself a path, and only its root is dynamic.
        // Evaluate the scope as a value and run the SCOPE OPERATOR's own
        // search on it. optparse's `self.class::Reason`.
        if let Some((parent, name)) = dynamic_const_scope(&cp)? {
            let scope = lower_node(result, hir, &parent)?;
            return Ok(hir.push(HirNode::DynConstRead {
                scope,
                name,
                lenient: false,
            }));
        }
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
            .ok_or("`+=` on a method call with no receiver isn't supported (zeo limitation)")?;
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
            .ok_or("`&&=` on a method call with no receiver isn't supported (zeo limitation)")?;
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
            .ok_or("`||=` on a method call with no receiver isn't supported (zeo limitation)")?;
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
        let recv = op.receiver().ok_or(
            "`+=` on an indexing expression with no receiver isn't supported (zeo limitation)",
        )?;
        let idx = index_arguments(op.arguments())?;
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        let (binds, read_call, recv_tmp, idx_tmps) =
            bind_index_target_once(result, hir, &recv, &idx)?;
        let combined = hir.push(HirNode::Call {
            receiver: Some(read_call),
            name: op_name,
            args: vec![ArrayElem::Single(rhs)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        });
        let write_call = build_index_target_write(hir, &recv_tmp, &idx_tmps, combined);
        let mut stmts = binds;
        stmts.push(write_call);
        return Ok(hir.push(HirNode::Seq(stmts)));
    }
    if let Some(op) = node.as_index_and_write_node() {
        let recv = op.receiver().ok_or(
            "`&&=` on an indexing expression with no receiver isn't supported (zeo limitation)",
        )?;
        let idx = index_arguments(op.arguments())?;
        let rhs = lower_node(result, hir, &op.value())?;
        let (binds, read_call, recv_tmp, idx_tmps) =
            bind_index_target_once(result, hir, &recv, &idx)?;
        let write_call = build_index_target_write(hir, &recv_tmp, &idx_tmps, rhs);
        let and_node = hir.push(HirNode::And(read_call, write_call));
        let mut stmts = binds;
        stmts.push(and_node);
        return Ok(hir.push(HirNode::Seq(stmts)));
    }
    if let Some(op) = node.as_index_or_write_node() {
        let recv = op.receiver().ok_or(
            "`||=` on an indexing expression with no receiver isn't supported (zeo limitation)",
        )?;
        let idx = index_arguments(op.arguments())?;
        let rhs = lower_node(result, hir, &op.value())?;
        let (binds, read_call, recv_tmp, idx_tmps) =
            bind_index_target_once(result, hir, &recv, &idx)?;
        let write_call = build_index_target_write(hir, &recv_tmp, &idx_tmps, rhs);
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
        // `defined?(@@x)` outside any class body answers nil rather than
        // raising -- `defined?` never evaluates its operand, so the raise
        // `cvar_read` would otherwise put there must not be lowered at all.
        if defined.value().as_class_variable_read_node().is_some() && hir.cvar_is_toplevel() {
            return Ok(hir.push(HirNode::NilLit));
        }
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
        let (mut args, kwargs, _fwd_block) = lower_call_args(result, hir, yield_node.arguments())?;
        // A `*expr` splat needs no handling here: `Yield` carries the same
        // `Vec<ArrayElem>` a `Call`'s positional args do, and codegen flattens
        // a `Splat` element at runtime. Keyword args (literal pairs AND `**h`
        // double-splats) fold into one trailing `HashLit`, which codegen
        // builds via the shared `KwArg` emitter and `emit_proc_param_bindings`
        // binds a block's keyword params from.
        //
        // The fold is recorded, because a hash that arrived as KEYWORDS is
        // dropped when it turns out empty at runtime while one the source wrote
        // is not -- see `Hir::kwargs_hash_nodes`.
        if !kwargs.is_empty() {
            let hash = hir.push(HirNode::HashLit(kwargs));
            hir.kwargs_hash_nodes.insert(hash);
            args.push(ArrayElem::Single(hash));
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
        // The same argument lowering a CALL gets, for the same reasons:
        // positional args stay positional (carrying splats as
        // `ArrayElem::Splat`), a trailing keyword hash becomes `kwargs` bound
        // to the parent's keyword params by name, and `super(...)` inside
        // `def m(...)` expands to the three internal forwarding params.
        //
        // Lowering these by hand here is what made `super(...)` a gap while
        // `n(...)` worked: the hand-rolled loop reached `lower_array_elem`,
        // which has no forwarding arm, so the `...` fell through to the
        // generic rejection.
        let (args, kwargs, fwd_block) = lower_call_args(result, hir, sup.arguments())?;
        // A literal `super(x) { ... }` block vs a `super(x, &blk)` block-pass --
        // the same either/or a call carries (`BlockNode` vs `BlockArgumentNode`).
        // A `...`-forwarded block is a block-pass the arguments produced, and
        // an explicit one written beside it wins.
        let (block, block_arg) = match sup.block() {
            None => (None, fwd_block),
            Some(b) => {
                if let Some(barg) = b.as_block_argument_node() {
                    let expr = match barg.expression() {
                        Some(e) => lower_node(result, hir, &e)?,
                        None => hir.push(HirNode::LocalRead("__anon_blk".to_string())),
                    };
                    (None, Some(expr))
                } else {
                    (Some(lower_block(result, hir, &b)?), None)
                }
            }
        };
        return Ok(hir.push(HirNode::SuperCall {
            args,
            kwargs,
            zsuper: false,
            block,
            block_arg,
        }));
    }

    // Bare `super` (no parens) -- a distinct prism node from `super(...)`
    // since it forwards the enclosing method's arguments implicitly (as
    // currently bound, including reassignments -- oracle-verified). The
    // `zsuper` flag carries that distinction to codegen's
    // `emit_super_arg_bindings`; see `HirNode::SuperCall`'s docs.
    if let Some(fsup) = node.as_forwarding_super_node() {
        let block = match fsup.block() {
            None => None,
            Some(b) => Some(lower_block(result, hir, &b.as_node())?),
        };
        return Ok(hir.push(HirNode::SuperCall {
            args: Vec::new(),
            kwargs: Vec::new(),
            zsuper: true,
            block,
            block_arg: None,
        }));
    }

    if let Some(class) = node.as_class_node() {
        let name = constant_path_name(&class.constant_path())?;
        // A superclass that isn't a constant path (`class Point <
        // Struct.new(:x, :y)`) names a class that only comes into existence at
        // RUN time, so the subclass can't be one of the statically emitted
        // Rust structs -- it has to be minted at runtime too. See
        // `lower_runtime_class`.
        if let Some(sc) = class.superclass() {
            let runtime_parent = match superclass_name(hir, &sc) {
                // Not a constant path at all (`< Struct.new(:x)`).
                Err(_) => true,
                // A constant path that holds a runtime class VALUE (`Base =
                // Class.new` earlier in the file) rather than naming a
                // compile-time one -- the subclass has to be built at runtime
                // for the same reason. A name that is also a `class`
                // definition stays on the static path.
                Ok(n) => {
                    (const_is_assigned(hir, &n)
                        || crate::lower::defs::qualified_const_mints_runtime_class(hir, &n))
                        && !const_is_class_def(hir, &n)
                }
            };
            if runtime_parent {
                return lower_runtime_class(result, hir, &name, &sc, class.body());
            }
        } else if const_holds_runtime_class(hir, &name)
            && !const_is_class_def(hir, &name)
            && runtime_class_body_is_expressible(class.body())
        {
            // No superclass clause, and the name holds a runtime class value
            // (`D = Data.define(:x)`) -- this REOPENS that class rather than
            // defining a new one, so it lowers to a runtime reopen instead of
            // a `ClassDef` the static path would register as a fresh
            // (memberless) class.
            //
            // A body the runtime form can't express falls back to the STATIC
            // path rather than erroring: a constant alias to a builtin
            // (`INT_ALIAS = 1.class; class INT_ALIAS; include M; end`) is a
            // real Ruby shape the static path at least compiles, and turning
            // a program that ran into one that won't build is a worse
            // failure than the one it already had.
            return lower_runtime_class_reopen(result, hir, &name, class.body());
        }
        let superclass = match class.superclass() {
            None => None,
            Some(sc) => Some(superclass_name(hir, &sc)?),
        };
        let body = lower_class_body(
            result,
            hir,
            class.body(),
            superclass.as_deref(),
            Some(&name),
        )?;
        hir.record_class_def(&name);
        return Ok(hir.push(HirNode::ClassDef {
            name,
            superclass,
            body,
            is_module: false,
        }));
    }

    // `module Name ... end` -- see `HirNode::ClassDef`'s docs for why this
    // shares the same node as `class`. Nested modules/namespaced constant
    // paths (`module Foo::Bar`) aren't supported yet (zeo limitation, matching
    // today's existing top-level-only class restriction) -- `constant_name`
    // already rejects anything but a plain `ConstantReadNode`.
    if let Some(module) = node.as_module_node() {
        let name = constant_path_name(&module.constant_path())?;
        let body = lower_class_body(result, hir, module.body(), None, Some(&name))?;
        hir.record_class_def(&name);
        return Ok(hir.push(HirNode::ClassDef {
            name,
            superclass: None,
            body,
            is_module: true,
        }));
    }

    // `undef :a, :b` in EXPRESSION position -- reached when a class-body
    // `undef` sits under a guard zeo can't decide at compile time
    // (`undef :to_a if respond_to?(:to_a)`, drb). `HirNode::Undef` records a
    // compile-time fact and has no value form, so this becomes the runtime
    // send the guard can actually gate: `undef_method` on the class body's
    // `self`, whose overlay tombstone terminates lookup exactly as the static
    // form's does. The unguarded statement form still takes the static path
    // (`lower::defs::lower_class_body_statement`).
    if let Some(undef) = node.as_undef_node() {
        let args = undef
            .names()
            .iter()
            .map(|n| {
                let name = defs::alias_target_name(&n)?;
                Ok(ArrayElem::Single(hir.push(HirNode::SymbolLit(name))))
            })
            .collect::<PResult<Vec<_>>>()?;
        return Ok(hir.push(HirNode::Call {
            receiver: None,
            name: "undef_method".to_string(),
            args,
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
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
            // `def SMTP.default_port` written INSIDE `class SMTP` is the older
            // spelling of `def self.default_port` -- net/smtp uses it
            // throughout -- so it has to register as a class method, not as a
            // runtime per-object singleton the compile-time tables never see
            // (a `class << self; alias a b` naming one couldn't resolve `b`).
            Some(r) if names_enclosing_class(hir, &r) => true,
            Some(r) => {
                // `def obj.name` on a NON-`self` receiver -- a
                // per-object singleton method. Desugar to a runtime install:
                //   RECV.define_singleton_method(:name, ->(params) { body })
                // A lambda body gives method-like strict arity and
                // `return`-exits-the-method semantics; `define_singleton_method`
                // rebinds `self` to RECV when the method runs (see
                // `runtime_meta::dynamic_from_proc`). Documented divergence: a
                // real `def` opens a FRESH scope, but the lambda closes over
                // enclosing locals -- so a body referencing an enclosing local
                // reads it here rather than raising `NameError` (rare; the
                // common `@ivar`/param/`self` uses are exact).
                let recv = lower_node(result, hir, &r)?;
                let params = lower_params(result, hir, def.parameters())?;
                let body = lower_body(result, hir, def.body())?;
                // A method-body lambda: its `yield`/`block_given?`/`&block`
                // reach the block the METHOD is called with, threaded through
                // `ProcData`'s call-site block slot (see `HirNode::Lambda`'s
                // `method_body`).
                let lambda = hir.push(HirNode::Lambda {
                    params,
                    body,
                    method_body: true,
                });
                let sym = hir.push(HirNode::SymbolLit(name));
                return Ok(hir.push(HirNode::Call {
                    receiver: Some(recv),
                    name: "define_singleton_method".to_string(),
                    args: vec![ArrayElem::Single(sym), ArrayElem::Single(lambda)],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                }));
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
            is_def: true,
        }));
    }

    // `class << obj` at expression/statement position -- top level or
    // inside a method body. Desugars to a sequence of per-object
    // `define_singleton_method` installs on the receiver; its value is the last
    // (Ruby's own rule, the last `def`'s symbol). `class << self` takes the
    // same route: the receiver lowers to `self` -- `main` at the top level, or
    // a method's own receiver inside a body -- and the runtime install attaches
    // the singleton to whatever object that is. (A `class << self` inside a
    // CLASS body is handled earlier by `lower_class_body`, defining class
    // methods; this generic path is only top-level/method-body.)
    if let Some(singleton) = node.as_singleton_class_node() {
        let stmts = desugar_singleton_class_defs(result, hir, &singleton)?;
        return Ok(hir.push(HirNode::Seq(stmts)));
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
        return Ok(cvar_read(hir, name));
    }

    if let Some(cvar) = node.as_class_variable_write_node() {
        let name = String::from_utf8_lossy(cvar.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let value = lower_node(result, hir, &cvar.value())?;
        return Ok(cvar_write(hir, name, value));
    }

    if let Some(call) = node.as_call_node() {
        let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();

        // `using M` -- the top-level spelling, where it activates for the
        // rest of the file. The class-body spelling is recognized by
        // `defs::lower_class_body_statement`, which reaches the same helper.
        // The anonymous `using Module.new { }` form needs the class
        // registration only a class-body statement position provides, so at
        // the top level it falls through to the ordinary (raising) call.
        if let Some(nodes) = defs::lower_using(result, hir, node, &name, &call)?
            && let [id] = nodes[..]
        {
            return Ok(id);
        }

        /// Kernel's module functions that zeo answers with a COMPILE-TIME form
        /// rather than a runtime method row, so `Kernel.<name>` has to be
        /// recognized here to reach the same form. The list is
        /// `Kernel.singleton_methods(false) - Module.instance_methods` (which
        /// is what keeps Module's own `Kernel.name`/`Kernel.inspect` out),
        /// narrowed to the ones with no row.
        const KERNEL_FOLDED_FUNCTIONS: &[&str] = &[
            "__callee__",
            "__dir__",
            "__method__",
            "abort",
            "at_exit",
            "binding",
            "block_given?",
            "exec",
            "exit",
            "exit!",
            "fork",
            "gets",
            "global_variables",
            "iterator?",
            "lambda",
            "local_variables",
            "printf",
            "rand",
            "readline",
            "readlines",
            "select",
            "set_trace_func",
            "srand",
            "syscall",
            "test",
            "trace_var",
            "untrace_var",
        ];

        // `Kernel.foo(...)` -- an explicit module receiver in front of one of
        // Kernel's MODULE FUNCTIONS. Ruby defines each of them twice, as a
        // private instance method and as a singleton method on the module, and
        // both copies read the CALLER's frame: `Kernel.block_given?` and
        // `Kernel.binding` ask about the enclosing method exactly as the bare
        // spellings do. So the receiver carries no information, and dropping it
        // here lets one lowering -- and one codegen form -- serve both
        // spellings. Only the names zeo answers with a compile-time form are
        // listed; the rest already reach Kernel's own runtime row through
        // ordinary dispatch, which is the more faithful route anyway (there,
        // a user `def puts` cannot shadow `Kernel.puts`).
        let receiver = call.receiver().filter(|r| {
            !(KERNEL_FOLDED_FUNCTIONS.contains(&name.as_str())
                && r.as_constant_read_node()
                    .is_some_and(|c| String::from_utf8_lossy(c.name().as_slice()) == "Kernel"))
        });

        // `ClassName.new(args)` -- a distinct node; see hir.rs. The
        // concurrency builtins (`Fiber.new { }`, `Thread.new { }`,
        // `Mutex.new`, `Queue.new`) are deliberately NOT this shape:
        // `Fiber`/`Thread` must keep their BLOCK (the body), which
        // `HirNode::New` has no slot for, so all four fall through to the
        // generic `Call` lowering below (receiver becomes an ordinary
        // `ClassRef(name)`) and are intercepted by
        // `codegen::call::emit_call`'s builtin-constructor dispatch.
        if name == "new"
            && let Some(recv) = call.receiver()
            // `box::Widget.new(...)`: the ordinary static `New`, resolved
            // inside the box.
            && let Some((box_ctx, class_name)) = box_rooted_path(&recv)
                .map(|(bx, path)| (Some(bx), path))
                // Only a receiver that NAMES a class at compile time is a
                // static `New`. A dynamic constant scope (faraday's
                // `self.class::Handler.new`) spells a `ConstantPathNode` but
                // resolves its constant only at run time, so it joins every
                // other non-constant receiver (`x.new` on a local holding a
                // class value) on the generic `Call` lowering, which
                // dispatches via `TyKind::ClassObj`/the runtime constructor.
                .or_else(|| constant_path_name(&recv).ok().map(|n| (None, n)))
        {
            // `Enumerator.new { |y| ... }` joins the block-keeping set
            //: it falls through to the generic `Call`
            // lowering so the block reaches the runtime allocator via
            // the dynamic Class#new arm. `Proc.new { ... }` is in the
            // set for the same reason -- its block IS the value it
            // answers, and `HirNode::New` has no slot to carry one.
            // `Array.new(n) { |i| ... }` likewise: its block computes
            // each element, and routing it through `HirNode::New` would
            // silently DROP the block and answer `[nil, nil, ...]`.
            // A `*args` positional splat or `**h` double-splat can't bind
            // on the STATIC `New` path (`New.args` is `Vec<NodeId>`, no
            // runtime arg-vector, and a `**h`'s keys aren't known until
            // runtime). Fall through to the generic `Call` lowering, which
            // evaluates the constant to a `RubyValue::Class` and dispatches
            // `new` through the runtime constructor (the same path a
            // non-literal `x.new` receiver already takes).
            let has_dynamic_args = call
                .arguments()
                .map(|a| {
                    a.arguments().iter().any(|n| {
                        n.as_splat_node().is_some()
                            // `Klass.new(...)` inside `def m(...)`. The
                            // forwarding expands to `*rest, **kw, &blk`, which
                            // is a runtime arg vector by definition -- exactly
                            // what the static path cannot take.
                            || n.as_forwarding_arguments_node().is_some()
                            || n.as_keyword_hash_node().is_some_and(|kw| {
                                kw.elements()
                                    .iter()
                                    .any(|e| e.as_assoc_splat_node().is_some())
                            })
                    })
                })
                .unwrap_or(false);
            // A LITERAL block (`Foo.new(x) { ... }`) is captured and
            // forwarded to `initialize`; a block-PASS (`&p`) has no
            // `.as_block_node()` and falls through to the generic `Call`
            // lowering (its dynamic `new` dispatch threads the block arg).
            let block_pass = call.block().is_some_and(|b| b.as_block_node().is_none());
            if !has_dynamic_args
                && !block_pass
                && !matches!(
                    // An absolute `::Proc`/`::Fiber` path names the same
                    // builtin; match on the leaf so it keeps its block too.
                    class_name.strip_prefix("::").unwrap_or(class_name.as_str()),
                    // `Class.new(Super) { body }` keeps its block --
                    // the block IS the anonymous class's body; `HirNode::New`
                    // has no slot for it, so it falls through to the generic
                    // `Call` and the runtime `Class#new`.
                    // `Struct.new(...)` (and `Data.define`, which uses
                    // `.define` and never enters this `.new` path) MINTS A
                    // CLASS at runtime (`rstruct::struct_new`) in EVERY
                    // position -- Batch E: whether anonymous (a local/inline
                    // value) or bound to a constant (`Name = Struct.new(...)`,
                    // an ordinary constant write whose value is this call).
                    // It must reach the generic dynamic `new` dispatch rather
                    // than a static `New`; its block is the new class's body,
                    // kept the same way `Class.new`'s is.
                    "Fiber"
                        | "Thread"
                        | "Mutex"
                        | "Queue"
                        | "SizedQueue"
                        | "Ractor"
                        | "Enumerator"
                        | "Proc"
                        | "Array"
                        | "Hash"
                        | "Set"
                        | "Class"
                        | "Module"
                        | "Struct"
                )
            {
                // A trailing keyword hash lands in `kwargs`, kept apart
                // from the positionals exactly as an ordinary call's is,
                // so `initialize`'s keyword params bind as keywords.
                // A callee declaring NO keyword params still sees the
                // options hash it expects --
                // `emit_call_args_to` converts trailing keywords back to
                // one positional Hash in that case, which is Ruby's own
                // rule and what keyword_init Structs bind through.
                let mut args = Vec::new();
                let mut kwargs = Vec::new();
                if let Some(a) = call.arguments() {
                    for n in a.arguments().iter() {
                        if let Some(kw) = n.as_keyword_hash_node() {
                            let elements: Vec<Node<'_>> = kw.elements().iter().collect();
                            kwargs = lower_kwargs(result, hir, &elements)?;
                            continue;
                        }
                        args.push(lower_node(result, hir, &n)?);
                    }
                }
                let block = match call.block() {
                    Some(b) if b.as_block_node().is_some() => Some(lower_block(result, hir, &b)?),
                    _ => None,
                };
                let new_id = hir.push(HirNode::New {
                    class_name,
                    args,
                    kwargs,
                    block,
                });
                return Ok(match box_ctx {
                    Some(bx) => hir.push(HirNode::BoxScope {
                        box_id: bx,
                        body: vec![new_id],
                    }),
                    None => new_id,
                });
            }
        }

        // `define_method(:literal) { block }` -- desugars to a plain
        // `DefMethod`, identical treatment to `def`, mirroring zeo's
        // `walk_scope`. Every other shape -- a computed name, or a body passed
        // as a value rather than written as a block -- falls through to the
        // generic `Call` below and is served at run time by
        // `Module#define_method`.
        if name == "define_method"
            && receiver.is_none()
            && let (Some(args), Some(block_node)) = (call.arguments(), call.block())
        {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() == 1
                && let Some(sym) = arg_list[0].as_symbol_node()
            {
                let method_name = String::from_utf8_lossy(sym.unescaped()).into_owned();
                // `define_method(:name, &:other)` -- a symbol-to-proc
                // block argument rather than a literal block.
                //
                // In CRuby the `&` conversion happens at the CALL SITE,
                // before `rb_mod_define_method` ever runs (proc.c:2872,
                // which rejects a bare Symbol as its second positional
                // argument), so the method body is the symbol proc:
                // `->(recv, *rest) { recv.other(*rest) }`. That is why
                // the defined method takes its RECEIVER as the first
                // argument -- `w.as_str(7)` answers `7.to_s`.
                if let Some(target) = block_node
                    .as_block_argument_node()
                    .and_then(|b| b.expression())
                    .and_then(|e| e.as_symbol_node())
                {
                    let target = String::from_utf8_lossy(target.unescaped()).into_owned();
                    let recv = hir.push(HirNode::LocalRead("__sp_recv".to_string()));
                    let rest = hir.push(HirNode::LocalRead("__sp_args".to_string()));
                    let call = hir.push(HirNode::Call {
                        receiver: Some(recv),
                        name: target,
                        args: vec![ArrayElem::Splat(rest)],
                        kwargs: Vec::new(),
                        block: None,
                        block_arg: None,
                        safe: false,
                    });
                    return Ok(hir.push(HirNode::DefMethod {
                        name: method_name,
                        params: Params {
                            required: vec!["__sp_recv".to_string()],
                            rest: Some(Some("__sp_args".to_string())),
                            ..Params::default()
                        },
                        body: vec![call],
                        is_class_method: false,
                        visibility: Visibility::Public,
                        // An explicit `define_method` call, not a `def`.
                        is_def: false,
                    }));
                }
                // `define_method(:name, &proc_expr)` -- the body is a value the
                // program computes, so there is no source to desugar into a
                // `def`. Fall through to the ordinary call, which reaches
                // `Module#define_method` in the runtime; that row installs a
                // Proc, a Method or an UnboundMethod body and exists for
                // exactly the shapes this desugar cannot take.
                if let Some(block) = block_node.as_block_node() {
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
                        // An explicit `define_method` call, not a `def`.
                        is_def: false,
                    }));
                }
            }
        }

        // `define_singleton_method(:literal) { block }` -- desugars to a
        // `def self.name` on the target class. The target comes from the
        // receiver: none / `self` (inside a class body) means the enclosing
        // class, so a bare `DefMethod { is_class_method: true }` lands in the
        // current body and registers there; a literal-constant / constant-
        // path receiver (`C.` / `M::D.`) reopens that named class with an
        // inline `ClassDef`. A computed name, a computed receiver, or a
        // capturing block that this desugar can't model falls through to the
        // generic (unsupported) `Call`.
        // `&callable` is NOT a literal block: `define_singleton_method(:now,
        // &block)` hands over a Proc the caller already holds, which this
        // compile-time desugar has no body to install. The runtime row takes
        // it (`Kernel#define_singleton_method` accepts a block ARGUMENT and a
        // Method/Proc positional alike), so fall through rather than refuse --
        // ddtrace, mcp and datasource all write it that way.
        if name == "define_singleton_method"
            && let (Some(args), Some(block_node)) = (call.arguments(), call.block())
            && block_node.as_block_node().is_some()
        {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if let (1, Some(sym)) = (
                arg_list.len(),
                arg_list.first().and_then(|a| a.as_symbol_node()),
            ) {
                let target = match &receiver {
                    None => Some(None),
                    Some(r) if r.as_self_node().is_some() => Some(None),
                    // A constant receiver reopens that named class -- but
                    // ONLY when the constant actually names one. A constant
                    // the program ASSIGNS (`B = Box.new`, or even `Foo =
                    // Class.new`) holds a value, not a compile-time class,
                    // and reopening it here would mint a bogus empty class
                    // named `B`: `B.class` then answered `Class` and the
                    // installed method's `self` was that phantom class, so
                    // its body couldn't reach the real object's methods.
                    // Those fall through to the generic runtime
                    // `define_singleton_method` call, which installs a
                    // per-object singleton correctly.
                    Some(r) => constant_path_name(r)
                        .ok()
                        .filter(|n| !const_is_assigned(hir, n))
                        .map(Some),
                };
                if let Some(target) = target {
                    let method_name = String::from_utf8_lossy(sym.unescaped()).into_owned();
                    let block = block_node
                        .as_block_node()
                        .ok_or("define_singleton_method's argument must be a block")?;
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
                    let def = hir.push(HirNode::DefMethod {
                        name: method_name,
                        params,
                        body,
                        is_class_method: true,
                        visibility: Visibility::Public,
                        // A `define_singleton_method` CALL, not a `def` -- the
                        // same value its `define_method` sibling above carries,
                        // and three separate behaviours read it. The body is a
                        // CLOSURE, so the capture walk must descend into it
                        // (`captures`'s `DefMethod` arm skips a real `def`,
                        // which is a scope of its own); a bare `super` in it
                        // raises ruby's "implicit argument passing of super
                        // from method defined by define_method()" instead of
                        // running (`scope_is_define_method`); and its frame is
                        // labelled after where the BLOCK was written --
                        // `block in <class:E>`, not `E.trace`.
                        is_def: false,
                    });
                    return Ok(match target {
                        None => def,
                        Some(class_name) => hir.push(HirNode::ClassDef {
                            name: class_name,
                            superclass: None,
                            body: vec![def],
                            is_module: false,
                        }),
                    });
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
        if name == "loop" && receiver.is_none() {
            let no_args = call
                .arguments()
                .is_none_or(|a| a.arguments().iter().next().is_none());
            if no_args
                && let Some(block_node) = call.block()
                && let Some(block) = block_node.as_block_node()
            {
                let has_params = block
                    .parameters()
                    .is_some_and(|p| p.as_block_parameters_node().is_some());
                if !has_params {
                    let body = lower_body(result, hir, block.body())?;
                    // `Kernel#loop`'s REAL definition (CRuby
                    // kernel.rb:151) rescues StopIteration and
                    // returns its `result` -- desugared here into
                    // the ordinary Begin/rescue machinery, so
                    // `loop { e.next }` terminates
                    // cleanly with the enumeration's result and a
                    // manual `raise StopIteration` returns nil.
                    let native_loop = hir.push(HirNode::Loop { body });
                    let exc_read = hir.push(HirNode::LocalRead("__loop_stop".to_string()));
                    let result_call = hir.push(HirNode::Call {
                        receiver: Some(exc_read),
                        name: "result".to_string(),
                        args: Vec::new(),
                        kwargs: Vec::new(),
                        block: None,
                        block_arg: None,
                        safe: false,
                    });
                    return Ok(hir.push(HirNode::Begin {
                        body: vec![native_loop],
                        rescues: vec![crate::hir::RescueClause {
                            classes: vec!["StopIteration".to_string()],
                            splats: Vec::new(),
                            binding: Some("__loop_stop".to_string()),
                            body: vec![result_call],
                        }],
                        else_body: None,
                        ensure_body: None,
                    }));
                }
            }
        }

        // `block_given?` -- an ordinary zero-arg `Kernel` method call at the
        // `ruby-prism` level (not a distinct node, unlike `yield` above), so
        // this is a lowering-time call-shape desugar exactly like
        // `loop`/`define_method`. An explicit `self` receiver
        // (`self.block_given?`) is the same query about the current method's
        // block, so it desugars identically. `iterator?` is CRuby's (deprecated)
        // alias for `block_given?` and folds the same way.
        // `send(:block_given?)` / `send(:binding)` / `send(:iterator?)` /
        // `send(:local_variables)` with a LITERAL symbol is the reflective
        // spelling of the same caller-scope query, so it folds exactly like
        // the direct one (the `is_sent_eval` precedent) -- rewritten here to
        // the direct name so every recognizer below, and the Binding cell
        // promotion in `codegen::captures`, sees the plain spelling.
        // `public_send` is excluded: all four are private, so CRuby raises
        // NoMethodError there. A COMPUTED name stays a runtime send and
        // reaches the loud refusal rows (see
        // tests/gaps/kernel_scope_intrinsics_dynamic_send.rb).
        let name = if matches!(name.as_str(), "send" | "__send__")
            && receiver.is_none()
            && call.block().is_none()
        {
            let sent: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            let target = sent
                .first()
                .and_then(|n| n.as_symbol_node())
                .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned());
            match target.as_deref() {
                Some(t @ ("block_given?" | "iterator?" | "binding" | "local_variables"))
                    if sent.len() == 1 =>
                {
                    match t {
                        "block_given?" | "iterator?" => {
                            return Ok(hir.push(HirNode::BlockGiven));
                        }
                        "binding" => {
                            return Ok(hir.push(HirNode::Call {
                                receiver: None,
                                name: "binding".to_string(),
                                args: vec![],
                                kwargs: vec![],
                                block: None,
                                block_arg: None,
                                safe: false,
                            }));
                        }
                        _ => {
                            let binding = hir.push(HirNode::Call {
                                receiver: None,
                                name: "binding".to_string(),
                                args: vec![],
                                kwargs: vec![],
                                block: None,
                                block_arg: None,
                                safe: false,
                            });
                            return Ok(hir.push(HirNode::Call {
                                receiver: Some(binding),
                                name: "local_variables".to_string(),
                                args: vec![],
                                kwargs: vec![],
                                block: None,
                                block_arg: None,
                                safe: false,
                            }));
                        }
                    }
                }
                _ => name,
            }
        } else {
            name
        };
        let bg_self_or_none = match &receiver {
            None => true,
            Some(r) => r.as_self_node().is_some(),
        };
        if (name == "block_given?" || name == "iterator?") && bg_self_or_none {
            let no_args = call
                .arguments()
                .is_none_or(|a| a.arguments().iter().next().is_none());
            if no_args && call.block().is_none() {
                return Ok(hir.push(HirNode::BlockGiven));
            }
        }
        // `__dir__` -- a `Kernel` METHOD (not a keyword like `__FILE__`), but
        // one whose answer is fixed by where it was written, so it folds to
        // the same kind of literal. Defined as
        // `File.dirname(File.realpath(__FILE__))`, oracle-verified:
        // `__dir__ == File.dirname(File.expand_path(__FILE__))`.
        //
        // Folded rather than implemented as a runtime row, because a runtime
        // one could only ever answer the MAIN file's directory -- by then
        // every required file's statements share one `Program` and the
        // authorship is gone. That would be silently wrong for a `__dir__`
        // inside a required file, which is the main reason to write one.
        if name == "__dir__" && receiver.is_none() {
            let no_args = call
                .arguments()
                .is_none_or(|a| a.arguments().iter().next().is_none());
            if no_args && call.block().is_none() {
                let dir = current_dir_str()?;
                return Ok(hir.push(HirNode::StringLit(vec![StrPart::Lit(dir)])));
            }
        }

        // `local_variables` -- the names in scope where the call is written.
        // A Binding of this scope already carries exactly those, in exactly
        // that order, so this desugars to `binding.local_variables` and
        // inherits the whole Binding machinery, the analysis that promotes
        // those locals to shared cells included.
        if name == "local_variables" && receiver.is_none() && call.block().is_none() {
            let no_args = call
                .arguments()
                .is_none_or(|a| a.arguments().iter().next().is_none());
            if no_args {
                let binding = hir.push(HirNode::Call {
                    receiver: None,
                    name: "binding".to_string(),
                    args: vec![],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                });
                return Ok(hir.push(HirNode::Call {
                    receiver: Some(binding),
                    name: "local_variables".to_string(),
                    args: vec![],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                }));
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
        if name == "lambda" && receiver.is_none() {
            let no_args = call
                .arguments()
                .is_none_or(|a| a.arguments().iter().next().is_none());
            if no_args
                && let Some(block_node) = call.block()
                && let Some(block) = block_node.as_block_node()
            {
                let params = lower_block_like_params(result, hir, block.parameters())?;
                let body = lower_body(result, hir, block.body())?;
                return Ok(hir.push(HirNode::Lambda {
                    params,
                    body,
                    method_body: false,
                }));
            }
        }

        // `raise`/`fail` (exact synonyms) -- a zero/one/two positional-arg
        // call-shape desugar, same posture as `block_given?` above. The
        // `cause:` keyword form isn't lowered yet (see `HirNode::Raise`'s
        // docs) -- rejected here rather than silently dropped, matching
        // this project's "clean rejection over silent wrongness" rule.
        // `raise(*exc)` -- a splat arg has no static positional shape (its count
        // is a runtime value), so the special static-form lowering can't build
        // `HirNode::Raise`'s fixed 0..3 args. Skip it here; the general call
        // lowering handles it via `emit_splat_call` over the runtime
        // `Kernel#raise` builtin (`optparse.rb`'s `{|*exc| raise(*exc)}`).
        let raise_has_splat = || {
            call.arguments()
                .is_some_and(|a| a.arguments().iter().any(|n| n.as_splat_node().is_some()))
        };
        if (name == "raise" || name == "fail") && receiver.is_none() && !raise_has_splat() {
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            // A trailing keyword hash carries `cause:`. Splitting it off the
            // positional list is what keeps the three-state distinction: an
            // ABSENT `cause:` chains from `$!`, while `cause: nil` is
            // `Explicit` with a nil value and suppresses chaining.
            let (kw_nodes, positional): (Vec<_>, Vec<_>) = arg_list
                .iter()
                .partition(|n| n.as_keyword_hash_node().is_some());
            // `cause:` is the ONE keyword `raise` reads. Every other one is not
            // a keyword at all: ruby collapses the rest into a single Hash and
            // hands it over as an ordinary positional argument, so
            // `raise NotAuthorizedError, query: q, record: r` (pundit) is
            // `raise NotAuthorizedError, {query: q, record: r}` and reaches
            // `NotAuthorizedError.exception(hash)`. With nothing left after
            // `cause:` is taken out, no extra positional is passed at all.
            let mut cause = RaiseCause::Absent;
            let mut rest: Vec<crate::hir::KwArg> = Vec::new();
            for kw in &kw_nodes {
                let hash = kw.as_keyword_hash_node().expect("partitioned on this");
                for element in hash.elements().iter() {
                    let Some(assoc) = element.as_assoc_node() else {
                        // `**h` -- part of the collapsed hash like any pair.
                        let splat = element
                            .as_assoc_splat_node()
                            .ok_or("unsupported element in a `raise` keyword list")?;
                        let value = splat.value().ok_or("`**` needs a hash to splat")?;
                        rest.push(crate::hir::KwArg::DoubleSplat(lower_node(
                            result, hir, &value,
                        )?));
                        continue;
                    };
                    let key = assoc
                        .key()
                        .as_symbol_node()
                        .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
                        .unwrap_or_default();
                    if key == "cause" {
                        cause = RaiseCause::Explicit(lower_node(result, hir, &assoc.value())?);
                        continue;
                    }
                    let k = lower_node(result, hir, &assoc.key())?;
                    let v = lower_node(result, hir, &assoc.value())?;
                    rest.push(crate::hir::KwArg::Pair(k, v));
                }
            }
            let collapsed = usize::from(!rest.is_empty());
            if positional.len() + collapsed > 3 {
                return Err(format!(
                    "wrong number of arguments (given {}, expected 0..3)",
                    positional.len() + collapsed
                )
                .into());
            }
            if positional.is_empty() && collapsed == 0 && matches!(cause, RaiseCause::Explicit(_)) {
                return Err("only cause is given with no arguments".to_string().into());
            }
            let mut args = positional
                .iter()
                .map(|n| lower_node(result, hir, n))
                .collect::<PResult<Vec<_>>>()?;
            if !rest.is_empty() {
                args.push(hir.push(HirNode::HashLit(rest)));
            }
            return Ok(hir.push(HirNode::Raise(args, cause)));
        }

        // `eval("literal string")` -- ONLY the compile-time-constant-string
        // form. Unlike `define_method`/`loop` above, this is intercepted
        // UNCONDITIONALLY: those two have a genuine second runtime path for
        // their non-desugared shape (an ordinary implicit-self `Call`), but
        // `eval` doesn't -- this path has no runtime parser/interpreter (see
        // docs/EVAL_VM.md), so letting a non-literal `eval(...)` fall through
        // as a plain `Call` would compile cleanly and only fail at RUNTIME
        // with a confusing `NoMethodError`, strictly worse than a clear
        // compile-time rejection.
        // `require`/`require_relative`/`load` reaching THIS function means
        // the statement was NOT in direct top-level statement position (the
        // one place `parse::loader`'s file-level loop recognizes and resolves
        // them) -- a method body, a `begin` block, a conditional, an `eval`
        // body, a class body. A LITERAL, RESOLVABLE `require`/`require_relative`
        // folds to its load-result bool (its target was already spliced by the
        // loader's pre-pass, or a builtin feature activated). Everything else --
        // `load`, a non-literal target, or a plain `require` the loader's
        // resolvability pre-scan marked UNRESOLVABLE -- falls through to the
        // runtime `Kernel#{require,load}` below, which raises CRuby's `LoadError`
        // if and when it executes. That is what makes the optional-dependency
        // idiom (`begin; require "x"; rescue LoadError`) behave at runtime
        // exactly as in CRuby, rather than a compile error.
        if receiver.is_none() && matches!(name.as_str(), "require" | "require_relative" | "load") {
            // A non-top-level `require`/`require_relative` of a LITERAL feature
            // is usually a compile-time no-op: the loader's eager pre-pass
            // (`Loader::lower_file_statements`) already spliced the target, so
            // the CALL only reports a load result.
            //
            // A native builtin has nothing to splice; its whole effect is to
            // activate a gated feature, which is a compile-time act from any
            // position. `activated_features` doubles as CRuby's loaded-features
            // table, so `require` folds to the bool `insert` reports
            // (`load.c:1413`: true the first time, false thereafter). A spliced
            // file folds to `true`: it loads at program start, so the `unless
            // defined?`/`if <cond>` guards around these requires short-circuit
            // and the return value is rarely read.
            //
            // Two kinds of require are NOT spliced and must keep their call: a
            // plain `require` the loader could not resolve, and one only a
            // method body reaches (`Hir::deferred_requires`). Both fall through
            // to the runtime `Kernel#require`, which answers `false` for an
            // already-loaded feature and raises `LoadError` otherwise.
            if matches!(name.as_str(), "require" | "require_relative")
                && let Some(feature) = single_literal_string_arg(result, hir, &call)?
            {
                if name == "require" && features::is_builtin_feature(&feature) {
                    let newly_loaded = hir
                        .activated_features
                        .insert(features::canonical_ext_feature(&feature).to_string());
                    let first = newly_loaded && !features::is_preloaded_at_boot(&feature);
                    return Ok(hir.push(HirNode::BoolLit(first)));
                }
                let unresolvable =
                    name == "require" && hir.unresolvable_requires.contains(&feature);
                if !unresolvable && !hir.deferred_requires.contains(&feature) {
                    return Ok(hir.push(HirNode::BoolLit(true)));
                }
            }
            // `load`, or a `require` of a NON-literal (runtime-computed) target:
            // whole-program AOT can't splice a path it only learns at runtime.
            // Rather than fail the whole compile, FALL THROUGH to the ordinary
            // implicit-self `Call` lowering below (the same trick a non-literal
            // `eval` uses), which dispatches to the runtime `Kernel#{require,
            // require_relative,load}` -- raising CRuby's `LoadError` if and when
            // the call actually executes (zeo has no runtime Ruby loader). This
            // lets a guarded dynamic load -- `load ENV["X"] if ENV["X"]` -- and
            // the `begin; require dyn; rescue LoadError` idiom COMPILE, with the
            // guard/rescue behaving at runtime.
        }
        // `autoload :Const, "feature"` -- the loader's eager pre-pass
        // (`Loader::lower_file_statements`) has already SPLICED the feature
        // file so `Const` is defined, treating autoload as a compile-time
        // require. The call itself is therefore a runtime no-op. We still
        // validate the target resolves at compile time here (`autoload_feature`
        // errors on a dynamic path/symbol, exactly like a non-top-level
        // `require`), so a genuinely dynamic autoload is a clean rejection
        // rather than a silently-undefined constant.
        if name == "autoload" && receiver.is_none() {
            // Always a real call. A target the pre-pass could name is already
            // spliced, so the runtime row finds it in `$LOADED_FEATURES` and
            // does nothing; a computed one -- including the one-argument form
            // every `autoload` DSL defines over `Module#autoload` -- computes
            // its string there and loads the unit compiled in for it.
            //
            // Lowering it to a no-op on the strength of `autoload_feature`
            // alone was a false pass: the pre-pass walks class/module bodies,
            // not method bodies, so a literal `autoload` inside a `def`
            // resolved here, spliced nowhere, and silently defined nothing.
            if autoload_feature(&call).is_err() {
                hir.demand_feature_units();
            }
        }

        // `Ruby::Box` guard rails. Everything but ALLOCATION is an ordinary
        // runtime call (the class carries real rows); an unassigned/nested
        // `.new` would allocate a box nothing could ever reference, so that
        // one shape stays a clean compile-time rejection.
        if let Some(recv) = &receiver {
            if constant_path_name(recv).is_ok_and(|n| n == "Ruby::Box") && name == "new" {
                return Err(format!(
                    "`Ruby::Box.{name}` isn't supported here (zeo limitation) -- the one supported allocation shape is `box = Ruby::Box.new` as a top-level statement"
                ).into());
            }
            // Operations on a bound box handle outside their recognized
            // positions: `box.require`-family must be a TOP-LEVEL
            // statement (same rule as receiver-less `require`);
            // expression-position `box.eval` is allowed but, like root
            // `eval`, can't define classes/methods.
            if let Some(lv) = recv.as_local_variable_read_node() {
                let lname = String::from_utf8_lossy(lv.name().as_slice()).into_owned();
                if let Some(bx) = context::current_box_binding(&lname) {
                    match name.as_str() {
                        "require" | "require_relative" | "load" => {
                            return Err(format!(
                                "`{lname}.{name}` is only supported as a top-level statement (same rule as the receiver-less `{name}`)"
                            ).into());
                        }
                        "eval" => {
                            return lower_box_eval(hir, result, &call, bx, false);
                        }
                        _ => {}
                    }
                }
            }
        }

        if name == "eval" && receiver.is_none() {
            // A single string-LITERAL argument keeps the zero-cost AOT path:
            // the source is parsed and INLINED at compile time (`HirNode::Eval`),
            // needs no runtime parser, and still sees the surrounding scope's
            // locals -- prism parses the snippet on its own, so a bare name
            // arrives as a vcall, and `codegen::call` resolves it back against
            // the scope (`Ctx::in_eval_splice`). Every other shape -- a
            // non-literal source expression, or the `binding`/`filename`/
            // `lineno` argument forms -- falls through to the ordinary
            // implicit-self `Call` lowering below, which routes `Kernel#eval`
            // into the runtime eval VM (feature-gated, so a build without it
            // raises NotImplementedError at the call), carrying a `Binding` of
            // the calling scope so that path sees the caller's locals too.
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if arg_list.len() == 1
                && let Some(s) = arg_list[0].as_string_node()
            {
                let src = String::from_utf8_lossy(s.unescaped()).into_owned();
                // Try the zero-cost AOT inline path. If the literal source
                // doesn't parse, or defines at the top level (which the
                // inline path can't express), DON'T fail the compile: fall
                // through to the runtime eval VM so the program still
                // builds and the error/behaviour surfaces at runtime,
                // catchably, exactly as CRuby's `eval` does.
                if let Ok(body) = parse_and_lower_into(hir, &src)
                    && reject_top_level_defs(hir, &body).is_ok()
                {
                    return Ok(hir.push(HirNode::Eval(body)));
                }
            }
        }

        let receiver = match receiver {
            None => None,
            Some(r) => Some(lower_node(result, hir, &r)?),
        };
        let (args, kwargs, fwd_block) = lower_call_args(result, hir, call.arguments())?;
        // A call's `block()` slot is one of two distinct shapes: a literal
        // `{ }`/`do..end` (`BlockNode`), or `&existing_proc` forwarding an
        // already-built Proc value onward (`BlockArgumentNode`) -- real Ruby
        // syntax forbids a call from having both, so this is a clean
        // either/or, not a "prefer one" choice. A `...` in the argument
        // list contributes its own block forwarding (`fwd_block`).
        let (block, block_arg) = match call.block() {
            None => (None, fwd_block),
            Some(b) => {
                if let Some(barg) = b.as_block_argument_node() {
                    let expr = match barg.expression() {
                        Some(e) => lower_node(result, hir, &e)?,
                        // Anonymous `&` forwarding -- references the
                        // enclosing method's internally-named `&` param
                        // (see `lower_params`).
                        None => hir.push(HirNode::LocalRead("__anon_blk".to_string())),
                    };
                    (None, Some(expr))
                } else {
                    (Some(lower_block(result, hir, &b)?), None)
                }
            }
        };
        let is_vcall = call.is_variable_call();
        let built = HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: call.is_safe_navigation(),
        };
        // prism flags the call it built out of assignment syntax -- `s.x = v`
        // and `s[i] = v` are both plain `CallNode`s, distinguished from an
        // explicit `s.[]=(i, v)` by nothing else.
        if call.is_attribute_write() {
            return Ok(assign::push_assignment_call(hir, built));
        }
        let node = hir.push(built);
        if is_vcall {
            hir.vcall_nodes.insert(node);
        }
        // A literal block on a re-homing call runs under the RECEIVER's
        // `self` -- see `Hir::rehomed_blocks`.
        if let HirNode::Call {
            receiver: Some(_),
            name,
            block: Some(b),
            ..
        } = &hir[node]
            && matches!(
                name.as_str(),
                "instance_eval"
                    | "instance_exec"
                    | "class_eval"
                    | "class_exec"
                    | "module_eval"
                    | "module_exec"
            )
        {
            let b = *b;
            hir.rehomed_blocks.insert(b);
        }
        // A computed-name `define_method(name) { }` block IS a method body
        // at run time -- see `Hir::dynamic_define_method_blocks`.
        if let HirNode::Call {
            name,
            block: Some(b),
            ..
        } = &hir[node]
            && matches!(name.as_str(), "define_method" | "define_singleton_method")
        {
            let b = *b;
            hir.dynamic_define_method_blocks.insert(b);
        }
        return Ok(node);
    }

    if let Some(s) = node.as_string_node() {
        return Ok(hir.push(HirNode::StringLit(vec![string_literal_part(s.unescaped())])));
    }

    // `:"hello_#{x}"` -- an interpolated symbol is exactly its interpolated
    // STRING, interned. Lowered as that string plus a `to_sym` call rather
    // than given its own HIR node: the parts are the same shape, and the
    // name isn't known until runtime anyway, so there is nothing a
    // dedicated node could do that this doesn't.
    if let Some(isym) = node.as_interpolated_symbol_node() {
        let parts = lower_string_parts(result, hir, isym.parts().iter())?;
        let text = hir.push(HirNode::StringLit(parts));
        return Ok(hir.push(HirNode::Call {
            receiver: Some(text),
            name: "to_sym".to_string(),
            args: Vec::new(),
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        }));
    }

    if let Some(istr) = node.as_interpolated_string_node() {
        let parts = lower_string_parts(result, hir, istr.parts().iter())?;
        return Ok(hir.push(HirNode::StringLit(parts)));
    }

    // `` `cmd` `` / `%x{cmd}` (and the interpolated form) -- CRuby compiles
    // both to `putself` + an ordinary send of `` :` `` with the command
    // String as its one argument (compile.c), so they are the exact
    // string-literal shapes above wrapped in an implicit-self fcall to the
    // overridable `Kernel#\``. Not a direct syscall: a user who reopens
    // `Kernel#\`` (or defines `` def `(cmd) ``) wins, real Ruby's rule.
    if let Some(xs) = node.as_x_string_node() {
        let cmd = hir.push(HirNode::StringLit(vec![string_literal_part(
            xs.unescaped(),
        )]));
        return Ok(hir.push(HirNode::Call {
            receiver: None,
            name: "`".to_string(),
            args: vec![ArrayElem::Single(cmd)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        }));
    }

    if let Some(xs) = node.as_interpolated_x_string_node() {
        let parts = lower_string_parts(result, hir, xs.parts().iter())?;
        let cmd = hir.push(HirNode::StringLit(parts));
        return Ok(hir.push(HirNode::Call {
            receiver: None,
            name: "`".to_string(),
            args: vec![ArrayElem::Single(cmd)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        }));
    }

    // `/pattern/flags` / `%r{pattern}flags` (`RegularExpressionNode` covers
    // BOTH delimiter spellings -- prism only distinguishes opening/closing
    // `Location`s, not a separate node kind). `o` is the one genuinely
    // ignorable flag: "only interpolate once" has no effect when every regexp
    // literal is freshly constructed anyway.
    if let Some(re) = node.as_regular_expression_node() {
        let content = String::from_utf8_lossy(re.unescaped()).into_owned();
        let encoding = forced_regexp_encoding(
            re.is_ascii_8bit(),
            re.is_euc_jp(),
            re.is_windows_31j(),
            re.is_utf_8(),
        );
        return Ok(hir.push(HirNode::RegexpLit(
            vec![StrPart::Lit(content)],
            RegexpFlags {
                ignore_case: re.is_ignore_case(),
                extended: re.is_extended(),
                multiline: re.is_multi_line(),
                encoding,
            },
        )));
    }

    if let Some(re) = node.as_interpolated_regular_expression_node() {
        let parts = lower_string_parts(result, hir, re.parts().iter())?;
        let encoding = forced_regexp_encoding(
            re.is_ascii_8bit(),
            re.is_euc_jp(),
            re.is_windows_31j(),
            re.is_utf_8(),
        );
        return Ok(hir.push(HirNode::RegexpLit(
            parts,
            RegexpFlags {
                ignore_case: re.is_ignore_case(),
                extended: re.is_extended(),
                multiline: re.is_multi_line(),
                encoding,
            },
        )));
    }

    // A bare regex literal used directly as an implicit condition against
    // `$_` (`if /foo/` -- `MatchLastLineNode`/its interpolated counterpart)
    // -- `$_`/the "last read line" concept isn't modeled at all, a clean
    // rejection rather than silently matching against an always-empty
    // string.
    if node.as_match_last_line_node().is_some()
        || node.as_interpolated_match_last_line_node().is_some()
    {
        return Err(
            "a bare Regexp literal used as an implicit condition (`if /foo/`, matching against `$_`) isn't supported yet (zeo limitation) -- write an explicit `=~`/`match?` against a real receiver instead".to_string().into(),
        );
    }

    // `/(?<name>...)/ =~ str` -- named-capture AUTO-BINDING: real Ruby
    // assigns each named group to a LOCAL of that name. prism hands this
    // over as its own `MatchWriteNode`, having already worked out both the
    // match call and the target names -- so this is a pure desugar over a
    // known list, with no pattern-scanning of our own.
    //
    // Only the literal-on-the-LEFT form is this node at all: `str =~
    // /(?<a>.)/` is an ordinary `CallNode` and binds nothing
    // (oracle-verified). That asymmetry is Ruby's, not an approximation --
    // the parser can only declare the locals when it can see the names.
    if let Some(mw) = node.as_match_write_node() {
        return lower_named_capture_match(result, hir, &mw);
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
        let elements: Vec<Node<'_>> = h.elements().iter().collect();
        let kwargs = lower_kwargs(result, hir, &elements)?;
        return Ok(hir.push(HirNode::HashLit(kwargs)));
    }

    // The BRACE-LESS hash as the last element of an ARRAY literal
    // (`[a, b, from: x, to: y]`). Ruby collapses it into one ordinary Hash
    // there -- `[1, k: 2]` has two elements, not three -- so it lowers exactly
    // like the braced form above; prism just spells it differently.
    //
    // A CALL's trailing hash never reaches here: `lower_call_args` peels it
    // off first, because in that position it is keyword arguments rather than
    // a value, and the two bind differently.
    if let Some(kw) = node.as_keyword_hash_node() {
        let elements: Vec<Node<'_>> = kw.elements().iter().collect();
        let kwargs = lower_kwargs(result, hir, &elements)?;
        return Ok(hir.push(HirNode::HashLit(kwargs)));
    }

    // A `..`/`...` prism decided is a CONDITION, not a Range -- see
    // `HirNode::FlipFlop`. An omitted side lowers to nil, which is falsy, and
    // that is exactly Ruby's behaviour for a one-sided flip-flop.
    if let Some(ff) = node.as_flip_flop_node() {
        let mut side = |n: Option<Node<'_>>| match n {
            None => Ok(hir.push(HirNode::NilLit)),
            Some(n) => lower_node(result, hir, &n),
        };
        let left = side(ff.left())?;
        let right = side(ff.right())?;
        let state = hir.flip_flops;
        hir.flip_flops += 1;
        return Ok(hir.push(HirNode::FlipFlop {
            state,
            left,
            right,
            exclusive: ff.is_exclude_end(),
        }));
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
    // The do-while form (`begin...end while cond`) is prism's begin-modifier
    // flag on the same node -- carried through as `post` so codegen runs the
    // body once before the first condition test.
    if let Some(while_node) = node.as_while_node() {
        let cond = lower_node(result, hir, &while_node.predicate())?;
        let body = lower_body(result, hir, while_node.statements().map(|s| s.as_node()))?;
        return Ok(hir.push(HirNode::While {
            cond,
            body,
            negate: false,
            post: while_node.is_begin_modifier(),
        }));
    }
    if let Some(until_node) = node.as_until_node() {
        let cond = lower_node(result, hir, &until_node.predicate())?;
        let body = lower_body(result, hir, until_node.statements().map(|s| s.as_node()))?;
        return Ok(hir.push(HirNode::While {
            cond,
            body,
            negate: true,
            post: until_node.is_begin_modifier(),
        }));
    }

    // `for var in iterable ... end` / `for a, b in pairs ... end` -- see
    // `lower_multi_target`'s docs for the full generalized target shape.
    if let Some(for_node) = node.as_for_node() {
        let target = lower_multi_target(result, hir, &for_node.index())?;
        let iterable = lower_node(result, hir, &for_node.collection())?;
        let body = lower_body(result, hir, for_node.statements().map(|s| s.as_node()))?;
        return Ok(hir.push(HirNode::For {
            target,
            iterable,
            body,
        }));
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
                splats: Vec::new(),
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

    // `alias new old` reached in a GENERAL context -- inside a `class_eval`/
    // `module_eval` block (delegate.rb's `kernel.class_eval do alias __raise__
    // raise end`), where `self` is the module being reopened. Class-body and
    // top-level `alias` are intercepted earlier (`defs::lower_class_body`,
    // `loader`) and never arrive here. Desugar to a runtime `alias_method(:new,
    // :old)` self-send -- the same runtime path the CALL form already takes
    // (`zeo_rt::runtime_meta::runtime_alias_method`, as ostruct's dynamic
    // aliasing does). Correct when the default definee IS `self` (an eval
    // block); the pathological in-method `alias` (definee = the owner class,
    // not self) stays unmodeled -- it was a hard error here before too.
    if let Some(alias) = node.as_alias_method_node() {
        let new_sym = hir.push(HirNode::SymbolLit(defs::alias_target_name(
            &alias.new_name(),
        )?));
        let old_sym = hir.push(HirNode::SymbolLit(defs::alias_target_name(
            &alias.old_name(),
        )?));
        return Ok(hir.push(HirNode::Call {
            receiver: None,
            name: "alias_method".to_string(),
            args: vec![ArrayElem::Single(new_sym), ArrayElem::Single(old_sym)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        }));
    }

    Err(format!(
        "unsupported syntax at {:?} (a zeo lowering gap, not necessarily invalid Ruby)",
        node.location()
    )
    .into())
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
            .ok_or("a bare `*` isn't supported inside an array literal (zeo limitation)")?;
        return Ok(ArrayElem::Splat(lower_node(result, hir, &expr)?));
    }
    Ok(ArrayElem::Single(lower_node(result, hir, node)?))
}

/// Lowers a keyword-hash / hash-literal element list into the ordered
/// `KwArg` list, preserving SOURCE ORDER between literal `k: v` pairs and
/// `**expr` double-splats (Ruby's insertion-ordered, last-wins merge makes
/// the interleaving observable). Shared by call kwargs, `.new` kwargs, and
/// `{ }` literals -- one representation, one builder.
fn lower_kwargs(result: &ParseResult, hir: &mut Hir, elements: &[Node<'_>]) -> PResult<Vec<KwArg>> {
    let mut kwargs = Vec::with_capacity(elements.len());
    for el in elements {
        if let Some(splat) = el.as_assoc_splat_node() {
            let expr = match splat.value() {
                Some(expr) => lower_node(result, hir, &expr)?,
                // Anonymous `**` forwarding -- the enclosing method's
                // internally-named `**` param (see `lower_params`).
                None => hir.push(HirNode::LocalRead("__anon_kwrest".to_string())),
            };
            kwargs.push(KwArg::DoubleSplat(expr));
        } else {
            let assoc = el
                .as_assoc_node()
                .ok_or("unsupported keyword-argument shape (zeo limitation)")?;
            let key = lower_node(result, hir, &assoc.key())?;
            let value = lower_node(result, hir, &assoc.value())?;
            kwargs.push(KwArg::Pair(key, value));
        }
    }
    Ok(kwargs)
}

/// The `/(?<a>..)/ =~ str` desugar: run the match (which records `$~`, as
/// every match does), then assign each named group to a local of that name.
///
/// Emitted as a `Seq` whose LAST statement is the match RESULT, so the
/// whole thing still answers what `=~` answers (the match index, or nil) --
/// `if /(?<a>.)/ =~ s` has to keep working as a condition.
///
/// Each local reads from `$~` rather than from a saved MatchData temp,
/// which is what makes the failed-match case need no branch: a failed match
/// CLEARS the slot, so `$~&.[](:a)` is nil, exactly Ruby's answer
/// (oracle-verified).
fn lower_named_capture_match(
    result: &ParseResult,
    hir: &mut Hir,
    mw: &ruby_prism::MatchWriteNode<'_>,
) -> PResult<NodeId> {
    // The match itself is an ordinary `=~` call -- lowered through the
    // normal path, so it picks up the Regexp/String dispatch and the
    // last-match recording without this desugar knowing about either.
    let match_call = lower_node(result, hir, &mw.call().as_node())?;
    // Bound to a temp first, so the result survives the assignments below
    // and can be the Seq's tail.
    let m_tmp = "__named_capture_result".to_string();
    let mut body = vec![hir.push(HirNode::LocalWrite(m_tmp.clone(), match_call))];
    for target in mw.targets().iter() {
        let lvt = target
            .as_local_variable_target_node()
            .ok_or("`=~`'s named-capture auto-binding only writes plain locals (zeo limitation)")?;
        let name = String::from_utf8_lossy(lvt.name().as_slice()).into_owned();
        let group = hir.push(HirNode::SymbolLit(name.clone()));
        let last = hir.push(HirNode::LastMatchRef(LastMatch::Data));
        let fetch = hir.push(HirNode::Call {
            receiver: Some(last),
            name: "[]".to_string(),
            args: vec![ArrayElem::Single(group)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            // `&.` -- nil when `$~` is nil, i.e. when the match failed.
            safe: true,
        });
        body.push(hir.push(HirNode::LocalWrite(name, fetch)));
    }
    body.push(hir.push(HirNode::LocalRead(m_tmp)));
    Ok(hir.push(HirNode::Seq(body)))
}

/// `__FILE__`'s answer: the path AS GIVEN on the command line, NOT an
/// absolute one -- oracle-verified (`ruby o_leaves.rb` prints
/// `"o_leaves.rb"`). `"-e"` when there is no file at all, which is real
/// Ruby's own answer for `ruby -e`, and is what a bare
/// `compile_to_rust(source)` gets.
fn current_file_str() -> PResult<String> {
    Ok(match context::current_source_file() {
        Some(p) => p.to_string_lossy().into_owned(),
        None => "-e".to_string(),
    })
}

/// The require-style feature an `autoload(:Const, <path>)` names, resolved at
/// compile time for the loader's eager-splice model (see
/// `Loader::lower_file_statements`). Two path forms are supported: a plain
/// string literal, and `File.expand_path("<literal>", __dir__)` (computed from
/// the current source file's directory -- the idiom stdlib/bundler use for a
/// sibling file). Any other path expression, or a non-two-arg call, is a clean
/// error: the splice target must be known at compile time (like a
/// non-top-level `require`). The caller has already confirmed `call` is a
/// receiver-less `autoload`. `pub(super)` for the loader's pre-pass.
pub fn autoload_feature(call: &CallNode<'_>) -> PResult<String> {
    let args: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    if args.len() != 2 {
        return Err(
            "`autoload` takes exactly two arguments (`autoload :Const, \"feature\"`)"
                .to_string()
                .into(),
        );
    }
    if let Some(feature) = compile_time_feature(&args[1])? {
        return Ok(feature);
    }
    Err(
        "`autoload` with a non-literal feature isn't supported (zeo limitation) -- the target must resolve at compile time: a string literal, `\"#{__dir__}/...\"`, or `File.expand_path(\"...\", __dir__)`".to_string().into(),
    )
}

/// An autoload target's feature text, when every piece is known at compile
/// time. Three forms, in the order gems use them: a plain string literal, an
/// interpolation whose only computed part is `__dir__`, and
/// `File.expand_path("<literal>", __dir__)`. `None` means the target is
/// genuinely dynamic.
fn compile_time_feature(node: &Node<'_>) -> PResult<Option<String>> {
    if let Some(lit) = node.as_string_node() {
        return Ok(Some(String::from_utf8_lossy(lit.unescaped()).into_owned()));
    }
    if let Some(feature) = interpolated_dir_feature(node)? {
        return Ok(Some(feature));
    }
    expand_path_dir_feature(node)
}

/// `"#{__dir__}/puma/const"` -- how puma, rack and sidekiq name a sibling
/// file. Every part must be a literal or `__dir__`; anything else makes the
/// whole target dynamic.
fn interpolated_dir_feature(node: &Node<'_>) -> PResult<Option<String>> {
    let Some(interp) = node.as_interpolated_string_node() else {
        return Ok(None);
    };
    let mut out = String::new();
    for part in interp.parts().iter() {
        if let Some(s) = part.as_string_node() {
            out.push_str(&String::from_utf8_lossy(s.unescaped()));
            continue;
        }
        let is_dir = part
            .as_embedded_statements_node()
            .and_then(|e| e.statements())
            .map(|s| s.body().iter().collect::<Vec<_>>())
            .and_then(|stmts| match stmts.as_slice() {
                [only] => only.as_call_node(),
                _ => None,
            })
            .is_some_and(|c| c.receiver().is_none() && c.name().as_slice() == b"__dir__");
        if !is_dir {
            return Ok(None);
        }
        out.push_str(&current_dir_str()?);
    }
    Ok(Some(out))
}

/// Recognizes `File.expand_path("<literal>", __dir__)` and computes the
/// absolute feature path from the current file's directory; `None` for any
/// other expression (so `autoload_feature` can fall through to its error).
fn expand_path_dir_feature(node: &Node<'_>) -> PResult<Option<String>> {
    let Some(call) = node.as_call_node() else {
        return Ok(None);
    };
    if call.name().as_slice() != b"expand_path" {
        return Ok(None);
    }
    let on_file = call
        .receiver()
        .and_then(|r| r.as_constant_read_node())
        .is_some_and(|c| c.name().as_slice() == b"File");
    if !on_file {
        return Ok(None);
    }
    let args: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    if args.len() != 2 {
        return Ok(None);
    }
    let Some(rel) = args[0].as_string_node() else {
        return Ok(None);
    };
    // The base must be `__dir__` (a receiver-less call), the only base whose
    // value is compile-time-known to be this file's directory.
    let base_is_dir = args[1]
        .as_call_node()
        .is_some_and(|c| c.receiver().is_none() && c.name().as_slice() == b"__dir__");
    if !base_is_dir {
        return Ok(None);
    }
    let rel = String::from_utf8_lossy(rel.unescaped()).into_owned();
    // An absolute `<dir>/<rel>`; `resolve_require` appends `.rb` and the OS
    // resolves any embedded `..`. `File.expand_path` would normalize `..`
    // lexically, but a filesystem check is equivalent for a real file.
    Ok(Some(format!("{}/{rel}", current_dir_str()?)))
}

/// `__dir__`'s answer: the ABSOLUTE directory holding the current file --
/// unlike `__FILE__`, which stays as-written. Real Ruby defines it as
/// `File.dirname(File.realpath(__FILE__))`, so it resolves symlinks too;
/// `canonicalize` is that, and it falls back to a plain absolute path when
/// the file can't be resolved (a source string with no file on disk).
fn current_dir_str() -> PResult<String> {
    let path = context::current_source_file()
        .ok_or("`__dir__` needs a real source file (there is none when compiling a bare string)")?;
    let resolved = path.canonicalize().unwrap_or(path);
    let dir = resolved
        .parent()
        .ok_or("the source file has no parent directory")?;
    Ok(dir.to_string_lossy().into_owned())
}

#[cfg(test)]
mod regexp_encoding_tests {
    use crate::lower_error::LowerErrorKind;

    fn lower(src: &str) -> Result<(), crate::lower_error::LowerError> {
        let mut hir = crate::hir::Hir::default();
        crate::lower::parse_and_lower_into(&mut hir, src).map(|_| ())
    }

    /// An ASCII pattern reads the same in every one of these encodings, so the
    /// flag is free -- it changes only what the regexp reports about itself.
    /// ruby2ruby opens with `ENC_EUC = /x/e.options` for exactly that.
    #[test]
    fn an_ascii_pattern_takes_any_encoding_flag() {
        for src in ["p(/x/n)", "p(/x/e)", "p(/x/s)", "p(/x/u)", "p(/x/)"] {
            lower(src).unwrap_or_else(|e| panic!("{src} lowers: {e}"));
        }
    }

    /// One non-ASCII byte and the readings genuinely differ, which ruby refuses
    /// to guess at. It settles that in the PARSER, so the rejection is prism's
    /// and the wording is ruby's -- this pins that zeo passes it through as the
    /// user's `SyntaxError` rather than dressing it up as a zeo gap.
    #[test]
    fn a_non_ascii_pattern_rejects_a_foreign_encoding_flag() {
        for (src, flag) in [("p(/fôo/n)", 'n'), ("p(/fôo/e)", 'e'), ("p(/fôo/s)", 's')] {
            let err = lower(src).expect_err("ruby refuses to guess which reading was meant");
            assert_eq!(err.kind, LowerErrorKind::Syntax, "{src}");
            assert!(
                err.message().ends_with(&format!(
                    "regexp encoding option '{flag}' differs from source encoding 'UTF-8'"
                )),
                "{src}: {}",
                err.message()
            );
        }
        // `/u` names the encoding the source already has, so it imposes nothing.
        lower("p(/fôo/u)").expect("the source is already UTF-8");
        lower("p(/fôo/)").expect("no flag, no question");
    }
}
