//! Which classes carry the SAME definition, so codegen can emit its body once
//! instead of once per class that inherited it.
//!
//! One `def` in a widely-inherited base reaches every descendant: in prism,
//! `Prism::Node`'s methods land on 152 classes. Before the entry/definition
//! split each of those was a cloned `Scope`, and grouping them meant asking
//! which clones came from one source -- `(defining_class, name, def_node)`, a
//! string-keyed hash that rediscovered by comparison what was once one object.
//!
//! [`crate::compiler::MethodEntry::def`] states it directly: every class that
//! inherited a `def` names the SAME [`ScopeId`]. So a group is just that id and
//! the classes whose entries point at it.
//!
//! This is the RAW grouping -- no semantic disqualifier is applied, so a caller
//! that means to share must filter (`codegen::share::shareable`), and whether
//! the members' emissions actually agree is `codegen::class_query`'s question.

use crate::compiler::{ClassId, Compiler, ScopeId};
use std::collections::HashMap;

/// One `def`, and every class carrying an entry that points at it.
pub struct Group {
    pub def: ScopeId,
    /// Sorted by class id, so emission order never depends on hash iteration.
    pub members: Vec<ClassId>,
}

/// Every definition reached by two or more classes that each emit an instance
/// method body for it.
///
/// That is NOT the same as "has a generated struct". A reopened builtin emits
/// its methods as free functions in a `pub mod __bm_<Name>`, and it reaches
/// them through the very same `emit_value_self_method_fn` a shared body uses --
/// so its bodies are already in shareable shape, and two builtins carrying one
/// definition emit byte-identical text. Excluding them cost activemodel 10MB:
/// activesupport mixes `index_with` into 29 `Enumerable` includers, nearly all
/// of them builtins, and it was emitted 29 times.
///
/// A group may now hold both kinds. What separates them is a real per-class
/// difference -- a struct-backed receiver indexes an ivar by SLOT where a
/// structless one goes by name -- and `codegen::class_query` asks it as
/// `HasStruct`, so such a group splits into two bodies on its own.
pub fn groups(compiler: &Compiler) -> Vec<Group> {
    let mut by_def: HashMap<ScopeId, Vec<ClassId>> = HashMap::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        let cid = ClassId(idx as u32);
        if !emits_instance_bodies(compiler, cid) {
            continue;
        }
        for entry in &class.methods {
            by_def.entry(entry.def).or_default().push(cid);
        }
    }
    let mut out: Vec<Group> = by_def
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .map(|(def, mut members)| {
            members.sort_by_key(|c| c.0);
            Group { def, members }
        })
        .collect();
    // Deterministic order: the emission must not depend on hash iteration.
    out.sort_by_key(|g| (g.def.0, g.members[0].0));
    out
}

/// Whether `cid` emits an instance-method body per entry, which is the only
/// thing a shared body can stand in for.
///
/// Two shapes do. A class with a generated struct emits `impl` methods
/// (`emit_class`); a reopened builtin -- and `Object`, which carries every
/// top-level `def` -- emits free functions into `pub mod __bm_<Name>`
/// (`emit_builtin_reopen`). Nothing else does: a module's instance methods are
/// materialized onto its includers and emitted THERE, and an exception-backed
/// class emits only the deltas that are not a pristine native body.
///
/// Kept beside `groups` and deliberately mirroring codegen's own two filters --
/// if a third emitter appears, this is where it has to be added, and the cost of
/// forgetting is a missed sharing opportunity rather than a wrong program.
fn emits_instance_bodies(compiler: &Compiler, cid: ClassId) -> bool {
    compiler.has_generated_struct(cid)
        || compiler.class(cid).is_builtin
        || cid == crate::compiler::OBJECT_CLASS
}

/// Whether `ZEO_VERIFY_SHARE` is set: every group's member bodies are emitted
/// and compared against what the trace predicted, and an unexplained difference
/// aborts the compile.
pub fn verify_enabled() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("ZEO_VERIFY_SHARE").is_some())
}
