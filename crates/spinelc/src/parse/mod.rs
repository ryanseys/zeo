//! Lowers a `ruby_prism::Node` tree straight into `Hir` -- the direct analog
//! of `spinel_parse.c`'s `flatten()` + `node_table.c`'s `nt_load_text()`
//! combined into one in-process step (no text-serialization round-trip; see
//! `hir.rs`'s module docs for why spinel needed that step and we don't).
//!
//! Covers exactly the node kinds the spike's 7 examples exercise; anything
//! else is a clean `Err` (mirroring spinel's `unsupported(c, id, "...")`
//! convention), not a panic.

mod gem_store;
mod gemspec;
mod loader;
mod lockfile;
mod rename;

/// How one lockfile gem fares against spinel, for `cargo xtask gem-compat`.
#[derive(Debug, Clone, PartialEq)]
pub struct GemCompatEntry {
    pub name: String,
    pub version: String,
    pub outcome: GemCompatOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GemCompatOutcome {
    /// Pure Ruby -- spinel resolved a require-path root and would compile it.
    Compiled,
    /// A name spinel provides via a built-in; the store copy is ignored.
    /// `diverges` when spinel's implementation is not the upstream gem.
    Builtin { diverges: bool, note: Option<String> },
    /// A native gem spinel can't provide -- the detected layout and why.
    NativeUnsupported { kind: String, reason: String },
    /// A GIT/PATH-source lockfile gem, not drawn from the RubyGems store.
    ExternalSource,
    /// Resolvable in principle but contributed no root (e.g. a default gem's
    /// empty placeholder dir) -- spinel provides it as stdlib, not from here.
    Skipped { reason: String },
}

/// Classify every gem in `lockfile` against an installed `store` (`gem env
/// gemdir`), reusing the Phase-3 provider. The out-of-the-box resolvability
/// matrix behind `cargo xtask gem-compat`.
pub fn gem_compat(
    store: &std::path::Path,
    lockfile: &std::path::Path,
) -> Result<Vec<GemCompatEntry>, String> {
    let parsed = lockfile::parse_file(lockfile)?;
    classify(store, &parsed)
}

/// Like [`gem_compat`], but over EVERY gem installed in the store rather than a
/// lockfile's subset -- the broad out-of-the-box sample `cargo xtask
/// gem-compat` runs when given no lockfile. Builds a synthetic gem set from the
/// store's own `specifications/`.
pub fn gem_compat_installed(
    store: &std::path::Path,
) -> Result<Vec<GemCompatEntry>, String> {
    let parsed = gem_store::installed_as_lockfile(store)?;
    classify(store, &parsed)
}

fn classify(
    store: &std::path::Path,
    parsed: &lockfile::Lockfile,
) -> Result<Vec<GemCompatEntry>, String> {
    use lockfile::GemSource;
    let resolution = gem_store::resolve(store, parsed)?;

    let compiled: std::collections::HashSet<&str> =
        resolution.roots.iter().map(|(n, _)| n.as_str()).collect();
    let disclosed: std::collections::HashMap<&str, &crate::gem_report::SatisfiedBy> = resolution
        .disclosures
        .iter()
        .map(|r| (r.name.as_str(), &r.by))
        .collect();

    let mut out = Vec::with_capacity(parsed.gems.len());
    for gem in &parsed.gems {
        let outcome = if gem.source != GemSource::Rubygems {
            GemCompatOutcome::ExternalSource
        } else if compiled.contains(gem.name.as_str()) {
            GemCompatOutcome::Compiled
        } else {
            match disclosed.get(gem.name.as_str()) {
                Some(crate::gem_report::SatisfiedBy::Excluded { kind, reason }) => {
                    GemCompatOutcome::NativeUnsupported {
                        kind: kind.clone(),
                        reason: reason.clone(),
                    }
                }
                Some(_) => GemCompatOutcome::Builtin {
                    diverges: crate::gem_report::substitution_note(&gem.name).is_some(),
                    note: crate::gem_report::substitution_note(&gem.name).map(str::to_string),
                },
                None => GemCompatOutcome::Skipped {
                    reason: "no require-path root (default-gem placeholder or empty)".to_string(),
                },
            }
        };
        out.push(GemCompatEntry {
            name: gem.name.clone(),
            version: gem.version.clone(),
            outcome,
        });
    }
    Ok(out)
}

use crate::hir::{
    ArrayElem, HashPatternRest, Hir, HirNode, KeywordParam, KwArg, LastMatch, NodeId, Params,
    Pattern, PatternArm, RaiseCause, RegexpFlags, RescueClause, StrPart, Visibility,
};
use ruby_prism::{CallNode, Node, ParseResult};

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
const BUILTIN_EXCEPTIONS_RB: &str = r##"
class Exception
  def initialize(msg = nil)
    @message = msg
  end
  def message
    to_s
  end
  def to_s
    @message || self.class.name
  end
  def backtrace
    []
  end
  def full_message
    self.class.name + ": " + message
  end
  def inspect
    s = message.to_s
    if s.empty?
      self.class.name
    elsif s.include?("\n")
      "#<" + self.class.name + ":" + s.inspect + ">"
    else
      "#<" + self.class.name + ": " + s + ">"
    end
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
class Encoding::UndefinedConversionError < EncodingError
end
class Encoding::InvalidByteSequenceError < EncodingError
end
class Encoding::CompatibilityError < EncodingError
end
class Encoding::ConverterNotFoundError < EncodingError
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
  def __set_result(v)
    @result = v
  end
  def result
    @result
  end
end
class NameError < StandardError
end
class NoMethodError < NameError
end
class RangeError < StandardError
end
class FloatDomainError < RangeError
end
class LocalJumpError < StandardError
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
class SystemCallError < StandardError
end
module Errno
  class ENOENT < SystemCallError
  end
  class EACCES < SystemCallError
  end
  class EEXIST < SystemCallError
  end
  class ENOTDIR < SystemCallError
  end
  class EISDIR < SystemCallError
  end
  class ENOTEMPTY < SystemCallError
  end
  class EPIPE < SystemCallError
  end
  class EINVAL < SystemCallError
  end
  class EAGAIN < SystemCallError
  end
  class EBADF < SystemCallError
  end
  class ESPIPE < SystemCallError
  end
  class EXDEV < SystemCallError
  end
end
# Object's default copy hook: a no-op -- the runtime clone/dup already
# performed the shallow ivar copy before this user-overridable hook runs. A
# top-level `def` lands in Object's own_methods, exactly where `super`'s
# compile-time ancestor walk resolves a user `initialize_copy`'s bare `super`.
def initialize_copy(orig)
  self
end
"##;

/// Returns the built `Hir` plus the id of its `Program` root -- `Hir` itself
/// doesn't track a root (it's just an arena), so lowering hands the root id
/// back explicitly rather than requiring callers to know it's always the
/// last-pushed node.
pub fn parse_and_lower(source: &str) -> PResult<(Hir, NodeId)> {
    parse_and_lower_with(source, None, &[], &[], None, None)
}

/// The `# encoding:`/`# coding:` magic comment's value, honored only on the
/// first line (or the second, after a `#!` shebang) exactly as CRuby does --
/// `coding\s*[:=]\s*NAME` inside that comment. `None` when absent.
fn magic_encoding_comment(source: &str) -> Option<String> {
    let mut lines = source.lines();
    let first = lines.next()?;
    let line = if first.starts_with("#!") { lines.next()? } else { first };
    let line = line.trim_start();
    if !line.starts_with('#') {
        return None;
    }
    let lower = line.to_ascii_lowercase();
    let start = lower.find("coding")? + "coding".len();
    let rest = line[start..].trim_start();
    let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix('='))?;
    let name: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// A `# frozen_string_literal: true` magic comment in the leading comment
/// block (after an optional shebang). CRuby only honors it there, before any
/// code; a blank or code line ends the region.
fn magic_frozen_string_literal(source: &str) -> bool {
    for (i, line) in source.lines().enumerate() {
        let line = line.trim_start();
        if i == 0 && line.starts_with("#!") {
            continue;
        }
        if !line.starts_with('#') {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(idx) = lower.find("frozen_string_literal") {
            let rest = line[idx + "frozen_string_literal".len()..].trim_start();
            if let Some(rest) = rest.strip_prefix(':') {
                let value: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                return value.eq_ignore_ascii_case("true");
            }
        }
    }
    false
}

/// Maps a magic-comment encoding name to its `Encoding::` constant spelling
/// (`None` = the UTF-8 default, needing no override), rejecting an
/// unsupported encoding with a clean compile error.
fn encoding_const_name(name: &str) -> PResult<Option<&'static str>> {
    let norm: String = name
        .chars()
        .filter(|c| *c != '-' && *c != '_' && *c != '.')
        .flat_map(char::to_lowercase)
        .collect();
    Ok(match norm.as_str() {
        "utf8" | "cp65001" => None,
        "usascii" | "ascii" | "ansix341968" | "646" => Some("US_ASCII"),
        "ascii8bit" | "binary" => Some("ASCII_8BIT"),
        "iso88591" | "latin1" => Some("ISO_8859_1"),
        _ => {
            return Err(format!(
                "unsupported source encoding in magic comment: '{name}' \
                 (supported: UTF-8, US-ASCII, ASCII-8BIT/BINARY, ISO-8859-1)"
            ))
        }
    })
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
#[allow(clippy::too_many_arguments)]
pub fn parse_and_lower_with(
    source: &str,
    input_path: Option<&std::path::Path>,
    load_roots: &[std::path::PathBuf],
    package_dirs: &[std::path::PathBuf],
    gem_path: Option<&std::path::Path>,
    lockfile: Option<&std::path::Path>,
) -> PResult<(Hir, NodeId)> {
    let mut hir = Hir::default();
    if let Some(name) = magic_encoding_comment(source) {
        hir.script_encoding = encoding_const_name(&name)?.map(str::to_string);
    }
    hir.frozen_string_literal = magic_frozen_string_literal(source);
    let mut statements = parse_and_lower_into(&mut hir, BUILTIN_EXCEPTIONS_RB)
        .map_err(|e| format!("internal error in spinelc's built-in exception classes (this is a spinelc bug): {e}"))?;
    hir.builtin_exceptions_len = statements.len();
    statements.extend(loader::lower_main_file(
        &mut hir,
        source,
        input_path,
        load_roots,
        package_dirs,
        gem_path,
        lockfile,
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

/// A statically-known boolean value for a class-body `if`/`unless` guard --
/// only the literal forms real programs use to compile-time-select a `def`/
/// `alias` (`... if true`, `... unless false`, `... if (true)`). `nil` counts
/// as false (Ruby's own truthiness). Anything else (a method call, a
/// constant, a comparison) is `None`, leaving the `if` to lower as an
/// ordinary runtime conditional.
fn static_bool(node: &Node<'_>) -> Option<bool> {
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

/// Lowers the branch a statically-folded class-body `if`/`unless` selected --
/// a `StatementsNode` (the `then`/`unless` body), an `ElseNode` (a final
/// `else`), a nested `IfNode` (an `elsif`, re-entering the fold), or `None`
/// (an omitted branch) -- routing each contained statement back through
/// `lower_class_body_statement` so an `alias`/`def`/visibility directive
/// inside the guard still registers.
fn lower_class_body_selected(
    result: &ParseResult,
    hir: &mut Hir,
    chosen: Option<Node<'_>>,
    visibility: &mut Visibility,
    module_function: &mut bool,
    out: &mut Vec<NodeId>,
) -> PResult<()> {
    let Some(node) = chosen else { return Ok(()) };
    if let Some(stmts) = node.as_statements_node() {
        for stmt in stmts.body().iter() {
            lower_class_body_statement(result, hir, &stmt, visibility, module_function, out)?;
        }
        return Ok(());
    }
    if let Some(else_node) = node.as_else_node() {
        if let Some(stmts) = else_node.statements() {
            for stmt in stmts.body().iter() {
                lower_class_body_statement(result, hir, &stmt, visibility, module_function, out)?;
            }
        }
        return Ok(());
    }
    // A nested `elsif` `IfNode`, or any single statement: re-enter the
    // class-body path (which folds the `elsif` in turn).
    lower_class_body_statement(result, hir, &node, visibility, module_function, out)
}

fn constant_name(node: &Node<'_>) -> PResult<String> {
    let cr = node
        .as_constant_read_node()
        .ok_or("expected a plain constant name (e.g. `Foo`, not `Foo::Bar`)")?;
    Ok(String::from_utf8_lossy(cr.name().as_slice()).into_owned())
}

/// A constant PATH wherever a class/module is being NAMED (Phase 15.3):
/// definitions (`class Store::Item`), superclasses, include/extend/prepend
/// targets, `.new` receivers, `rescue` lists, and pattern constants.
/// Produces the joined `"A::B::C"` form `Compiler::resolve_class` takes
/// apart again; a top-level-anchored `::Foo` keeps its leading `::` (the
/// anchor skips the lexical chain at resolution time). A dynamic parent
/// (`something::Foo` where `something` isn't itself a constant) stays a
/// clean rejection.
pub(super) fn constant_path_name(node: &Node<'_>) -> PResult<String> {
    if node.as_constant_read_node().is_some() {
        return constant_name(node);
    }
    let cp = node
        .as_constant_path_node()
        .ok_or("expected a constant name or path (e.g. `Foo` or `Foo::Bar`)")?;
    let name = cp
        .name()
        .ok_or("a `::` constant path with a dynamic/computed name isn't supported (spike scope)")?;
    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
    Ok(match cp.parent() {
        None => format!("::{name}"),
        Some(p) => format!("{}::{}", constant_path_name(&p)?, name),
    })
}

/// Whether `node` is exactly `Ruby::Box.new` (no args, no block) -- the
/// only allocation shape Phase 18 supports, recognized by the loader at
/// top-level `box = Ruby::Box.new` statements.
pub(super) fn is_ruby_box_new(node: &Node<'_>) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if call.name().as_slice() != b"new" || call.block().is_some() {
        return false;
    }
    if call.arguments().is_some_and(|a| a.arguments().iter().next().is_some()) {
        return false;
    }
    call.receiver()
        .is_some_and(|r| constant_path_name(&r).is_ok_and(|n| n == "Ruby::Box"))
}

/// `box::A::B` -- a constant path rooted at a LOCAL bound to a box handle
/// (Phase 18). Returns the box id plus the path INSIDE the box (`"A::B"`).
fn box_rooted_path(node: &Node<'_>) -> Option<(u32, String)> {
    let cp = node.as_constant_path_node()?;
    let name = cp.name()?;
    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
    let parent = cp.parent()?;
    if let Some(lv) = parent.as_local_variable_read_node() {
        let lname = String::from_utf8_lossy(lv.name().as_slice()).into_owned();
        let bx = loader::current_box_binding(&lname)?;
        return Some((bx, name));
    }
    let (bx, prefix) = box_rooted_path(&parent)?;
    Some((bx, format!("{prefix}::{name}")))
}

/// `box.eval("literal")`'s body splice -- shared by the loader's
/// STATEMENT-position recognizer (class definitions allowed: real Ruby's
/// `Box#eval` compiles a top-level iseq) and `lower_node`'s
/// expression-position one (which additionally rejects defs, same as root
/// `eval`).
pub(super) fn lower_box_eval_body(
    hir: &mut Hir,
    _result: &ruby_prism::ParseResult,
    call: &ruby_prism::CallNode<'_>,
) -> PResult<Vec<NodeId>> {
    if call.block().is_some() {
        return Err("`Ruby::Box#eval` doesn't take a block".to_string());
    }
    let args: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    if args.len() != 1 {
        return Err(
            "`Ruby::Box#eval` is only supported with exactly one string-literal argument (spike scope)"
                .to_string(),
        );
    }
    let Some(src) = args[0].as_string_node().map(|sn| String::from_utf8_lossy(sn.unescaped()).into_owned()) else {
        return Err(
            "`Ruby::Box#eval` with a non-literal argument isn't supported (spike scope) -- the source must be a plain string literal, resolvable at compile time"
                .to_string(),
        );
    };
    parse_and_lower_into(hir, &src).map_err(|e| format!("Ruby::Box#eval: {e}"))
}

/// `class << obj; def a; ...; end; ...; end` on a NON-`self` receiver (#97 F3):
/// desugar each `def` in the singleton body into a runtime
/// `obj.define_singleton_method(:a, ->(params) { body })`, the same shape
/// `def obj.a` uses. Returns the desugared statement nodes (empty for an empty
/// body). The receiver is re-lowered per def -- exact for the usual simple
/// receiver (a local, `@ivar`, or constant); a side-effecting receiver
/// EXPRESSION would re-evaluate (rare, documented divergence). Caller must have
/// already checked the receiver is not a bare `self`.
fn desugar_singleton_class_defs(
    result: &ruby_prism::ParseResult,
    hir: &mut Hir,
    singleton: &ruby_prism::SingletonClassNode<'_>,
) -> PResult<Vec<NodeId>> {
    let recv_node = singleton.expression();
    let inner = lower_class_body(result, hir, singleton.body())?;
    let mut out = Vec::with_capacity(inner.len());
    for &id in &inner {
        let (mname, params, body) = match &hir[id] {
            HirNode::DefMethod { name, params, body, is_class_method: false, .. } => {
                (name.clone(), params.clone(), body.clone())
            }
            _ => {
                return Err("`class << obj` (a per-instance singleton class) supports only instance `def`s here (spike scope)".to_string());
            }
        };
        let recv = lower_node(result, hir, &recv_node)?;
        let lambda = hir.push(HirNode::Lambda { params, body, method_body: true });
        let sym = hir.push(HirNode::SymbolLit(mname));
        out.push(hir.push(HirNode::Call {
            receiver: Some(recv),
            name: "define_singleton_method".to_string(),
            args: vec![ArrayElem::Single(sym), ArrayElem::Single(lambda)],
            kwargs: vec![],
            block: None,
            block_arg: None,
            safe: false,
        }));
    }
    Ok(out)
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

/// Registers `alias new old` / `alias_method :new, :old` into the current
/// class/module body. When `old` is defined EARLIER IN THIS SAME BODY, the
/// source `DefMethod` is cloned directly (nothing to defer -- no runtime
/// target needed). Otherwise `old` is an INHERITED method whose definition
/// isn't in this body and whose ancestry isn't linearized until `analyze`, so
/// a deferred `HirNode::AliasMethod` is emitted for `mro::resolve_aliases` to
/// resolve later. See `HirNode::AliasMethod`.
fn push_alias(hir: &mut Hir, out: &mut Vec<NodeId>, new_name: String, old_name: String) {
    if let Some(&old_id) = out
        .iter()
        .rev()
        .find(|&&id| matches!(&hir[id], HirNode::DefMethod { name, .. } if *name == old_name))
    {
        let HirNode::DefMethod {
            params,
            body,
            is_class_method,
            visibility,
            ..
        } = &hir[old_id]
        else {
            unreachable!("guarded by the `find` above")
        };
        let (params, body, is_class_method, visibility) =
            (params.clone(), body.clone(), *is_class_method, *visibility);
        out.push(hir.push(HirNode::DefMethod {
            name: new_name,
            params,
            body,
            is_class_method,
            visibility,
        }));
    } else {
        out.push(hir.push(HirNode::AliasMethod { new_name, old_name }));
    }
}

/// `Foo::BAR` / `Foo::Bar::BAZ` (`ConstantPathNode`) -- resolves to
/// `(scope path, name)` for a `HirNode::ConstWrite`/`QualifiedConstRead`'s
/// fields: the LAST segment is the constant being read/written, everything
/// before it is the owning class/module path (multi-segment since Phase
/// 15.3, resolved by `Compiler::resolve_class`). `::FOO` (no `parent` at
/// all -- an explicit top-level anchor) resolves against `Object` directly,
/// mirroring real Ruby's own representation of top-level constants as
/// living on `Object`.
fn constant_path_scope_and_name(node: &ruby_prism::ConstantPathNode<'_>) -> PResult<(String, String)> {
    let name = node
        .name()
        .ok_or("a `::` constant path with a dynamic/computed name isn't supported (spike scope)")?;
    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
    let scope = match node.parent() {
        None => "Object".to_string(),
        Some(p) => constant_path_name(&p)?,
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

/// One entry of `requireds()`/`posts()`: either a plain name, or a
/// parenthesized DESTRUCTURING target list (`|a, (b, c)|`), which prism
/// surfaces as a `MultiTargetNode` in the very same slot -- the same node
/// type, with the same `lefts()`/`rest()`/`rights()` grammar, that a
/// multi-assignment's nested group uses. So it lowers through the same
/// `lower_multi_target_group`, and the slot itself gets an internal name
/// (`__destr_<i>`) that behaves as an ordinary required param everywhere
/// else -- see `Params::destructures`.
///
/// Returns the slot's name, pushing onto `destructures` when it destructures.
fn required_param_slot(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    where_: &str,
    destructures: &mut Vec<(NodeId, crate::hir::MultiTargetGroup)>,
) -> PResult<String> {
    let Some(mt) = node.as_multi_target_node() else {
        return required_param_name(node, where_);
    };
    let group = lower_multi_target_group(result, hir, mt.lefts(), mt.rest(), mt.rights())?;
    let slot = format!("__destr_{}", destructures.len());
    let read = hir.push(HirNode::LocalRead(slot.clone()));
    destructures.push((read, group));
    Ok(slot)
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
    // `def m(...)` -- bare forwarding. Prism surfaces it as a
    // `ForwardingParameterNode` occupying the `keyword_rest` slot (with
    // `.rest()`/`.block()` both `None`). Desugared here into three
    // compiler-internal named params (`*__fwd_rest, **__fwd_kw,
    // &__fwd_blk`); the call-site `n(...)` (a `ForwardingArgumentsNode`)
    // references the same names -- no new HIR shape, no special runtime.
    let forwarding = params
        .keyword_rest()
        .is_some_and(|n| n.as_forwarding_parameter_node().is_some());
    // Anonymous `&` (`def m(&)`) forwards via the same internal-name trick
    // (`n(&)` references it); a named `&blk` stays itself.
    let block = match params.block() {
        Some(b) => Some(Some(match b.name() {
            Some(name) => String::from_utf8_lossy(name.as_slice()).into_owned(),
            None => "__anon_blk".to_string(),
        })),
        None if forwarding => Some(Some("__fwd_blk".to_string())),
        None => None,
    };

    let mut destructures = Vec::new();
    let required = params
        .requireds()
        .iter()
        .map(|n| required_param_slot(result, hir, &n, "before a `*rest`", &mut destructures))
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
        None if forwarding => Some(Some("__fwd_rest".to_string())),
        None => None,
        // A TRAILING COMMA (`|a, |`) -- prism's `ImplicitRestNode`. It means
        // "this block takes more than one parameter", which is what turns on
        // auto-splat, and then discards everything past the named ones:
        // `m([1, 2]) { |a, | a }` is `1`, not `[1, 2]` (oracle-verified).
        // That is exactly an anonymous `*`, so it lowers as one and the
        // existing arity/auto-splat rules cover it with no special case.
        Some(n) if n.as_implicit_rest_node().is_some() => Some(None),
        Some(n) => {
            let r = n.as_rest_parameter_node().ok_or(
                "unsupported rest-parameter form (spike scope)",
            )?;
            // Anonymous `*` (`def m(*)`) gets an internal name so `n(*)`
            // can forward it (Ruby 3.2's anonymous-forwarding semantics).
            Some(Some(match r.name() {
                Some(name) => String::from_utf8_lossy(name.as_slice()).into_owned(),
                None => "__anon_rest".to_string(),
            }))
        }
    };

    let post = params
        .posts()
        .iter()
        .map(|n| required_param_slot(result, hir, &n, "after a `*rest`", &mut destructures))
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
        // Bare `...` forwarding (a `ForwardingParameterNode` in this slot)
        // -- desugared to `**__fwd_kw` here; `rest`/`block` above already
        // synthesized their `__fwd_*` halves.
        Some(n) if n.as_forwarding_parameter_node().is_some() => {
            Some(Some("__fwd_kw".to_string()))
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
            // Anonymous `**` gets an internal name so `n(**)` can forward
            // it, same as the anonymous-`*` rule above.
            Some(Some(match r.name() {
                Some(name) => String::from_utf8_lossy(name.as_slice()).into_owned(),
                None => "__anon_kwrest".to_string(),
            }))
        }
    };

    Ok(Params {
        required,
        destructures,
        optional,
        rest,
        post,
        keywords,
        keyword_rest,
        block,
        // Filled in by `lower_block_like_params` for a block: prism keeps
        // `|x; sum|`'s locals on the BlockParametersNode, not here on the
        // ParametersNode. Always empty for a method's params -- the syntax
        // doesn't exist there.
        block_locals: Vec::new(),
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
            let mut params = lower_params(result, hir, bp.parameters())?;
            // `|x; sum|`'s block-locals -- prism keeps them on the
            // `BlockParametersNode` itself (`locals()`), not in the
            // `ParametersNode` `lower_params` handles, precisely because
            // they are not parameters. See `Params::block_locals`' docs.
            params.block_locals = bp
                .locals()
                .iter()
                .filter_map(|l| l.as_block_local_variable_node())
                .map(|l| String::from_utf8_lossy(l.name().as_slice()).into_owned())
                .collect();
            Ok(params)
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

fn lower_node(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<NodeId> {
    if let Some(int) = node.as_integer_node() {
        // prism's own arbitrary-precision value (LSB-first u32 digits) --
        // which also handles `0xff`/`0b101`/`1_000` uniformly, unlike the
        // old source-text `parse::<i64>()`. Values that fit stay the
        // ordinary `IntegerLit(i64)`; anything bigger is a bignum literal
        // (Phase 17.1).
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
        return Ok(hir.push(HirNode::Lambda { params, body, method_body: false }));
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
        let name_of = |n: &Node<'_>| -> PResult<String> {
            let g = n
                .as_global_variable_read_node()
                .ok_or("`alias`'s global targets must both be plain `$name` globals (spike scope)")?;
            Ok(String::from_utf8_lossy(g.name().as_slice()).into_owned())
        };
        let new_name = name_of(&alias.new_name())?;
        let old_name = name_of(&alias.old_name())?;
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
    // is the encoding phase's own deliverable (plan Part 4: an RObj wrapping
    // an EncodingId, with `Encoding::UTF_8` et al as real constants).
    // Rejected rather than stubbed: inventing a placeholder Encoding now
    // would pre-empt that design, and `__ENCODING__` is only useful if the
    // object it answers actually behaves like one.
    // `__ENCODING__` answers the script's own encoding -- UTF-8 by default,
    // or whatever a `# encoding:` magic comment set. Lowered to the ordinary
    // `Encoding::<NAME>` constant read, which resolves to the seeded singleton.
    if node.as_source_encoding_node().is_some() {
        let const_name = hir.script_encoding.clone().unwrap_or_else(|| "UTF_8".to_string());
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
                    "the `{other}` back-reference global isn't supported yet (spike scope)"
                ))
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
        return Ok(lower_compound_op_write(hir, Storage::Const { scope: None, name }, op_name, rhs));
    }
    if let Some(op) = node.as_constant_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_and_write(hir, Storage::Const { scope: None, name }, rhs));
    }
    // A `# shareable_constant_value:` magic comment makes prism wrap the
    // constant write in a `ShareableConstantNode`. Spinel enforces no Ractor
    // sharing, so unwrap to the inner write and lower it verbatim.
    if let Some(sc) = node.as_shareable_constant_node() {
        return lower_node(result, hir, &sc.write());
    }
    if let Some(op) = node.as_constant_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(lower_or_write(hir, Storage::Const { scope: None, name }, rhs));
    }
    if let Some(cw) = node.as_constant_write_node() {
        let name = String::from_utf8_lossy(cw.name().as_slice()).into_owned();
        // `Name = Struct.new(:a, :b)` / `Name = Data.define(...)` is an ordinary
        // constant write whose value is a runtime `Struct.new`/`Data.define`
        // call (Batch E): the call MINTS a real class at runtime
        // (`rstruct::struct_new`), the write binds it to the constant, and
        // `const_set` names the freshly anonymous class (`RUBY`'s "assigning an
        // anonymous class to a constant names it"). No compile-time synthesis.
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
        // `box::X` (Phase 18): an external access into the box -- the
        // ordinary bare-name lowering, wrapped in the box's scope.
        // A single segment lowers as a bare `ClassRef` (codegen's class-
        // or-constant rule under the box); deeper paths as the qualified
        // read they'd be inside the box.
        if let Some((bx, path)) = box_rooted_path(node) {
            let inner = match path.rsplit_once("::") {
                Some((scope, leaf)) => hir.push(HirNode::QualifiedConstRead(
                    scope.to_string(),
                    leaf.to_string(),
                )),
                None => hir.push(HirNode::ClassRef(path)),
            };
            return Ok(hir.push(HirNode::BoxScope { box_id: bx, body: vec![inner] }));
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
        let (mut args, kwargs, _fwd_block) =
            lower_call_args(result, hir, yield_node.arguments())?;
        // A `*expr` splat needs no handling here: `Yield` carries the same
        // `Vec<ArrayElem>` a `Call`'s positional args do, and codegen flattens
        // a `Splat` element at runtime. Keyword args (literal pairs AND `**h`
        // double-splats) fold into one trailing `HashLit`, which codegen
        // builds via the shared `KwArg` emitter and `emit_proc_param_bindings`
        // binds a block's keyword params from.
        //
        // Divergence: a runtime-empty `**{}` still contributes a trailing `{}`
        // here (`yield(1, **{})` -> `[1, {}]`), where real Ruby drops it -- the
        // call path suppresses that via `emit_splat_call`'s non-empty guard,
        // but yield's trailing-`HashLit` shape has none. Rare, and strictly
        // better than the previous flat rejection of `yield **h`.
        if !kwargs.is_empty() {
            args.push(ArrayElem::Single(hir.push(HirNode::HashLit(kwargs))));
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
        // Positional args stay in `args`; a trailing keyword hash
        // (`super(x: 1, y: 2)`) becomes `kwargs`, bound to the parent's
        // keyword params by name.
        let mut args = Vec::new();
        let mut kwargs = Vec::new();
        if let Some(a) = sup.arguments() {
            for n in a.arguments().iter() {
                if let Some(kw) = n.as_keyword_hash_node() {
                    kwargs = lower_kwargs(result, hir, &kw.elements().iter().collect::<Vec<_>>())?;
                } else {
                    args.push(lower_node(result, hir, &n)?);
                }
            }
        }
        let block = match sup.block() {
            None => None,
            Some(b) => Some(lower_block(result, hir, &b)?),
        };
        return Ok(hir.push(HirNode::SuperCall { args, kwargs, zsuper: false, block }));
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
        }));
    }

    if let Some(class) = node.as_class_node() {
        let name = constant_path_name(&class.constant_path())?;
        let superclass = match class.superclass() {
            None => None,
            Some(sc) => Some(constant_path_name(&sc)?),
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
        let name = constant_path_name(&module.constant_path())?;
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
            Some(r) => {
                // `def obj.name` on a NON-`self` receiver (#97 F3) -- a
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
                let lambda = hir.push(HirNode::Lambda { params, body, method_body: true });
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
        }));
    }

    // `class << obj` at expression/statement position (#97 F3) -- top level or
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
            if let Some(recv) = call
                .receiver()
                // Only a LITERAL constant/path receiver is a static `New`;
                // any other receiver (`x.new` on a local holding a class
                // value -- Phase 16.1) falls through to the generic `Call`
                // lowering and dispatches via `TyKind::ClassObj`/the
                // runtime constructor.
                .filter(|r| r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some())
            {
                // `box::Widget.new(...)` (Phase 18): the ordinary static
                // `New`, resolved inside the box.
                let box_ctx = box_rooted_path(&recv);
                let class_name = match &box_ctx {
                    Some((_, path)) => path.clone(),
                    None => constant_path_name(&recv)?,
                };
                // `Enumerator.new { |y| ... }` joins the block-keeping set
                // (Phase 17.2): it falls through to the generic `Call`
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
                                || n.as_keyword_hash_node().is_some_and(|kw| {
                                    kw.elements().iter().any(|e| e.as_assoc_splat_node().is_some())
                                })
                        })
                    })
                    .unwrap_or(false);
                // A LITERAL block (`Foo.new(x) { ... }`) is captured and
                // forwarded to `initialize`; a block-PASS (`&p`) has no
                // `.as_block_node()` and falls through to the generic `Call`
                // lowering (its dynamic `new` dispatch threads the block arg).
                let block_pass = call
                    .block()
                    .is_some_and(|b| b.as_block_node().is_none());
                if !has_dynamic_args
                    && !block_pass
                    && !matches!(
                        class_name.as_str(),
                        // `Class.new(Super) { body }` (#97 F4) keeps its block --
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
                        "Fiber" | "Thread" | "Mutex" | "Queue" | "SizedQueue" | "Ractor" | "Enumerator" | "Proc" | "Array" | "Hash" | "Set" | "Class" | "Struct"
                    )
                {
                    // A trailing keyword hash lands in `kwargs`, kept apart
                    // from the positionals exactly as an ordinary call's is,
                    // so `initialize`'s keyword params bind as keywords.
                    // (It used to fold into a positional Hash literal, which
                    // made `Foo.new(1, k: 2)` look like two positionals to a
                    // `def initialize(a, k:)`.) A callee declaring NO keyword
                    // params still sees the options hash it expects --
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
                        Some(b) if b.as_block_node().is_some() => {
                            Some(lower_block(result, hir, &b)?)
                        }
                        _ => None,
                    };
                    let new_id = hir.push(HirNode::New { class_name, args, kwargs, block });
                    return Ok(match box_ctx {
                        Some((bx, _)) => {
                            hir.push(HirNode::BoxScope { box_id: bx, body: vec![new_id] })
                        }
                        None => new_id,
                    });
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
                            }));
                        }
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

        // `define_singleton_method(:literal) { block }` -- desugars to a
        // `def self.name` on the target class. The target comes from the
        // receiver: none / `self` (inside a class body) means the enclosing
        // class, so a bare `DefMethod { is_class_method: true }` lands in the
        // current body and registers there; a literal-constant / constant-
        // path receiver (`C.` / `M::D.`) reopens that named class with an
        // inline `ClassDef`. A computed name, a computed receiver, or a
        // capturing block that this desugar can't model falls through to the
        // generic (unsupported) `Call`.
        if name == "define_singleton_method" {
            if let (Some(args), Some(block_node)) = (call.arguments(), call.block()) {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if let (1, Some(sym)) = (arg_list.len(), arg_list.first().and_then(|a| a.as_symbol_node())) {
                    let recv = call.receiver();
                    let target = match &recv {
                        None => Some(None),
                        Some(r) if r.as_self_node().is_some() => Some(None),
                        Some(r) => constant_path_name(r).ok().map(Some),
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
                            // `Kernel#loop`'s REAL definition (CRuby
                            // kernel.rb:151) rescues StopIteration and
                            // returns its `result` -- desugared here into
                            // the ordinary Begin/rescue machinery (Phase
                            // 17.2), so `loop { e.next }` terminates
                            // cleanly with the enumeration's result and a
                            // manual `raise StopIteration` returns nil.
                            let native_loop = hir.push(HirNode::Loop { body });
                            let exc_read =
                                hir.push(HirNode::LocalRead("__loop_stop".to_string()));
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
                                    binding: Some("__loop_stop".to_string()),
                                    body: vec![result_call],
                                }],
                                else_body: None,
                                ensure_body: None,
                            }));
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
        if name == "__dir__" && call.receiver().is_none() {
            let no_args = call.arguments().is_none_or(|a| a.arguments().iter().next().is_none());
            if no_args && call.block().is_none() {
                let dir = current_dir_str()?;
                return Ok(hir.push(HirNode::StringLit(vec![StrPart::Lit(dir)])));
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
                        return Ok(hir.push(HirNode::Lambda { params, body, method_body: false }));
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
            // A trailing keyword hash carries `cause:`. Splitting it off the
            // positional list is what keeps the three-state distinction: an
            // ABSENT `cause:` chains from `$!`, while `cause: nil` is
            // `Explicit` with a nil value and suppresses chaining.
            let (kw_nodes, positional): (Vec<_>, Vec<_>) = arg_list
                .iter()
                .partition(|n| n.as_keyword_hash_node().is_some());
            let mut cause = RaiseCause::Absent;
            for kw in &kw_nodes {
                let hash = kw.as_keyword_hash_node().expect("partitioned on this");
                for element in hash.elements().iter() {
                    let assoc = element
                        .as_assoc_node()
                        .ok_or("`raise` accepts only a `cause:` keyword")?;
                    let key = assoc
                        .key()
                        .as_symbol_node()
                        .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
                        .unwrap_or_default();
                    if key != "cause" {
                        return Err(format!("`raise` doesn't accept the `{key}:` keyword"));
                    }
                    cause = RaiseCause::Explicit(lower_node(result, hir, &assoc.value())?);
                }
            }
            if positional.len() > 2 {
                return Err("`raise`/`fail` with more than 2 positional arguments isn't supported yet (spike scope)".to_string());
            }
            if positional.is_empty() && matches!(cause, RaiseCause::Explicit(_)) {
                return Err(
                    "only cause is given with no arguments".to_string()
                );
            }
            let args = positional
                .iter()
                .map(|n| lower_node(result, hir, n))
                .collect::<PResult<Vec<_>>>()?;
            return Ok(hir.push(HirNode::Raise(args, cause)));
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
            // ...EXCEPT when the feature is one spinel already provides
            // natively. Then there is nothing to splice and nothing to
            // search for: the whole effect of the require is to activate a
            // gated builtin, which is a compile-time act that works from any
            // position. CRuby's `require` returns true the first time a
            // feature is loaded and false thereafter (`load.c:1413`), and
            // `activated_features` doubles as that loaded-features table --
            // so the call folds to the bool `insert` reports.
            //
            // Only `require` folds. `load` re-executes unconditionally and
            // `require_relative` names a file that must actually be spliced,
            // neither of which a literal can stand in for.
            if name == "require" {
                if let Some(feature) = single_literal_string_arg(result, hir, &call)? {
                    if loader::is_builtin_feature(&feature) {
                        let newly_loaded = hir
                            .activated_features
                            .insert(loader::canonical_ext_feature(&feature).to_string());
                        let first = newly_loaded && !loader::is_preloaded_at_boot(&feature);
                        return Ok(hir.push(HirNode::BoolLit(first)));
                    }
                }
            }
            return Err(format!(
                "`{name}` is only supported as a top-level statement with a single string-literal argument (spike scope) -- it's resolved at compile time, so it can't appear inside a method, block, conditional, `begin`, or `eval` body"
            ));
        }
        // `autoload :Const, "feature"` -- the loader's eager pre-pass
        // (`Loader::lower_file_statements`) has already SPLICED the feature
        // file so `Const` is defined, treating autoload as a compile-time
        // require. The call itself is therefore a runtime no-op. We still
        // validate the target resolves at compile time here (`autoload_feature`
        // errors on a dynamic path/symbol, exactly like a non-top-level
        // `require`), so a genuinely dynamic autoload is a clean rejection
        // rather than a silently-undefined constant.
        if name == "autoload" && call.receiver().is_none() {
            autoload_feature(&call)?;
            return Ok(hir.push(HirNode::NilLit));
        }

        // Phase 18 guard rails. `Ruby::Box` class-method calls outside the
        // one recognized shape (`box = Ruby::Box.new` at top-level
        // statement position, handled by the loader) are clean rejections:
        // `.current`/`.root`/`.main`/`.enabled?` have no compile-time
        // meaning in this AOT model, and an unassigned/nested `.new` would
        // allocate a box nothing could ever reference.
        if let Some(recv) = call.receiver() {
            if constant_path_name(&recv).is_ok_and(|n| n == "Ruby::Box") {
                return Err(format!(
                    "`Ruby::Box.{name}` isn't supported here (spike scope) -- the one supported allocation shape is `box = Ruby::Box.new` as a top-level statement; `.current`/`.root`/`.main`/`.enabled?` have no compile-time meaning"
                ));
            }
            // Operations on a bound box handle outside their recognized
            // positions: `box.require`-family must be a TOP-LEVEL
            // statement (same rule as receiver-less `require`);
            // expression-position `box.eval` is allowed but, like root
            // `eval`, can't define classes/methods.
            if let Some(lv) = recv.as_local_variable_read_node() {
                let lname = String::from_utf8_lossy(lv.name().as_slice()).into_owned();
                if let Some(bx) = loader::current_box_binding(&lname) {
                    match name.as_str() {
                        "require" | "require_relative" | "load" => {
                            return Err(format!(
                                "`{lname}.{name}` is only supported as a top-level statement (same rule as the receiver-less `{name}`)"
                            ));
                        }
                        "eval" => {
                            let body = lower_box_eval_body(hir, result, &call)?;
                            reject_top_level_defs(hir, &body)?;
                            return Ok(hir.push(HirNode::BoxScope { box_id: bx, body }));
                        }
                        _ => {}
                    }
                }
            }
        }

        if name == "eval" && call.receiver().is_none() {
            // A single string-LITERAL argument keeps the zero-cost AOT path:
            // the source is parsed and INLINED at compile time (`HirNode::Eval`),
            // needs no runtime parser, and can even see the surrounding scope's
            // locals. Every other shape -- a non-literal source expression, or
            // the `binding`/`filename`/`lineno` argument forms -- falls through
            // to the ordinary implicit-self `Call` lowering below, which
            // dispatches `Kernel#eval` into the runtime eval VM (#97 stage 2;
            // feature-gated, so a build without it raises NotImplementedError
            // at the call). The VM runs in a top-level-first scope: correct
            // `self`, but no access to the caller's own locals (a first-class
            // `binding` is the next increment).
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if arg_list.len() == 1 {
                if let Some(s) = arg_list[0].as_string_node() {
                    let src = String::from_utf8_lossy(s.unescaped()).into_owned();
                    // Try the zero-cost AOT inline path. If the literal source
                    // doesn't parse, or defines at the top level (which the
                    // inline path can't express), DON'T fail the compile: fall
                    // through to the runtime eval VM so the program still
                    // builds and the error/behaviour surfaces at runtime,
                    // catchably, exactly as CRuby's `eval` does.
                    if let Ok(body) = parse_and_lower_into(hir, &src) {
                        if reject_top_level_defs(hir, &body).is_ok() {
                            return Ok(hir.push(HirNode::Eval(body)));
                        }
                    }
                }
            }
        }

        let receiver = match call.receiver() {
            None => None,
            Some(r) => Some(lower_node(result, hir, &r)?),
        };
        let (args, kwargs, fwd_block) =
            lower_call_args(result, hir, call.arguments())?;
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
        return Ok(hir.push(HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: call.is_safe_navigation(),
        }));
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
        let cmd = hir.push(HirNode::StringLit(vec![string_literal_part(xs.unescaped())]));
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
        let parts = lower_string_parts(result, hir, re.parts().iter())?;
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
                .ok_or("unsupported keyword-argument shape (spike scope)")?;
            let key = lower_node(result, hir, &assoc.key())?;
            let value = lower_node(result, hir, &assoc.value())?;
            kwargs.push(KwArg::Pair(key, value));
        }
    }
    Ok(kwargs)
}

/// Splits a call's raw argument list into (positional `ArrayElem`s, an
/// ordered `KwArg` list, an optional forwarded block). A trailing
/// `KeywordHashNode` (`foo(x: 1, **h)`) is the only prism shape recognized as
/// keyword arguments (lowered via `lower_kwargs`); every other entry lowers as
/// a positional argument via `lower_array_elem`. The returned `Option<NodeId>`
/// is a `...`-forwarded block (`&__fwd_blk`); a call's own literal block lives
/// outside this function.
fn lower_call_args(
    result: &ParseResult,
    hir: &mut Hir,
    arguments: Option<ruby_prism::ArgumentsNode<'_>>,
) -> PResult<(Vec<ArrayElem>, Vec<KwArg>, Option<NodeId>)> {
    let Some(arguments) = arguments else {
        return Ok((Vec::new(), Vec::new(), None));
    };
    let mut list: Vec<_> = arguments.arguments().iter().collect();
    // `n(...)` inside `def m(...)` -- a `ForwardingArgumentsNode` in the
    // list. Expands to the three internal params `lower_params`
    // synthesized: `*__fwd_rest, **__fwd_kw, &__fwd_blk` (the block half
    // returned separately -- a call's block slot lives outside this
    // function). `**__fwd_kw` enters the ordered `kwargs` list as a trailing
    // double-splat.
    if let Some(pos) = list.iter().position(|n| n.as_forwarding_arguments_node().is_some()) {
        list.remove(pos);
        let fwd_kw = hir.push(HirNode::LocalRead("__fwd_kw".to_string()));
        let fwd_block = Some(hir.push(HirNode::LocalRead("__fwd_blk".to_string())));
        // The rest-splat slots in positionally where `...` was written.
        let rest_read = hir.push(HirNode::LocalRead("__fwd_rest".to_string()));
        let mut args = Vec::new();
        for (i, n) in list.iter().enumerate() {
            if i == pos {
                args.push(ArrayElem::Splat(rest_read));
            }
            args.push(lower_array_elem_or_anon(result, hir, n)?);
        }
        if pos >= list.len() {
            args.push(ArrayElem::Splat(rest_read));
        }
        return Ok((args, vec![KwArg::DoubleSplat(fwd_kw)], fwd_block));
    }
    let kwargs = match list.last().and_then(|n| n.as_keyword_hash_node()) {
        Some(kw) => {
            list.pop();
            let elements: Vec<Node<'_>> = kw.elements().iter().collect();
            lower_kwargs(result, hir, &elements)?
        }
        None => Vec::new(),
    };
    let args = list
        .iter()
        .map(|n| lower_array_elem_or_anon(result, hir, n))
        .collect::<PResult<Vec<_>>>()?;
    Ok((args, kwargs, None))
}

/// `lower_array_elem`, plus the CALL-argument-only anonymous `*` forwarding
/// form (`n(*)` inside `def m(*)`) -- an array literal's own bare `*` stays
/// rejected in `lower_array_elem` itself.
fn lower_array_elem_or_anon(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<ArrayElem> {
    if let Some(splat) = node.as_splat_node() {
        if splat.expression().is_none() {
            return Ok(ArrayElem::Splat(
                hir.push(HirNode::LocalRead("__anon_rest".to_string())),
            ));
        }
    }
    lower_array_elem(result, hir, node)
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
    let mut module_function = false;
    for stmt in &stmts {
        lower_class_body_statement(result, hir, stmt, &mut visibility, &mut module_function, &mut out)?;
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
    module_function: &mut bool,
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
    // `undef foo, bar` -- a keyword like `alias`, same target shape (prism
    // gives each name as a SymbolNode either way), so it reuses
    // `alias_target_name`. Recorded rather than resolved here: see
    // `HirNode::Undef` for why the inherited case rules out deleting a def.
    // A class-body `if`/`unless` guarding a `def`/`alias`/visibility directive
    // with a statically-literal predicate (`alias a b if true`) is folded at
    // definition time -- real Ruby runs these guards while the class body
    // executes, and an `alias`/`def` inside one has no ordinary value-`if`
    // lowering (they're class-body-only keywords). A dynamic predicate falls
    // through to the generic value-`if` path unchanged.
    if let Some(if_node) = node.as_if_node() {
        if let Some(cond) = static_bool(&if_node.predicate()) {
            let chosen = if cond {
                if_node.statements().map(|s| s.as_node())
            } else {
                if_node.subsequent()
            };
            return lower_class_body_selected(result, hir, chosen, visibility, module_function, out);
        }
    }
    if let Some(unless_node) = node.as_unless_node() {
        if let Some(cond) = static_bool(&unless_node.predicate()) {
            let chosen = if !cond {
                unless_node.statements().map(|s| s.as_node())
            } else {
                unless_node.else_clause().map(|e| e.as_node())
            };
            return lower_class_body_selected(result, hir, chosen, visibility, module_function, out);
        }
    }

    if let Some(undef) = node.as_undef_node() {
        let names = undef
            .names()
            .iter()
            .map(|n| alias_target_name(&n))
            .collect::<PResult<Vec<_>>>()?;
        out.push(hir.push(HirNode::Undef(names)));
        return Ok(());
    }

    if let Some(alias) = node.as_alias_method_node() {
        let new_name = alias_target_name(&alias.new_name())?;
        let old_name = alias_target_name(&alias.old_name())?;
        push_alias(hir, out, new_name, old_name);
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
    // the ORDINARY class-body path (so `attr_reader`/`private`/`alias`/
    // nested `def`s all work exactly as they would directly in the class
    // body), then each result is mapped onto the ENCLOSING class:
    //   - a `def`     -> retagged as a class method (`set_method_is_class_method`);
    //   - a constant  -> spliced onto the enclosing class. Real Ruby scopes a
    //     `class << self` constant to the SINGLETON class (so `C::NAME`
    //     NameErrors), but its only common use is lexical reference from the
    //     singleton's own methods -- which are now the enclosing class's class
    //     methods, and those resolve the enclosing class's constants (verified
    //     against the oracle). Documented divergence: external `C::NAME`
    //     resolves here where CRuby raises.
    //   - `include M` -> `extend M` on the enclosing class (M's instance
    //     methods become class methods either way -- same effect).
    // `extend`/`prepend`/a nested `class << self` inside the singleton stay a
    // clean rejection: those act on the singleton's OWN singleton, which plain
    // enclosing-class retagging can't express (deferred).
    if let Some(singleton) = node.as_singleton_class_node() {
        if singleton.expression().as_self_node().is_none() {
            // `class << obj` on a NON-`self` receiver (#97 F3): each `def` in
            // the body is a per-object singleton method (see
            // `desugar_singleton_class_defs`).
            out.extend(desugar_singleton_class_defs(result, hir, &singleton)?);
            return Ok(());
        }
        let inner = lower_class_body(result, hir, singleton.body())?;
        for &id in &inner {
            // Classify without holding the `&hir[id]` borrow across the
            // mutations below (`include` re-pushes a fresh `Extend` node).
            enum Item {
                Method,
                Passthrough,
                Extend(String),
                Reject,
            }
            let item = match &hir[id] {
                HirNode::DefMethod { .. } => Item::Method,
                HirNode::ConstWrite { .. } => Item::Passthrough,
                HirNode::Include(m) => Item::Extend(m.clone()),
                _ => Item::Reject,
            };
            match item {
                Item::Method => {
                    hir.set_method_is_class_method(id);
                    out.push(id);
                }
                Item::Passthrough => out.push(id),
                Item::Extend(m) => out.push(hir.push(HirNode::Extend(m))),
                Item::Reject => {
                    return Err(
                        "unsupported statement in `class << self` (spike scope) -- only `def`s, constants, `include`, and `attr_*`/`private`/`alias` are handled here; `extend`/`prepend`/ivars/a nested `class << self` aren't supported yet".to_string(),
                    );
                }
            }
        }
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
                        } else {
                            // Re-declaring an INHERITED method's visibility (no
                            // local `def` to retag): recorded for codegen to
                            // apply after materialization. See
                            // `HirNode::MethodVisibility`.
                            out.push(hir.push(HirNode::MethodVisibility {
                                name: target,
                                visibility: new_vis,
                            }));
                        }
                    }
                    return Ok(());
                }
                // Falls through to the generic `Call` lowering below --
                // a dynamic/computed argument (e.g. `private(*names)`).
            }
            // `module_function` -- recognized in the same two forms as
            // `private`/`public`/`protected`: (1) a bare call switches a mode
            // so every subsequent `def` in this body becomes a MODULE method
            // (`Mod.name`); (2) `module_function :a, :b` retroactively
            // promotes already-defined method(s) of those names. Real Ruby
            // ALSO keeps a private instance copy for `include`-mixin; spinel
            // models only the module-method form (see
            // `module_function_namespace.rb`), so promotion is an in-place
            // retag to `is_class_method`, not an added copy -- which also
            // lets an uncalled module function be dead-code-eliminated
            // exactly like any other uncalled class method. A dynamic/
            // computed argument falls through to a generic `Call`.
            if name == "module_function" {
                let arg_list: Vec<_> = call
                    .arguments()
                    .map(|a| a.arguments().iter().collect())
                    .unwrap_or_default();
                if arg_list.is_empty() {
                    *module_function = true;
                    return Ok(());
                }
                if arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
                    for n in &arg_list {
                        let target = String::from_utf8_lossy(
                            n.as_symbol_node().expect("checked above").unescaped(),
                        )
                        .into_owned();
                        if let Some(&id) = out.iter().rev().find(|&&id| {
                            matches!(&hir[id], HirNode::DefMethod { name: existing, .. } if *existing == target)
                        }) {
                            hir.set_method_is_class_method(id);
                        }
                    }
                    return Ok(());
                }
            }
            // `alias_method :new, :old` -- the method-call spelling of the
            // `alias` keyword, routed through the same `push_alias` (so an
            // inherited source defers to `mro::resolve_aliases`). Only two
            // literal symbol/string names; anything else falls through to a
            // generic `Call`.
            if name == "alias_method" {
                let arg_list: Vec<_> = call
                    .arguments()
                    .map(|a| a.arguments().iter().collect())
                    .unwrap_or_default();
                if arg_list.len() == 2 {
                    let names: Option<Vec<String>> = arg_list
                        .iter()
                        .map(|n| {
                            n.as_symbol_node()
                                .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
                                .or_else(|| {
                                    n.as_string_node()
                                        .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
                                })
                        })
                        .collect();
                    if let Some(names) = names {
                        push_alias(hir, out, names[0].clone(), names[1].clone());
                        return Ok(());
                    }
                }
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
                            .map(constant_path_name)
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
        // Under a bare `module_function`, every following `def` is promoted
        // to a module method (see the recognizer above).
        if *module_function {
            hir.set_method_is_class_method(id);
        }
        out.push(id);
        return Ok(());
    }
    out.push(lower_node(result, hir, node)?);
    Ok(())
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
fn lower_single_optional_argument(
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
    // The same group shape reached from a PARAMETER list (`|a, (b, c)|` --
    // see `required_param_slot`) names its leaves with parameter nodes rather
    // than target nodes: prism distinguishes the two contexts, but a
    // destructuring param binds a plain local exactly as an assignment target
    // does, so both spell the same `MultiTarget::Local`.
    if let Some(t) = node.as_required_parameter_node() {
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
    if let Some(t) = node.as_constant_path_target_node() {
        // Same parent-path + leaf split `ConstWrite`'s own `Foo::BAR = v`
        // lowering uses: the parent must be a static constant path.
        let parent = match t.parent() {
            Some(p) => constant_path_name(&p)?,
            None => String::new(), // `::BAR` -- top-level anchored
        };
        let name = String::from_utf8_lossy(t.name().expect("a constant path target always has a name").as_slice()).into_owned();
        return Ok(MultiTarget::ScopedConst { scope: parent, name });
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
        // A PARAMETER-context group (`|(a, *r)|`) spells its splat as a
        // `RestParameterNode` where an assignment-context one uses a
        // `SplatNode` -- same meaning, and both allow the anonymous form
        // (`Some(None)`: absorbs and discards the middle slice).
        Some(n) if n.as_rest_parameter_node().is_some() => {
            let r = n.as_rest_parameter_node().unwrap();
            Some(r.name().map(|name| {
                Box::new(crate::hir::MultiTarget::Local(
                    String::from_utf8_lossy(name.as_slice()).into_owned(),
                ))
            }))
        }
        Some(n) => {
            let splat = n
                .as_splat_node()
                .ok_or("expected `*name` as a multi-assignment's splat target")?;
            match splat.expression() {
                // Anonymous `*` -- absorbs (and discards) the middle slice;
                // `MultiTargetGroup::splat`'s `Some(None)` shape models
                // exactly this.
                None => Some(None),
                Some(expr) => Some(Some(Box::new(lower_multi_target(result, hir, &expr)?))),
            }
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
            .map(|n| constant_path_name(&n))
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
        let constant = arr.constant().map(|c| constant_path_name(&c)).transpose()?;
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
        let constant = find.constant().map(|c| constant_path_name(&c)).transpose()?;
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
        let constant = hp.constant().map(|c| constant_path_name(&c)).transpose()?;
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
    // A bare constant with no capture (`in Integer`, `in SomeClass`, or a
    // qualified `in Store::Item` -- Phase 15.3) -- an `is_a?`-style check,
    // resolved (built-in tag vs. user-class ancestry) entirely in
    // `codegen::patterns::emit_class_check`.
    if let Some(c) = node.as_constant_read_node() {
        let name = String::from_utf8_lossy(c.name().as_slice()).into_owned();
        return Ok(Pattern::ClassCheck(name));
    }
    if node.as_constant_path_node().is_some() {
        return Ok(Pattern::ClassCheck(constant_path_name(node)?));
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
    // A trailing comma (`in [0, 1, ]`) is prism's `ImplicitRestNode`: an
    // anonymous "at least this many elements" rest that binds nothing --
    // exactly the `Some(None)` shape codegen already emits for a bare `*`.
    if node.as_implicit_rest_node().is_some() {
        return Ok(None);
    }
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
/// The single string-literal argument of a `require`-shaped call, or `None`
/// when the call has any other argument shape (no arguments, several, or one
/// that isn't a compile-time-constant string).
///
/// Lowering the argument through the ordinary path is deliberate -- it picks
/// up prism's adjacent-literal folding for free -- and the throwaway node left
/// behind on a `None` return is harmless append-only arena bookkeeping.
fn single_literal_string_arg(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
) -> PResult<Option<String>> {
    let args: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let [arg] = args.as_slice() else {
        return Ok(None);
    };
    let id = lower_node(result, hir, arg)?;
    Ok(literal_string_text(hir, id))
}

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
            // A raw-byte (non-UTF-8) or interpolated segment can't fold to a
            // static UTF-8 string (e.g. a `require` path).
            StrPart::Bytes(_) | StrPart::Interp(_) => return None,
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
        let lvt = target.as_local_variable_target_node().ok_or(
            "`=~`'s named-capture auto-binding only writes plain locals (spike scope)",
        )?;
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
    Ok(match loader::current_source_file() {
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
pub(super) fn autoload_feature(call: &CallNode<'_>) -> PResult<String> {
    let args: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    if args.len() != 2 {
        return Err(
            "`autoload` takes exactly two arguments (`autoload :Const, \"feature\"`)".to_string(),
        );
    }
    if let Some(lit) = args[1].as_string_node() {
        return Ok(String::from_utf8_lossy(lit.unescaped()).into_owned());
    }
    if let Some(feature) = expand_path_dir_feature(&args[1])? {
        return Ok(feature);
    }
    Err(
        "`autoload` with a non-literal feature isn't supported (spike scope) -- the target must resolve at compile time: a string literal, or `File.expand_path(\"...\", __dir__)`".to_string(),
    )
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
    let path = loader::current_source_file()
        .ok_or("`__dir__` needs a real source file (there is none when compiling a bare string)")?;
    let resolved = path.canonicalize().unwrap_or(path);
    let dir = resolved
        .parent()
        .ok_or("the source file has no parent directory")?;
    Ok(dir.to_string_lossy().into_owned())
}

/// The 1-based line a byte offset falls on. `Location` only carries
/// offsets, so this counts the newlines before it -- fine for the handful
/// of `__LINE__`/`__dir__` sites a program has (this is not on any hot
/// path; it runs once per occurrence, at compile time).
fn line_of(result: &ParseResult, offset: usize) -> i64 {
    let src = result.source();
    1 + src[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as i64
}

/// One `parts()` entry of an `InterpolatedStringNode`:
///
///   - a literal chunk (`StringNode`);
///   - an `#{ }` (`EmbeddedStatementsNode`) -- several statements answer the
///     LAST, via the same `Seq` a parenthesized `(a; b)` lowers to, and an
///     EMPTY `#{}` interpolates the empty string (real Ruby: `"x#{}y"` is
///     `"xy"`);
///   - a brace-less `#@ivar`/`#@@cvar`/`#$global` (`EmbeddedVariableNode`),
///     whose `variable()` is an ordinary read node and so needs no special
///     handling beyond unwrapping it.
/// A literal string segment's bytes as a `StrPart`: readable UTF-8 text when
/// the bytes form valid UTF-8 (the overwhelmingly common case), else the raw
/// bytes preserved for the encoding engine (a `"\xNN"` escape that isn't a
/// character -- Ruby tags such a literal ASCII-8BIT).
fn string_literal_part(bytes: &[u8]) -> StrPart {
    match std::str::from_utf8(bytes) {
        Ok(s) => StrPart::Lit(s.to_string()),
        Err(_) => StrPart::Bytes(bytes.to_vec()),
    }
}

/// Lower an interpolated literal's parts, FLATTENING any nested interpolated
/// string into the outer list.
///
/// A part is not always a leaf: backslash-continued adjacent literals
/// (`"<a w='#{px}' " \ "h='#{px}'>"`) parse as an `InterpolatedStringNode`
/// whose own parts are themselves `InterpolatedStringNode`s. Splicing the
/// inner parts in is exactly the concatenation the source spells, and it
/// composes to any nesting depth. Shared by the string, symbol, and regexp
/// literal paths, all of which can carry the same adjacency.
fn lower_string_parts<'a>(
    result: &ParseResult,
    hir: &mut Hir,
    parts: impl Iterator<Item = Node<'a>>,
) -> PResult<Vec<StrPart>> {
    let mut out = Vec::new();
    for part in parts {
        match part.as_interpolated_string_node() {
            Some(inner) => out.extend(lower_string_parts(result, hir, inner.parts().iter())?),
            None => out.push(lower_string_part(result, hir, &part)?),
        }
    }
    Ok(out)
}

fn lower_string_part(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<StrPart> {
    if let Some(s) = node.as_string_node() {
        return Ok(string_literal_part(s.unescaped()));
    }
    if let Some(embedded) = node.as_embedded_statements_node() {
        let stmts: Vec<_> = embedded
            .statements()
            .map(|s| s.body().iter().collect())
            .unwrap_or_default();
        return match stmts.as_slice() {
            [] => Ok(StrPart::Lit(String::new())),
            [only] => Ok(StrPart::Interp(lower_node(result, hir, only)?)),
            _ => {
                let ids = stmts
                    .iter()
                    .map(|s| lower_node(result, hir, s))
                    .collect::<PResult<Vec<_>>>()?;
                Ok(StrPart::Interp(hir.push(HirNode::Seq(ids))))
            }
        };
    }
    if let Some(embedded) = node.as_embedded_variable_node() {
        return Ok(StrPart::Interp(lower_node(result, hir, &embedded.variable())?));
    }
    Err("unsupported string interpolation part (spike scope)".to_string())
}
