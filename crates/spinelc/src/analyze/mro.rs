//! Real MRO: ancestor linearization, method/class-method materialization,
//! and class-variable ownership resolution -- run once, after every
//! `ClassDef` has been registered (`analyze::register_class`), since
//! `include`/`extend`/`prepend`/`< Super` targets must already exist by the
//! same "defined earlier in the file" rule `superclass` resolution already
//! enforces today.
//!
//! See the plan's Part 6 for the full design rationale. The short version:
//! rather than spinel's clone-with-mangled-shadow-name scheme (needed only
//! because C has no generics), a module's method body gets MATERIALIZED --
//! re-run through the exact same per-method analysis pipeline
//! (`register_method`) that an ordinary `def` already goes through, once
//! per including/inheriting class -- onto the concrete class that will
//! actually call it. This is DRY at the Ruby-source/HIR level (one
//! `NodeId`, safely referenced from multiple `Scope`s) even though the
//! generated Rust text ends up with one copy per class, exactly the same
//! trade a generic Rust function already makes via monomorphization.

use super::register_method;
use crate::compiler::{ClassId, Compiler};
use crate::hir::HirNode;
use std::collections::HashSet;

/// Real Ruby's actual linearization (not classic C3): for `class_id` with
/// prepends `P1..Pk` and includes `M1..Mn`, both in source order,
/// `ancestors = expand(Pk)..expand(P1) ++ [self] ++ expand(Mn)..expand(M1)
/// ++ ancestors(parent)`, recursively expanding each module's own
/// prepends/includes the same way, deduped keeping the FIRST occurrence.
/// This is the one place spinel-rs deliberately does NOT copy spinel's own
/// shortcut -- spinel's separately generated `.ancestors` reflection only
/// expands a class's own DIRECT includes, which is measurably wrong for a
/// module-including-module diamond (confirmed by running a repro against
/// spinel's own binary -- see the plan). Consulting this SAME list for
/// dispatch, reflection, AND class-variable ownership avoids that class of
/// bug entirely.
pub fn compute_ancestors(compiler: &Compiler, class_id: ClassId) -> Vec<ClassId> {
    let mut out = Vec::new();
    expand_into(compiler, class_id, &mut out);
    out
}

fn expand_into(compiler: &Compiler, class_id: ClassId, out: &mut Vec<ClassId>) {
    if out.contains(&class_id) {
        return;
    }
    let info = compiler.class(class_id);
    for &p in info.prepends.iter().rev() {
        expand_into(compiler, p, out);
    }
    if !out.contains(&class_id) {
        out.push(class_id);
    }
    for &m in info.includes.iter().rev() {
        expand_into(compiler, m, out);
    }
    if let Some(parent) = info.parent {
        expand_into(compiler, parent, out);
    }
}

/// Computes `ancestors` for every registered class/module, then
/// materializes `methods`/`ivars` for every non-module class and
/// `class_methods` for EVERY class/module (a module can have its own `def
/// self.x` "module functions", e.g. `Math.sqrt`, which need no instance/
/// struct at all -- see `codegen::mod::emit_class_methods`), then resolves
/// class-variable ownership. Call once, after `analyze::analyze`'s
/// top-level registration loop has processed every `ClassDef`.
pub fn materialize(compiler: &mut Compiler) -> Result<(), String> {
    let all_ids: Vec<ClassId> = (0..compiler.classes.len() as u32).map(ClassId).collect();

    for &cid in &all_ids {
        let ancestors = compute_ancestors(compiler, cid);
        compiler.classes[cid.0 as usize].ancestors = ancestors;
    }

    for &cid in &all_ids {
        if !compiler.class(cid).is_module {
            materialize_methods(compiler, cid)?;
        }
        materialize_class_methods(compiler, cid)?;
    }

    resolve_cvars(compiler)?;

    Ok(())
}

/// One clean rule handles override precedence for prepend/include/plain
/// inheritance uniformly, with no special-casing: walk `ancestors(class_id)`
/// in strict MRO order; the FIRST ancestor with an `own_methods` entry for a
/// given name wins. A prepended module sits BEFORE the class in the list
/// (so it naturally wins over the class's own definition); an included
/// module sits AFTER (so the class's own definition naturally wins over
/// it); plain inheritance is just "nothing closer defines it". Reusing the
/// SAME `register_method` pipeline an ordinary `def` goes through means a
/// materialized method's ivars/local-types/bare-block-use are all correctly
/// (re)computed against `class_id` as the owner, with ZERO aliasing/casting
/// trick needed (unlike spinel's C "common initial sequence" struct-prefix
/// hack) -- `self.#ivar` inside it is trivially valid Rust, since the body
/// is freshly re-typechecked against `class_id`'s own concrete struct.
fn materialize_methods(compiler: &mut Compiler, class_id: ClassId) -> Result<(), String> {
    let ancestors = compiler.class(class_id).ancestors.clone();
    let mut seen: HashSet<String> = HashSet::new();
    let mut materialized: Vec<_> = Vec::new();
    for &anc_id in &ancestors {
        let own = compiler.class(anc_id).own_methods.clone();
        for sid in own {
            let name = compiler.scope(sid).name.clone();
            if !seen.insert(name.clone()) {
                continue; // a closer ancestor already won this name
            }
            if anc_id == class_id {
                materialized.push(sid); // this class's own definition -- reuse verbatim
            } else {
                let scope = compiler.scope(sid);
                let (params, body) = (scope.params.clone(), scope.body.clone());
                let new_id = register_method(compiler, class_id, anc_id, name, params, body)?;
                materialized.push(new_id);
            }
        }
    }

    // Ivars are collected from EVERY ancestor's OWN methods -- not just the
    // MRO-winning ones in `materialized` above. `super` can splice in a
    // shadowed ancestor's body at codegen time (`emit_super_inline`) even
    // though that body never gets its own entry in `methods[class_id]` --
    // only the winning override does -- so an ivar only ever touched
    // through a `super`-reachable (but not otherwise winning) method would
    // otherwise be missing from this class's own generated struct entirely.
    // Over-including a field for a method that's shadowed and never even
    // reachable via `super` is harmless (an unused, always-`Nil` field),
    // so this doesn't try to be more precise than "every ancestor's own
    // body, unconditionally".
    let mut ivars = Vec::new();
    for &anc_id in &ancestors {
        for &sid in &compiler.class(anc_id).own_methods.clone() {
            let scope = compiler.scope(sid);
            let body = scope.body.clone();
            let default_ids = scope.params.default_ids();
            for &n in &body {
                super::collect_ivars(&compiler.hir, n, &mut ivars);
            }
            for id in default_ids {
                super::collect_ivars(&compiler.hir, id, &mut ivars);
            }
        }
    }

    let ci = &mut compiler.classes[class_id.0 as usize];
    ci.methods = materialized;
    ci.ivars = ivars;
    Ok(())
}

/// Class methods are inherited via the plain SUPERCLASS chain only -- never
/// through `include`d modules (`include` only ever affects instance-method
/// resolution; that's exactly what `extend` is for on the class side) --
/// mirroring real Ruby's own singleton-class-inherits-from-singleton-class
/// rule, without actually modeling singleton classes. At each level: this
/// class/ancestor's own `def self.x` always wins; otherwise its most
/// recently `extend`ed module wins (mirrors `include`'s "closer" rule,
/// pulling in the module's INSTANCE methods, `own_methods` -- matching real
/// Ruby, where `def self.x` on the module itself stays put). The nearest
/// level (`class_id` itself) always wins over anything found further up.
fn materialize_class_methods(compiler: &mut Compiler, class_id: ClassId) -> Result<(), String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut materialized = Vec::new();
    let mut level = Some(class_id);

    while let Some(cid) = level {
        for sid in compiler.class(cid).own_class_methods.clone() {
            let name = compiler.scope(sid).name.clone();
            if !seen.insert(name.clone()) {
                continue;
            }
            if cid == class_id {
                materialized.push(sid); // this class's own definition -- reuse verbatim
            } else {
                let scope = compiler.scope(sid);
                let (params, body) = (scope.params.clone(), scope.body.clone());
                let new_id = register_method(compiler, class_id, cid, name, params, body)?;
                materialized.push(new_id);
            }
        }
        for &m in compiler.class(cid).extends.clone().iter().rev() {
            for sid in compiler.class(m).own_methods.clone() {
                let name = compiler.scope(sid).name.clone();
                if !seen.insert(name.clone()) {
                    continue;
                }
                let scope = compiler.scope(sid);
                let (params, body) = (scope.params.clone(), scope.body.clone());
                let new_id = register_method(compiler, class_id, m, name, params, body)?;
                materialized.push(new_id);
            }
        }
        level = compiler.class(cid).parent;
    }

    compiler.classes[class_id.0 as usize].class_methods = materialized;
    Ok(())
}

/// `@@x` ownership: nearest ancestor (including self) that ever claimed the
/// name first owns the runtime storage -- fixes a real bug found in
/// spinel's own C implementation (a subclass writing a superclass-only cvar
/// silently allocates fresh, WRONG per-class storage there, an outright C
/// compile failure in spinel's case; see the plan). Processed in
/// declaration order (`compiler.classes`' index order IS file order, since
/// `class_by_name` resolution already requires a target to be defined
/// earlier), so every ancestor's own `cvar_owners` is already fully
/// resolved by the time a later class searches it.
fn resolve_cvars(compiler: &mut Compiler) -> Result<(), String> {
    let all_ids: Vec<ClassId> = (0..compiler.classes.len() as u32).map(ClassId).collect();
    for &cid in &all_ids {
        let names = own_cvar_names(compiler, cid);
        for name in names {
            owner_of(compiler, cid, &name);
        }
    }
    Ok(())
}

/// Every `@@x` name referenced anywhere in `class_id`'s own literal body --
/// its class-body-top-level statements (`@@x = 0` directly inside `class
/// Foo; ... end`, never inside any method), every `own_methods` body, AND
/// every `own_class_methods` body (`def self.x` can reference `@@x` too).
fn own_cvar_names(compiler: &Compiler, class_id: ClassId) -> Vec<String> {
    let mut names = Vec::new();
    for &n in &compiler.class(class_id).class_body_stmts.clone() {
        collect_cvars(&compiler.hir, n, &mut names);
    }
    let own_methods = compiler.class(class_id).own_methods.clone();
    let own_class_methods = compiler.class(class_id).own_class_methods.clone();
    for sid in own_methods.into_iter().chain(own_class_methods) {
        let body = compiler.scope(sid).body.clone();
        for &n in &body {
            collect_cvars(&compiler.hir, n, &mut names);
        }
    }
    names
}

/// Resolves (and memoizes onto `ClassInfo::cvar_owners`) which class/module
/// owns `@@name`'s storage as referenced from `class_id`: reuse an
/// already-resolved owner if `class_id` itself has one, else search
/// `ancestors` (excluding `class_id`, nearest first, guaranteed already
/// resolved by file-order) for the first ancestor that claims it, else
/// `class_id` becomes the new owner.
fn owner_of(compiler: &mut Compiler, class_id: ClassId, name: &str) -> ClassId {
    if let Some(&owner) = compiler.class(class_id).cvar_owners.get(name) {
        return owner;
    }
    let ancestors = compiler.class(class_id).ancestors.clone();
    let owner = ancestors
        .iter()
        .skip(1) // ancestors[0] is class_id itself
        .find_map(|&anc| compiler.class(anc).cvar_owners.get(name).copied())
        .unwrap_or(class_id);
    compiler.classes[class_id.0 as usize]
        .cvar_owners
        .insert(name.to_string(), owner);
    owner
}

/// Mirrors `collect_ivars`'s traversal shape exactly, over `ClassVarRead`/
/// `ClassVarWrite` instead of `IvarRead`/`IvarWrite`.
fn collect_cvars(hir: &crate::hir::Hir, id: crate::hir::NodeId, out: &mut Vec<String>) {
    use crate::hir::{ArrayElem, StrPart};
    match &hir[id] {
        HirNode::ClassVarRead(name) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        HirNode::ClassVarWrite(name, value) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
            collect_cvars(hir, *value, out);
        }
        HirNode::IvarWrite(_, value) | HirNode::LocalWrite(_, value) => {
            collect_cvars(hir, *value, out)
        }
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            collect_cvars(hir, *l, out);
            collect_cvars(hir, *r, out);
        }
        HirNode::Defined(v) => collect_cvars(hir, *v, out),
        HirNode::If { cond, then_body, else_body } => {
            collect_cvars(hir, *cond, out);
            for &n in then_body {
                collect_cvars(hir, n, out);
            }
            for &n in else_body {
                collect_cvars(hir, n, out);
            }
        }
        HirNode::CaseWhen { subject, arms, else_body } => {
            if let Some(s) = subject {
                collect_cvars(hir, *s, out);
            }
            for (values, body) in arms {
                for &v in values {
                    collect_cvars(hir, v, out);
                }
                for &n in body {
                    collect_cvars(hir, n, out);
                }
            }
            for &n in else_body {
                collect_cvars(hir, n, out);
            }
        }
        HirNode::Call { receiver, args, kwargs, block, block_arg, .. } => {
            if let Some(r) = receiver {
                collect_cvars(hir, *r, out);
            }
            for &a in args {
                collect_cvars(hir, a, out);
            }
            for pair in kwargs {
                collect_cvars(hir, pair.0, out);
                collect_cvars(hir, pair.1, out);
            }
            if let Some(b) = block {
                collect_cvars(hir, *b, out);
            }
            if let Some(b) = block_arg {
                collect_cvars(hir, *b, out);
            }
        }
        HirNode::New { args, .. } | HirNode::SuperCall { args } => {
            for &a in args {
                collect_cvars(hir, a, out);
            }
        }
        HirNode::Block { body, .. } => {
            for &n in body {
                collect_cvars(hir, n, out);
            }
        }
        HirNode::ArrayLit(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                collect_cvars(hir, *n, out);
            }
        }
        HirNode::HashLit(pairs) => {
            for pair in pairs {
                collect_cvars(hir, pair.0, out);
                collect_cvars(hir, pair.1, out);
            }
        }
        HirNode::RangeLit { start, end, .. } => {
            if let Some(s) = start {
                collect_cvars(hir, *s, out);
            }
            if let Some(e) = end {
                collect_cvars(hir, *e, out);
            }
        }
        HirNode::StringLit(parts) => {
            for p in parts {
                if let StrPart::Interp(n) = p {
                    collect_cvars(hir, *n, out);
                }
            }
        }
        HirNode::While { cond, body, .. } => {
            collect_cvars(hir, *cond, out);
            for &n in body {
                collect_cvars(hir, n, out);
            }
        }
        HirNode::Loop { body } => {
            for &n in body {
                collect_cvars(hir, n, out);
            }
        }
        HirNode::For { iterable, body, .. } => {
            collect_cvars(hir, *iterable, out);
            for &n in body {
                collect_cvars(hir, n, out);
            }
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            if let Some(v) = v {
                collect_cvars(hir, *v, out);
            }
        }
        HirNode::MultiWrite { value, .. } => collect_cvars(hir, *value, out),
        HirNode::Eval(body) => {
            for &n in body {
                collect_cvars(hir, n, out);
            }
        }
        HirNode::Yield(args) | HirNode::Raise(args) => {
            for &a in args {
                collect_cvars(hir, a, out);
            }
        }
        HirNode::CaseIn { subject, arms, else_body } => {
            collect_cvars(hir, *subject, out);
            for arm in arms {
                arm.pattern.for_each_node(&mut |n| collect_cvars(hir, n, out));
                if let Some((g, _)) = arm.guard {
                    collect_cvars(hir, g, out);
                }
                for &n in &arm.body {
                    collect_cvars(hir, n, out);
                }
            }
            if let Some(body) = else_body {
                for &n in body {
                    collect_cvars(hir, n, out);
                }
            }
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            collect_cvars(hir, *subject, out);
            pattern.for_each_node(&mut |n| collect_cvars(hir, n, out));
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            for &n in body {
                collect_cvars(hir, n, out);
            }
            for r in rescues {
                for &n in &r.body {
                    collect_cvars(hir, n, out);
                }
            }
            if let Some(b) = else_body {
                for &n in b {
                    collect_cvars(hir, n, out);
                }
            }
            if let Some(b) = ensure_body {
                for &n in b {
                    collect_cvars(hir, n, out);
                }
            }
        }
        HirNode::Retry => {}
        HirNode::Redo
        | HirNode::BlockGiven
        | HirNode::SelfRef
        | HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoolLit(_)
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => {}
    }
}
