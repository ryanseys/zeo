//! How long a `VALUE` lives.
//!
//! MRI answers this with a garbage collector that scans the machine stack, so
//! a C local holding a `VALUE` is a root by accident of where it sits. zeo
//! cannot do that: its heap is `Arc`s, its collector reconciles refcounts
//! over an allocation registry, and a C stack slot is invisible to both.
//!
//! So the answer is a **scope**. Every Ruby-to-C entry pushes one. Every
//! handle a `rb_*` hands back is pinned in the innermost scope, which is a
//! strong reference by construction, so a `VALUE` an extension is holding
//! cannot go away underneath it. When the entry returns, the scope pops and
//! its pins go with it.
//!
//! What this buys over scanning: it needs no cooperation from the extension,
//! no `RB_GC_GUARD`, and no correct guess about where the compiler put a
//! local. `RB_GC_GUARD` is a no-op under zeo for exactly that reason.
//!
//! What it costs: a `VALUE` an extension stashes in a C global outlives its
//! scope and would dangle. That is what `rb_gc_register_address` is for, and
//! MRI requires it for the same value in the same place -- an unregistered
//! global `VALUE` is a bug against MRI too.
//!
//! # Nesting
//!
//! Scopes nest, and the innermost one takes the pin. A value returned out of
//! an inner scope has to survive the pop, so [`Scope::keep`] re-pins it in
//! the parent before the child drops. That is the one place the two-scope
//! interaction is visible, and it is deliberately explicit.

use std::cell::RefCell;

thread_local! {
    /// The stack of open scopes on this thread. Thread-local rather than
    /// global: a Ractor runs C on its own thread, and a scope belongs to the
    /// call, not the process.
    static SCOPES: RefCell<Vec<Vec<usize>>> = const { RefCell::new(Vec::new()) };
}

/// One Ruby-to-C entry's worth of pinned handles.
///
/// Held by value at the entry point, so the pop is the `Drop` and cannot be
/// forgotten on an early return. A `longjmp` out of C skips it -- see
/// `cext::jmp`, which pops the stack down to the landing scope by hand.
pub struct Scope {
    /// Where this scope sits, so a mismatched drop is caught rather than
    /// silently popping someone else's.
    depth: usize,
}

impl Scope {
    pub fn enter() -> Scope {
        SCOPES.with_borrow_mut(|s| {
            s.push(Vec::new());
            Scope { depth: s.len() }
        })
    }

    /// Move a pin into the parent scope, so a value returned from a C call
    /// survives this scope's pop. A no-op at the outermost scope, where there
    /// is no parent and the pin is released with everything else.
    pub fn keep(&self, addr: usize) {
        SCOPES.with_borrow_mut(|s| {
            if s.len() < 2 {
                return;
            }
            let parent = s.len() - 2;
            s[parent].push(addr);
            super::handles::pin_raw(addr);
        });
    }

    /// How many handles this scope holds. Tests read it.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        SCOPES.with_borrow(|s| s.last().map_or(0, Vec::len))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let pinned = SCOPES.with_borrow_mut(|s| {
            debug_assert_eq!(s.len(), self.depth, "cext scopes popped out of order");
            s.pop().unwrap_or_default()
        });
        for addr in pinned {
            super::handles::unpin(addr);
        }
    }
}

/// Record `addr` in the innermost open scope.
///
/// A handle minted with no scope open is a bug in the entry point, not in the
/// extension: every Ruby-to-C entry pushes one before it converts an
/// argument. In release it leaks that handle rather than dropping a live
/// `VALUE`, which is the safe direction.
pub(super) fn pin(addr: usize) {
    SCOPES.with_borrow_mut(|s| match s.last_mut() {
        Some(scope) => scope.push(addr),
        None => debug_assert!(false, "a cext handle was minted with no scope open"),
    });
}

/// How deep the scope stack is. `cext::jmp` unwinds against it.
pub(super) fn depth() -> usize {
    SCOPES.with_borrow(Vec::len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RubyValue;

    fn a_string(s: &str) -> RubyValue {
        crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, s)
    }

    #[test]
    fn a_scope_pop_releases_its_handles() {
        let before = super::super::handles::live_count();
        {
            let scope = Scope::enter();
            let s = a_string("scoped");
            super::super::handles::pin(&s).expect("a String has a handle");
            assert_eq!(scope.len(), 1);
            assert_eq!(super::super::handles::live_count(), before + 1);
        }
        assert_eq!(
            super::super::handles::live_count(),
            before,
            "the handle outlived its scope"
        );
    }

    #[test]
    fn a_kept_value_survives_the_inner_pop() {
        let before = super::super::handles::live_count();
        let outer = Scope::enter();
        let addr = {
            let inner = Scope::enter();
            let s = a_string("returned");
            let v = super::super::handles::pin(&s).expect("a String has a handle");
            inner.keep(v);
            v
        };
        assert_eq!(
            super::super::handles::live_count(),
            before + 1,
            "the returned value was released by the inner pop"
        );
        drop(outer);
        assert_eq!(super::super::handles::live_count(), before);
        let _ = addr;
    }

    #[test]
    fn a_second_pin_of_one_object_needs_both_pops() {
        let before = super::super::handles::live_count();
        let s = a_string("twice");
        let outer = Scope::enter();
        let a = super::super::handles::pin(&s).expect("a String has a handle");
        {
            let _inner = Scope::enter();
            let b = super::super::handles::pin(&s).expect("a String has a handle");
            assert_eq!(a, b);
            assert_eq!(super::super::handles::live_count(), before + 1);
        }
        assert_eq!(
            super::super::handles::live_count(),
            before + 1,
            "the inner pop released a handle the outer scope still holds"
        );
        drop(outer);
        assert_eq!(super::super::handles::live_count(), before);
    }

    #[test]
    fn scopes_nest_and_unwind_in_order() {
        assert_eq!(depth(), 0);
        let a = Scope::enter();
        let b = Scope::enter();
        assert_eq!(depth(), 2);
        drop(b);
        assert_eq!(depth(), 1);
        drop(a);
        assert_eq!(depth(), 0);
    }
}
