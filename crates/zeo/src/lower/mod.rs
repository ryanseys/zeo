//! Lowers a `ruby_prism::Node` tree straight into `Hir` -- the direct analog
//! of `zeo_parse.c`'s `flatten()` + `node_table.c`'s `nt_load_text()`
//! combined into one in-process step (no text-serialization round-trip; see
//! `hir.rs`'s module docs for why zeo needed that step and we don't).
//!
//! Covers the node kinds the corpus exercises; anything
//! else is a clean `Err` (mirroring zeo's `unsupported(c, id, "...")`
//! convention), not a panic.

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

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

use crate::hir::{
    ArrayElem, Hir, HirNode, KwArg, LastMatch, NodeId, Params, RaiseCause, Span, StrPart,
    Visibility,
};
use crate::lower_error::LowerError;
use ruby_prism::{CallNode, Node, ParseResult};

use calls::{lower_block, lower_block_like_params, lower_call_args};
use consts::{box_rooted_path, constant_path_name};
use defs::{const_is_assigned, lower_params};
use eval_splice::{lower_box_eval, reject_top_level_defs, single_literal_string_arg};
pub use literals::encoding_const_name;
use literals::line_of;

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
/// Whether a `define_method`/`define_singleton_method` block reads or writes a
/// local belonging to a scope OUTSIDE itself.
///
/// The desugar to a plain `DefMethod` turns the block into a method the class
/// owns, and a method is a Rust function of its own -- it cannot reach a local
/// living on the enclosing class-body (or top-level) frame. rubygems writes
/// exactly that shape:
///
/// ```ruby
/// module Kernel
///   original_warn = instance_method(:warn)
///   module_function define_method(:warn) { |*m, **kw| original_warn.bind_call(self, *m, **kw) }
/// end
/// ```
///
/// which emitted a method body naming `original_warn` and stopped bundler at
/// rustc with E0425. A capturing block falls through to the generic call
/// instead, where `Module#define_method` installs a real closure -- the same
/// path a `define_method` inside a method body already took, and the reason
/// that one always worked.
///
/// Prism answers this directly: a local-variable node carries the number of
/// scopes it reaches UP, and only `Block`/`Lambda` share the chain -- a
/// `def`/`class`/`module` starts a fresh one, so nothing inside it can be
/// reaching a local of ours and the walk stops there. Over-reporting is the
/// safe direction (the runtime path is correct, just less direct), so any
/// scope-maker this misses costs optimization rather than correctness.
fn closes_over_an_enclosing_local(block: &Node<'_>) -> bool {
    struct Free {
        nesting: u32,
        found: bool,
    }
    impl<'pr> ruby_prism::Visit<'pr> for Free {
        fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
            self.nesting += 1;
            ruby_prism::visit_block_node(self, node);
            self.nesting -= 1;
        }
        fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
            self.nesting += 1;
            ruby_prism::visit_lambda_node(self, node);
            self.nesting -= 1;
        }
        // A fresh scope chain: its locals are its own, and prism's depths
        // inside it are counted from there.
        fn visit_def_node(&mut self, _: &ruby_prism::DefNode<'pr>) {}
        fn visit_class_node(&mut self, _: &ruby_prism::ClassNode<'pr>) {}
        fn visit_module_node(&mut self, _: &ruby_prism::ModuleNode<'pr>) {}
        fn visit_singleton_class_node(&mut self, _: &ruby_prism::SingletonClassNode<'pr>) {}

        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
            self.found |= n.depth() >= self.nesting;
        }
        fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'pr>) {
            self.found |= n.depth() >= self.nesting;
            ruby_prism::visit_local_variable_write_node(self, n);
        }
        fn visit_local_variable_target_node(
            &mut self,
            n: &ruby_prism::LocalVariableTargetNode<'pr>,
        ) {
            self.found |= n.depth() >= self.nesting;
        }
        fn visit_local_variable_and_write_node(
            &mut self,
            n: &ruby_prism::LocalVariableAndWriteNode<'pr>,
        ) {
            self.found |= n.depth() >= self.nesting;
            ruby_prism::visit_local_variable_and_write_node(self, n);
        }
        fn visit_local_variable_or_write_node(
            &mut self,
            n: &ruby_prism::LocalVariableOrWriteNode<'pr>,
        ) {
            self.found |= n.depth() >= self.nesting;
            ruby_prism::visit_local_variable_or_write_node(self, n);
        }
        fn visit_local_variable_operator_write_node(
            &mut self,
            n: &ruby_prism::LocalVariableOperatorWriteNode<'pr>,
        ) {
            self.found |= n.depth() >= self.nesting;
            ruby_prism::visit_local_variable_operator_write_node(self, n);
        }
    }
    // Starts at 0 because the walk enters through the block ITSELF, whose
    // `visit_block_node` takes it to 1 -- so a read of the block's own local
    // (depth 0) is under the bar and a read one scope out (depth 1) is at it.
    let mut free = Free {
        nesting: 0,
        found: false,
    };
    ruby_prism::Visit::visit(&mut free, block);
    free.found
}

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
    // `calls.rs` -- see its `try_lower`. The ordinary CALL arm stays below
    // (`lower_call_node`): its recognizers are interwoven with the
    // loader/box machinery that lives in this module.
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
        return lower_call_node(result, hir, node, call);
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
    // that `codegen::call` emits as `zeo_rt::alias_in_default_definee`.
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

/// The ordinary `CallNode` arm of [`lower_node_inner`], extracted whole: the
/// require/eval/box recognizers, the operator/`lambda { }`/`block_given?`
/// specials, and the general call lowering. It stays in this module because
/// its recognizers are interwoven with the loader/box machinery here; every
/// path returns.
fn lower_call_node(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    call: ruby_prism::CallNode<'_>,
) -> PResult<NodeId> {
    let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();

    // `using M` -- the top-level spelling, where it activates for the
    // rest of the file. The class-body spelling is recognized by
    // `defs::lower_class_body_statement`, which reaches the same helper.
    // `using Module.new { refine C do ... end }` answers with a PAIR (the
    // anonymous holder module, then the activation), and a statement is one
    // node -- so the pair rides in a `Seq`, which the analyze walk descends
    // to register the holder exactly as it does a `ClassDef` written in any
    // other value position.
    if let Some(nodes) = defs::lower_using(result, hir, node, &name, &call)? {
        return Ok(match nodes[..] {
            [id] => id,
            _ => hir.push(HirNode::Seq(nodes)),
        });
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
    // `walk_scope`. Every other shape -- a computed name, a body passed
    // as a value rather than written as a block, or a block that CLOSES OVER
    // an enclosing local -- falls through to the generic `Call` below and is
    // served at run time by `Module#define_method`.
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
                    params: Box::new(Params {
                        required: vec!["__sp_recv".to_string()],
                        rest: Some(Some("__sp_args".to_string())),
                        ..Params::default()
                    }),
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
            // Declined only inside a `class`/`module` body. A method body
            // never needed it (the def is already emitted as a closure there),
            // and at the TOP LEVEL the generic call would dispatch
            // `define_method` on `main`, which zeo's runtime has no row for --
            // trading a compile error for a NoMethodError, which is the wrong
            // direction. That one stays a gap.
            if let Some(block) = block_node.as_block_node()
                && !(hir.enclosing_class().is_some()
                    && !hir.is_in_def_body()
                    && closes_over_an_enclosing_local(&block_node))
            {
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
                let id = hir.push(HirNode::DefMethod {
                    name: method_name,
                    params: Box::new(params),
                    body,
                    is_class_method: false,
                    visibility: Visibility::Public,
                    // An explicit `define_method` call, not a `def`.
                    is_def: false,
                });
                hir.set_flag(id, crate::hir::NodeFlag::BLOCK_BODIED_DEF);
                return Ok(id);
            }
        }
    }

    // `ruby2_keywords def fwd(*a)` written at the TOP LEVEL, where the
    // directive is a PRIVATE SINGLETON method of `main` -- an object zeo
    // dispatches by name, with no such row. The def is answered in the call's
    // place, carrying the mark `analyze::mark_ruby2_keywords_defs` sets for
    // every other position (where `Module#ruby2_keywords` is a real row and
    // the statement must stay put -- consuming it there changed how the
    // class-body walk saw the body, and delegate.rb's `method_missing` stopped
    // reaching WeakRef's instances).
    if name == "ruby2_keywords"
        && receiver.is_none()
        && hir.enclosing_class().is_none()
        && let Some(args) = call.arguments()
    {
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if let [only] = arg_list.as_slice()
            && only.as_def_node().is_some()
        {
            let id = lower_node(result, hir, only)?;
            hir.set_flag(id, crate::hir::NodeFlag::RUBY2_KEYWORDS);
            return Ok(id);
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
    // ...and never INSIDE a `def`'s body: the constant-receiver desugar
    // below mints a `ClassDef` marker, and the analyze walk registers no
    // class-body site in a method body (ruby itself rejects a `class`
    // keyword there), so the marker died in codegen as "a position the
    // analyze walk doesn't register". The generic runtime call is the
    // honest form -- `Object.define_singleton_method(:const_missing) { }`
    // inside rails_admin's suppressor method installs through the overlay,
    // and `collect_patch_call` already de-optimizes the name's call sites.
    if name == "define_singleton_method"
        && !hir.is_in_def_body()
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
                //
                // ...and only when the call sits in a CLASS BODY. An explicit
                // constant receiver at STATEMENT position
                // (`Later.define_singleton_method(:mode) { }` after a call to
                // `Later.mode`) is a runtime event with a document position:
                // desugaring it to a reopen made last-`def`-wins answer the
                // new body at BOTH sites, so the method reached back in time.
                // The runtime row installs into the overlay at the right
                // moment, and `collect_patch_call` has already de-optimized
                // the name's call sites.
                Some(_) if hir.enclosing_class().is_none() => None,
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
                    params: Box::new(params),
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
                hir.set_flag(def, crate::hir::NodeFlag::BLOCK_BODIED_DEF);
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
                // A FRESH binding per `loop`: a fixed name aliased every
                // `loop` in the program to one local, so a scope's own
                // desugar looked like a capture of the enclosing scope's
                // -- `Ractor.new { loop { ... } }` inside a method that
                // also used `loop` refused isolation over it.
                let stop_name = hir.gensym("__loop");
                let exc_read = hir.push(HirNode::LocalRead(stop_name.clone()));
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
                        binding: Some(stop_name),
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
                params: Box::new(params),
                body,
                method_body: false,
            }));
        }
    }

    // `raise` -- a zero/one/two positional-arg call-shape desugar, same
    // posture as `block_given?` above. The `cause:` keyword form isn't
    // lowered yet (see `HirNode::Raise`'s docs) -- rejected here rather
    // than silently dropped, matching this project's "clean rejection
    // over silent wrongness" rule.
    // `fail` is NOT desugared here even though it is `raise`'s exact
    // synonym: it is also a popular USER method name (riot's reporter
    // takes four arguments), and a parse-time desugar binds the Kernel
    // meaning before method resolution can see the user's `def fail`.
    // It lowers as an ordinary call and gains its raise meaning in
    // codegen's universal implicit forms, after sibling resolution.
    // `raise(*exc)` -- a splat arg has no static positional shape (its count
    // is a runtime value), so the special static-form lowering can't build
    // `HirNode::Raise`'s fixed 0..3 args. Skip it here; the general call
    // lowering handles it via `emit_splat_call` over the runtime
    // `Kernel#raise` builtin (`optparse.rb`'s `{|*exc| raise(*exc)}`).
    // `raise(...)` -- argument forwarding is the same no-static-shape
    // case as the splat (sus forwards a matcher's whole failure into
    // `raise`); the general call lowering expands `...` into
    // `*rest, **kw, &blk` and reaches the runtime row.
    let raise_has_splat = || {
        call.arguments().is_some_and(|a| {
            a.arguments()
                .iter()
                .any(|n| n.as_splat_node().is_some() || n.as_forwarding_arguments_node().is_some())
        })
    };
    if name == "raise" && receiver.is_none() && !raise_has_splat() {
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
        // CRuby's `Kernel#raise` rejects a bad shape at RUNTIME: the call
        // evaluates its arguments, then raises `ArgumentError`, and only
        // when execution is reached (appnexusapi passes two messages inside
        // a `rescue` arm that may never fire). A compile error here would
        // reject a program CRuby loads fine.
        let runtime_argument_error =
            |result: &ParseResult, hir: &mut Hir, msg: String| -> PResult<NodeId> {
                let mut stmts = positional
                    .iter()
                    .map(|n| lower_node(result, hir, n))
                    .collect::<PResult<Vec<_>>>()?;
                if !rest.is_empty() {
                    stmts.push(hir.push(HirNode::HashLit(rest.clone())));
                }
                if let RaiseCause::Explicit(v) = &cause {
                    stmts.push(*v);
                }
                let class_ref = hir.push(HirNode::ClassRef("ArgumentError".to_string()));
                let message = hir.push(HirNode::StringLit(vec![StrPart::Lit(msg)]));
                stmts.push(hir.push(HirNode::Raise(vec![class_ref, message], RaiseCause::Absent)));
                Ok(hir.push(HirNode::Seq(stmts)))
            };
        if positional.len() + collapsed > 3 {
            let msg = format!(
                "wrong number of arguments (given {}, expected 0..3)",
                positional.len() + collapsed
            );
            return runtime_argument_error(result, hir, msg);
        }
        if positional.is_empty() && collapsed == 0 && matches!(cause, RaiseCause::Explicit(_)) {
            return runtime_argument_error(
                result,
                hir,
                "only cause is given with no arguments".to_string(),
            );
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
        if matches!(name.as_str(), "require" | "require_relative") {
            if let Some(feature) = single_literal_string_arg(result, hir, &call)? {
                if name == "require" && features::is_builtin_feature(&feature) {
                    let newly_loaded = hir
                        .activated_features
                        .insert(features::canonical_ext_feature(&feature).to_string());
                    let first = newly_loaded && !features::is_preloaded_at_boot(&feature);
                    return Ok(hir.push(HirNode::BoolLit(first)));
                }
                let unresolvable =
                    name == "require" && hir.unresolvable_requires.contains(&feature);
                // A rescued-and-missing `require_relative` keeps its call
                // to raise the LoadError its rescue catches; a
                // guard-gated site keeps its call to load its unit only
                // when the guard passes.
                let site_kept = hir.lowering_file.is_some_and(|file| {
                    let key = (file, call.location().start_offset() as u32);
                    (name == "require_relative" && hir.optional_require_sites.contains(&key))
                        || hir.conditional_require_sites.contains(&key)
                });
                if !unresolvable && !site_kept && !hir.deferred_requires.contains(&feature) {
                    return Ok(hir.push(HirNode::BoolLit(true)));
                }
            } else if name == "require_relative"
                && let Some(dir) = computed_relative_demand_dir(&call)
            {
                // A computed `require_relative` gets a BOUNDED demand, not
                // the package-wide one: either the literal directory prefix
                // its interpolation starts with, or the requiring file's
                // SIDECAR directory (`foo.rb` alongside `foo/`, ruby's
                // conventional split). This is what keeps a MAIN-file
                // loader working: the main file deliberately demands no
                // whole directory (a demander at the tree root would sweep
                // everything -- the webrick_not_bundled hazard), but a
                // bounded subtree is its own tree.
                hir.unit_demand.insert((hir.lowering_package.clone(), dir));
            } else {
                // A COMPUTED target can name any file of the demanding
                // package -- `Dir[...].each { |t| require t }` is how a
                // test suite or a plugin registry loads itself. The
                // honest AOT answer is the one a dynamic `autoload`
                // already gets: compile the package's files in as
                // callable units, and let the runtime require resolve
                // the string the program actually builds.
                hir.demand_feature_units();
            }
        }
        // `load`, or the computed `require` above: whole-program AOT
        // can't splice a path it only learns at runtime. FALL THROUGH to
        // the ordinary implicit-self `Call` lowering below (the same
        // trick a non-literal `eval` uses), which dispatches to the
        // runtime `Kernel#{require,require_relative,load}` -- answering
        // from the compiled-in units, and raising CRuby's `LoadError`
        // otherwise. This lets a guarded dynamic load -- `load ENV["X"]
        // if ENV["X"]` -- and the `begin; require dyn; rescue LoadError`
        // idiom COMPILE, with the guard/rescue behaving at runtime.
    }
    // `autoload :Const, "feature"` -- the loader's pre-pass
    // (`Loader::lower_file_statements`) has already compiled the feature
    // file in as a LAZY unit; the runtime `autoload` row loads it when
    // the declaration executes, at its document position.
    if name == "autoload" && receiver.is_none() {
        // Always a real call. A computed target -- including the
        // one-argument form every `autoload` DSL defines over
        // `Module#autoload` -- computes its string at runtime, so its
        // lowering demands the whole load path as units instead.
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
            // doesn't parse, or defines something the inline path can't
            // express -- a top-level `def`, or a `class`/`module` written
            // inside a METHOD body, where the registration walk never
            // reaches the splice -- DON'T fail the compile: fall through to
            // the runtime eval VM so the program still builds and the
            // error/behaviour surfaces at runtime, catchably, exactly as
            // CRuby's `eval` does.
            if let Ok(body) = parse_and_lower_into(hir, &src)
                && reject_top_level_defs(hir, &body).is_ok()
                && !(hir.is_in_def_body() && eval_splice::defines_a_class(hir, &body))
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
        hir.set_flag(node, crate::hir::NodeFlag::VCALL);
    }
    // A literal block on a re-homing call runs under the RECEIVER's
    // `self` -- see `NodeFlag::REHOMED_BLOCK`.
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
        hir.set_flag(b, crate::hir::NodeFlag::REHOMED_BLOCK);
    }
    // A computed-name `define_method(name) { }` block IS a method body
    // at run time -- see `NodeFlag::DYNAMIC_DEFINE_METHOD_BLOCK`.
    if let HirNode::Call {
        name,
        block: Some(b),
        ..
    } = &hir[node]
        && matches!(name.as_str(), "define_method" | "define_singleton_method")
    {
        let b = *b;
        hir.set_flag(b, crate::hir::NodeFlag::DYNAMIC_DEFINE_METHOD_BLOCK);
    }
    Ok(node)
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
