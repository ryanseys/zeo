//! Which scopes need a `$~` scope of their own.
//!
//! `$~` (and the `$1`../`` $` ``/`$'` family reading through it) is
//! FRAME-LOCAL in ruby: a method that matches and returns must not leave its
//! match in the caller's `$1`. A scope that can touch the family therefore
//! brackets its body with an svar scope; blocks share their method's, so the
//! walk descends through them.

use crate::compiler::Compiler;
use crate::hir::{Hir, HirNode, NodeId};

/// Method names that WRITE `$~`. A scope containing one needs an svar scope
/// even if it never reads an svar (`def g(s); s.scan(/(\d)/); 1; end` is
/// exactly that shape).
///
/// Names only, no receiver: a false positive costs one dead scope push, a
/// false negative leaks a match upward.
const SVAR_WRITERS: &[&str] = &[
    "=~",
    "match",
    "match?",
    "scan",
    "sub",
    "sub!",
    "gsub",
    "gsub!",
    "split",
    "slice",
    "slice!",
    "index",
    "rindex",
    "partition",
    "rpartition",
    "start_with?",
    "end_with?",
    "grep",
    "grep_v",
    "===",
];

/// Whether `body` (nested blocks included) touches the `$~` family: a
/// `LastMatchRef` read, a `$~` write, a `Regexp.last_match` call, or a call
/// that PERFORMS a match and so writes `$~` without naming it
/// ([`SVAR_WRITERS`]). The `last_match` check ignores the receiver for the
/// same reason the writer list does.
pub(crate) fn body_mentions_svars(compiler: &Compiler, body: &[NodeId]) -> bool {
    let mut found = false;
    for &n in body {
        walk(&compiler.hir, n, &mut found);
        if found {
            break;
        }
    }
    found
}

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "structural: a probe -- every other node kind contributes nothing of \
              its own and is answered by its children"
)]
fn walk(hir: &Hir, id: NodeId, found: &mut bool) {
    if *found {
        return;
    }
    match &hir[id] {
        HirNode::LastMatchRef(_) => *found = true,
        HirNode::GlobalWrite(name, _) if name == "$~" => *found = true,
        HirNode::Call { name, .. } if name == "last_match" => *found = true,
        HirNode::Call { name, .. } if SVAR_WRITERS.contains(&name.as_str()) => *found = true,
        _ => hir[id].for_each_child(&mut |c| walk(hir, c, found)),
    }
}
