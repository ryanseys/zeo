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
pub(crate) mod control;
pub mod defs;
pub mod eval_splice;
pub mod features;
pub(crate) mod ffi;
mod literals;
mod pattern;

use crate::diagnostics::lower::LowerError;
use crate::hir::{ArrayElem, Hir, HirNode, KwArg, LastMatch, NodeId, Span, StrPart};
use ruby_prism::{CallNode, Node, ParseResult};

pub use literals::encoding_const_name;
use literals::line_of;

pub type PResult<T> = Result<T, crate::diagnostics::lower::LowerError>;

/// Parses `source` as a standalone program and lowers it into `hir`, which
/// may already contain other nodes -- the primitive the compiler's top-level
/// drivers (`zeo::parse`), the loader's file splicing, and the runtime
/// `eval` compile (`zeo::eval`, via `clif::eval`) all share.
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

/// The span-stamping wrapper around the big lowering match: every prism node
/// entering lowering pushes its byte range (in the file currently being
/// lowered -- `Hir::lowering_file`) onto the arena's span stack, so each
/// `hir.push` during that node's lowering is stamped with ITS provenance,
/// and an error propagating out picks up the innermost frame's span
/// (`LowerError::with_span_if_missing`). `HirNode` itself carries no span
/// field -- the parallel `Hir::spans` table is the whole design.
pub fn lower_node(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<NodeId> {
    let span = span_of(hir, node);
    hir.push_span(span);
    let out = lower_node_inner(result, hir, node);
    hir.pop_span();
    if let Ok(id) = out {
        record_call_line(hir, id, node);
    }
    out.map_err(|e| e.with_span_if_missing(span))
}

/// Note a call whose method NAME is on a later line than the expression it
/// belongs to -- `recv\n  .m`, or rack's `lambda do .. end.must_raise(..)`.
///
/// Ruby reports such a call at the name's line, which is not where the
/// expression starts and not what coverage counts: for
/// `[1].map do |i|\n i\n end.boom`, the backtrace says the `end.boom`
/// line and `Coverage` counts the `[1].map` one. Both oracle-verified, so
/// the two answers are kept apart -- the span stays the whole expression's
/// and this records the other.
fn record_call_line(hir: &mut Hir, id: NodeId, node: &Node<'_>) {
    let Some(call) = node.as_call_node() else {
        return;
    };
    let Some(msg) = call.message_loc() else {
        return;
    };
    let (Some(file), Some(span)) = (hir.lowering_file, hir.span(id)) else {
        return;
    };
    let Some(src) = hir.files.get(file.0 as usize) else {
        return;
    };
    let at = msg.start_offset() as u32;
    if src.line_at(at) > src.line_at(span.start) {
        hir.set_call_message(id, at);
    }
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

/// The class a `class Sub < ... end` header names as its superclass.
///
/// `< self` inside a class body is the ENCLOSING class -- a compile-time fact,
/// and the shape optparse gives every argument style (`class NoArgument <
/// self` inside `class Switch`). Read as a dynamic superclass instead, the
/// subclass is minted at runtime over a compiled parent, and its instances are
/// name-keyed `DynObject`s the parent's own methods cannot run against.
pub(crate) fn superclass_name(hir: &Hir, sc: &Node<'_>) -> PResult<String> {
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
    // The literal/collection family lives in `literals.rs` -- see its
    // `try_lower`. First in the chain because literals dominate any real
    // program's node count.
    if let Some(id) = literals::try_lower(result, hir, node)? {
        return Ok(id);
    }

    // Variable reads/writes, compound assignment, and multi-assignment live
    // in `assign.rs` -- see its `try_lower`.
    if let Some(id) = assign::try_lower(result, hir, node)? {
        return Ok(id);
    }

    // Control flow (branches, loops, begin/rescue, break/next/return,
    // defined?, pattern-match one-liners) lives in `control.rs` -- see its
    // `try_lower`.
    if let Some(id) = control::try_lower(result, hir, node)? {
        return Ok(id);
    }

    // Constant reads/writes (bare, qualified paths, compound forms) live in
    // `consts.rs` -- see its `try_lower`.
    if let Some(id) = consts::try_lower(result, hir, node)? {
        return Ok(id);
    }

    // `class`/`module`/`def`/`undef`/`class << obj` definitions live in
    // `defs.rs` -- see its `try_lower_definition`.
    if let Some(id) = defs::try_lower_definition(result, hir, node)? {
        return Ok(id);
    }

    // Lambda literals, `yield`, and both `super` spellings live in
    // `calls.rs` -- see its `try_lower`. The ordinary CALL arm lives there
    // too (`calls::lower_call_node`, dispatched below).
    if let Some(id) = calls::try_lower(result, hir, node)? {
        return Ok(id);
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
            params: Box::default(),
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

    if let Some(call) = node.as_call_node() {
        return calls::lower_call_node(result, hir, node, call);
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

    // `alias new old` reached in a GENERAL context -- inside a `class_eval`/
    // `module_eval`/`instance_exec` block, or a method body. Class-body and
    // top-level `alias` are intercepted earlier (`defs::lower_class_body`,
    // `loader`) and never arrive here. The KEYWORD aliases on the frame's
    // DEFAULT DEFINEE -- the module under `class_eval`, the receiver's
    // SINGLETON under `instance_eval`/`instance_exec` (rspec's top-level DSL
    // installs `RSpec.shared_examples` then `alias shared_context
    // shared_examples` that way), the cref's class in a method body -- which
    // is `define_in_default_definee`'s rule exactly, and NOT `self` (the
    // CALL form `alias_method(:new, :old)` is the one that dispatches to its
    // receiver). Desugared to the internal `__zeo_alias_keyword` marker call
    // that `clif::expr` emits as `zeo_rt::alias_in_default_definee`.
    if let Some(alias) = node.as_alias_method_node() {
        // An interpolated name lowers as the runtime EXPRESSION it is --
        // the marker call reads both operands at runtime either way.
        let operand = |hir: &mut Hir, n: &Node<'_>| -> PResult<NodeId> {
            match n.as_interpolated_symbol_node() {
                Some(_) => lower_node(result, hir, n),
                None => {
                    let name = defs::alias_target_name(n)?;
                    Ok(hir.push(HirNode::SymbolLit(name)))
                }
            }
        };
        let new_sym = operand(hir, &alias.new_name())?;
        let old_sym = operand(hir, &alias.old_name())?;
        let send = hir.push(HirNode::Call {
            receiver: None,
            name: "__zeo_alias_keyword".to_string(),
            args: vec![ArrayElem::Single(new_sym), ArrayElem::Single(old_sym)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        });
        // `Module#alias_method` answers the new name; the `alias` KEYWORD
        // answers nil -- see the `undef` arm above, same split.
        let nil = hir.push(HirNode::NilLit);
        return Ok(hir.push(HirNode::Seq(vec![send, nil])));
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

/// The BOUNDED unit-demand directory for a COMPUTED `require_relative`:
/// the literal directory prefix its interpolation starts with
/// (`"unit_tree/#{lib}/#{f}"` names only files under `unit_tree/`), or --
/// for a wholly-dynamic argument -- the requiring file's SIDECAR directory
/// (`foo.rb` alongside `foo/`, ruby's conventional file-plus-tree split).
/// `None` when neither bound exists; the caller falls back to the
/// package-wide demand.
fn computed_relative_demand_dir(call: &CallNode<'_>) -> Option<std::path::PathBuf> {
    let file = context::current_source_file()?;
    // A bare filename's parent is the empty path; canonicalize needs `.`.
    let parent = match file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => std::path::Path::new("."),
    };
    let dir = parent.canonicalize().ok()?;
    let literal_prefix = || {
        let args = call.arguments()?;
        let arg = args.arguments().iter().next()?;
        let istr = arg.as_interpolated_string_node()?;
        let first = istr.parts().iter().next()?;
        let s = first.as_string_node()?;
        let text = String::from_utf8_lossy(s.unescaped()).into_owned();
        let (prefix, _) = text.rsplit_once('/')?;
        if prefix.is_empty() || prefix.split('/').any(|c| c == "..") {
            return None;
        }
        Some(dir.join(prefix))
    };
    let sidecar = || {
        let stem = file.file_stem()?;
        let side = dir.join(stem);
        side.is_dir().then_some(side)
    };
    literal_prefix().or_else(sidecar)
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
/// The constant an `autoload :Const, "feature"` names, when its first
/// argument is a literal symbol. `None` for a computed one, which the
/// compiler cannot gate and the runtime row loads eagerly instead.
pub fn autoload_const_name(call: &CallNode<'_>) -> Option<String> {
    let args: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let sym = args.first()?.as_symbol_node()?;
    Some(String::from_utf8_lossy(sym.unescaped()).into_owned())
}

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
        let Some(stmts) = part
            .as_embedded_statements_node()
            .and_then(|e| e.statements())
            .map(|s| s.body().iter().collect::<Vec<_>>())
        else {
            return Ok(None);
        };
        let [only] = stmts.as_slice() else {
            return Ok(None);
        };
        let is_dir = only
            .as_call_node()
            .is_some_and(|c| c.receiver().is_none() && c.name().as_slice() == b"__dir__");
        if is_dir {
            out.push_str(&current_dir_str()?);
            continue;
        }
        // The two forms COMPOSE: rubygems writes
        // `"#{File.expand_path("lib", __dir__)}/x"`, which is neither a bare
        // `__dir__` interpolation nor a whole `File.expand_path` call.
        match compile_time_feature(only)? {
            Some(text) => out.push_str(&text),
            None => return Ok(None),
        }
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
    use crate::diagnostics::lower::LowerErrorKind;

    fn lower(src: &str) -> Result<(), crate::diagnostics::lower::LowerError> {
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
