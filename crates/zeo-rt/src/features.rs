//! The compiled-in load path: every file the front end could not splice at a
//! fixed position, emitted as a callable unit and registered here under the
//! feature name a `require` would spell.
//!
//! Whole-program AOT resolves an ordinary `require "json"` at compile time and
//! splices it, so it never reaches this table. What does reach it is a require
//! whose target is only known when the program RUNS -- a computed string, or
//! the one every `autoload`-DSL gem builds (`ActiveSupport::Autoload#autoload`
//! joins the module name to the constant and calls `super const_name, path`).
//! Those are ordinary Ruby, and they work here for the ordinary reason: the
//! file is compiled in, and its top-level statements are a function this table
//! can call.
//!
//! What a unit does NOT change is registration. A class defined in a unit is in
//! the dispatch tables from startup, exactly like a spliced one -- zeo has
//! always separated "which classes exist" (compile time) from "when their
//! bodies run" (document position). Running a unit runs its bodies.
//!
//! Loading is once-only and re-entrant-safe, mirroring CRuby's loading table: a
//! feature already loaded answers `false`, one currently loading answers
//! `false` too (that is what makes a require cycle terminate rather than
//! recurse), and a fresh one runs its unit and answers `true`.

use crate::RubyValue;
use crate::signal::Signal;
use std::collections::{HashMap, HashSet};

/// A compiled unit: one file's top-level statements.
pub type UnitFn = fn() -> Result<RubyValue, Signal>;

static UNITS: std::sync::OnceLock<HashMap<&'static str, UnitFn>> = std::sync::OnceLock::new();

#[derive(Default)]
struct LoadState {
    /// Keyed by UNIT IDENTITY (the fn pointer), not the feature spelling: one
    /// unit registers under several spellings (the load-path-relative name AND
    /// the absolute path), and a per-spelling table ran the same file once per
    /// spelling -- rspec-core's exception_presenter loaded through both and
    /// its `PENDING_DETAIL_FORMATTER =` executed twice, warning where CRuby
    /// (one entry per FILE in `$LOADED_FEATURES`) is silent.
    loaded: HashSet<usize>,
    loading: HashSet<usize>,
    /// How many unit loads are on the stack right now.
    depth: u32,
    /// Autoload targets declared DURING a unit load, run when the outermost
    /// load returns -- see [`defer_autoload_target`].
    autoload_queue: Vec<String>,
}

fn state() -> &'static parking_lot::Mutex<LoadState> {
    static S: std::sync::LazyLock<parking_lot::Mutex<LoadState>> =
        std::sync::LazyLock::new(Default::default);
    &S
}

/// Installs the program's units. Emitted once, at startup, before `main`'s
/// first statement -- a require in the very first line must already see them.
pub fn install_feature_units(rows: &'static [(&'static str, UnitFn)]) {
    let _ = UNITS.set(rows.iter().copied().collect());
}

/// The key a feature string resolves under: the `.rb` suffix is optional in
/// every `require`, so it is not part of the identity.
fn key(feature: &str) -> &str {
    feature.strip_suffix(".rb").unwrap_or(feature)
}

/// Runs `feature`'s unit if this program compiled one in.
///
/// `None` means no such unit -- the caller raises the LoadError it would have
/// raised anyway. `Some(Ok(false))` is CRuby's answer for a feature already
/// loaded (or one loading further up the stack: a cycle).
pub fn load_feature(feature: &str) -> Option<Result<bool, Signal>> {
    let name = key(feature);
    let unit = *UNITS.get()?.get(name)?;
    let identity = unit as usize;
    {
        let mut st = state().lock();
        if st.loaded.contains(&identity) || !st.loading.insert(identity) {
            return Some(Ok(false));
        }
        st.depth += 1;
    }
    let result = unit();
    let outermost;
    let outcome = {
        let mut st = state().lock();
        st.loading.remove(&identity);
        st.depth -= 1;
        outermost = st.depth == 0;
        match result {
            // A unit that raised is NOT loaded: CRuby leaves the feature out
            // of `$LOADED_FEATURES` so a later require retries it.
            Err(e) => Some(Err(e)),
            Ok(_) => {
                st.loaded.insert(identity);
                Some(Ok(true))
            }
        }
        // The guard drops HERE, before the drain below re-locks the state --
        // holding it across `drain_autoload_queue` was a self-deadlock.
    };
    if matches!(outcome, Some(Ok(true))) {
        crate::globals::append_loaded_feature(name);
    }
    if outermost {
        drain_autoload_queue();
    }
    outcome
}

/// Queues an autoload target declared while a unit load is on the stack; the
/// queue drains when the OUTERMOST load returns. An autoload's declarer (and
/// everything up its require chain) must finish executing before the target
/// runs -- rspec-expectations' `built_in.rb` declares `autoload :Has` while
/// `matchers.rb` (which defines the `HAS_REGEX` the target reads) is still
/// mid-execution. CRuby loads at first constant ACCESS, which is always after
/// the declaring require graph completes; end-of-outermost-load is the closest
/// point this eager model has. Answers whether the target was queued -- at
/// depth 0 (eager main-line code) the caller keeps its immediate load.
pub fn defer_autoload_target(feature: &str) -> bool {
    let mut st = state().lock();
    if st.depth == 0 {
        return false;
    }
    st.autoload_queue.push(feature.to_string());
    true
}

/// Loads everything [`defer_autoload_target`] queued. Runs at outermost-load
/// return, looping because a drained target's own unit may declare more
/// autoloads. A target whose load raises -- ANY class, not just `LoadError`
/// -- rolls back to pending: CRuby runs nothing until the constant's first
/// ACCESS, so "declared, never loaded" is exactly its state (rspec-core's
/// bisect formatter drags in drb, and only a `--bisect` run ever touches it).
/// `load_feature` leaves a failed unit retryable, so a target the program
/// genuinely reaches still surfaces its error at that reach.
fn drain_autoload_queue() {
    loop {
        let next = {
            let mut st = state().lock();
            if st.autoload_queue.is_empty() {
                return;
            }
            st.autoload_queue.remove(0)
        };
        let _ = load_feature(&next);
    }
}

/// Whether `feature` names a unit this program compiled in -- asked before a
/// LoadError is raised, and by `autoload` to decide whether it can serve the
/// registration it just recorded.
pub fn has_feature(feature: &str) -> bool {
    UNITS.get().is_some_and(|u| u.contains_key(key(feature)))
}

/// Whether this program compiled a load path in at all -- i.e. whether some
/// file in it computes a require/autoload target. Only then is a feature that
/// is missing from the table a real failure: everywhere else, `autoload` keeps
/// its documented register-but-never-load behaviour, which is what CRuby's own
/// laziness looks like from the outside until the constant is referenced.
pub fn any_units() -> bool {
    UNITS.get().is_some_and(|u| !u.is_empty())
}

static DECLINED: std::sync::OnceLock<HashMap<&'static str, &'static str>> =
    std::sync::OnceLock::new();

/// Files on a compiled-in load path that zeo could not lower, with the reason.
///
/// A load path is compiled in WHOLE, so it includes files the program may well
/// never require -- a gem's optional adapters, its test helpers. One of those
/// hitting a lowering gap must not fail the build for code that never runs, but
/// it must not vanish either: the feature is recorded here, and requiring it
/// raises `LoadError` naming the gap. Loud at the point of use, which is the
/// only place the difference is observable.
pub fn install_declined_features(rows: &'static [(&'static str, &'static str)]) {
    let _ = DECLINED.set(rows.iter().copied().collect());
}

/// Why `feature` is not available, for the `LoadError` message.
pub fn decline_reason(feature: &str) -> Option<&'static str> {
    DECLINED.get()?.get(key(feature)).copied()
}
