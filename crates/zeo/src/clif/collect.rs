//! Collection passes over the analyzed program: the eligible top-level
//! defs, the class-body markers with their inline guards and tails, and
//! the builtin-reopen flags.

use super::module::Emitter;
use crate::analyze::Analyzed;
use crate::codegen_error::{CResult, CodegenError};
use cranelift_module::{FuncId, Linkage, Module};

/// One eligible top-level `def`'s facts (from `Compiler.classes[0]` --
/// analyze hoists method scopes out of `main_statements`).
pub(crate) struct DefSpec {
    pub name: String,
    pub(super) hir_params: crate::hir::Params,
    pub(super) ruby2_keywords: bool,
    pub(super) body: Vec<crate::hir::NodeId>,
    pub(super) visibility: crate::hir::Visibility,
    pub(super) node: Option<crate::hir::NodeId>,
    pub(super) has_blk: bool,
    pub(super) alias_of: Option<String>,
    /// Where the `def` was WRITTEN. Object's table holds the rows a module
    /// materialized onto it, so a `def require` in `module Kernel` arrives
    /// here -- and it needs Kernel's reopen flag, not none at all.
    pub(super) defining_class: crate::compiler::ClassId,
}

/// Collect and DECLARE every top-level `def` the backend can compile
/// (unconditional; no aliases or accessors). Prelude-native rows are the
/// runtime's own, never emitted.
pub(super) fn collect_methods(em: &mut Emitter, analyzed: &Analyzed) -> CResult<Vec<DefSpec>> {
    let compiler = &analyzed.compiler;
    let mut out = Vec::new();
    for entry in &compiler.classes[0].methods {
        let scope = compiler.scope(entry.def);
        if scope.native_default {
            continue;
        }
        // A package-compiled spine body (a top-level def or a `module
        // Kernel` reopen in a merged package) has no arena body here; the
        // merged value-channel row answers, and a receiverless call site
        // takes the dynamic path by not finding an `em.methods` entry.
        if scope.extern_symbol.is_some() {
            continue;
        }
        let name = compiler.names.str(entry.name).to_string();
        let span = scope.def_node.and_then(|n| compiler.hir.span(n));
        let refuse = |what: &str| {
            Err(CodegenError::unsupported(
                format!("the CLIF backend cannot lower {what} yet"),
                span,
            ))
        };
        if scope.runtime_conditional {
            return refuse("a conditionally-defined method");
        }
        let params = &scope.params;
        if let Err(what) = super::emit::check_params(params) {
            return refuse(what);
        }
        let layout = super::params::layout_of(params)?;
        let has_blk = scope.needs_block_param();
        let body_sig = super::params::body_sig(em, layout.n_slots, has_blk);
        let body_id = em
            .module
            .declare_function(
                &em.pkg_symbol(super::names::method_symbol("Object", &name)),
                Linkage::Local,
                &body_sig,
            )
            .map_err(|e| CodegenError::internal(format!("declaring {name}: {e}")))?;
        let tramp_sig = super::params::value_fn_sig(em);
        let tramp_id = em
            .module
            .declare_function(
                &em.pkg_symbol(super::names::trampoline_symbol("Object", &name)),
                Linkage::Local,
                &tramp_sig,
            )
            .map_err(|e| CodegenError::internal(format!("declaring {name}'s trampoline: {e}")))?;
        // `collect_reopen_flags` ran first, so the answer is already known:
        // this row REPLACES a builtin body and its guard lives in the
        // trampoline, which a direct call would jump straight past.
        let reopen_flagged = em
            .reopen_flags
            .contains_key(&(scope.defining_class.0, name.clone()));
        em.methods.insert(
            name.clone(),
            super::module::MethodDecl {
                body: body_id,
                tramp: tramp_id,
                arity: params.required.len(),
                plain: layout.plain,
                kw_direct: layout.kw_direct.clone(),
                has_blk,
                reopen_flagged,
                concealed: scope.unit.is_some(),
            },
        );
        out.push(DefSpec {
            name,
            hir_params: params.clone(),
            ruby2_keywords: scope.ruby2_keywords,
            body: scope.body.clone(),
            visibility: scope.visibility,
            node: scope.def_node,
            has_blk,
            alias_of: scope.alias_of.clone(),
            defining_class: scope.defining_class,
        });
    }
    Ok(out)
}

/// One method scope's reflection row: what ruby can ask back about a `def`
/// that its function pointer cannot answer -- the signature
/// (`#arity`/`#parameters`), the `def` keyword's own line
/// (`#source_location`, and `#inspect`'s tail), and the name an alias
/// copied from.
pub(super) fn meta_row(
    analyzed: &Analyzed,
    class: u32,
    singleton: bool,
    name: &str,
    params: &crate::hir::Params,
    node: Option<crate::hir::NodeId>,
    alias_of: Option<&str>,
) -> super::statics::MetaRowSpec {
    let (file, line) = node
        .and_then(|n| crate::analyze::source::source_location(&analyzed.compiler, n))
        .map_or((String::new(), 0), |(f, l)| (f.to_string(), l));
    super::statics::MetaRowSpec {
        class,
        singleton,
        name: name.to_string(),
        params: super::emit::param_entries(params, false),
        file,
        line,
        aliased_from: alias_of.unwrap_or_default().to_string(),
    }
}

/// shape).
/// What a `class`/`module` marker EVALUATES to. A `class` is an
/// expression in Ruby (`x = class C; 7; end` binds 7), and almost every
/// site runs for effect -- so the body fn keeps its tail only where the
/// tail is a value at all.
#[derive(Clone, PartialEq)]
pub(crate) enum BodyTail {
    /// The body fn computes it: its last statement IS an expression.
    Own,
    /// Analyze CONSUMED the body's last source statement, so the emitted
    /// statements no longer end where ruby's value comes from -- a `def`
    /// answers its name, `private_constant` the module it hid it on.
    Sym(String),
    OwnClass,
    /// A definition-level construct with no value zeo can name: nil in
    /// tail position (where nothing necessarily reads it), a refusal in
    /// expression position.
    Unknown(&'static str),
}

#[derive(Clone)]
pub(crate) struct ClassBodyCall {
    pub class: u32,
    pub func: Option<FuncId>,
    /// `(owner, leaf, file, line)` -- recorded only by the DECLARING site.
    pub const_loc: Option<(u32, String, String, u32)>,
    /// `(owner, leaf)` -- the `const_added` this declaration announces, from
    /// the DECLARING site only. `None` for a reopen, which creates nothing.
    pub const_added: Option<(u32, String)>,
    /// The superclass to announce this declaration to (`Super.inherited(C)`)
    /// -- `None` unless this site DECLARES the class and the superclass
    /// chain answers `inherited` by then.
    pub inherited: Option<u32>,
    /// Whether this site must REVEAL its runtime-conditional class: the
    /// guarded definition just ran, so the constant exists from here on.
    pub reveal: bool,
    /// The FROZEN-REOPEN guard's name list: the methods THIS site would
    /// install that no earlier site for the same class already did. Empty
    /// when the program freezes nothing, when this is the class's first
    /// site, or when the site installs nothing new.
    pub freeze_guard: Vec<String>,
    /// The site's Ruby value -- read only by a marker in value or tail
    /// position; a statement marker discards it.
    pub tail: BodyTail,
    /// A trailing `if`/`unless` on the `class` keyword (`class Set ... end if
    /// set_pp`). Ruby evaluates it in the ENCLOSING scope -- the oracle's
    /// backtrace for a raise inside one says `<main>`, not `<class:Set>` --
    /// so it stays OUT of the lifted body and runs at the marker, where the
    /// enclosing locals it reads are in scope. `(cond, run_when)`: `unless`
    /// puts the body in the else branch and runs when the condition is
    /// FALSE. Everything the site registers rides inside the branch too: a
    /// class whose guard failed was never defined.
    pub guard: Option<(crate::hir::NodeId, bool)>,
    /// Bytes in `zeo_reopen_flags` this site's `def`s own -- stored at the
    /// site's own document position, which is what makes a reopen positional.
    pub reopen_flags: Vec<u32>,
    /// The class an explicit `class Sub < Super` names, when this site
    /// DECLARES the class. `class Sub < Super` READS the constant `Super`,
    /// which is what runs an `autoload` target -- and zeo resolves the
    /// superclass edge at compile time, so without this the read never
    /// happens. bundler's `plugin/dsl.rb` has no `require` for `Bundler::Dsl`
    /// at all: `class DSL < Bundler::Dsl` is the only thing that loads it, and
    /// `Bundler::Dsl`'s class body is where `VALID_KEYS` is assigned.
    pub superclass_touch: Option<u32>,
    /// Builtin-alias SOURCE names THIS site's own `alias` statements put on
    /// the class -- what its body-end check validates. A row another stream's
    /// body wrote validates when that body runs: rss's 0.9 `Item` body must
    /// not check 2.0's `alias date pubDate` before 2.0's `def pubDate` ran.
    pub alias_checks: Vec<String>,
}

/// One compiled class body (a separate Ruby scope, lifted to its own
/// function).
pub(crate) struct ClassBodySpec {
    pub call: ClassBodyCall,
    pub class: zeo_abi::ClassId,
    pub label: String,
    pub stmts: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    /// Marker reachable INLINE from the statement stream: the body runs at
    /// its marker. Hoisted otherwise (a `class` inside a `def`): the body
    /// runs once in the toplevel prelude.
    pub inline: bool,
}

/// The markers whose class bodies run AT their document
/// position: statement containers
/// descend, a `def`'s body waits to be called (so its markers hoist),
/// except a block-bodied `define_method` def, whose body is a block.
fn inline_markers(
    compiler: &crate::compiler::Compiler,
    top_statements: &[&[crate::hir::NodeId]],
) -> std::collections::HashSet<crate::hir::NodeId> {
    // The reachability walk itself is `Compiler::class_marker_streams`, which
    // also records WHICH stream reached each marker -- what `--dump=classes`
    // reports. One walk, so the dump can never describe a program codegen
    // laid out differently.
    compiler
        .class_marker_streams(top_statements)
        .into_keys()
        .collect()
}

/// Collect + declare every class-body site; refusals are loud. Mirrors
/// `emit_class_body_site_lifted`'s head registrations: the shapes whose
/// registrations the slice cannot emit yet (const_added/inherited hooks,
/// the frozen-reopen guard) refuse rather than drop.
pub(super) fn collect_class_bodies(
    em: &mut Emitter,
    analyzed: &Analyzed,
) -> CResult<Vec<ClassBodySpec>> {
    let compiler = &analyzed.compiler;
    // Every TOP-LEVEL statement stream: main's, and each compiled-in
    // feature unit's. A unit's `class` marker runs where it stands IN THE
    // UNIT -- hoisting it into main's prelude would run a never-loaded
    // unit's body at program start.
    let mut tops: Vec<&[crate::hir::NodeId]> = vec![&analyzed.main_statements];
    tops.extend(analyzed.feature_units.iter().map(|(_, _, s)| s.as_slice()));
    let inline = inline_markers(compiler, &tops);
    let mut specs = Vec::new();
    for (i, site) in compiler.class_body_sites.iter().enumerate() {
        let ci = compiler.class(site.class);
        let name = compiler.fq_name(site.class);
        let refuse = |what: &str| {
            Err(CodegenError::unsupported(
                format!("the CLIF backend cannot lower {what} yet (class {name})"),
                site.def_node.and_then(|n| compiler.hir.span(n)),
            ))
        };
        // A BUILTIN reopen's body runs like any other -- but the class was
        // never DECLARED by it (the constant pre-exists), so nothing below
        // that hangs off "declares" (const-location record, const_added /
        // inherited announcements) applies.
        let builtin = ci.is_builtin || ci.is_bootstrap || site.class.0 == 0;
        let mut reopen_flags: Vec<u32> = site
            .installs
            .iter()
            .filter_map(|n| em.reopen_flags.get(&(site.class.0, n.clone())).copied())
            .collect();
        reopen_flags.sort_unstable();
        reopen_flags.dedup();
        if builtin && site.stmts.is_empty() && reopen_flags.is_empty() {
            continue;
        }
        if builtin && !compiler.feature_active(site.class) {
            return refuse("a require-gated builtin reopen body");
        }
        if site.def_node.is_none() {
            if site.stmts.is_empty() {
                continue;
            }
            return refuse("a synthetic class body");
        }
        let declares = !builtin
            && compiler
                .class_body_sites
                .iter()
                .find(|s| s.class == site.class)
                .is_some_and(|s| std::ptr::eq(s, site));
        // `class Foo; end` DEFINES a constant, so ruby announces it; a
        // hook observing that announcement is not emitted yet.
        let decl_owner = ci.lexical_parent.unwrap_or(crate::compiler::OBJECT_CLASS);
        // `class Foo; end` DEFINES a constant, so ruby announces it on the
        // lexically enclosing module -- only from the site that CREATES it.
        let const_added = (declares
            && (compiler.global_def_hooks.contains("const_added")
                || crate::analyze::def_hooks::hook_answers(
                    compiler,
                    decl_owner,
                    "const_added",
                    site.def_node,
                )))
        .then(|| (decl_owner.0, compiler.leaf_name(site.class).to_string()));
        // `Super.inherited(C)` fires when the class is CREATED, so only its
        // FIRST site announces; a reopen creates nothing. A hook written
        // BELOW this declaration is not installed yet and stays silent.
        let inherited = declares
            .then_some(ci.parent)
            .flatten()
            .filter(|&parent| {
                crate::analyze::def_hooks::hook_answers(
                    compiler,
                    parent,
                    "inherited",
                    site.def_node,
                )
            })
            .map(|parent| parent.0);
        // The superclass constant READ, which is what runs an `autoload`.
        // Unlike `inherited` this is not about a hook: it happens whatever the
        // superclass is, and only where the source wrote one.
        let superclass_touch = (declares && ci.explicit_superclass)
            .then_some(ci.parent)
            .flatten()
            .map(|parent| parent.0);
        // The frozen-reopen guard: a REOPEN under a program that freezes
        // classes raises `FrozenError` for the names it would newly
        // install. Registration order IS document order, so "earlier" is
        // simply the sites for this class before this one.
        let freeze_guard = if compiler.program_freezes {
            let earlier: Vec<&crate::compiler::ClassBodySite> = compiler.class_body_sites[..i]
                .iter()
                .filter(|s| s.class == site.class)
                .collect();
            if earlier.is_empty() {
                Vec::new()
            } else {
                let mut names: Vec<String> = site
                    .installs
                    .iter()
                    .filter(|n| !earlier.iter().any(|s| s.installs.contains(n)))
                    .cloned()
                    .collect();
                names.sort_unstable();
                names.dedup();
                names
            }
        } else {
            Vec::new()
        };
        let const_loc = (declares)
            .then(|| {
                site.def_node
                    .and_then(|n| crate::analyze::source::source_location(compiler, n))
                    .map(|(file, line)| {
                        (
                            decl_owner.0,
                            compiler.leaf_name(site.class).to_string(),
                            file.to_string(),
                            line,
                        )
                    })
            })
            .flatten();
        let tail = body_tail(compiler, site);
        // A trailing `if`/`unless` on the `class` keyword arrives as the
        // body's ONE statement, wrapping everything. It belongs to the
        // ENCLOSING scope, so split it back out: the lifted body gets the
        // taken branch, the marker gets the condition.
        let (guard, body_stmts) = split_guard(compiler, &site.stmts);
        let func = if body_stmts.is_empty() {
            None
        } else {
            let sig = super::params::body_sig(em, 0, false);
            Some(
                em.module
                    .declare_function(&format!("zeo_cb_{i}"), Linkage::Local, &sig)
                    .map_err(|e| {
                        CodegenError::internal(format!("declaring the {name} class body: {e}"))
                    })?,
            )
        };
        let label = super::body::body_frame_label(compiler, site.class);
        let call = ClassBodyCall {
            class: site.class.0,
            func,
            const_loc,
            const_added,
            inherited,
            reveal: ci.runtime_conditional || compiler.class_waits_for_its_unit(site.class),
            freeze_guard,
            tail,
            guard,
            reopen_flags,
            superclass_touch,
            alias_checks: site_alias_checks(compiler, site),
        };
        let is_inline = site.def_node.is_some_and(|n| inline.contains(&n));
        tracing::debug!(
            class = site.class.0,
            fq = %compiler.fq_name(site.class),
            reveal = call.reveal,
            has_body = call.func.is_some(),
            is_inline,
            def_node = ?site.def_node,
            "class body site"
        );
        if is_inline && let Some(marker) = site.def_node {
            em.class_bodies.insert(marker, call.clone());
        }
        specs.push(ClassBodySpec {
            call,
            class: site.class,
            label,
            stmts: body_stmts,
            node: site.def_node,
            inline: is_inline,
        });
    }
    Ok(specs)
}

/// The builtin-alias SOURCE names THIS site's own `alias` statements put on
/// its class: the terminal old names from `builtin_aliases` rows whose alias
/// this site's walk consumed. These are what the site's body-end check
/// validates -- CRuby raises at the `alias` statement's position, so a row
/// another stream's body wrote is that body's to check, not this one's.
pub(crate) fn site_alias_checks(
    compiler: &crate::compiler::Compiler,
    site: &crate::compiler::ClassBodySite,
) -> Vec<String> {
    let class = compiler.class(site.class);
    if class.builtin_aliases.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<String> = site
        .defs
        .iter()
        .filter_map(|d| match &compiler.hir[d.node] {
            crate::hir::HirNode::AliasMethod {
                new_name,
                is_class_method: false,
                ..
            } => class
                .builtin_aliases
                .iter()
                .find(|(new, _, _)| new == new_name)
                .map(|(_, terminal, _)| terminal.clone()),
            _ => None,
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Allocate one `zeo_reopen_flags` byte per `(builtin class, method name)`
/// this program reopens at COMPILE time.
///
/// A reopen's row registers at startup, so without a flag every call written
/// ABOVE the `class Foo ... end` answers with the reopened body -- statically
/// bound and dynamic sites alike. The class body stores 1 at its own document
/// position and the reopened body reads it, forwarding to the row it replaced
/// until then.
///
/// Runs before anything is emitted: a flagged body needs its caller's BLOCK to
/// forward, so it takes a block parameter whether or not it names one, and the
/// signature is decided in `collect_classes`.
///
/// Excluded: `Object` (a top-level `def` reopens nothing) and a BOOTSTRAP
/// builtin (the exception prelude, whose bodies are the runtime's own).
pub(super) fn collect_reopen_flags(em: &mut Emitter, analyzed: &Analyzed) {
    let compiler = &analyzed.compiler;
    // A LAZY unit's file gets a flag too, MARKED as a unit's. Its body runs
    // on require rather than at a document position this compile can point
    // at, so a unit that is never required leaves the byte at zero for the
    // whole program. That used to turn a working reopen into `undefined
    // method`, which is why the flag was skipped -- and skipping it made
    // rubygems' `def require` live from BOOT, before the `module Kernel`
    // body that declares the constant it reads.
    //
    // A unit's flag reads the other way instead: at zero, forward to the
    // native row IF THERE IS ONE, and otherwise run the body. A name the
    // unit ADDS keeps answering as before; a name it REPLACES answers
    // natively until the unit runs, which is ruby's own order.
    let unit_files: crate::compiler::FSet<String> = analyzed
        .feature_units
        .iter()
        .map(|(_, absolute, _)| format!("{absolute}.rb"))
        .collect();
    for site in &compiler.class_body_sites {
        if !compiler.class(site.class).is_builtin {
            continue;
        }
        let in_unit = site.def_node.is_some_and(|n| {
            crate::analyze::source::source_location(compiler, n)
                .is_some_and(|(file, _)| unit_files.contains(file))
        });
        let mut names: Vec<&String> = site.installs.iter().collect();
        names.sort();
        names.dedup();
        for n in names {
            let next = em.reopen_flags.len() as u32;
            let idx = *em
                .reopen_flags
                .entry((site.class.0, n.clone()))
                .or_insert(next);
            if in_unit {
                em.unit_reopen_flags.insert(idx);
            }
        }
    }
}

/// A class body whose ONE statement is a SYNTHESIZED `If` is a
/// `class ... end if cond` (or `unless`): analyze wraps the whole body in the
/// guard, but the condition's locals live in the ENCLOSING scope. CLIF lifts
/// the body to its own function, so the condition has to
/// come back out -- which is also where ruby runs it (the oracle's backtrace
/// for a raise in one reads `<main>`, never `<class:X>`).
///
/// [`NodeFlag::HOISTED_CLASS_GUARD`] is what says the guard was synthesized.
/// A guard the SOURCE wrote inside the body -- `class Platform; unless
/// respond_to?(:generic); ...` in bundler's rubygems_ext -- looks identical in
/// shape and must stay put: lifting it evaluates it before the class-body
/// frame, where `self` is the enclosing module, so the probe asks about the
/// wrong object and answers false.
///
/// One branch of such an `If` is always empty: `if` fills the then branch,
/// `unless` the else. Anything else is an ordinary `if` the body wrote, and
/// stays in the body.
fn split_guard(
    compiler: &crate::compiler::Compiler,
    stmts: &[crate::hir::NodeId],
) -> (Option<(crate::hir::NodeId, bool)>, Vec<crate::hir::NodeId>) {
    let keep = || (None, stmts.to_vec());
    let [only] = stmts else { return keep() };
    if !compiler
        .hir
        .has_flag(*only, crate::hir::NodeFlag::HOISTED_CLASS_GUARD)
    {
        return keep();
    }
    let crate::hir::HirNode::If {
        cond,
        then_body,
        else_body,
    } = &compiler.hir[*only]
    else {
        return keep();
    };
    match (then_body.is_empty(), else_body.is_empty()) {
        (false, true) => (Some((*cond, true)), then_body.clone()),
        (true, false) => (Some((*cond, false)), else_body.clone()),
        // Both empty: nothing to run either way, but the condition still
        // has to be evaluated. Both full: an ordinary `if` written as the
        // body's only statement, which the body lowers itself.
        (true, true) | (false, false) => keep(),
    }
}

/// [`BodyTail`] for one site: what the body's LAST SOURCE statement is
/// worth, split by value vs. statement position.
fn body_tail(
    compiler: &crate::compiler::Compiler,
    site: &crate::compiler::ClassBodySite,
) -> BodyTail {
    use crate::hir::HirNode;
    let last = site
        .def_node
        .and_then(|n| match &compiler.hir[n] {
            HirNode::ClassDef { body, .. } => body.last().copied(),
            _ => None,
        })
        .or_else(|| site.stmts.last().copied());
    let Some(last) = last else {
        return BodyTail::Own;
    };
    // Not the emitted tail: analyze consumed the source's last statement.
    if site.stmts.last() != Some(&last) {
        return match &compiler.hir[last] {
            HirNode::DefMethod { name, .. } => BodyTail::Sym(name.clone()),
            // `private_constant :Hidden` answers the module it hid the
            // constant on, which is the body's own class.
            HirNode::ConstantVisibility { .. } => BodyTail::OwnClass,
            node => BodyTail::Unknown(crate::hir::definition_kind(node)),
        };
    }
    // A definition-level construct that survived into the statement list
    // runs for effect; everything else is an ordinary expression the tail
    // lowering computes (and refuses loudly where it cannot).
    match &compiler.hir[last] {
        // `private_constant :Hidden` answers the module it hid the
        // constant on -- and it RUNS where it is written, so it is an
        // ordinary body statement whose value is the body's own class.
        HirNode::ConstantVisibility { .. } => BodyTail::OwnClass,
        HirNode::Program(_)
        | HirNode::Refine { .. }
        | HirNode::Using(_)
        | HirNode::Undef(_)
        | HirNode::ClassMethodUndef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ClassMethodVisibility { .. }
        | HirNode::ModuleFunction(_)
        | HirNode::MethodRedefine { .. }
        | HirNode::DefHook { .. }
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_) => {
            BodyTail::Unknown(crate::hir::definition_kind(&compiler.hir[last]))
        }
        _ => BodyTail::Own,
    }
}
