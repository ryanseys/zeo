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

/// The same unit through the C ABI: a compiled unit function from a
/// Cranelift-emitted program (`zeo_abi::abi::UnitFn`).
pub type CUnitFn = unsafe extern "C" fn(out: *mut RubyValue) -> i32;

/// Which emitter produced a unit. The two backends hand the runtime the
/// same thing in the two shapes their calling conventions allow.
#[derive(Clone, Copy)]
pub enum UnitImpl {
    Rust(UnitFn),
    C(CUnitFn),
}

impl UnitImpl {
    /// The identity a load cycle is detected by -- the function itself.
    fn identity(self) -> usize {
        match self {
            UnitImpl::Rust(f) => f as usize,
            UnitImpl::C(f) => f as usize,
        }
    }

    fn call(self) -> Result<RubyValue, Signal> {
        match self {
            UnitImpl::Rust(f) => f(),
            UnitImpl::C(f) => {
                let mut out = std::mem::MaybeUninit::<RubyValue>::uninit();
                match unsafe { f(out.as_mut_ptr()) } {
                    0 => Ok(unsafe { out.assume_init() }),
                    _ => Err(crate::signal::take_pending()
                        .expect("a status of 1 leaves a pending signal")),
                }
            }
        }
    }
}

/// The seam a file the compiler never saw reaches the compiler through --
/// the twin of [`crate::eval::EvalCompiler`], installed by the same
/// `zeo_eval_install`, so a program that can reach a run-time load carries
/// exactly the machinery a program that can `eval` does.
///
/// The compiler-side half compiles the source as a TOP-LEVEL file: its own
/// `def`s land on `Object`, its classes mint, and its own `require`s come
/// straight back here.
pub trait UnitCompiler: Send + Sync {
    fn load(&self, source: &str, path: &str, box_id: u32) -> Result<RubyValue, Signal>;
}

static UNIT_COMPILER: std::sync::OnceLock<&'static dyn UnitCompiler> = std::sync::OnceLock::new();

/// See [`UnitCompiler`].
pub fn install_unit_compiler(compiler: &'static dyn UnitCompiler) {
    let _ = UNIT_COMPILER.set(compiler);
}

/// The path `feature` names on DISK for box `box_id`, or `None`.
///
/// CRuby's own resolution: a path that is absolute or explicitly relative
/// (`./`, `../`) stands alone; every other spelling is tried under each
/// `$LOAD_PATH` root in order. A spelling with no `.rb` suffix is tried
/// with one first, which is what makes `require "json"` and `require
/// "json.rb"` the same feature.
pub fn resolve_on_disk(feature: &str, box_id: u32, append_rb: bool) -> Option<std::path::PathBuf> {
    let spellings = |base: std::path::PathBuf| -> Vec<std::path::PathBuf> {
        if feature.ends_with(".rb") || !append_rb {
            vec![base]
        } else {
            vec![base.with_extension("rb"), base]
        }
    };
    let first_readable = |candidates: Vec<std::path::PathBuf>| {
        candidates
            .into_iter()
            .find(|p| p.is_file())
            .and_then(|p| p.canonicalize().ok())
    };
    if feature.starts_with('/') || feature.starts_with("./") || feature.starts_with("../") {
        return first_readable(spellings(std::path::PathBuf::from(feature)));
    }
    let RubyValue::Array(roots) = crate::globals::global_get(box_id, "$LOAD_PATH") else {
        return None;
    };
    let roots: Vec<String> = roots
        .lock()
        .iter()
        .filter_map(|r| match r {
            RubyValue::Str(s) => Some(s.lock().to_utf8_lossy().into_owned()),
            _ => None,
        })
        .collect();
    roots
        .into_iter()
        .find_map(|root| first_readable(spellings(std::path::Path::new(&root).join(feature))))
}

/// The `--embed-sources` pack: the ruby source that travelled inside the
/// program, keyed by the load-path-relative spelling a `require` writes.
static SOURCES: std::sync::OnceLock<HashMap<&'static str, &'static str>> =
    std::sync::OnceLock::new();

/// Installs the pack. Emitted once, at startup, beside the feature units.
pub fn install_sources(rows: &'static [(&'static str, &'static str)]) {
    let _ = SOURCES.set(rows.iter().copied().collect());
}

/// The embedded source `feature` names, with the spelling it answers to.
///
/// The pack is consulted BEFORE disk: a hermetic binary answers the same way
/// wherever it runs, which is the point of embedding at all.
fn resolve_embedded(feature: &str) -> Option<(&'static str, &'static str)> {
    let pack = SOURCES.get()?;
    let bare = feature.strip_suffix(".rb").unwrap_or(feature);
    pack.get_key_value(bare).map(|(k, v)| (*k, *v))
}

/// Compile and run the file `feature` names on disk, in box `box_id`.
///
/// `None` means it resolved to nothing -- the caller raises the `LoadError`
/// it would have raised anyway. `Some(Ok(false))` is CRuby's answer for a
/// file already loaded, or one loading further up the stack (which is what
/// makes a require CYCLE terminate rather than recurse). `reload` is
/// `Kernel#load`'s rule: run it again, and answer `true` either way.
pub fn load_from_disk(feature: &str, box_id: u32, reload: bool) -> Option<Result<bool, Signal>> {
    // `load` names an exact file; `require` appends `.rb` to a suffix-less
    // spelling, which is what makes `require "json"` and `require "json.rb"`
    // one feature.
    // The embedded pack first, then disk -- see `resolve_embedded`.
    let (path, embedded) = match resolve_embedded(feature) {
        Some((name, text)) => (format!("<embedded>/{name}.rb"), Some(text)),
        None => (
            resolve_on_disk(feature, box_id, !reload)?
                .to_string_lossy()
                .into_owned(),
            None,
        ),
    };
    let key = (box_id, path.clone());
    if !reload {
        let mut st = state().lock();
        if st.disk_loaded.contains(&key) || !st.disk_loading.insert(key.clone()) {
            return Some(Ok(false));
        }
    }
    let Some(compiler) = UNIT_COMPILER.get() else {
        state().lock().disk_loading.remove(&key);
        return Some(Err(crate::builtins::not_impl_error!(
            "this program was compiled without the unit compiler, so it cannot load `{feature}`              at run time (zeo links it only into a program it can see reach a computed require)"
        )));
    };
    let source = match embedded {
        Some(text) => text.to_string(),
        None => match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                state().lock().disk_loading.remove(&key);
                return Some(Err(crate::dispatch::raise_error(
                    "LoadError",
                    format!("cannot load such file -- {feature} ({e})"),
                )));
            }
        },
    };
    let result = compiler.load(&source, &path, box_id);
    let mut st = state().lock();
    st.disk_loading.remove(&key);
    match result {
        // A file that raised is NOT loaded: CRuby leaves it out of
        // `$LOADED_FEATURES` so a later require retries it.
        Err(e) => Some(Err(e)),
        Ok(_) => {
            st.disk_loaded.insert(key);
            drop(st);
            if !reload {
                crate::globals::append_loaded_feature_in(box_id, &path);
            }
            Some(Ok(true))
        }
    }
}

static UNITS: std::sync::OnceLock<HashMap<&'static str, UnitImpl>> = std::sync::OnceLock::new();

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
    /// The same two sets for a file loaded from DISK at run time, keyed by
    /// `(box, canonical path)`. The path is the identity CRuby's
    /// `$LOADED_FEATURES` uses and the only one available for a file no
    /// unit was compiled for; the BOX is what makes a file re-execute per
    /// box, which is CRuby's own rule and falls out of each box having its
    /// own `$LOADED_FEATURES`.
    disk_loaded: HashSet<(u32, String)>,
    disk_loading: HashSet<(u32, String)>,
}

fn state() -> &'static parking_lot::Mutex<LoadState> {
    static S: std::sync::LazyLock<parking_lot::Mutex<LoadState>> =
        std::sync::LazyLock::new(Default::default);
    &S
}

/// Installs the program's units. Emitted once, at startup, before `main`'s
/// first statement -- a require in the very first line must already see them.
pub fn install_feature_units(rows: &'static [(&'static str, UnitFn)]) {
    let _ = UNITS.set(
        rows.iter()
            .map(|&(name, f)| (name, UnitImpl::Rust(f)))
            .collect(),
    );
}

/// [`install_feature_units`] for a Cranelift-emitted program.
pub fn install_feature_units_c(rows: &'static [(&'static str, CUnitFn)]) {
    let _ = UNITS.set(
        rows.iter()
            .map(|&(name, f)| (name, UnitImpl::C(f)))
            .collect(),
    );
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
    let identity = unit.identity();
    {
        let mut st = state().lock();
        if st.loaded.contains(&identity) || !st.loading.insert(identity) {
            return Some(Ok(false));
        }
        st.depth += 1;
    }
    let result = unit.call();
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
