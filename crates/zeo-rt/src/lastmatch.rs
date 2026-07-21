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
//! THREAD-LOCAL, which is the plan's specified representation (G4(j)) and
//! is the right call for the thread half -- two Threads matching
//! concurrently must not clobber each other's `$1`.
//!
//! It is NOT frame-local, and real Ruby's IS. That is a genuine divergence,
//! oracle-verified, and worth stating precisely because it is silent:
//!
//! ```ruby
//! def m
//!   "q7" =~ /(\d)/
//!   $1
//! end
//! "a1" =~ /(\d)/
//! m      # => "7"
//! $1     # real ruby: "1" -- `$~` is scoped to m's own frame, so m's
//!        #   match never touched the caller's.
//!        # here:      "7" -- one slot per thread, so m's match overwrote it.
//! ```
//!
//! CRuby stores `$~` in the method's control frame (a "special variable"),
//! so a callee's matching is invisible to its caller. Reproducing that
//! needs a per-frame slot: every generated method that mentions `$~`/`$1`/
//! etc. would carry its own, and the runtime's match functions would have
//! to write to the CURRENT frame's rather than a static one -- which means
//! threading a frame pointer through the regexp API. Deferred rather than
//! faked; the common shapes (match and read in the same method; match in a
//! condition and read in its body) are unaffected.

use crate::regexp::RMatchData;
use crate::RubyValue;
use std::cell::RefCell;

thread_local! {
    static LAST_MATCH: RefCell<Option<RMatchData>> = const { RefCell::new(None) };
}

/// Records a successful match, or clears the slot on a failed one -- pass
/// `None` for "did not match". Called by every regexp entry point that
/// builds MatchData.
pub fn set_last_match(m: Option<RMatchData>) {
    LAST_MATCH.with(|c| *c.borrow_mut() = m);
}

/// `$~` -- the MatchData, or nil.
pub fn last_match() -> RubyValue {
    LAST_MATCH.with(|c| match &*c.borrow() {
        Some(m) => RubyValue::MatchData(m.clone()),
        None => RubyValue::Nil,
    })
}

/// `$1`..`$9` (and `$&`, which is group 0) -- the nth capture group, or nil
/// when there was no match or the group didn't participate.
pub fn last_match_group(n: usize) -> RubyValue {
    LAST_MATCH.with(|c| match &*c.borrow() {
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
    LAST_MATCH.with(|c| match &*c.borrow() {
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
    LAST_MATCH.with(|c| match &*c.borrow() {
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
            // "ell" of "hello", with one capture group over "ll".
            groups: vec![Some((1, 4)), Some((2, 4))],
            names: Vec::new(),
            regexp: crate::regexp::regexp_new("(ll)", false, false, false).expect("valid test regexp"),
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
