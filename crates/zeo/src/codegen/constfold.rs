//! Compile-time constant/condition folding shared across codegen.
//!
//! Two concerns live here so they can be reused and unit-tested in one place:
//!
//! * **Constant resolvability** ([`class_const_in`], [`value_const_defined_in`],
//!   [`const_form_resolves`]) -- whether a `Scope::NAME`/bare-`NAME` reference
//!   names something that provably exists at compile time. Drives both
//!   `const_get`/`const_defined?` folding (`codegen::call`) and `defined?`
//!   classification (`codegen::expr`).
//! * **Static condition truthiness** ([`static_cond`]) -- when an `if`/`unless`/
//!   ternary guard can be decided at compile time. The one case that resolves
//!   is a `defined?(Const)` naming a constant that provably doesn't exist ->
//!   statically `false`. CRuby folds such a guarded branch away entirely; its
//!   body may be MRI-only/uncompilable code (`if defined?(RubyVM::YJIT);
//!   RubyVM::YJIT.enable; end`) that must never reach Rust emission.

use super::Ctx;
use crate::compiler::{ClassId, OBJECT_CLASS};
use crate::hir::{HirNode, NodeId};

/// The class/module `target::cname` names, if any -- a nested definition in
/// `target`'s own namespace, or (const lookup inherits) a top-level class,
/// which lives on `Object` and so is visible from every receiver.
pub(super) fn class_const_in(cx: &Ctx, target: ClassId, cname: &str) -> Option<ClassId> {
    if let Some(c) = cx.resolve_class(&format!("{}::{cname}", cx.compiler.fq_name(target))) {
        return Some(c);
    }
    cx.compiler.resolve_class(cname, &[], cx.box_id)
}

/// Whether a VALUE constant named `cname` is defined on `target` or any
/// ancestor (constant lookup inherits, up through `Object`) -- read from the
/// compile-time `const_owners` registry `resolve_consts` populates.
pub(super) fn value_const_defined_in(cx: &Ctx, target: ClassId, cname: &str) -> bool {
    let mut chain = cx.compiler.class(target).ancestors.clone();
    if !chain.contains(&OBJECT_CLASS) {
        chain.push(OBJECT_CLASS);
    }
    // An ACTUAL `NAME = ...` in the body, not merely a `const_owners` entry:
    // that map records where a name WOULD resolve, and a bare reference to a
    // name nothing defines still gets one (`uri/common.rb` calls `Parser.new`
    // in the very method whose `defined?(::URI::Parser)` guard must answer nil
    // until its own `const_set` runs).
    chain
        .iter()
        .any(|&anc| crate::analyze::mro::directly_defines_const(cx.compiler, anc, cname))
}

/// Whether a constant-reference HIR node (the operand of a `defined?`) provably
/// resolves at compile time. `Some(true)` = names a live class or value
/// constant; `Some(false)` = provably absent; `None` = undecidable here, so
/// the caller must ask at runtime (or, for a non-constant node, make no claim).
///
/// A `Scope::NAME` whose scope exists but whose name isn't statically there is
/// `None`, never `Some(false)`: `const_set` can add it later, and folding the
/// guard away would run the very branch it protects. A BARE name stays
/// decidable, because that is the version/feature-gate idiom (`defined?(Ractor)`)
/// whose whole value is dropping unreachable code at compile time.
pub(super) fn const_form_resolves(cx: &Ctx, id: NodeId) -> Option<bool> {
    match &cx.compiler.hir[id] {
        // A bare name is a class OR a value constant. Only the first was
        // consulted, so `ASSIGNED = 7; defined?(ASSIGNED)` answered nil where
        // ruby answers "constant" -- `resolve_class` has nothing to say about a
        // constant that doesn't name a class. Resolved through the lexical
        // chain and then `Object`, which is where a top-level one lands.
        HirNode::ClassRef(name) => {
            if cx.resolve_class(name).is_some() {
                return Some(true);
            }
            let mut scopes = cx.cref_chain().to_vec();
            scopes.push(OBJECT_CLASS);
            Some(
                scopes
                    .iter()
                    .rev()
                    .any(|&s| value_const_defined_in(cx, s, name)),
            )
        }
        HirNode::QualifiedConstRead(scope, name) => {
            let Some(scope_id) = cx.resolve_class(scope) else {
                return Some(false);
            };
            let known = class_const_in(cx, scope_id, name).is_some()
                || value_const_defined_in(cx, scope_id, name);
            known.then_some(true)
        }
        _ => None,
    }
}

/// Compile-time truthiness of an `if`/`unless`/ternary condition, when it can
/// be decided at compile time; `None` otherwise (emit the condition and both
/// branches normally). The resolvable cases:
///
/// * `defined?(Const)` -> `Some(const_form_resolves(..))` for a constant form.
/// * `a && b` -> folds through short-circuit: a statically-false left is
///   `Some(false)` (the right is never reached, so its potentially-unresolvable
///   operand is dropped); a statically-true left defers to the right.
///
/// Only used to pick which branch to EMIT, so truthiness is all that matters
/// (the `&&` value identity CRuby would compute is irrelevant here). The left
/// operand of a folded `&&` is always a pure `defined?` guard, so dropping the
/// unreached right operand never elides a side effect.
pub(super) fn static_cond(cx: &Ctx, id: NodeId) -> Option<bool> {
    // A build-time target-constant guard -- a version gate (`Gem.rubygems_version
    // < Gem::Version.new("3.5.22")`, `RUBY_VERSION < "3.0"`) or a feature probe
    // (`unless VALIDATES_FOR_RESOLUTION`) -- folds against zeo's fixed version /
    // method tables. Resolved in the emit site's lexical cref -- see
    // `crate::guard_fold`.
    if let Some(b) = crate::guard_fold::static_cond(cx.compiler, cx.cref_chain(), cx.box_id, id) {
        return Some(b);
    }
    match &cx.compiler.hir[id] {
        HirNode::Defined(inner) => const_form_resolves(cx, *inner),
        HirNode::And(l, r) => match static_cond(cx, *l) {
            Some(false) => Some(false),
            Some(true) => static_cond(cx, *r),
            None => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::{Analyzed, analyze};
    use crate::compiler::Compiler;
    use proc_macro2::Ident;
    use quote::format_ident;
    use std::borrow::Cow;
    use std::cell::Cell;
    use std::collections::{HashMap, HashSet};

    /// A compiled-to-`Analyzed` program that can hand out a root-scope [`Ctx`]
    /// borrowing its owned state, for exercising the fold helpers against the
    /// same class/constant registry real codegen sees.
    struct Fixture {
        analyzed: Analyzed,
        labels: Cell<u32>,
        empty_captures: HashSet<String>,
        empty_locals: HashMap<String, crate::types::TyKind>,
        self_ident: Ident,
    }

    fn fixture(source: &str) -> Fixture {
        let (hir, root) = crate::parse::parse_and_lower(source).expect("parse");
        Fixture {
            analyzed: analyze(hir, root).expect("analyze"),
            labels: Cell::new(0),
            empty_captures: HashSet::new(),
            empty_locals: HashMap::new(),
            self_ident: format_ident!("self"),
        }
    }

    impl Fixture {
        fn compiler(&self) -> &Compiler {
            &self.analyzed.compiler
        }

        fn ctx(&self) -> Ctx<'_> {
            Ctx {
                compiler: &self.analyzed.compiler,
                box_id: 0,
                current_class: None,
                defining_class: None,
                class_self: None,
                current_method: None,
                local_types: Cow::Borrowed(&self.empty_locals),
                label_counter: &self.labels,
                loop_labels: None,
                for_var_override: None,
                captured_locals: Cow::Borrowed(&self.empty_captures),
                self_ident: self.self_ident.clone(),
                in_real_proc: false,
                self_is_dynamic: false,
                runtime_super_params: None,
                block_depth: 0,
                has_blk_binding: false,
            }
        }

        /// The condition `NodeId` of the first top-level `if` statement.
        fn first_if_cond(&self) -> NodeId {
            let hir = &self.compiler().hir;
            for &id in &self.analyzed.main_statements {
                if let HirNode::If { cond, .. } = &hir[id] {
                    return *cond;
                }
            }
            panic!("no top-level `if` in fixture program");
        }
    }

    #[test]
    fn a_defined_guard_over_a_missing_constant_is_statically_false() {
        let fx = fixture("if defined?(NoSuchConst)\n  1\nend\n");
        assert_eq!(static_cond(&fx.ctx(), fx.first_if_cond()), Some(false));
    }

    #[test]
    fn a_defined_guard_over_a_live_class_is_statically_true() {
        let fx = fixture("if defined?(String)\n  1\nend\n");
        assert_eq!(static_cond(&fx.ctx(), fx.first_if_cond()), Some(true));
    }

    #[test]
    fn a_defined_and_short_circuits_on_the_missing_left() {
        let fx = fixture("if defined?(Nope) && Nope.on?\n  1\nend\n");
        assert_eq!(static_cond(&fx.ctx(), fx.first_if_cond()), Some(false));
    }

    #[test]
    fn a_missing_qualified_scope_is_statically_false() {
        let fx = fixture("if defined?(MissingRoot::Sub::Thing)\n  1\nend\n");
        assert_eq!(static_cond(&fx.ctx(), fx.first_if_cond()), Some(false));
    }

    #[test]
    fn a_non_defined_condition_does_not_fold() {
        let fx = fixture("x = 1\nif x\n  1\nend\n");
        assert_eq!(static_cond(&fx.ctx(), fx.first_if_cond()), None);
    }
}
