//! The compiler's front-end DRIVER: the top-level parse -> lower entry
//! points plus everything `require` resolution needs (the loader, gemspecs,
//! lockfiles, the external gem store). The lowering itself -- prism tree ->
//! typed HIR -- lives in `crate::lower` (never touches the filesystem); this
//! module is the filesystem-touching layer that drives it file-by-file, feeding
//! per-file state through `crate::lower::context`.

pub mod gem_compat;
mod gem_store;
pub(crate) use gem_store::store_gems;
pub(crate) mod gemspec;
mod loader;
pub use loader::absolutize_feature;
pub(crate) use loader::bundled_gem_roots;
pub(crate) mod lockfile;
pub(crate) mod syntax_report;

pub use loader::read_source;

use crate::diagnostics::CompileError;
use crate::hir::{Hir, HirNode, NodeId};
use crate::lower::{encoding_const_name, parse_and_lower_into};

/// The built-in exception hierarchy: ordinary Ruby source, spliced into
/// EVERY compiled program ahead of the
/// user's own code via the exact same `parse_and_lower_into` mechanism
/// `eval`'s literal-splice already uses (see that recognizer's docs below).
/// This is the whole point: real classes/inheritance (the MRO work)
/// already makes `class X < Y; end` meaningful, so the built-in hierarchy
/// needs ZERO dedicated Rust construction code -- it's just Ruby, using the
/// same machinery a user's own classes do. A custom hierarchy (`class MyError
/// < StandardError; def initialize(x); super(...); @x = x; end; end`) needs
/// NO new machinery either -- it's just ordinary inheritance + materialized
/// `super`. `msg` is a plain REQUIRED param,
/// not a Ruby-level default (`msg = "..."`, which real Ruby's own
/// `Exception.new` supports) -- every construction site this compiler
/// generates (`raise`'s lowering -- see `clif::stmt`'s `Raise` arm)
/// always supplies a message explicitly (the raising class's own name as a
/// compile-time string literal, when `raise` itself gave none), so this
/// narrower shape needs no optional-parameter support at all.
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
# The namespace only. Its classes come from `zeo_abi::ERRNO_CLASSES`, which
# names one per errno THIS platform defines -- a set no fixed Ruby source can
# spell.
module Errno
end
"##;

/// Returns the built `Hir` plus the id of its `Program` root -- `Hir` itself
/// doesn't track a root (it's just an arena), so lowering hands the root id
/// back explicitly rather than requiring callers to know it's always the
/// last-pushed node.
pub fn parse_and_lower(source: &str) -> Result<(Hir, NodeId), CompileError> {
    let (hir, root, _gem_records) =
        parse_and_lower_with(source, &crate::CompileOptions::default())?;
    Ok((hir, root))
}

/// The `# encoding:`/`# coding:` magic comment's value, honored only on the
/// first line (or the second, after a `#!` shebang) exactly as CRuby does --
/// `coding\s*[:=]\s*NAME` inside that comment. `None` when absent.
fn magic_encoding_comment(source: &str) -> Option<String> {
    let mut lines = source.lines();
    let first = lines.next()?;
    let line = if first.starts_with("#!") {
        lines.next()?
    } else {
        first
    };
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

/// `parse_and_lower` plus the file context compile-time `require`
/// resolution needs -- see [`CompileOptions`](crate::CompileOptions). The
/// main file's statements go through `loader::lower_main_file` (which
/// recognizes the require/load call shapes at top-level statement position
/// and splices resolved files into this same arena); the exception prelude
/// and `eval` bodies keep going through `parse_and_lower_into`, where those
/// shapes are rejected by `lower_node` instead.
pub fn parse_and_lower_with(
    source: &str,
    opts: &crate::CompileOptions,
) -> Result<(Hir, NodeId, Vec<crate::gem_report::GemRecord>), CompileError> {
    // The exception-class prefix is the same source for every compile, so it
    // is lowered ONCE into a template arena and cloned in, rather than
    // re-parsed per compile. Sound because the prefix is always the FIRST
    // thing in the arena (node/file ids are identical either way) and its
    // source contains nothing that reads ambient state (`__ENCODING__`).
    //
    // An exception body is BOOTSTRAP -- the runtime installs it, so codegen
    // must not re-emit it and the statement never joins the emitted stream.
    // `builtin_exceptions_len` counts them.
    static BUILTIN_PREFIX: std::sync::LazyLock<
        Result<(Hir, Vec<NodeId>), crate::lower_error::LowerError>,
    > = std::sync::LazyLock::new(|| {
        let mut hir = Hir::default();
        let statements = parse_and_lower_into(&mut hir, BUILTIN_EXCEPTIONS_RB).map_err(|e| {
            crate::lower_error::LowerError {
                message: format!(
                    "internal error in zeo's built-in exception classes (this is a zeo bug): {e}"
                ),
                ..e
            }
        })?;
        Ok((hir, statements))
    });
    let (mut hir, mut statements) = match &*BUILTIN_PREFIX {
        Ok((h, s)) => (h.clone(), s.clone()),
        Err(e) => return Err(CompileError::lower(e.clone(), &[])),
    };
    let exceptions_len = statements.len();
    if let Some(name) = magic_encoding_comment(source) {
        hir.script_encoding = match encoding_const_name(&name) {
            Ok(n) => n.map(str::to_string),
            Err(e) => return Err(CompileError::lower(e, &hir.files)),
        };
    }
    hir.frozen_string_literal = crate::hir::magic_frozen_string_literal(source);
    // Before a single statement lowers: what a compile is FOR decides a
    // handful of folds (see `Hir::cvar_is_toplevel`).
    hir.mode = opts.mode;
    hir.builtin_exceptions_len = exceptions_len;
    // The package options ride the Hir, because neither
    // analyze nor emit sees `CompileOptions` -- see `Hir::pkg_build`.
    hir.pkg_build = opts.package_build.clone();
    for up in &opts.use_packages {
        let m = crate::package::Manifest::parse(&up.manifest_text).map_err(|e| {
            CompileError::analyze(format!("{}: {e}", up.manifest_path.display()))
        })?;
        hir.pkg_merge.push(m);
        hir.pkg_source_roots.push(up.source_root.clone());
    }
    // A package's constants land when its units run -- exactly what
    // `unrun_unit_consts` describes. Seeded BEFORE the main lowering,
    // because the loader's own guard folder decides `defined?(NAME)`
    // branches during splicing, ahead of analyze's fact union.
    for m in &hir.pkg_merge {
        hir.loader
            .unrun_unit_consts
            .extend(m.facts.const_names.iter().cloned());
    }
    // A snippet is its own compile and would otherwise have never heard of an
    // FFI type an earlier one -- or the program -- declared.
    if opts.mode.is_eval() {
        crate::ffi_vocab::seed(&mut hir);
    }
    // This is THE boundary where a located `LowerError` becomes a renderable
    // `CompileError`: the error and the `Hir::files` table it points into
    // are both in scope here and nowhere further out.
    let (main_statements, gem_records) = match loader::lower_main_file(&mut hir, source, opts) {
        Ok(v) => v,
        Err(e) => return Err(CompileError::lower(e, &hir.files)),
    };
    statements.extend(main_statements);
    crate::ffi_vocab::publish(&hir);
    let root = hir.push(HirNode::Program(statements));
    Ok((hir, root, gem_records))
}
