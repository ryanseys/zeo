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

/// Every definition reached by two or more classes that each have a generated
/// struct to emit an instance method into.
pub fn groups(compiler: &Compiler) -> Vec<Group> {
    let mut by_def: HashMap<ScopeId, Vec<ClassId>> = HashMap::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        let cid = ClassId(idx as u32);
        if !compiler.has_generated_struct(cid) {
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

/// Whether `ZEO_VERIFY_SHARE` is set: every group's member bodies are emitted
/// and compared against what the trace predicted, and an unexplained difference
/// aborts the compile.
pub fn verify_enabled() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("ZEO_VERIFY_SHARE").is_some())
}
