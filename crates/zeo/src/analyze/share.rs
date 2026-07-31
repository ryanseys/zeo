//! Which materialized method copies are the SAME body, so codegen can emit it
//! once instead of once per class that inherited it.
//!
//! `mro::materialize_methods` gives every class its own [`ScopeId`] for every
//! name reachable on it, inherited ones included -- which is what makes a
//! method body trivially valid Rust against that class's own concrete struct,
//! with no aliasing trick. The cost is that one `def` in a widely-inherited
//! base emits once per descendant: in prism, `Prism::Node`'s methods land on
//! 152 classes, and 61% of the whole emission is copies.
//!
//! A group here is one `(defining_class, name, def_node)` -- the `def` a set of
//! classes all inherited -- and its sharing set is every class carrying a copy.
//! The bodies in a group are token-identical once the receiver's own struct
//! type is abstracted away, which is what `ZEO_VERIFY_SHARE=1` proves per
//! compile rather than asserting.

use crate::compiler::{ClassId, Compiler, ScopeId};
use crate::hir::NodeId;
use std::collections::HashMap;

/// One `def`, and every class that carries a materialized copy of it.
pub struct Group {
    /// Where the `def` was written -- the same for every member by
    /// construction, since it is half the group key.
    pub defining_class: ClassId,
    pub name: String,
    /// The class each copy is emitted onto, with that copy's scope.
    pub members: Vec<(ClassId, ScopeId)>,
}

/// Every `(defining_class, name, def_node)` carried by two or more classes that
/// each have a generated struct to emit an instance method into.
///
/// This is the RAW grouping -- no semantic disqualifier is applied yet, so a
/// caller that means to share must filter. `def_node` is part of the key so two
/// unrelated `def`s of one name (a reopen replacing an earlier body) never land
/// in one group; a synthesized scope has none and can only group with copies of
/// itself, which is what `defining_class` already pins.
pub fn groups(compiler: &Compiler) -> Vec<Group> {
    /// The `def` a set of classes share: where it was written, what it is
    /// called, and which `def` node it came from.
    type Key<'a> = (ClassId, &'a str, Option<NodeId>);
    let mut by_key: HashMap<Key, Vec<(ClassId, ScopeId)>> = HashMap::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        let cid = ClassId(idx as u32);
        if !compiler.has_generated_struct(cid) {
            continue;
        }
        for &sid in &class.methods {
            let scope = compiler.scope(sid);
            by_key
                .entry((scope.defining_class, scope.name.as_str(), scope.def_node))
                .or_default()
                .push((cid, sid));
        }
    }
    let mut out: Vec<Group> = by_key
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .map(|((defining_class, name, _), mut members)| {
            members.sort_by_key(|&(c, _)| c.0);
            Group {
                defining_class,
                name: name.to_string(),
                members,
            }
        })
        .collect();
    // Deterministic order: the emission must not depend on hash iteration.
    out.sort_by(|a, b| {
        (a.defining_class.0, &a.name, a.members[0].0.0).cmp(&(
            b.defining_class.0,
            &b.name,
            b.members[0].0.0,
        ))
    });
    out
}

/// Whether `ZEO_VERIFY_SHARE` is set: every group's member bodies are emitted
/// and compared, and a mismatch aborts the compile.
pub fn verify_enabled() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("ZEO_VERIFY_SHARE").is_some())
}
