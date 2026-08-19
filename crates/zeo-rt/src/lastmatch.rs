//! The last-match slot behind `$~` and everything derived from it: `$1`..
//! `$9`, `$&`, `` $` ``, `$'`. Every successful regexp match writes it; a
//! FAILED one clears it (oracle-verified -- `"zzz" =~ /(\d+)/` leaves `$1`,
//! `$&` and `$~` all nil, it does not leave the previous match in place).
//!
//! `match?` deliberately does NOT write it: real Ruby's `match?` is
//! specifically the allocation-free predicate that skips building MatchData
//! at all, so `"xyz".match?(/y/)` leaves whatever the last real match
//! stored (oracle-verified).
//!
//! FRAME-SCOPED per CRuby's svar rule: `$~` lives in the method's control
//! frame, so a callee's matching is invisible to its caller. The stack here
//! is the compiled spelling of that: every generated METHOD whose body
//! mentions an svar (`$~`/`$1`/`Regexp.last_match`/...) pushes one scope
//! ([`svar_scope`], emitted by `codegen::scope_frame_guard`), blocks share
//! their method's, and the regexp entry points write the innermost scope --
//! falling back to a thread-base slot outside any (the top level).
//!
//! Documented narrowing: a method that matches but never MENTIONS an svar
//! pushes no scope, so its match writes the nearest mentioning caller's --
//! CRuby would confine it to the silent callee's own frame. THREAD-local
//! underneath either way (two Threads matching concurrently must not clobber
//! each other's `$1`), and part of the [`crate::ec::swap`] bundle so a fiber
//! carries its own.

use crate::RubyValue;
use crate::regexp::RMatchData;
use std::cell::RefCell;

thread_local! {
    static BASE: RefCell<Option<RMatchData>> = const { RefCell::new(None) };
    static SCOPES: RefCell<Vec<Option<RMatchData>>> = const { RefCell::new(Vec::new()) };
}

/// One method's svar scope -- push on entry, RAII-pop on any exit.
pub struct SvarScope(());

impl Drop for SvarScope {
    fn drop(&mut self) {
        SCOPES.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

/// Enter a fresh svar scope: `$~` and friends read nil here until this
/// scope's own first match, and its matches vanish when it drops.
pub fn svar_scope() -> SvarScope {
    SCOPES.with(|s| s.borrow_mut().push(None));
    SvarScope(())
}

/// [`svar_scope`] without the guard -- the capi push/pop twins, since
/// Cranelift-compiled code has no Rust drops and brackets the scope
/// explicitly.
pub(crate) fn svar_scope_push_raw() {
    std::mem::forget(svar_scope());
}

/// The explicit pop matching [`svar_scope_push_raw`].
pub(crate) fn svar_scope_pop_raw() {
    drop(SvarScope(()));
}

/// The fiber-switch handoff ([`crate::ec::swap`]): install a suspended
/// context's base+scopes, returning the running one's.
pub fn swap_svars(
    base: Option<RMatchData>,
    scopes: Vec<Option<RMatchData>>,
) -> (Option<RMatchData>, Vec<Option<RMatchData>>) {
    (
        BASE.with(|c| std::mem::replace(&mut *c.borrow_mut(), base)),
        SCOPES.with(|s| std::mem::replace(&mut *s.borrow_mut(), scopes)),
    )
}

/// Records a successful match, or clears the slot on a failed one -- pass
/// `None` for "did not match". Called by every regexp entry point that
/// builds MatchData; writes the innermost svar scope.
pub fn set_last_match(m: Option<RMatchData>) {
    SCOPES.with(|s| match s.borrow_mut().last_mut() {
        Some(top) => *top = m,
        None => BASE.with(|c| *c.borrow_mut() = m),
    })
}

/// The innermost scope's slot, read under `f`.
fn with_slot<R>(f: impl FnOnce(&Option<RMatchData>) -> R) -> R {
    SCOPES.with(|s| match s.borrow().last() {
        Some(top) => f(top),
        None => BASE.with(|c| f(&c.borrow())),
    })
}

/// `$~` -- the MatchData, or nil.
pub fn last_match() -> RubyValue {
    with_slot(|slot| match slot {
        Some(m) => RubyValue::MatchData(m.clone()),
        None => RubyValue::Nil,
    })
}

/// `$1`..`$9` (and `$&`, which is group 0) -- the nth capture group, or nil
/// when there was no match or the group didn't participate.
pub fn last_match_group(n: usize) -> RubyValue {
    with_slot(|slot| match slot {
        Some(m) => crate::regexp::matchdata_group(m, n as i64),
        None => RubyValue::Nil,
    })
}

/// `` $` `` -- the text BEFORE the match (`""` when the match started at 0,
/// `nil` when there was no match at all).
pub fn last_match_pre() -> RubyValue {
    last_match_slice(|m, start, _| m.haystack[..start].to_string())
}

/// `$'` -- the text AFTER the match.
pub fn last_match_post() -> RubyValue {
    last_match_slice(|m, _, end| m.haystack[end..].to_string())
}

/// `$+` -- the text of the highest-numbered group that actually PARTICIPATED
/// in the match.
///
/// CRuby's `rb_reg_match_last` (re.c:2093 -- note the confusingly-named
/// `rb_reg_last_match` is `$&`, a different function) scans groups from the
/// highest index downward, skipping any that were declared but did not match
/// (`beg == -1`, e.g. the losing arm of `(a)|(b)` or an unmatched optional
/// group), and stops at the first participant.
///
/// It yields nil when that search bottoms out at index 0: `$+` reports a
/// CAPTURE GROUP only, never the whole match.
pub fn last_match_last_group() -> RubyValue {
    with_slot(|slot| match slot {
        Some(m) => match m.groups.iter().rposition(|g| g.is_some()) {
            Some(i) if i > 0 => crate::regexp::matchdata_group(m, i as i64),
            _ => RubyValue::Nil,
        },
        None => RubyValue::Nil,
    })
}

/// Shared by `` $` ``/`$'`: both are a slice of the haystack cut at group
/// 0's span, and both are nil when nothing matched.
fn last_match_slice(f: impl Fn(&RMatchData, usize, usize) -> String) -> RubyValue {
    with_slot(|slot| match slot {
        Some(m) => match m.groups.first().copied().flatten() {
            Some((start, end)) => RubyValue::Str(crate::string_new(f(m, start, end))),
            None => RubyValue::Nil,
        },
        None => RubyValue::Nil,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(haystack: &str) -> RMatchData {
        std::sync::Arc::new(crate::regexp::MatchDataInner {
            haystack: haystack.to_string(),
            enc: crate::encoding::UTF_8,
            // "ell" of "hello", with one capture group over "ll".
            groups: vec![Some((1, 4)), Some((2, 4))],
            names: Vec::new(),
            regexp: crate::regexp::regexp_new("(ll)", false, false, false)
                .expect("valid test regexp"),
            frozen: std::sync::atomic::AtomicBool::new(false),
        })
    }

    #[test]
    fn an_unset_slot_reads_nil_everywhere() {
        set_last_match(None);
        assert!(matches!(last_match(), RubyValue::Nil));
        assert!(matches!(last_match_group(0), RubyValue::Nil));
        assert!(matches!(last_match_group(1), RubyValue::Nil));
        assert!(matches!(last_match_pre(), RubyValue::Nil));
        assert!(matches!(last_match_post(), RubyValue::Nil));
    }

    #[test]
    fn groups_and_surrounding_text_come_off_the_recorded_match() {
        set_last_match(Some(matched("hello")));
        assert_eq!(last_match_group(0).inspect_string(), "\"ell\"");
        assert_eq!(last_match_group(1).inspect_string(), "\"ll\"");
        // A group past the end is nil, not an error.
        assert!(matches!(last_match_group(9), RubyValue::Nil));
        assert_eq!(last_match_pre().inspect_string(), "\"h\"");
        assert_eq!(last_match_post().inspect_string(), "\"o\"");
        assert!(matches!(last_match(), RubyValue::MatchData(_)));
    }

    /// A failed match CLEARS the slot rather than leaving the previous one
    /// -- the property that makes `if s =~ re then $1 end` safe to reuse.
    #[test]
    fn a_failed_match_clears_a_previously_set_slot() {
        set_last_match(Some(matched("hello")));
        set_last_match(None);
        assert!(matches!(last_match_group(1), RubyValue::Nil));
        assert!(matches!(last_match(), RubyValue::Nil));
    }
}
