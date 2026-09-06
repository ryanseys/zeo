//! Per-file pre-scans over the prism tree: require/autoload
//! collection, FFI-library body marking, `__END__` data sections,
//! and parse warnings.

use super::*;

/// The feature a `require`/`require_relative` names, or `None` if the argument
/// is not one constant string. Lowers the argument and reads its folded
/// literal, exactly as `lower_require_statement` and `lower_call_general` do,
/// so all three agree on which requires are literal.
pub(super) fn literal_feature(
    result: &ruby_prism::ParseResult,
    hir: &mut Hir,
    call: &ruby_prism::CallNode<'_>,
) -> PResult<Option<String>> {
    let arg_list: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let [arg] = arg_list.as_slice() else {
        return Ok(None);
    };
    // One argument syntactically, but no compile-time text -- and a bare
    // `SplatNode`/`...` doesn't lower as an expression, so it must be
    // answered here (`require(*names)` in an optional-dependency helper).
    if arg.as_splat_node().is_some() || arg.as_forwarding_arguments_node().is_some() {
        return Ok(None);
    }
    let arg_id = lower_node(result, hir, arg)?;
    Ok(crate::lower::eval_splice::literal_string_text(hir, arg_id))
}

/// Collects every receiver-less `require`/`require_relative` call in a file's
/// tree, split by whether LOADING the file runs it. Top-level requires land in
/// `calls` too, but dedup skips the second splice attempt. `load` and
/// receiver-bearing (`box.require`) forms are excluded: they cannot be
/// eager-spliced.
#[derive(Default)]
pub(super) struct RequireCollector<'a> {
    /// Requires to splice where they are written: everything LOADING the
    /// file runs -- top level, a conditional, a `begin`, a class body, a
    /// block.
    pub(super) calls: Vec<ruby_prism::CallNode<'a>>,
    /// A `require_relative` inside a method BODY. Spliced (it names a file of
    /// this same program, which must stay loaded), but at the END of the
    /// file: the method cannot run before the file that defines it has
    /// finished loading, and the target routinely reopens a class this file
    /// is still building -- irb's `ext/eval_history.rb` pushes onto a
    /// `NOPRINTING_IVARS` that `context.rb` assigns below the `def` that
    /// requires it.
    pub(super) lazy: Vec<ruby_prism::CallNode<'a>>,
    /// A plain `require` only a method BODY reaches. These are not spliced, but
    /// their names are still wanted (see `LoaderState::deferred_requires`).
    pub(super) deferred: Vec<ruby_prism::CallNode<'a>>,
    /// `require_relative`s lexically inside a `begin` body whose rescue
    /// catches `LoadError` -- candidates for `Hir::optional_require_sites`
    /// when their target turns out not to exist.
    pub(super) optional_rel: Vec<ruby_prism::CallNode<'a>>,
    /// Requires under a runtime-UNDECIDABLE `if`/`unless`/`case` branch --
    /// CRuby runs these only when the guard passes, so they become gated
    /// feature units rather than eager splices.
    pub(super) conditional: Vec<ruby_prism::CallNode<'a>>,
    /// Requires written inside a `class`/`module` BODY (unguarded, outside
    /// any `def`). CRuby runs these MID-body, so the target's declarations
    /// -- FFI vocabulary above all (libuv's `require 'libuv/ext/types'`
    /// three lines above the `attach_function`s that spend its enums) --
    /// exist before the statements below them. These pre-LOWER ahead of the
    /// file's own statements; their nodes still emit at the file-trailing
    /// position, so runtime order is unchanged.
    pub(super) nested: Vec<ruby_prism::CallNode<'a>>,
    /// Enclosing `class`/`module` bodies. Nonzero puts a require in
    /// `nested`.
    pub(super) class_depth: u32,
    /// Enclosing `def`s. Nonzero means a `require` here is deferred.
    pub(super) defs: u32,
    /// Enclosing `begin` bodies whose rescue catches `LoadError`. Nonzero
    /// means a `require_relative` here is allowed to be missing.
    pub(super) load_error_rescues: u32,
    /// Enclosing branches whose guard `eval_static_guard` could NOT decide.
    /// Nonzero means a `require` here may or may not run.
    pub(super) runtime_cond: u32,
}

/// Whether one of this begin's rescue clauses catches `LoadError`. Named
/// classes only: a BARE `rescue` catches `StandardError`, and `LoadError <
/// ScriptError < Exception` sits outside that tree.
/// Whether any of `stmts`' subtrees holds a `raise` naming `LoadError` (or a
/// superclass) OUTSIDE method bodies -- a raise the require itself would run.
/// power_assert's TracePoint probe is the shape: `begin ... rescue; raise
/// LoadError, '...'; end` at the file's top level. Method and lambda bodies
/// don't run at load, so they don't count.
/// Records which `module` bodies in this file are FFI LIBRARIES, before any
/// file they require from inside one gets to lower.
///
/// `cref` carries the enclosing names, so a nested `module Curl` inside
/// `module Ethon` is marked under `Ethon::Curl` -- the same key
/// `lower::defs` builds through `Hir::cref_path`. A compact path
/// (`module A::B`) contributes both segments, matching how the real lowering
/// walks it.
///
/// Struct classes are deliberately NOT marked here. A struct name means
/// different things in different positions -- by-reference in a signature, the
/// inline layout in a field -- and the per-body alias table takes the first
/// answer it is given. Marking the class before its `layout` lowered seeded
/// `by_value` with the by-reference entry, which is a wrong ABI rather than a
/// missing one.
pub(super) fn mark_ffi_bodies(
    hir: &mut Hir,
    body: &ruby_prism::NodeList<'_>,
    cref: &mut Vec<String>,
) {
    for node in body.iter() {
        let (name, inner, superclass) = if let Some(m) = node.as_module_node() {
            (
                crate::lower::consts::constant_path_name(&m.constant_path()).ok(),
                m.body(),
                None,
            )
        } else if let Some(c) = node.as_class_node() {
            (
                crate::lower::consts::constant_path_name(&c.constant_path()).ok(),
                c.body(),
                c.superclass()
                    .and_then(|s| crate::lower::consts::constant_path_name(&s).ok()),
            )
        } else {
            continue;
        };
        let Some(name) = name else { continue };
        let depth = cref.len();
        cref.extend(
            name.trim_start_matches("::")
                .split("::")
                .map(str::to_string),
        );
        let path = match name.strip_prefix("::") {
            Some(absolute) => absolute.to_string(),
            None => cref.join("::"),
        };
        if let Some(inner) = inner.as_ref().and_then(|b| b.as_statements_node()) {
            if inner
                .body()
                .iter()
                .any(|s| crate::lower::ffi::is_extend_ffi_library(&s))
            {
                hir.mark_ffi_library(&path);
            }
            let union = matches!(superclass.as_deref(), Some("FFI::Union" | "::FFI::Union"));
            let is_struct = union
                || matches!(
                    superclass.as_deref(),
                    Some("FFI::Struct" | "::FFI::Struct" | "FFI::ManagedStruct")
                );
            for stmt in inner.body().iter() {
                match is_struct {
                    true => crate::lower::ffi::prescan_layout(hir, &stmt, &path, union),
                    false => crate::lower::ffi::prescan_declaration(hir, &stmt),
                }
            }
            mark_ffi_bodies(hir, &inner.body(), cref);
        }
        cref.truncate(depth);
    }
}

/// Whether a spliced require's body can raise something the enclosing
/// `begin`'s handlers would catch.
///
/// `caught` is the exception classes the rescue clauses name; empty means a
/// bare `rescue` (or an `ensure`), which any raise reaches.
///
/// The test is syntactic and looks only at raises the spliced statements make
/// THEMSELVES -- a `def` body does not run at load time, so it is skipped.
/// That is deliberately narrow: it keeps the everyday `begin; require "json";
/// rescue LoadError; <fallback>; end` unwrapped when json loads cleanly, so
/// the fallback stays dead and its classes stay out of `defined?`.
pub(super) fn spliced_may_raise(
    hir: &Hir,
    stmts: &[crate::hir::NodeId],
    caught: &[String],
) -> bool {
    use crate::hir::HirNode;
    let names = |n: &str| caught.is_empty() || caught.iter().any(|c| c == n);
    let mentions = |hir: &Hir, id: crate::hir::NodeId| -> bool {
        fn walk(hir: &Hir, id: crate::hir::NodeId, hit: &mut bool, f: &dyn Fn(&str) -> bool) {
            match &hir[id] {
                HirNode::ClassRef(n) | HirNode::QualifiedConstRead(_, n) if f(n.as_str()) => {
                    *hit = true;
                }
                _ => {}
            }
            hir[id].for_each_child(&mut |c| walk(hir, c, hit, f));
        }
        let mut hit = false;
        walk(hir, id, &mut hit, &names);
        hit
    };
    fn walk(
        hir: &Hir,
        id: crate::hir::NodeId,
        mentions: &dyn Fn(&Hir, crate::hir::NodeId) -> bool,
        bare: bool,
    ) -> bool {
        match &hir[id] {
            HirNode::DefMethod { .. } | HirNode::Lambda { .. } => false,
            // `raise` with no argument re-raises `$!` or a RuntimeError, so
            // only a handler that names nothing in particular sees it.
            HirNode::Raise(args, _) => match args.first() {
                Some(&a) => mentions(hir, a),
                None => bare,
            },
            node => {
                let mut hit = false;
                node.for_each_child(&mut |c| hit = hit || walk(hir, c, mentions, bare));
                hit
            }
        }
    }
    stmts
        .iter()
        .any(|&s| walk(hir, s, &mentions, caught.is_empty()))
}

/// The exception classes a `begin`'s rescue clauses name, in order. An empty
/// answer means a bare `rescue` -- which catches `StandardError` -- or an
/// `ensure` with no rescue at all.
pub(super) fn rescued_class_names(node: &ruby_prism::BeginNode<'_>) -> Vec<String> {
    let mut out = Vec::new();
    let mut clause = node.rescue_clause();
    while let Some(rescue) = clause {
        for ex in rescue.exceptions().iter() {
            let name = match (ex.as_constant_read_node(), ex.as_constant_path_node()) {
                (Some(read), _) => Some(read.name().as_slice().to_vec()),
                (None, Some(path)) if path.parent().is_none() => {
                    path.name().map(|n| n.as_slice().to_vec())
                }
                _ => None,
            };
            if let Some(n) = name {
                out.push(String::from_utf8_lossy(&n).into_owned());
            }
        }
        clause = rescue.subsequent();
    }
    out
}

/// Whether a `begin` has ANY handler its body's raise could reach.
///
/// [`rescues_load_error`] answers a narrower question -- whether a failed
/// `require` is OPTIONAL -- and only a rescue naming `LoadError` makes it
/// so. This one decides where a spliced require's BODY lands, and there the
/// exception class does not matter: CRuby runs the required file during the
/// `require` call, which is inside the `begin`, so every handler written
/// around it can see whatever the file raises.
///
/// A bare `rescue` counts (it catches `StandardError`), and so does an
/// `ensure`-only `begin` -- its clause runs on the way out either way.
pub(super) fn rescues_anything(node: &ruby_prism::BeginNode<'_>) -> bool {
    node.rescue_clause().is_some() || node.ensure_clause().is_some()
}

pub(super) fn rescues_load_error(node: &ruby_prism::BeginNode<'_>) -> bool {
    let mut clause = node.rescue_clause();
    while let Some(rescue) = clause {
        for ex in rescue.exceptions().iter() {
            let name = match (ex.as_constant_read_node(), ex.as_constant_path_node()) {
                (Some(read), _) => Some(read.name().as_slice().to_vec()),
                // `::LoadError`: root-anchored, no parent.
                (None, Some(path)) if path.parent().is_none() => {
                    path.name().map(|n| n.as_slice().to_vec())
                }
                _ => None,
            };
            if name.is_some_and(|n| {
                matches!(n.as_slice(), b"LoadError" | b"ScriptError" | b"Exception")
            }) {
                return true;
            }
        }
        clause = rescue.subsequent();
    }
    false
}

impl<'pr> ruby_prism::Visit<'pr> for RequireCollector<'pr> {
    fn visit_branch_node_enter(&mut self, node: ruby_prism::Node<'pr>) {
        if let Some(call) = node.as_call_node()
            && call.receiver().is_none()
            && matches!(call.name().as_slice(), b"require" | b"require_relative")
        {
            if call.name().as_slice() == b"require_relative"
                && self.load_error_rescues > 0
                && let Some(again) = node.as_call_node()
            {
                self.optional_rel.push(again);
            }
            // A method-body `require_relative` under an UNDECIDED guard is
            // conditional twice over -- CRuby loads it only when the method
            // runs AND the guard passes -- so it takes the gated-unit route,
            // never the eager splice (puppet's suidmanager loads
            // `windows/user` this way, and eager splicing compiled the
            // windows-only FFI vocabulary into every build).
            if self.runtime_cond > 0
                && (self.defs == 0 || call.name().as_slice() == b"require_relative")
                && let Some(again) = node.as_call_node()
            {
                self.conditional.push(again);
            }
            if self.defs == 0 {
                if self.class_depth > 0
                    && self.runtime_cond == 0
                    && let Some(again) = node.as_call_node()
                {
                    self.nested.push(again);
                }
                self.calls.push(call);
            } else if call.name().as_slice() == b"require_relative" {
                if self.runtime_cond == 0 {
                    self.lazy.push(call);
                }
            } else {
                self.deferred.push(call);
            }
        }
    }

    // Only the `begin` BODY is protected by its rescues; the rescue, else and
    // ensure clauses run outside that protection and visit at the old depth.
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        let optional = rescues_load_error(node) as u32;
        if let Some(stmts) = node.statements() {
            self.load_error_rescues += optional;
            self.visit(&stmts.as_node());
            self.load_error_rescues -= optional;
        }
        if let Some(r) = node.rescue_clause() {
            self.visit(&r.as_node());
        }
        if let Some(e) = node.else_clause() {
            self.visit(&e.as_node());
        }
        if let Some(en) = node.ensure_clause() {
            self.visit(&en.as_node());
        }
    }

    // A method body does not run at load time, so a `require` there names a
    // lazy LIBRARY: CRuby loads it only when the method is called, and eager
    // splicing gets both the order and the binary size wrong. Descend, but keep
    // those requires out of the splice. A `require_relative` is exempt -- it
    // names a file of this same program, not a library boundary, and it must
    // stay loaded for `def x; require_relative "part"; end` to work at all.
    // `def self.x` and a `class << self` body are the same case: both are
    // `DefNode`s.
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        self.defs += 1;
        ruby_prism::visit_def_node(self, node);
        self.defs -= 1;
    }

    // A `class`/`module` body runs at load time, statement by statement --
    // a require written there must have DECLARED before the statements below
    // it lower. See `RequireCollector::nested`.
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'pr>) {
        self.class_depth += 1;
        ruby_prism::visit_class_node(self, node);
        self.class_depth -= 1;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'pr>) {
        self.class_depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.class_depth -= 1;
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode<'pr>) {
        self.class_depth += 1;
        ruby_prism::visit_singleton_class_node(self, node);
        self.class_depth -= 1;
    }

    // A block BODY runs only when something yields to it, and how many times
    // is a runtime fact. rack's `separate_testing do require_relative
    // "../lib/rack/utils" end` is the shape: the non-SEPARATE definition of
    // that method does not yield, so CRuby never loads the target and the
    // eager splice loaded it anyway -- running rack/constants.rb a second
    // time and warning on all 57 of its constants. Same treatment as an
    // undecided guard: a gated unit, and the call stays live.
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        self.runtime_cond += 1;
        ruby_prism::visit_block_node(self, node);
        self.runtime_cond -= 1;
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.runtime_cond += 1;
        ruby_prism::visit_lambda_node(self, node);
        self.runtime_cond -= 1;
    }

    // A `require` under a statically-false guard (`require 'open3/jruby_windows'
    // if RUBY_ENGINE == 'jruby'`) names another engine's native code (its own
    // `require 'jruby'` cannot resolve/lower) and must NOT be spliced. Unlike
    // the default descent, this prunes a provably-dead branch: only the live
    // branch(es) are visited. The predicate is still visited (a `require` inside
    // the guard EXPRESSION is pathological but stays collected). The guard is
    // only decidable for the platform-detection idioms `eval_static_guard`
    // models; every runtime-conditional require descends exactly as before.
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        self.visit(&node.predicate());
        let guard = eval_static_guard(&node.predicate());
        // An UNDECIDED guard means either branch may or may not run: its
        // requires are collected as conditional so they load only when the
        // guard actually passes, the way CRuby runs them.
        let bump = (guard.is_none()) as u32;
        if guard != Some(false)
            && let Some(stmts) = node.statements()
        {
            self.runtime_cond += bump;
            self.visit(&stmts.as_node());
            self.runtime_cond -= bump;
        }
        if guard != Some(true)
            && let Some(sub) = node.subsequent()
        {
            self.runtime_cond += bump;
            self.visit(&sub);
            self.runtime_cond -= bump;
        }
    }

    // `unless C` runs `statements` when C is FALSE and `else_clause` when TRUE
    // -- the mirror of `visit_if_node`.
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        self.visit(&node.predicate());
        let guard = eval_static_guard(&node.predicate());
        let bump = (guard.is_none()) as u32;
        if guard != Some(true)
            && let Some(stmts) = node.statements()
        {
            self.runtime_cond += bump;
            self.visit(&stmts.as_node());
            self.runtime_cond -= bump;
        }
        if guard != Some(false)
            && let Some(els) = node.else_clause()
        {
            self.runtime_cond += bump;
            self.visit(&els.as_node());
            self.runtime_cond -= bump;
        }
    }

    // `case RUBY_ENGINE when 'jruby'` is the third spelling of the same
    // platform gate (psych requires `psych_jars` under exactly this one), so
    // it prunes the same way the `if` does. Arms are decided in document
    // order, as ruby tests them: a decided-false arm's body is skipped, a
    // decided-true arm ends the walk (later arms and the `else` never run),
    // and any undecidable condition keeps its arm live without killing the
    // arms after it. A subject the build doesn't bake descends exactly as
    // before.
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode<'pr>) {
        let subject = node.predicate().as_ref().and_then(baked_subject);
        let Some(subject) = subject else {
            // An undecided subject: every arm may or may not run.
            self.runtime_cond += 1;
            ruby_prism::visit_case_node(self, node);
            self.runtime_cond -= 1;
            return;
        };
        if let Some(pred) = node.predicate() {
            self.visit(&pred);
        }
        for cond in node.conditions().iter() {
            let Some(when) = cond.as_when_node() else {
                // Not a shape this prunes; fall back to full descent of the
                // remainder by visiting the node itself.
                self.visit(&cond);
                continue;
            };
            // `Some(false)` until a condition matches or declines to answer.
            let mut arm = Some(false);
            for c in when.conditions().iter() {
                self.visit(&c);
                match literal_when_match(subject, &c) {
                    Some(true) => {
                        arm = Some(true);
                        break;
                    }
                    Some(false) => {}
                    None => arm = None,
                }
            }
            match arm {
                Some(false) => continue,
                None => {
                    if let Some(stmts) = when.statements() {
                        self.runtime_cond += 1;
                        self.visit(&stmts.as_node());
                        self.runtime_cond -= 1;
                    }
                }
                Some(true) => {
                    if let Some(stmts) = when.statements() {
                        self.visit(&stmts.as_node());
                    }
                    return;
                }
            }
        }
        if let Some(els) = node.else_clause() {
            self.visit(&els.as_node());
        }
    }
}

/// Collects every receiver-less `autoload` call in a statement tree,
/// descending through the STRUCTURAL containers stdlib nests them in --
/// `module`/`class`/`class << self` bodies (e.g. `module URI; autoload
/// :Generic, "uri/generic"; end`, `module Bundler; class Settings; autoload
/// :Mirror, File.expand_path("mirror", __dir__); end; end`). An `autoload`
/// inside a method/block/conditional body isn't collected here (it's
/// genuinely runtime-dynamic, like a non-top-level `require`); it lowers to a
/// no-op without a splice, so its constant stays undefined -- a loud
/// NameError on reference, not silent, and documented.
pub(super) fn collect_autoloads<'a>(
    node: &ruby_prism::Node<'a>,
    out: &mut Vec<ruby_prism::CallNode<'a>>,
) {
    if let Some(stmts) = node.as_statements_node() {
        for n in stmts.body().iter() {
            collect_autoloads(&n, out);
        }
    } else if let Some(m) = node.as_module_node() {
        if let Some(body) = m.body()
            && readable_cref(&m.constant_path())
        {
            collect_autoloads(&body, out);
        }
    } else if let Some(c) = node.as_class_node() {
        if let Some(body) = c.body()
            && readable_cref(&c.constant_path())
        {
            collect_autoloads(&body, out);
        }
    } else if let Some(sc) = node.as_singleton_class_node() {
        if let Some(body) = sc.body() {
            collect_autoloads(&body, out);
        }
    } else if let Some(call) = node.as_call_node()
        && call.receiver().is_none()
        && call.name().as_slice() == b"autoload"
    {
        out.push(call);
    }
}

/// Whether a `class`/`module` header names a path this pass can read. A body
/// behind an unreadable header is not walked: an autoload found there would
/// hang off an owner this pass cannot name, and skipping it only leaves the
/// runtime row. (The consumer keys on the constant's LEAF alone -- see
/// `autoload_consts` -- so the path's segments themselves are not kept.)
fn readable_cref(path: &ruby_prism::Node<'_>) -> bool {
    crate::lower::consts::constant_path_name(path).is_ok()
}

/// Where the main script's `DATA` starts, if it has an `__END__` marker.
///
/// prism's `data_loc` spans the marker AND the bytes after it, so the offset
/// is past `__END__` plus its line terminator -- CRuby skips at most one `\r`
/// and one `\n` there (`ruby.c:2287`), never more, so a blank line after the
/// marker is DATA's first line.
///
/// A source with no `__END__` answers `None`, which is what leaves `DATA`
/// undefined rather than empty.
pub(super) fn data_section(
    result: &ruby_prism::ParseResult<'_>,
    path: &std::path::Path,
) -> Option<crate::hir::DataSection> {
    const MARKER: usize = "__END__".len();
    let loc = result.data_loc()?;
    let after_marker = &result.as_slice(&loc)[MARKER..];
    let terminator = usize::from(after_marker.starts_with(b"\r")) + 1;
    Some(crate::hir::DataSection {
        // Absolute: the compiled binary can run from anywhere, and this path is
        // reopened at startup.
        path: std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string(),
        offset: (loc.start_offset() + MARKER + terminator) as u64,
    })
}

/// Ruby's own parse-time warnings for one file, as prism reports them --
/// `key :k is duplicated and overwritten on line 24` and friends. Collected
/// here rather than re-derived: prism already knows the rules (which keys
/// count, and which line the surviving one is on), and re-implementing them
/// would be a second source of truth.
pub(super) fn collect_parse_warnings(
    hir: &mut Hir,
    result: &ruby_prism::ParseResult<'_>,
    file: &str,
    source: &str,
) {
    let plain_regexp_conditions = plain_regexp_conditions(result);
    for warning in result.warnings() {
        let message = warning.message();
        if !is_default_level(message, &plain_regexp_conditions, warning.location().start_offset())
        {
            continue;
        }
        let upto = warning.location().start_offset().min(source.len());
        let line = 1 + source.as_bytes()[..upto]
            .iter()
            .filter(|&&b| b == b'\n')
            .count() as u32;
        hir.warnings.push(crate::diagnostics::CompileWarning {
            file: file.to_string(),
            line,
            message: message.to_string(),
        });
    }
}

/// prism reports its parse warnings at two levels: `default` (what plain
/// `ruby foo.rb` prints) and `verbose` (what only `ruby -w` prints). Its Rust
/// binding exposes a warning's message and location but NOT its level, and
/// zeo has no `-w` to justify the verbose tier -- so the shapes worth
/// forwarding are named here explicitly.
///
/// A message is safe to forward only when NO verbose-level diagnostic renders
/// the same text. `literal in condition` is the shape that fails that test:
/// prism's default and verbose rows share the format `%sliteral in %s`, so a
/// message-keyed filter cannot tell them apart, and forwarding a verbose-only
/// warning by mistake is worse than forwarding none. Reading the real level
/// would need the binding to expose `pm_diagnostic_t::level`.
///
/// The two shapes below have no such twin: `equal_in_conditional` (`= literal`
/// in a conditional -- the classic `if x = 1` typo) is spelled two ways, one
/// per parser version, and both rows are default-level.
fn is_default_level(message: &str, plain_regexps: &[usize], offset: usize) -> bool {
    message.starts_with("key ") && message.contains(" is duplicated and overwritten on line ")
        || message.ends_with("literal' in conditional, should be ==")
        // `regex literal in condition` is default-level for a plain regexp
        // and verbose-level for an interpolated one, and both render the
        // same sentence. The node kind at the warning's own offset is what
        // separates them.
        || (message == "regex literal in condition" && plain_regexps.contains(&offset))
}

/// Where a PLAIN (non-interpolated) regexp stands as a condition -- prism
/// rewrites each into a `MatchLastLineNode`, and only that shape warns at
/// default level.
fn plain_regexp_conditions(result: &ruby_prism::ParseResult<'_>) -> Vec<usize> {
    use ruby_prism::Visit;

    struct Collect(Vec<usize>);
    impl<'pr> Visit<'pr> for Collect {
        fn visit_match_last_line_node(&mut self, node: &ruby_prism::MatchLastLineNode<'pr>) {
            self.0.push(node.location().start_offset());
        }
    }
    let mut collect = Collect(Vec::new());
    collect.visit(&result.node());
    collect.0
}

#[cfg(test)]
mod guard_tests {
    use super::*;
    use ruby_prism::Visit;

    /// The static answer for `if <src> ...`'s predicate.
    fn guard(pred: &str) -> Option<bool> {
        let src = format!("if {pred}\n  1\nend\n");
        let res = ruby_prism::parse(src.as_bytes());
        let root = res.node();
        let prog = root.as_program_node().unwrap();
        let first = prog.statements().body().iter().next().unwrap();
        eval_static_guard(&first.as_if_node().unwrap().predicate())
    }

    /// The features the collector would SPLICE from `src` (load-time
    /// requires; deferred/lazy are not the question here).
    fn spliced(src: &str) -> Vec<String> {
        let res = ruby_prism::parse(src.as_bytes());
        let mut c = RequireCollector::default();
        c.visit(&res.node());
        c.calls
            .iter()
            .filter_map(|call| {
                let args = call.arguments()?;
                let first = args.arguments().iter().next()?;
                Some(String::from_utf8_lossy(first.as_string_node()?.unescaped()).into_owned())
            })
            .collect()
    }

    /// Every spelling of "am I another engine" answers `false` at build time,
    /// and the windows family answers whatever this build is.
    #[test]
    fn the_platform_guards_fold_to_build_facts() {
        assert_eq!(guard("RUBY_ENGINE == 'jruby'"), Some(false));
        assert_eq!(guard("RUBY_PLATFORM == 'java'"), Some(false));
        assert_eq!(guard("'java' == RUBY_PLATFORM"), Some(false));
        assert_eq!(guard("defined?(JRUBY_VERSION)"), Some(false));
        assert_eq!(guard("defined?(RUBINIUS_VERSION)"), Some(false));
        assert_eq!(
            guard("Gem.win_platform?"),
            Some(static_guards::build_is_windows())
        );
        assert_eq!(
            guard("FFI::Platform.windows?"),
            Some(static_guards::build_is_windows())
        );
        assert_eq!(
            guard("FFI::Platform.unix?"),
            Some(!static_guards::build_is_windows())
        );
        assert_eq!(
            guard("RbConfig::CONFIG['host_os'] =~ /mswin|mingw/"),
            Some(static_guards::build_is_windows())
        );
        // Runtime state stays three-valued.
        assert_eq!(guard("ENV['FAST']"), None);
        assert_eq!(guard("defined?(SomeGemConstant)"), None);
    }

    /// psych's shape: the JRuby arm of a `case RUBY_ENGINE` must not splice
    /// -- its target is another engine's native code.
    #[test]
    fn a_case_on_a_baked_subject_prunes_its_dead_arms() {
        let live = spliced(
            "case RUBY_ENGINE\n\
             when 'jruby' then require 'psych_jars'\n\
             when 'ruby' then require 'psych_native'\n\
             else require 'psych_fallback'\n\
             end\n",
        );
        assert_eq!(live, vec!["psych_native"]);

        // No arm matches: only the `else` runs.
        let fallback = spliced(
            "case RUBY_PLATFORM\n\
             when /java/ then require 'a'\n\
             else require 'b'\n\
             end\n",
        );
        assert_eq!(fallback, vec!["b"]);
    }

    /// A subject the build does not bake descends exactly as before, and an
    /// undecidable arm keeps the arms after it live.
    #[test]
    fn an_undecided_case_keeps_every_arm_live() {
        let all = spliced(
            "case adapter\n\
             when 'jruby' then require 'a'\n\
             else require 'b'\n\
             end\n",
        );
        assert_eq!(all, vec!["a", "b"]);

        let mixed = spliced(
            "case RUBY_ENGINE\n\
             when computed then require 'a'\n\
             when 'ruby' then require 'b'\n\
             end\n",
        );
        assert_eq!(mixed, vec!["a", "b"]);
    }
}
