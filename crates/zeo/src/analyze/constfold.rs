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

use crate::compiler::{ClassId, Compiler, OBJECT_CLASS};
use crate::hir::{HirNode, NodeId};

/// The lexical environment a constant-resolution question needs -- the
/// backend-neutral projection of an emitter context: which compiler, which
/// defining class (for the cref chain), which box. Everything here is
/// derivable at analyze time; `codegen::Ctx::const_env` builds one.
#[derive(Clone, Copy)]
pub(crate) struct ConstEnv<'a> {
    pub(crate) compiler: &'a Compiler,
    pub(crate) defining_class: Option<ClassId>,
    pub(crate) box_id: u32,
}

impl<'a> ConstEnv<'a> {
    /// The lexical scope chain enclosing the current code, outermost first --
    /// `Compiler::cref_of`'s rule (see `codegen::Ctx::cref_chain`, whose twin
    /// this is). Empty at the top level.
    pub(crate) fn cref_chain(&self) -> &'a [ClassId] {
        self.defining_class
            .map(|c| self.compiler.cref_of_ref(c))
            .unwrap_or(&[])
    }

    fn resolve_class(&self, name: &str) -> Option<ClassId> {
        self.compiler
            .resolve_class(name, self.cref_chain(), self.box_id)
    }
}

/// Whether a search may answer with a constant `Object` itself owns -- ruby's
/// `exclude` flag (`variable.c`'s `rb_const_search`). `Object` is an ancestor
/// of every class, so a search that reads its table from a `Foo` receiver makes
/// every top-level constant answer as `Foo`'s own.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectReach {
    /// `const_get`/`const_defined?`, and a bare name (whose lookup ENDS at the
    /// top level).
    Included,
    /// The `::` operator, and the `defined?` that gates it. A scope that IS
    /// `Object` still reads `Object`'s constants -- it excludes the ancestor,
    /// not the receiver.
    Excluded,
}

/// The class/module `target::cname` names, if any -- a nested definition in
/// `target`'s own namespace, or, where the search reaches `Object`, a top-level
/// class, which lives there and so is visible from every receiver.
pub(crate) fn class_const_in(
    env: &ConstEnv,
    target: ClassId,
    cname: &str,
    reach: ObjectReach,
) -> Option<ClassId> {
    if let Some(c) = env.resolve_class(&format!("{}::{cname}", env.compiler.fq_name(target))) {
        return Some(c);
    }
    if reach == ObjectReach::Excluded && target != OBJECT_CLASS {
        return None;
    }
    env.compiler.resolve_class(cname, &[], env.box_id)
}

/// Whether a VALUE constant named `cname` is defined on `target` or any
/// ancestor -- read from the compile-time `const_owners` registry
/// `resolve_consts` populates. `reach` decides whether `Object`'s own
/// constants count, which is what tells `Foo::BAR` apart from
/// `Foo.const_get(:BAR)`.
pub(crate) fn value_const_defined_in(
    env: &ConstEnv,
    target: ClassId,
    cname: &str,
    reach: ObjectReach,
) -> bool {
    let mut chain = env.compiler.class(target).ancestors.clone();
    if reach == ObjectReach::Excluded && target != OBJECT_CLASS {
        chain.retain(|&anc| anc != OBJECT_CLASS);
    } else if !chain.contains(&OBJECT_CLASS) {
        chain.push(OBJECT_CLASS);
    }
    // An ACTUAL `NAME = ...` in the body, not merely a `const_owners` entry:
    // that map records where a name WOULD resolve, and a bare reference to a
    // name nothing defines still gets one (`uri/common.rb` calls `Parser.new`
    // in the very method whose `defined?(::URI::Parser)` guard must answer nil
    // until its own `const_set` runs).
    chain
        .iter()
        .any(|&anc| crate::analyze::mro::directly_defines_const(env.compiler, anc, cname))
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
pub(crate) fn const_form_resolves(env: &ConstEnv, id: NodeId) -> Option<bool> {
    match &env.compiler.hir[id] {
        // A bare name is a class OR a value constant. Only the first was
        // consulted, so `ASSIGNED = 7; defined?(ASSIGNED)` answered nil where
        // ruby answers "constant" -- `resolve_class` has nothing to say about a
        // constant that doesn't name a class. Resolved through the lexical
        // chain and then `Object`, which is where a top-level one lands.
        HirNode::ClassRef(name) => {
            // A path spelled as one name (`M::Hidden`) is still an explicit
            // scope, so the private-constant rule applies to it too.
            let path = crate::constpath::ConstPath::parse(name);
            if let Some(scope) = path.scope()
                && let Some(sid) = env.resolve_class(scope)
                && env
                    .compiler
                    .class(sid)
                    .private_constants
                    .contains(path.base())
            {
                return Some(false);
            }
            let mut scopes = env.cref_chain().to_vec();
            scopes.push(OBJECT_CLASS);
            if defined_only_later(env, id, &scopes, name) {
                return Some(false);
            }
            if let Some(cid) = env.resolve_class(name) {
                // Registered but not PROMISED -- whether a runtime-conditional
                // class's constant exists is a runtime fact, foldable in
                // neither direction.
                if env.compiler.class(cid).runtime_conditional {
                    return None;
                }
                return Some(true);
            }
            // `DATA` is a genuine top-level constant for a script with an
            // `__END__`, but nothing in the program body assigns it -- it is
            // installed at startup (`zeo_rt::install_data_section`), so the
            // body scan below cannot see it.
            if name == "DATA" && env.compiler.hir.data_section.is_some() {
                return Some(true);
            }
            // `RUBY_ENGINE`, `ENV`, `STDOUT` -- installed on `Object` at
            // startup for the same reason `DATA` is, and invisible to the same
            // scan. Only the bare spelling reaches this arm; `Object::ENV`
            // resolves its scope and asks the run time, which already answers.
            if zeo_abi::SEEDED_OBJECT_CONSTANTS.contains(&name.as_str()) {
                return Some(true);
            }
            Some(
                scopes
                    .iter()
                    .rev()
                    .any(|&s| value_const_defined_in(env, s, name, ObjectReach::Included)),
            )
        }
        HirNode::QualifiedConstRead(scope, name) => {
            let Some(scope_id) = env.resolve_class(scope) else {
                return Some(false);
            };
            // A runtime-conditional SCOPE makes the whole path a runtime
            // question.
            if env.compiler.class(scope_id).runtime_conditional {
                return None;
            }
            // `defined?(M::S)` is nil for a private constant -- the same
            // rejection of the scope operator the read itself gets.
            if env
                .compiler
                .class(scope_id)
                .private_constants
                .contains(name)
            {
                return Some(false);
            }
            if defined_only_later(env, id, &[scope_id], name) {
                return Some(false);
            }
            // The scope OPERATOR, so a top-level constant does not answer:
            // `defined?(K::TOP)` is nil even where `TOP` is set.
            let nested = class_const_in(env, scope_id, name, ObjectReach::Excluded);
            if nested.is_some_and(|c| env.compiler.class(c).runtime_conditional) {
                return None;
            }
            let known = nested.is_some()
                || value_const_defined_in(env, scope_id, name, ObjectReach::Excluded);
            known.then_some(true)
        }
        _ => None,
    }
}

/// Whether every definition of `name` the document-order walk could position
/// runs LATER than the reference at `at`. That is the shape a whole-program
/// view gets wrong: a compat shim guarding on a constant its own file defines
/// further down reads "constant" here and nil under ruby, because ruby only
/// knows a constant once its `class` statement or assignment has executed.
///
/// Conservative in both directions, which is what keeps it a pure narrowing:
/// a reference with no position (it sits in a `def` body, which runs at call
/// time, or in a block), or a name with no positioned definition at all, keeps
/// whatever whole-program answer the caller already had.
fn defined_only_later(env: &ConstEnv, at: NodeId, scopes: &[ClassId], name: &str) -> bool {
    let mut later = false;
    for &s in scopes {
        match env.compiler.const_defined_before(s, name, at) {
            Some(true) => return false,
            Some(false) => later = true,
            None => {}
        }
    }
    later
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
pub(crate) fn static_cond(env: &ConstEnv, id: NodeId) -> Option<bool> {
    // A build-time target-constant guard -- a version gate (`Gem.rubygems_version
    // < Gem::Version.new("3.5.22")`, `RUBY_VERSION < "3.0"`) or a feature probe
    // (`unless VALIDATES_FOR_RESOLUTION`) -- folds against zeo's fixed version /
    // method tables. Resolved in the emit site's lexical cref -- see
    // `crate::guard_fold`.
    if let Some(b) = crate::guard_fold::static_cond(env.compiler, env.cref_chain(), env.box_id, id)
    {
        return Some(b);
    }
    match &env.compiler.hir[id] {
        HirNode::Defined(inner) => const_form_resolves(env, *inner),
        HirNode::And(l, r) => match static_cond(env, *l) {
            Some(false) => Some(false),
            Some(true) => static_cond(env, *r),
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

    /// A compiled-to-`Analyzed` program that can hand out a root-scope
    /// [`ConstEnv`] borrowing its owned state, for exercising the fold
    /// helpers against the same class/constant registry real codegen sees.
    struct Fixture {
        analyzed: Analyzed,
    }

    fn fixture(source: &str) -> Fixture {
        let (hir, root) = crate::parse::parse_and_lower(source).expect("parse");
        Fixture {
            analyzed: analyze(hir, root).expect("analyze"),
        }
    }

    impl Fixture {
        fn compiler(&self) -> &Compiler {
            &self.analyzed.compiler
        }

        fn env(&self) -> ConstEnv<'_> {
            ConstEnv {
                compiler: &self.analyzed.compiler,
                defining_class: None,
                box_id: 0,
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
        assert_eq!(static_cond(&fx.env(), fx.first_if_cond()), Some(false));
    }

    #[test]
    fn a_defined_guard_over_a_live_class_is_statically_true() {
        let fx = fixture("if defined?(String)\n  1\nend\n");
        assert_eq!(static_cond(&fx.env(), fx.first_if_cond()), Some(true));
    }

    #[test]
    fn a_defined_and_short_circuits_on_the_missing_left() {
        let fx = fixture("if defined?(Nope) && Nope.on?\n  1\nend\n");
        assert_eq!(static_cond(&fx.env(), fx.first_if_cond()), Some(false));
    }

    #[test]
    fn a_missing_qualified_scope_is_statically_false() {
        let fx = fixture("if defined?(MissingRoot::Sub::Thing)\n  1\nend\n");
        assert_eq!(static_cond(&fx.env(), fx.first_if_cond()), Some(false));
    }

    #[test]
    fn a_non_defined_condition_does_not_fold() {
        let fx = fixture("x = 1\nif x\n  1\nend\n");
        assert_eq!(static_cond(&fx.env(), fx.first_if_cond()), None);
    }

    /// A build-time question a gem gave a NAME is still a build-time question.
    /// `RUBY_ENGINE` is `"ruby"` for every target zeo builds, so each of these
    /// is a decided `false`.
    #[test]
    fn a_named_zero_arg_predicate_folds_through_its_body() {
        for module in [
            // The memoized spelling, verbatim from sass' `Sass::Util.rbx?`.
            "module E\n  extend self\n  def rbx?\n    return @rbx if defined?(@rbx)\n    \
             @rbx = RUBY_ENGINE == \"rbx\"\n  end\nend\n",
            // The same, as a class method -- lutaml-model's `.opal?`.
            "module E\n  def self.rbx?\n    return @rbx if defined?(@rbx)\n    \
             @rbx = RUBY_ENGINE == \"rbx\"\n  end\nend\n",
            // No memo at all.
            "module E\n  def self.rbx? = RUBY_ENGINE == \"rbx\"\nend\n",
        ] {
            let fx = fixture(&format!("{module}if E.rbx?\n  1\nend\n"));
            assert_eq!(
                static_cond(&fx.env(), fx.first_if_cond()),
                Some(false),
                "{module}"
            );
        }
    }

    /// The fold reads a predicate's body, so a body it cannot decide leaves the
    /// guard alone -- and so does one that takes an argument, whose answer is
    /// the caller's rather than the target's.
    #[test]
    fn a_predicate_zeo_cannot_decide_leaves_its_guard_alone() {
        for (module, guard) in [
            (
                "module E\n  def self.on? = ENV[\"X\"] == \"1\"\nend\n",
                "E.on?",
            ),
            // An argument the caller chose is not a build-time fact.
            ("module E\n  def self.on?(flag) = flag\nend\n", "E.on?(1)"),
            // Two definitions that disagree: which one a call reaches is
            // exactly what this fold declines to model.
            (
                "module E\n  extend self\n  def on? = RUBY_ENGINE == \"ruby\"\n  \
                 def self.on? = RUBY_ENGINE == \"rbx\"\nend\n",
                "E.on?",
            ),
        ] {
            let fx = fixture(&format!("{module}if {guard}\n  1\nend\n"));
            assert_eq!(static_cond(&fx.env(), fx.first_if_cond()), None, "{module}");
        }
    }
}
