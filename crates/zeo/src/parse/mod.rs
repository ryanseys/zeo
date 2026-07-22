//! The compiler's front-end DRIVER: the top-level parse -> lower entry
//! points plus everything `require` resolution needs (the loader, gemspecs,
//! lockfiles, the external gem store). The lowering itself -- prism tree ->
//! typed HIR -- lives in the `zeo-hir` crate (`zeo_hir::lower`); this module
//! is the filesystem-touching layer that drives it file-by-file, feeding
//! per-file state through `zeo_hir::lower::context`.

pub mod gem_compat;
mod gem_store;
mod gemspec;
mod loader;
mod lockfile;

use crate::diagnostics::CompileError;
use crate::hir::{Hir, HirNode, NodeId};
use zeo_hir::lower::{encoding_const_name, parse_and_lower_into};

/// The built-in exception hierarchy (originally a minimal "raise/exception
/// foundation", extended to zeo's own ~20-class set) --
/// ordinary Ruby source, spliced into EVERY compiled program ahead of the
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
pub fn parse_and_lower(source: &str) -> Result<(Hir, NodeId), CompileError> {
    let (hir, root, _gem_records) = parse_and_lower_with(source, None, &[], &[], None, None)?;
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

/// `parse_and_lower` plus the file context compile-time
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
) -> Result<(Hir, NodeId, Vec<crate::gem_report::GemRecord>), CompileError> {
    let mut hir = Hir::default();
    if let Some(name) = magic_encoding_comment(source) {
        hir.script_encoding = match encoding_const_name(&name) {
            Ok(n) => n.map(str::to_string),
            Err(e) => return Err(CompileError::lower(e, &hir.files)),
        };
    }
    hir.frozen_string_literal = magic_frozen_string_literal(source);
    let mut statements = match parse_and_lower_into(&mut hir, BUILTIN_EXCEPTIONS_RB) {
        Ok(stmts) => stmts,
        Err(e) => {
            return Err(CompileError::lower(
                zeo_hir::lower_error::LowerError {
                    message: format!(
                        "internal error in zeo's built-in exception classes (this is a zeo bug): {e}"
                    ),
                    ..e
                },
                &hir.files,
            ));
        }
    };
    hir.builtin_exceptions_len = statements.len();
    // This is THE boundary where a located `LowerError` becomes a renderable
    // `CompileError`: the error and the `Hir::files` table it points into
    // are both in scope here and nowhere further out.
    let (main_statements, gem_records) = match loader::lower_main_file(
        &mut hir,
        source,
        input_path,
        load_roots,
        package_dirs,
        gem_path,
        lockfile,
    ) {
        Ok(v) => v,
        Err(e) => return Err(CompileError::lower(e, &hir.files)),
    };
    statements.extend(main_statements);
    let root = hir.push(HirNode::Program(statements));
    Ok((hir, root, gem_records))
}
