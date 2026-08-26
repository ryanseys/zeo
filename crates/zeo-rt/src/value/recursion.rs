//! The traversal stack a self-referential container needs.
//!
//! CRuby's `rb_exec_recursive` family, in the two shapes ruby actually uses:
//! a SINGLE stack (`Array#inspect`, `#join`, `#flatten` -- "is this object
//! its own ancestor?") and a PAIRED one (`Array#==`, `#<=>` -- "are these
//! two already being compared against each other?").
//!
//! A push/pop stack, not a monotonic set. `[a, a].join` is legal and must
//! stay legal; only `a` inside `a` recurses.
//!
//! The scope is a CLOSURE rather than an RAII guard, and that shape is
//! load-bearing: a guard holding `&mut self` cannot be nested, which is the
//! only thing a recursion guard ever does. Taking the closure pops however
//! the body returns -- an early `?` included -- and hands `&mut Self` back
//! so the body can descend.
//!
//! Identity is [`container_identity`](super::container_identity) -- the
//! `Arc` data pointer, since every collection clone shares one payload.
//!
//! ## Why this is not the same thing as `assert_unheld`
//!
//! `collections`' re-entrancy assert is DEBUG-ONLY and stays that way: it is
//! the proof obligation for the `sole_thread()` `&mut` fast path, not a
//! guard. Promoting it would put an atomic swap under every array index and
//! still not make the release failure mode safe, because in release the
//! failure mode is UB. The invariant has to be MAINTAINED by callers, and
//! the caller-side rule is precise: never hold a `.lock()` inside a
//! call-argument expression that also contains the recursive call. That
//! pattern is the whole bug class, and it is what made `join` abort.

use super::{RubyValue, container_identity};

/// A single traversal stack -- "is this container its own ancestor?".
#[derive(Default)]
pub(crate) struct Visited(Vec<usize>);

impl Visited {
    /// Runs `f` with `v` on the stack, popping however `f` returns.
    ///
    /// `None` means `v` was ALREADY on the stack: it is its own ancestor,
    /// and the caller raises or substitutes CRuby's marker. A value with no
    /// container identity can never recurse, so it simply runs.
    pub(crate) fn with<T>(&mut self, v: &RubyValue, f: impl FnOnce(&mut Self) -> T) -> Option<T> {
        let Some(id) = container_identity(v) else {
            return Some(f(self));
        };
        if self.0.contains(&id) {
            return None;
        }
        self.0.push(id);
        let out = f(self);
        self.0.pop();
        Some(out)
    }
}

/// A paired traversal stack -- "are these two already being compared?".
///
/// The pair is ordered, matching CRuby's `rb_exec_recursive_paired`: it
/// records the comparison, not the operands.
#[derive(Default)]
pub(crate) struct VisitedPair(Vec<(usize, usize)>);

impl VisitedPair {
    /// [`Visited::with`]'s paired twin. `None` = this exact comparison is
    /// already in progress.
    pub(crate) fn with<T>(
        &mut self,
        a: &RubyValue,
        b: &RubyValue,
        f: impl FnOnce(&mut Self) -> T,
    ) -> Option<T> {
        let (Some(ia), Some(ib)) = (container_identity(a), container_identity(b)) else {
            return Some(f(self));
        };
        if self.0.contains(&(ia, ib)) {
            return None;
        }
        self.0.push((ia, ib));
        let out = f(self);
        self.0.pop();
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array_new;

    fn an_array() -> RubyValue {
        RubyValue::Array(array_new(vec![RubyValue::Int(1)]))
    }

    #[test]
    fn a_sibling_is_not_a_cycle() {
        let a = an_array();
        let mut seen = Visited::default();
        // `[a, a]`: the first visit pops before the second begins.
        for _ in 0..2 {
            assert_eq!(seen.with(&a, |_| 1), Some(1), "a sibling is not a cycle");
        }
    }

    #[test]
    fn an_ancestor_is_a_cycle() {
        let a = an_array();
        let mut seen = Visited::default();
        let inner = seen.with(&a, |seen| seen.with(&a, |_| 1));
        assert_eq!(inner, Some(None), "a inside a recurses");
        assert_eq!(seen.with(&a, |_| 1), Some(1), "the frame popped on the way out");
    }

    #[test]
    fn an_early_return_still_pops() {
        let a = an_array();
        let mut seen = Visited::default();
        let r: Option<Result<(), ()>> = seen.with(&a, |_| Err(()));
        assert_eq!(r, Some(Err(())));
        assert_eq!(seen.with(&a, |_| 1), Some(1));
    }

    #[test]
    fn an_immediate_has_no_identity() {
        let mut seen = Visited::default();
        let one = RubyValue::Int(1);
        assert_eq!(seen.with(&one, |seen| seen.with(&one, |_| 1)), Some(Some(1)));
    }

    #[test]
    fn a_pair_records_the_comparison_not_the_operands() {
        let (a, b) = (an_array(), an_array());
        let mut seen = VisitedPair::default();
        let r = seen.with(&a, &b, |seen| {
            // The reversed pair is a DIFFERENT comparison and still runs.
            (seen.with(&b, &a, |_| 1), seen.with(&a, &b, |_| 1))
        });
        assert_eq!(r, Some((Some(1), None)));
    }
}
