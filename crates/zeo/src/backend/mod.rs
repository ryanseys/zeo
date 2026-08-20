//! Compiles generated Rust source directly against the WORKSPACE's own
//! already-built `zeo-rt` artifacts -- a single `rustc` invocation with
//! `--extern zeo_rt=<the built lib>` and `-L dependency=target/debug/deps`,
//! not a fresh throwaway `cargo` project.
//!
//! Two things guard the identity of what comes out. `--crate-name` is pinned
//! (see `cache::GENERATED_CRATE_NAME`) and the temp source is content-addressed
//! (see `cache::generated_source_path`), because `rustc` otherwise derives the
//! crate name from the source FILENAME and embeds that path as panic-location
//! metadata -- either one makes identical Ruby compile to different bytes. With
//! both, the output is a pure function of the generated source, which is what
//! lets `cache::cache_path` hand back an earlier build instead of re-running
//! `rustc`.
//!
//! Only ONE `--extern` is ever needed. `zeo-rt`'s transitive dependencies are
//! deliberately not enumerated by hand: the generated program references
//! `zeo_rt` alone, and `rustc` resolves the rest through the `-L` search path
//! from metadata embedded in the built `zeo-rt`. This is the same shape
//! `trybuild`/`compiletest` use, and it avoids the previous approach's
//! throwaway cargo project, which rebuilt the whole dependency graph on every
//! call because no target dir was reused.
//!
//! A generated program links the runtime either STATICALLY (a self-contained
//! binary, for `zeo foo.rb -o app`) or DYNAMICALLY (against a shared
//! `libzeo_rt`, for the throwaway test corpus). See [`Linkage`] -- dynamic
//! linking is what keeps the bin-cache from ballooning, since the static
//! runtime cannot be dead-stripped: the builtin dispatch tables reference
//! every method fn.
//!
//! `build_binary` is PURE. It links an already-built `zeo-rt` and errors if
//! the artifact is missing, never running cargo or mutating the workspace, so
//! parallel `zeo` subprocesses never contend on Cargo's exclusive build-
//! directory lock. Building the runtime is the separate explicit step
//! `ensure_runtime_built`, called by entrypoints that cannot assume a prior
//! workspace build. It lives here rather than in `main.rs` so the CLI and the
//! in-process test harness share one implementation.

mod cache;
pub mod jit;
pub mod link;
pub mod object;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

/// How many jobs a runtime `cargo` build may run at once.
///
/// Two cores are held back rather than a hard ceiling: the historical `min(6)`
/// existed for `[profile.release] codegen-units = 1`, which made every crate
/// one whole-crate LLVM module and could exhaust memory on a 16GB box at full
/// width. The workspace builds at codegen-units 16 now, so per-rustc peaks are
/// a fraction of that; the headroom covers the invoking process (a test
/// harness or an outer cargo) and the machine staying interactive.
fn job_cap() -> usize {
    if let Some(n) = std::env::var("ZEO_BUILD_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return n;
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    cores.saturating_sub(2).max(1)
}

/// An exclusive cross-process lock serializing runtime `cargo` builds.
///
/// The four (runtime, linkage) combinations build into SEPARATE target dirs (see
/// [`variant_target_dir`]), so cargo's own per-target-dir build lock does NOT
/// serialize them -- and because the dirs share no compiled dependencies, two
/// concurrent builds compile two full copies of the graph at once. Under nextest
/// (process per test, so [`ensure_runtime_built`]'s per-process memo collapses
/// nothing) the lean and eval dynamic builds raced exactly that way and exhausted
/// memory. One lock across all combinations keeps the peak to a single graph.
///
/// `flock(2)` is released by the kernel when the fd closes -- including on a killed
/// process -- so an interrupted build can never leave a stale lock behind.
#[cfg(unix)]
fn lock_runtime_build() -> Option<std::fs::File> {
    use std::os::fd::AsRawFd;
    let path = target_dir().join(".zeo-rt-build.lock");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .ok()?;
    // EINTR is the only retryable error here; anything else means we proceed
    // unlocked rather than fail a build over a lock we couldn't take.
    while unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return None;
        }
    }
    Some(file)
}

#[cfg(not(unix))]
fn lock_runtime_build() -> Option<std::fs::File> {
    None
}

/// The SHARED side of [`lock_runtime_build`]'s lock, held around a generated
/// program's own `rustc` link: cargo replaces `libzeo_rt.dylib`/`.rlib`
/// non-atomically, so a link overlapping a concurrent rebuild (nextest is a
/// process per test; the first suite run after a `zeo-rt` source edit
/// rebuilds lazily) intermittently saw `ld: errno=2` on the vanished
/// artifact. Shared holders never contend with each other -- only with the
/// exclusive rebuild.
#[cfg(unix)]
fn lock_runtime_shared() -> Option<std::fs::File> {
    use std::os::fd::AsRawFd;
    let path = target_dir().join(".zeo-rt-build.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .ok()?;
    while unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH) } != 0 {
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return None;
        }
    }
    Some(file)
}

#[cfg(not(unix))]
fn lock_runtime_shared() -> Option<std::fs::File> {
    None
}

/// The workspace crates whose sources are inputs to the `zeo-rt` artifact --
/// its own crate plus its workspace path-dependency closure. `ensure_runtime_built`
/// stat-walks exactly these to decide whether a built runtime is stale, so this
/// list MUST track `zeo-rt`'s `path = "..."` dependencies (see its
/// `Cargo.toml`). Getting it wrong under-scopes the freshness check and lets a
/// stale runtime be linked silently -- the very bug the check exists to prevent.
const RUNTIME_CRATES: &[&str] = &[
    "zeo-rt",
    "zeo-abi",
    // Compile-time deps whose output is baked into `zeo-rt`: the `ruby_class!`/
    // `ruby_module!` proc-macro and its shared parser. Editing either changes
    // the generated runtime, and `zeo` has no cargo edge to catch it, so the
    // stat-gate must watch them too.
    "zeo-macros",
    "zeo-dsl",
];

/// The cargo workspace `build_runtime` shells `cargo build -p zeo-rt` in.
///
/// Dev tree: the repo root itself. Installed: the payload's `runtime/`
/// mini-workspace (`cargo xtask dist` stages the four runtime crates there
/// under the same `crates/<name>` layout, so every path derived from this
/// root -- the staleness walk, the `-p zeo-rt` build -- works unchanged).
fn runtime_workspace_dir() -> PathBuf {
    match crate::home::zeo_home() {
        crate::home::ZeoHome::DevTree { root } => root.clone(),
        crate::home::ZeoHome::Installed { payload, .. } => payload.join("runtime"),
        // The materialized anchor workspace (see `build_runtime_from_registry`).
        crate::home::ZeoHome::Registry { cache } => registry_anchor_dir(cache),
    }
}

/// Whether this process runs from the zeo repo (where sources change and
/// `target/` is cargo's own) rather than an installed/registry distribution.
fn in_dev_tree() -> bool {
    matches!(
        crate::home::zeo_home(),
        crate::home::ZeoHome::DevTree { .. }
    )
}

/// The build root everything hangs off: variant target dirs, the runtime
/// build lock, and the bin cache.
///
/// Dev tree: `CARGO_TARGET_DIR` (the standard Cargo override) if set, else
/// the ordinary `<root>/target` default -- not a full `cargo metadata` query
/// (this project's build layout is simple enough that the common cases are
/// all that's needed). Installed: the per-user cache -- the prefix is never
/// written to, and ambient `CARGO_TARGET_DIR` from an unrelated shell is
/// deliberately ignored (see `home::cache_root`).
fn target_dir() -> PathBuf {
    match crate::home::zeo_home() {
        crate::home::ZeoHome::DevTree { root } => match std::env::var_os("CARGO_TARGET_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => root.join("target"),
        },
        crate::home::ZeoHome::Installed { cache, .. }
        | crate::home::ZeoHome::Registry { cache } => cache.join(runtime_cache_key()),
    }
}

/// The name of the per-(compiler, toolchain) cache dir an installed zeo
/// builds into: `rt-<fnv64(version ; compiler fingerprint ; rustc -vV)>`.
///
/// Folding `rustc -vV` in is the load-bearing part: rlibs are locked to the
/// exact rustc that built them (Rust has no stable ABI), so a toolchain
/// update must land in a FRESH dir and rebuild cleanly rather than link a
/// mismatched artifact. The version + fingerprint fold means a zeo upgrade
/// gets the same treatment.
///
/// Inherited `RUSTFLAGS` is folded in for the same reason, and it is why zeo
/// does not strip the ambient environment: flags a user sets DO change the
/// artifact, so the honest fix is a key that covers them, not a scrub that
/// silently discards the user's configuration. (Cargo prefers
/// `CARGO_ENCODED_RUSTFLAGS` and ignores `RUSTFLAGS` when it is set, so the
/// key reads them in that order.)
///
/// Because every input to the artifact is part of the key, existence IS
/// freshness inside the dir -- see [`runtime_artifact_is_stale`]'s
/// installed-mode short-circuit.
fn runtime_cache_key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        let rustflags = std::env::var("CARGO_ENCODED_RUSTFLAGS")
            .or_else(|_| std::env::var("RUSTFLAGS"))
            .unwrap_or_default();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for part in [
            env!("CARGO_PKG_VERSION"),
            env!("ZEO_COMPILER_FINGERPRINT"),
            &rustc_version(),
            &rustflags,
        ] {
            // `;` separates parts so ("ab","c") can't collide with ("a","bc").
            h = cache::fnv1a64_with(h, part.as_bytes());
            h = cache::fnv1a64_with(h, b";");
        }
        format!("rt-{h:016x}")
    })
}

/// The full `rustc -vV` identity (version, commit hash, host), memoized.
/// Empty string if rustc can't be run -- the subsequent build fails with
/// cargo's own clearer error.
fn rustc_version() -> String {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| {
        std::process::Command::new("rustc")
            .arg("-vV")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    })
    .clone()
}

/// Best-effort reaper for SIBLING cache keys -- dirs left behind by an older
/// zeo or a replaced toolchain. Runs after a successful installed-mode build;
/// only dirs whose contents have been untouched for 30 days go, so a second
/// toolchain in active use is never yanked out from under its user. Removal
/// failures are ignored (another process may hold the dir open).
fn reap_stale_cache_keys(cache: &Path, current_key: &str) {
    const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);
    let Ok(entries) = std::fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("rt-") || name == current_key {
            continue;
        }
        let dir = entry.path();
        let last_used = newest_mtime_under(&dir);
        if let Ok(age) = SystemTime::now().duration_since(last_used)
            && age > MAX_AGE
        {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// The target ROOT a runtime variant's artifacts live under (before the
/// profile subdir).
///
/// Each FEATURE SET needs its own dir because cargo writes every build to the
/// same `libzeo_rt.*` path: a lean build and an `--features eval-vm` build
/// would clobber each other. Linkage no longer splits the dirs -- one
/// `--crate-type rlib,dylib` build produces both artifacts side by side, so
/// the static and dynamic linkers read from the same dir (this halved the
/// number of full dependency graphs on disk: every dir used to carry its own
/// vendored OpenSSL, oniguruma, libffi, prism and mimalloc builds).
///
/// Not the shared `target/` either: sharing meant `zeo`'s runtime rebuild
/// took cargo's flock on the whole build dir, and a concurrent suite/dev
/// build held it -- a bare `zeo file.rb` after a runtime edit sat blocked for
/// the other build's full duration (measured at two minutes mid-suite). The
/// stat gate (`ensure_runtime_built`) keeps the fresh path cargo-free either
/// way; the redirect makes the STALE path private too. Both dirs stay under
/// `target/` so `cargo clean` reaps them. See [`Runtime`].
fn variant_target_dir(runtime: Runtime) -> PathBuf {
    let base = target_dir();
    match runtime {
        Runtime::Lean => base.join("zeo-rt-lean"),
        Runtime::Eval => base.join("zeo-rt-eval"),
    }
}

/// The toolchain directory holding the shared `libstd-<hash>.dylib`, from
/// `rustc --print target-libdir`.
///
/// A dynamically-linked generated program links std as a dylib -- forced by
/// `-C prefer-dynamic`, which a `zeo-rt` DYLIB dependency requires (a Rust dylib
/// and its consumer must share ONE std, or upstream crates like `panic_unwind`
/// "show up twice") -- so this dir must be on the binary's rpath for it to run
/// standalone. Queried once. `None` if `rustc` can't be run, in which case
/// dynamic linkage fails loudly at link time, which is the correct signal.
fn std_libdir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let out = std::process::Command::new("rustc")
            .arg("--print")
            .arg("target-libdir")
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?;
        Some(PathBuf::from(s.trim()))
    })
    .clone()
}

/// Ensure the `zeo-rt` runtime artifact `build_binary` links against exists.
///
/// `build_binary` is pure -- it only LINKS an already-built runtime -- so an
/// entrypoint that can't assume a prior workspace build calls this first: the
/// CLI on a fresh tree (so `zeo foo.rb` just works), and the in-process e2e
/// harness (`cargo test -p zeo` doesn't build `zeo-rt`, since `zeo`
/// has no cargo dependency on it). It builds AT MOST ONCE per
/// (profile, runtime, linkage), and only when the artifact is missing OR STALE
/// -- a cheap stat-only freshness gate ([`runtime_artifact_is_stale`]) compares
/// the runtime crates' sources against the built artifact, so an edited `zeo-rt`
/// is rebuilt rather than silently linked stale (zeo has no cargo dependency
/// edge that would catch it). When the artifact is already current -- the common
/// case under a driver that prebuilt the workspace (the conformance harness) --
/// the gate is a handful of `stat`s that skip the build, with none of the cargo
/// build-lock contention that made an unconditional per-call `cargo build` a
/// significant share of the conformance suite's runtime.
pub fn ensure_runtime_built(
    profile: Profile,
    runtime: Runtime,
    linkage: Linkage,
) -> Result<(), String> {
    // Memoized PER (PROFILE, RUNTIME, LINKAGE): a single process almost always
    // touches one combination (the harness builds `Release`+`Dynamic`; only
    // `zeo foo.rb -o app` builds `Release`+`Static`; only a program that reaches
    // eval asks for `Eval`), but keying by all three axes keeps it correct if
    // several are exercised, and still collapses the e2e harness's many `#[test]`
    // threads to one build per combination. Eight cells = 2 profiles x 2 runtimes
    // x 2 linkages, indexed positionally.
    static CELLS: [OnceLock<Result<(), String>>; 8] = [const { OnceLock::new() }; 8];
    let idx = profile.idx() * 4 + runtime.idx() * 2 + linkage.idx();
    CELLS[idx]
        .get_or_init(|| {
            // Fresh already -- the common case (harness prebuild, or a prior build
            // in this tree).
            //
            // Freshness, not bare existence: an entrypoint here can't assume a
            // prior `build_runtime`, and `zeo` has no cargo dependency on
            // `zeo-rt`, so nothing else would rebuild an edited runtime. The stat
            // gate catches that. When it reports fresh, cargo is never invoked --
            // so a driver that prebuilt the workspace still gets the cheap path.
            // When stale (or missing), `build_runtime` shells cargo, which does
            // the real incremental rebuild.
            // `ZEO_ASSUME_RUNTIME_FRESH` (CI sets it right after its explicit
            // prebuild step): existence IS freshness, skipping the ~150-stat
            // recursive source walk every nextest test process otherwise
            // repeats. A missing artifact still falls through and builds, so
            // the env can never link nothing.
            if std::env::var_os("ZEO_ASSUME_RUNTIME_FRESH").is_some()
                && runtime_artifact(profile, runtime, linkage).is_ok()
            {
                return Ok(());
            }
            if !runtime_artifact_is_stale(profile, runtime, linkage) {
                return Ok(());
            }
            build_runtime(profile, runtime, linkage)
        })
        .clone()
}

/// Whether the built `zeo-rt` artifact is missing or older than any source
/// cargo would treat as an input to it -- a `stat`-only gate so
/// [`ensure_runtime_built`] rebuilds a STALE runtime instead of linking it,
/// without paying a cargo build-lock round-trip on the already-fresh path.
///
/// Scans exactly the runtime's workspace dependency closure ([`RUNTIME_CRATES`])
/// plus the resolved lockfile (external-dep bumps), comparing the newest source
/// mtime against the artifact's. Missing artifact -> stale (must build). This is
/// an mtime heuristic, not cargo's full fingerprint, but it only ever
/// over-reports staleness (a redundant build that no-ops), never under-reports
/// for an in-tree edit -- so it cannot reintroduce the silent-stale bug.
fn runtime_artifact_is_stale(profile: Profile, runtime: Runtime, linkage: Linkage) -> bool {
    let Ok(artifact) = runtime_artifact(profile, runtime, linkage) else {
        return true; // not built yet
    };
    // Installed/registry mode: existence IS freshness. The payload (or the
    // pinned registry dep) is immutable and every input to the artifact --
    // compiler version, fingerprint, exact rustc -- is folded into the cache
    // dir's name (`runtime_cache_key`), so an input change lands in a
    // different dir and builds there. The mtime walk below exists for the
    // dev tree, where sources actually change.
    if !in_dev_tree() {
        return false;
    }
    let Some(artifact_mtime) = file_mtime(&artifact) else {
        return true;
    };
    let root = runtime_workspace_dir();
    let mut newest_source = SystemTime::UNIX_EPOCH;
    for crate_name in RUNTIME_CRATES {
        let crate_dir = root.join("crates").join(crate_name);
        newest_source = newest_source.max(newest_mtime_under(&crate_dir.join("src")));
        newest_source = newest_source.max(newest_mtime_under(&crate_dir.join("Cargo.toml")));
    }
    newest_source = newest_source.max(newest_mtime_under(&root.join("Cargo.lock")));
    // The workspace manifest holds the `[profile.*]` sections, which shape the
    // artifact as much as any source file.
    newest_source = newest_source.max(newest_mtime_under(&root.join("Cargo.toml")));
    newest_source > artifact_mtime
}

/// The modification time of a single file, or `None` if it can't be stat'd.
fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The newest modification time of `path` (a file) or anything beneath it (a
/// directory, walked recursively), `UNIX_EPOCH` if it doesn't exist. A `target`
/// directory is skipped so build output never counts as a source input.
fn newest_mtime_under(path: &Path) -> SystemTime {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return SystemTime::UNIX_EPOCH;
    };
    if meta.is_file() {
        return meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    }
    if !meta.is_dir() || path.file_name().is_some_and(|n| n == "target") {
        return SystemTime::UNIX_EPOCH;
    }
    let mut newest = SystemTime::UNIX_EPOCH;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            newest = newest.max(newest_mtime_under(&entry.path()));
        }
    }
    newest
}

/// Shell out to cargo for one (profile, runtime, linkage) combination,
/// unconditionally (cargo itself no-ops when the artifact is fresh and rebuilds
/// it when stale). This is the freshness-correct entry a prebuild step calls;
/// `ensure_runtime_built` wraps it behind a stat gate for the pure-link
/// entrypoints that can assume a prior prebuild.
///
/// ONE `cargo rustc --crate-type rlib,dylib` build produces BOTH linkage
/// artifacts: the plain `libzeo_rt.rlib` a static binary bakes in, and the
/// shared `libzeo_rt.dylib` (linked with `-C prefer-dynamic` so it shares one
/// std -- see [`std_libdir`] -- and stamped with an `@rpath` install name so a
/// consumer that adds the right rpaths finds it). `prefer-dynamic` is a
/// LINK-time preference, so it shapes only the dylib output; the rlib in the
/// same invocation is the archive a static link always got. The `linkage`
/// parameter no longer selects a build shape -- it survives for the registry
/// path, which builds rlib-only and must refuse a dynamic request loudly. The
/// `Eval` variant adds `--features eval-vm` (so prism links in); each feature
/// set builds into its own target dir -- see [`variant_target_dir`].
pub fn build_runtime(profile: Profile, runtime: Runtime, linkage: Linkage) -> Result<(), String> {
    if let crate::home::ZeoHome::Registry { cache } = crate::home::zeo_home() {
        return build_runtime_from_registry(cache, profile, runtime, linkage);
    }
    let mut cmd = std::process::Command::new("cargo");
    // `--jobs` must precede the `--` separator or cargo hands it to rustc.
    cmd.arg("rustc")
        .arg("--quiet")
        .arg("--jobs")
        .arg(job_cap().to_string());
    if let Some(flag) = profile.cargo_flag() {
        cmd.arg(flag);
    }
    cmd.args(["-p", "zeo-rt"]);
    if runtime == Runtime::Eval {
        cmd.arg("--features").arg("eval-vm");
    }
    cmd.arg("--crate-type").arg("rlib,dylib");
    // Never the shared `target/` -- see `variant_target_dir`: sharing made a
    // stale-runtime `zeo` call block on the suite's cargo flock. (Outside the
    // dev tree the redirect was always required: cargo would write into the
    // payload's own `target/`, and the prefix may be read-only, a Homebrew
    // Cellar.)
    cmd.arg("--target-dir").arg(variant_target_dir(runtime));
    cmd.arg("--");
    cmd.arg("-C").arg("prefer-dynamic");
    cmd.arg("-C").arg(install_name_arg());
    let workspace = runtime_workspace_dir();
    if let Some(payload) = installed_payload() {
        // The shipped lockfile is part of the payload's identity; never let
        // cargo rewrite dependency resolution under an installed compiler.
        cmd.arg("--locked");
        // A vendored payload (the default dist shape) builds fully offline;
        // its .cargo/config.toml redirects crates.io to the vendor tree.
        if payload.join("runtime/vendor").is_dir() {
            cmd.arg("--offline");
        }
    }
    cmd.current_dir(&workspace);
    decline_parent_jobserver(&mut cmd);
    let label = build_label(profile, runtime, linkage);
    if installed_payload().is_some() {
        // The one place an installed zeo is slow: the first compile per
        // (zeo version, toolchain) builds the runtime into the cache. Say so,
        // or a first `zeo hello.rb` looks hung for minutes.
        eprintln!(
            "zeo: building the runtime for this toolchain (one-time per zeo/rustc version)..."
        );
    }
    // Held for the whole cargo call: one runtime build machine-wide at a time.
    let _lock = lock_runtime_build();
    let result = match cmd.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "`{label}` (in {}) exited with {status}",
            workspace.display()
        )),
        Err(e) => Err(format!("running `{label}`: {e}")),
    };
    if result.is_ok()
        && let crate::home::ZeoHome::Installed { cache, .. }
        | crate::home::ZeoHome::Registry { cache } = crate::home::zeo_home()
    {
        reap_stale_cache_keys(cache, runtime_cache_key());
    }
    result
}

/// Build the runtime for a `cargo install`ed zeo: no runtime sources anywhere,
/// so a tiny ANCHOR workspace pinning `zeo-rt = "=X.Y.Z"` from crates.io is
/// materialized in the cache and built; cargo fetches and compiles the exact
/// runtime this compiler was released with.
///
/// A registry dependency's rlib lands under `deps/` with a HASHED filename
/// (only workspace members get the plain `libzeo_rt.rlib` path
/// `runtime_artifact` expects), so the build runs with
/// `--message-format=json-render-diagnostics`, the artifact message for
/// zeo-rt names the real file, and it is copied to the plain path. Everything
/// downstream -- `runtime_artifact`, `build_binary`'s `--extern` + `-L
/// dependency=deps` -- then works unchanged.
fn build_runtime_from_registry(
    cache: &Path,
    profile: Profile,
    runtime: Runtime,
    linkage: Linkage,
) -> Result<(), String> {
    if linkage == Linkage::Dynamic {
        // Dynamic linkage exists for the in-repo test harness's bin-cache;
        // nothing reaches it from an installed CLI (which always ships
        // self-contained static binaries).
        return Err(
            "dynamic runtime linkage is not supported for a registry-installed zeo".to_string(),
        );
    }
    let anchor = registry_anchor_dir(cache);
    materialize_anchor(&anchor)?;
    eprintln!(
        "zeo: fetching and building the zeo-rt runtime from crates.io \
         (one-time per zeo/rustc version)..."
    );
    let _lock = lock_runtime_build();
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("build")
        .arg("--message-format=json-render-diagnostics")
        .arg("--jobs")
        .arg(job_cap().to_string());
    if let Some(flag) = profile.cargo_flag() {
        cmd.arg(flag);
    }
    cmd.args(["-p", "zeo-rt-anchor"]);
    if runtime == Runtime::Eval {
        cmd.arg("--features").arg("eval-vm");
    }
    let variant_dir = variant_target_dir(runtime);
    cmd.arg("--target-dir").arg(&variant_dir);
    cmd.current_dir(&anchor);
    decline_parent_jobserver(&mut cmd);
    // Keep cargo's progress/download chatter visible; only the JSON stream is
    // captured.
    cmd.stderr(std::process::Stdio::inherit());
    let out = cmd
        .output()
        .map_err(|e| format!("running cargo for the registry runtime build: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "the registry runtime build (in {}) exited with {}",
            anchor.display(),
            out.status
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let built = zeo_rt_rlib_from_messages(&stdout).ok_or_else(|| {
        "the registry runtime build produced no zeo-rt rlib artifact message".to_string()
    })?;
    let dest_dir = variant_dir.join(profile.subdir());
    let dest = dest_dir.join("libzeo_rt.rlib");
    std::fs::create_dir_all(&dest_dir)
        .and_then(|_| std::fs::copy(&built, &dest).map(|_| ()))
        .map_err(|e| format!("copying {} to {}: {e}", built.display(), dest.display()))?;
    Ok(())
}

/// The anchor workspace's dir: keyed by zeo version only (its CONTENT depends
/// on nothing else); built artifacts go to the toolchain-keyed target dirs.
fn registry_anchor_dir(cache: &Path) -> PathBuf {
    cache.join(format!("anchor-{}", env!("CARGO_PKG_VERSION")))
}

/// Write the anchor package (idempotent; rewritten in full each time so a
/// half-written earlier attempt can't wedge it).
fn materialize_anchor(anchor: &Path) -> Result<(), String> {
    let write = |rel: &str, contents: &str| -> Result<(), String> {
        let path = anchor.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, contents).map_err(|e| format!("writing {}: {e}", path.display()))
    };
    // The [profile.*] sections MIRROR the zeo workspace's root Cargo.toml --
    // they shape the runtime rlib exactly as much as source does. Keep in sync.
    let manifest = format!(
        r#"# @generated by zeo -- the anchor workspace a registry-installed zeo
# builds its runtime through. Safe to delete; recreated on demand.
[package]
name = "zeo-rt-anchor"
version = "0.0.0"
edition = "2024"

[lib]
path = "src/lib.rs"

[dependencies]
zeo-rt = "={version}"

[features]
eval-vm = ["zeo-rt/eval-vm"]

[profile.dev.package.zeo-rt]
debug = "line-tables-only"

[profile.dev.package."*"]
opt-level = 1
debug = "line-tables-only"

[profile.dev.build-override]
opt-level = 2

[profile.release.build-override]
opt-level = 2

[profile.release]
strip = "symbols"
codegen-units = 16
lto = "thin"
"#,
        version = env!("CARGO_PKG_VERSION")
    );
    write("Cargo.toml", &manifest)?;
    write(
        "src/lib.rs",
        "// Intentionally empty: exists to anchor zeo-rt.\n",
    )
}

/// The zeo-rt rlib path from a `--message-format=json` stream: the
/// compiler-artifact message whose package id names zeo-rt, first `.rlib`
/// filename. Serde-parsed per line; non-JSON lines are skipped.
fn zeo_rt_rlib_from_messages(stdout: &str) -> Option<PathBuf> {
    let mut found = None;
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["reason"].as_str() != Some("compiler-artifact") {
            continue;
        }
        let target_name = v["target"]["name"].as_str().unwrap_or_default();
        if target_name != "zeo-rt" && target_name != "zeo_rt" {
            continue;
        }
        if let Some(files) = v["filenames"].as_array() {
            for f in files {
                if let Some(path) = f.as_str().filter(|p| p.ends_with(".rlib")) {
                    found = Some(PathBuf::from(path));
                }
            }
        }
    }
    found
}

/// Decline an enclosing cargo's jobserver when zeo is itself run under cargo
/// (its own test harness does exactly this, and so would a build script).
///
/// The descriptors named in `CARGO_MAKEFLAGS` are `FD_CLOEXEC` by default, so
/// they do not survive into a grandchild; rustc then warns that the build
/// environment looks misconfigured. Declining is the same thing the blessed
/// mechanism does -- `jobserver::Client::configure` clobbers these variables
/// for the child it configures -- and it is honest here: building the runtime
/// is independent work, not a subtask of the outer build's job budget, and
/// [`job_cap`] already governs its width via `--jobs`.
///
/// Nothing else is stripped. Inherited `RUSTFLAGS` genuinely does shape the
/// artifact, so it is folded into [`runtime_cache_key`] rather than discarded:
/// a cache key that covers every input is what makes honoring the environment
/// safe.
fn decline_parent_jobserver(cmd: &mut std::process::Command) {
    cmd.env_remove("CARGO_MAKEFLAGS");
    cmd.env_remove("MAKEFLAGS");
}

/// The payload dir when running from a relocatable install, `None` otherwise.
fn installed_payload() -> Option<&'static Path> {
    match crate::home::zeo_home() {
        crate::home::ZeoHome::Installed { payload, .. } => Some(payload),
        crate::home::ZeoHome::DevTree { .. } | crate::home::ZeoHome::Registry { .. } => None,
    }
}

/// The `-C link-arg=...` that stamps the dylib's own name so a consumer resolves
/// it through its rpath rather than a fixed absolute path. macOS uses an
/// `@rpath` install name; ELF platforms use a bare soname. (Only the macOS path
/// is exercised today; the ELF form mirrors the standard shared-object recipe.)
fn install_name_arg() -> String {
    let file = format!(
        "{}zeo_rt{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    if cfg!(target_os = "macos") {
        format!("link-arg=-Wl,-install_name,@rpath/{file}")
    } else {
        format!("link-arg=-Wl,-soname,{file}")
    }
}

/// The human-readable cargo invocation for an error message -- reflects the
/// static (`cargo build`) vs dynamic (`cargo rustc --crate-type dylib`) shape,
/// the profile flag, the `Eval` feature, and the redirected target dir.
fn build_label(profile: Profile, runtime: Runtime, _linkage: Linkage) -> String {
    let mut label = String::from("cargo rustc");
    if let Some(flag) = profile.cargo_flag() {
        label.push(' ');
        label.push_str(flag);
    }
    label.push_str(" -p zeo-rt");
    if runtime == Runtime::Eval {
        label.push_str(" --features eval-vm");
    }
    label.push_str(" --crate-type rlib,dylib --target-dir ");
    label.push_str(&variant_target_dir(runtime).display().to_string());
    label.push_str(" -- -C prefer-dynamic");
    label
}

/// Which cargo profile's `zeo-rt` a generated program links against.
///
/// `Debug` is the fast-iteration answer: the run-once `-e` path, the e2e harness,
/// and the conformance suite all use it so they never pay an optimized runtime
/// build. `Release` is for a SHIPPED artifact (`zeo foo.rb -o app`): it links
/// the release-profiled runtime, so the produced binary is small and fast
/// instead of embedding the unoptimized debug runtime. Keyed off intent
/// (`-o` output vs `-e`/throwaway).
///
/// Passed explicitly rather than read from the environment down here: the e2e
/// harness calls this from a dozen `#[test]` threads at once, and a `set_var`
/// racing a `var_os` is a real data race. The CLI reads the environment once,
/// at startup, while still single-threaded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    /// The caller's default, overridable by `ZEO_RUNTIME_PROFILE`
    /// (`debug`/`release`). Lets the conformance harness force the release
    /// runtime for its `-o` compiles (12x faster per-program link against the
    /// optimized runtime), and lets a developer force `debug` back on to get a
    /// symbolicated runtime when chasing a panic. An unrecognized value keeps
    /// the default.
    pub fn from_env_or(default: Profile) -> Self {
        match std::env::var_os("ZEO_RUNTIME_PROFILE")
            .as_deref()
            .and_then(|s| s.to_str())
        {
            Some("release") => Profile::Release,
            Some("debug") => Profile::Debug,
            _ => default,
        }
    }

    /// Positional index for the `ensure_runtime_built` memo array.
    fn idx(self) -> usize {
        match self {
            Profile::Debug => 0,
            Profile::Release => 1,
        }
    }

    /// The `target/` subdirectory cargo writes this profile's artifacts to.
    fn subdir(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }

    /// The `cargo build` flag that produces it (`Debug` is the default, no flag).
    fn cargo_flag(self) -> Option<&'static str> {
        match self {
            Profile::Debug => None,
            Profile::Release => Some("--release"),
        }
    }

    fn tag(self) -> &'static [u8] {
        match self {
            Profile::Debug => b"debug;",
            Profile::Release => b"release;",
        }
    }

    /// Extra `rustc` flags for the GENERATED crate when it follows the
    /// profile (`GenOpt::Optimized`): Release optimizes the generated code,
    /// not just the linked runtime rlib -- the generated main is where typed
    /// fast paths (inline Int arithmetic, direct calls) live, and at `-O0`
    /// those and every cross-crate `#[inline]` hint (`FrameGuard::push`,
    /// `set_line`) stay unoptimized calls. Debug keeps `-O0` for compile
    /// speed.
    ///
    /// BOTH strip symbols from the final binary. Most of a statically-linked
    /// program's size is the `zeo-rt` rlib's debug info, which is worthless
    /// here: zeo stamps its OWN Ruby backtraces (`stamp_backtrace`) and never
    /// relies on Rust-level symbols.
    /// Stripping shrinks each cached binary several-fold; for dynamic linkage,
    /// where the runtime is a shared dylib and the binary is already tiny, it
    /// still trims the last of the per-program symbols.
    fn rustc_flags(self) -> &'static [&'static str] {
        match self {
            // `-O0` frames are an order of magnitude fatter than optimized
            // ones, so a Debug binary gets a roomier main stack (64MB vs the
            // 8MB default): deep Ruby recursion that comfortably fits
            // optimized frames (the corpus's 2000-deep returning-proc chain)
            // overflows the default at `-O0`.
            Profile::Debug if cfg!(target_os = "macos") => &[
                "-C",
                "strip=symbols",
                "-C",
                "link-arg=-Wl,-stack_size,0x4000000",
            ],
            Profile::Debug => &["-C", "strip=symbols"],
            Profile::Release => &["-C", "opt-level=2", "-C", "strip=symbols"],
        }
    }
}

/// Which optimization level the GENERATED crate itself is compiled at --
/// deliberately decoupled from `Profile`, which also selects the runtime
/// artifact. The golden suites link the RELEASE runtime (fast per-program
/// link, and the hot paths all live in the runtime dylib) but build the
/// generated crate `Unoptimized`: `-O2` over a gem-scale generated main costs
/// rustc orders of magnitude more time for no output difference. `-o`/bench
/// artifacts stay
/// `Optimized` -- the generated main is where the typed fast paths live,
/// and a shipped binary is built once.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GenOpt {
    Optimized,
    Unoptimized,
}

impl GenOpt {
    /// The generated crate's `rustc` flags -- `Profile::rustc_flags` when
    /// following the profile, the Debug (strip-only) set when unoptimized.
    fn rustc_flags(self, profile: Profile) -> &'static [&'static str] {
        match self {
            GenOpt::Optimized => profile.rustc_flags(),
            GenOpt::Unoptimized => Profile::Debug.rustc_flags(),
        }
    }

    pub(super) fn tag(self) -> &'static [u8] {
        match self {
            GenOpt::Optimized => b"gen-opt;",
            GenOpt::Unoptimized => b"gen-o0;",
        }
    }
}

/// Which `zeo-rt` VARIANT a generated program links -- orthogonal to
/// `Profile` and `Linkage`.
///
/// `Lean` is the default runtime built by a plain `cargo build -p zeo-rt`:
/// parser-free, no `ruby-prism`, so the vast majority of programs (which never
/// reach a runtime `eval`) ship a small binary. `Eval` adds the `eval-vm`
/// feature -- and with it prism, a C library pulled in via bindgen/cc -- for the
/// programs `zeo` detects need prism at runtime: a runtime eval site, or a
/// `require "prism"` (`CompileOutput::needs_prism_runtime`). It keeps the
/// CARGO FEATURE's name, which is what it turns on; what has widened is the
/// set of reasons to want it. The two are built into SEPARATE target dirs
/// (see `variant_target_dir`) precisely because cargo cannot hold both feature
/// sets in one `target/<profile>/` at once.
///
/// Passed explicitly, like `Profile`: only the caller that compiled
/// the program knows whether it reaches prism.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Lean,
    Eval,
}

impl Runtime {
    /// Map the compiler's `needs_prism_runtime` verdict to a variant. The
    /// single place the boolean becomes a runtime choice, so every caller
    /// agrees. The variant keeps the CARGO FEATURE's name (`eval-vm`), which
    /// is what it actually turns on; what has widened is the set of reasons
    /// a program needs it.
    pub fn for_prism(needs_prism: bool) -> Self {
        if needs_prism {
            Runtime::Eval
        } else {
            Runtime::Lean
        }
    }

    /// Positional index for the `ensure_runtime_built` memo array.
    fn idx(self) -> usize {
        match self {
            Runtime::Lean => 0,
            Runtime::Eval => 1,
        }
    }

    fn tag(self) -> &'static [u8] {
        match self {
            Runtime::Lean => b"lean;",
            Runtime::Eval => b"eval;",
        }
    }
}

/// Whether a generated program links the runtime STATICALLY or DYNAMICALLY.
///
/// `Static` is for a SHIPPED artifact (`zeo foo.rb -o app`): the whole `zeo-rt`
/// is baked in, so the user gets one standalone file with no external
/// dependency. `Dynamic` is for the TEST corpus: the harness compiles thousands
/// of throwaway programs, and baking the runtime into each -- which
/// `-Wl,-dead_strip` cannot shrink, since the builtin dispatch tables reference
/// every method fn -- made the bin-cache enormous. Linking each against ONE
/// shared `libzeo_rt` drops it by orders of magnitude. Those binaries are only ever
/// run from within `target/` by the harness, so the rpaths baked in (the dylib
/// dir and [`std_libdir`]) always resolve; a shipped binary must never depend on
/// a dylib sitting in a build tree, hence `Static` there.
///
/// Passed explicitly, like `Profile`/`Runtime`: only the caller knows whether it
/// is producing a deliverable or a disposable test binary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Linkage {
    Static,
    Dynamic,
}

impl Linkage {
    /// Positional index for the `ensure_runtime_built` memo array.
    fn idx(self) -> usize {
        match self {
            Linkage::Static => 0,
            Linkage::Dynamic => 1,
        }
    }

    fn tag(self) -> &'static [u8] {
        match self {
            Linkage::Static => b"static;",
            Linkage::Dynamic => b"dynamic;",
        }
    }
}

/// The built runtime artifact a generated program links against: the static
/// `libzeo_rt.rlib`, or -- for dynamic linkage -- the shared `libzeo_rt.dylib`.
/// Errors (with the exact build command to run) when it isn't built yet.
fn runtime_artifact(
    profile: Profile,
    runtime: Runtime,
    linkage: Linkage,
) -> Result<PathBuf, String> {
    let dir = variant_target_dir(runtime).join(profile.subdir());
    let file = match linkage {
        Linkage::Static => "libzeo_rt.rlib".to_string(),
        Linkage::Dynamic => format!(
            "{}zeo_rt{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ),
    };
    let path = dir.join(&file);
    if !path.exists() {
        return Err(format!(
            "zeo-rt is not built: expected {} -- run `{}` \
             (or call `ensure_runtime_built` first)",
            path.display(),
            build_label(profile, runtime, linkage),
        ));
    }
    Ok(path)
}

/// Compiles `rust_source` to `output`, serving the content-keyed binary cache
/// when this exact source has been built before.
///
/// There is deliberately NO incremental-state variant. One existed, keyed by
/// the program's canonical path so an edit-run loop could reuse rustc's
/// unchanged codegen units, and it was retired as the cache's only shared
/// MUTABLE state -- see `cache::sweep_retired_incremental_state` for the two
/// ways it corrupted concurrent builds. Every entry this cache serves is
/// immutable once published, and that is what makes concurrent zeo processes
/// safe.
pub fn build_binary(
    rust_source: &str,
    output: &Path,
    profile: Profile,
    runtime: Runtime,
    linkage: Linkage,
    gen_opt: GenOpt,
) -> Result<(), String> {
    // Reclaim dead cache generations (old compiler/runtime) once per process,
    // off the hot path -- see `cache::maybe_sweep_stale_cache`.
    cache::maybe_sweep_stale_cache();
    // Pure: only LINKS the already-built runtime, never runs cargo or mutates
    // the workspace. Callers that can't assume a prior build (`zeo`'s own
    // CLI, the e2e harness) run `ensure_runtime_built` first; a driver that
    // prebuilds (the conformance harness) needs nothing here.
    let runtime_lib = runtime_artifact(profile, runtime, linkage)?;
    let variant_dir = variant_target_dir(runtime).join(profile.subdir());
    let deps_dir = variant_dir.join("deps");

    let cached = cache::cache_path(rust_source, profile, runtime, linkage, gen_opt)?;
    // A failed link is treated as a miss rather than an error: a concurrent
    // process pruning a stale generation can unlink an entry between the check
    // and the link, and rebuilding is always a correct answer.
    //
    // `is_usable_entry`, not a bare `exists()`: existence is not validity, and
    // treating it as validity makes any corrupt entry STICKY -- it is served
    // as a successful build forever, and the program silently does nothing.
    // That cost real debugging time (two examples "regressed" to empty output
    // with a green exit code). A miss is always safe; a false hit never is.
    //
    // `ZEO_CACHE=bypass` skips the read side only (the build still publishes),
    // so `xtask compile-bench` can measure the real rustc cost on a warm cache.
    let bypass = std::env::var_os("ZEO_CACHE").is_some_and(|v| v == "bypass");
    if !bypass && cache::is_usable_entry(&cached) && cache::link_or_copy(&cached, output).is_ok() {
        if crate::timings_enabled() {
            let bytes = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
            eprintln!("zeo-timings: rustc=cached bin_bytes={bytes}");
        }
        return Ok(());
    }

    // rustc writes into a private directory under a STABLE basename, then the
    // finished binary is renamed into place. The basename has to be the cache
    // key rather than something per-invocation: macOS ad-hoc-signs every arm64
    // binary and the signature's identifier defaults to the output filename, so
    // a unique filename here would make otherwise-identical builds differ.
    // Staging lives OUTSIDE the cache: a sweep of a stale generation is a
    // `remove_dir_all`, and when staging sat inside the cache that could delete
    // a directory another process was still writing rustc output into.
    let staging = std::env::temp_dir().join(format!(
        "zeo-staging-{}-{}",
        std::process::id(),
        cache::thread_unique_suffix()
    ));
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("creating {}: {e}", staging.display()))?;
    let staged = staging.join(
        cached
            .file_name()
            .expect("cache_path always has a file name"),
    );

    let src_path = cache::generated_source_path(rust_source);
    cache::write_atomically(&src_path, rust_source)?;

    let mut cmd = std::process::Command::new("rustc");
    cmd.arg("--edition")
        .arg("2021")
        .arg("--crate-name")
        .arg(cache::GENERATED_CRATE_NAME)
        .arg(&src_path)
        .arg("-o")
        .arg(&staged)
        .arg("--extern")
        .arg(format!("zeo_rt={}", runtime_lib.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps_dir.display()));
    cmd.args(gen_opt.rustc_flags(profile));
    if linkage == Linkage::Static {
        // Arms the generated crate's mimalloc `#[global_allocator]` (see
        // codegen's main assembly): a self-contained binary owns every
        // allocation, so the swap is safe there and only there.
        cmd.arg("--cfg").arg("zeo_static_alloc");
    }
    if linkage == Linkage::Dynamic {
        // Share ONE std with the dylib (a Rust dylib forces this), and bake the
        // rpaths that let the produced binary find both the runtime dylib and
        // that shared std at run time -- so it runs standalone, not just under a
        // DYLD_*-primed shell.
        cmd.arg("-C").arg("prefer-dynamic");
        cmd.arg("-C")
            .arg(format!("link-arg=-Wl,-rpath,{}", variant_dir.display()));
        match std_libdir() {
            Some(std_dir) => {
                cmd.arg("-C")
                    .arg(format!("link-arg=-Wl,-rpath,{}", std_dir.display()));
            }
            None => {
                let _ = std::fs::remove_dir_all(&staging);
                return Err("dynamic linkage needs the std lib dir, but `rustc --print \
                     target-libdir` failed"
                    .to_string());
            }
        }
    }
    let t_rustc = std::time::Instant::now();
    // Held (shared) across the link so the runtime artifact can't be
    // replaced out from under `ld` -- see `lock_runtime_shared`.
    let _runtime_read_lock = lock_runtime_shared();
    let status = cmd.status().map_err(|e| format!("running rustc: {e}"))?;

    if !status.success() {
        let _ = std::fs::remove_dir_all(&staging);
        // The build path skips the in-compiler syn validation; re-parse here
        // so a codegen token bug still reports as a zeo bug, not a bare
        // rustc error in a temp file.
        let triage = match syn::parse_file(rust_source) {
            Err(e) => format!(" (generated Rust fails to parse -- a zeo bug: {e})"),
            Ok(_) => String::new(),
        };
        return Err(format!(
            "rustc failed compiling the generated program (source at {}){triage}",
            src_path.display()
        ));
    }
    if crate::timings_enabled() {
        let bytes = std::fs::metadata(&staged).map(|m| m.len()).unwrap_or(0);
        eprintln!(
            "zeo-timings: rustc={}ms bin_bytes={bytes}",
            t_rustc.elapsed().as_millis()
        );
    }
    // Publish atomically. A concurrent build of the same source raced us to the
    // same key; since the key covers the whole input, whichever lands is
    // byte-identical to ours, so the loser is harmless.
    //
    // The generation dir can vanish between `cache_path` and here (a sweep or
    // `clean-cache` racing this build), so a failed rename recreates it and
    // retries once; if the cache stays unwritable, the staged binary itself
    // still answers the caller -- losing the cache entry must not fail the
    // build.
    let published = std::fs::rename(&staged, &cached).or_else(|_| {
        let _ = std::fs::create_dir_all(cached.parent().expect("cache_path always has a parent"));
        std::fs::rename(&staged, &cached)
    });
    let result = match published {
        Ok(()) => {
            cache::seal(&cached);
            cache::link_or_copy(&cached, output)
        }
        Err(_) => cache::link_or_copy(&staged, output),
    };
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// The runtime artifact's fingerprint (len + mtime), folded into the cache
/// generation so a rebuilt runtime retires the old cached binaries. Exposed to
/// the `cache` submodule, which owns the generation key. Errors when the
/// artifact isn't built (its generation is then uncomputable, by design).
fn runtime_artifact_fingerprint(
    profile: Profile,
    runtime: Runtime,
    linkage: Linkage,
) -> Result<String, String> {
    let lib = runtime_artifact(profile, runtime, linkage)?;
    let meta = std::fs::metadata(&lib).map_err(|e| format!("stat {}: {e}", lib.display()))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok(format!("zeo-rt:{}:{mtime};", meta.len()))
}

/// Which code-generation backend turns a compiled program into machine code.
///
/// `Rustc`: emitted Rust text -> `rustc` -> binary, linking the prebuilt
/// `zeo-rt` artifact -- the default during the dual period. `Aot`: the
/// Cranelift path -- HIR -> CLIF -> object file (`clif/`), linked against
/// `libzeo.a` (`link.rs`). `Jit`: the same CLIF finalized into THIS
/// process's memory and run in place (`jit.rs`) -- run mode only, no
/// artifact.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Rustc,
    Aot,
    Jit,
}

impl Backend {
    /// A `--backend`/`ZEO_BACKEND` value.
    pub fn parse(value: &str) -> Result<Backend, String> {
        match value {
            "rustc" => Ok(Backend::Rustc),
            "aot" => Ok(Backend::Aot),
            "jit" => Ok(Backend::Jit),
            other => Err(format!(
                "unknown backend `{other}` (expected rustc, aot, or jit)"
            )),
        }
    }

    /// The backend this invocation uses: the CLI flag, else `ZEO_BACKEND`,
    /// else the default for the MODE. Cranelift is the default backend, and
    /// the two Cranelift modes are not interchangeable: the JIT runs a
    /// program in place, and only the AOT backend produces the artifact
    /// `-o`/`--compile` asks for. `--backend rustc` stays selectable as the
    /// differential oracle until M3.
    pub fn select(cli: Option<Backend>, wants_artifact: bool) -> Result<Backend, String> {
        if let Some(backend) = cli {
            return Ok(backend);
        }
        match std::env::var("ZEO_BACKEND") {
            Ok(value) if !value.is_empty() => Backend::parse(&value),
            Ok(_) | Err(_) => Ok(if wants_artifact {
                Backend::Aot
            } else {
                Backend::Jit
            }),
        }
    }
}

/// A compiled program in whichever form its backend produced -- the input
/// the two mode entries below dispatch on.
pub enum CompiledProgram<'a> {
    Rustc(&'a crate::CompileOutput),
    Aot(&'a crate::ObjectOutput),
}

/// Run mode (`zeo file.rb`, `zeo -e`): produce a throwaway program for
/// `compiled`, run it with `program_args`, and exit this process with the
/// program's status. Never returns on success.
///
/// Rustc: compile to a temp binary and exec it. Run-once, so it DEFAULTS to
/// the fast-to-build `Debug` runtime (`ZEO_RUNTIME_PROFILE` overrides) and
/// DYNAMIC linkage -- the binary exists for milliseconds and runs only from
/// this machine's build tree, and static linkage made every `zeo file.rb`
/// pay a full runtime link just to print and exit.
pub fn run_program(
    compiled: &CompiledProgram<'_>,
    program_args: &[String],
) -> Result<std::convert::Infallible, String> {
    match compiled {
        CompiledProgram::Rustc(compiled) => {
            let runtime = Runtime::for_prism(compiled.needs_prism_runtime);
            let linkage = Linkage::Dynamic;
            let profile = Profile::from_env_or(Profile::Debug);
            ensure_runtime_built(profile, runtime, linkage)?;
            let bin = std::env::temp_dir().join(format!("zeo-e-{}", std::process::id()));
            // An unchanged program is served from the content-keyed binary
            // cache; a changed one is a full rustc run. The path-keyed
            // incremental state that used to sit between those two was
            // retired -- it was the cache's only shared mutable state, and
            // it broke concurrent builds (see `build_binary`).
            build_binary(
                &compiled.rust_source,
                &bin,
                profile,
                runtime,
                linkage,
                GenOpt::Optimized,
            )?;
            let status = std::process::Command::new(&bin)
                .args(program_args)
                .status()
                .map_err(|e| format!("running compiled program: {e}"))?;
            let _ = std::fs::remove_file(&bin);
            std::process::exit(status.code().unwrap_or(1));
        }
        // Aot run mode links a throwaway binary and runs it -- the
        // in-process JIT replaces this at M0.5.
        CompiledProgram::Aot(compiled) => {
            let bin = std::env::temp_dir().join(format!("zeo-e-{}", std::process::id()));
            object::object_to_binary(&compiled.object, &bin)?;
            let status = std::process::Command::new(&bin)
                .args(program_args)
                .status()
                .map_err(|e| format!("running compiled program: {e}"))?;
            let _ = std::fs::remove_file(&bin);
            std::process::exit(status.code().unwrap_or(1));
        }
    }
}

/// Artifact mode (`zeo file.rb -o app`, `--compile`): produce the SHIPPED
/// binary at `output` for `compiled`.
///
/// Rustc: SELF-CONTAINED (static runtime -- an artifact must never depend on
/// a dylib in a build tree), DEFAULTED to the release-profiled runtime
/// (optimized + stripped); `ZEO_RUNTIME_PROFILE` overrides (e.g. to
/// symbolicate a runtime panic).
pub fn build_artifact(compiled: &CompiledProgram<'_>, output: &Path) -> Result<(), String> {
    match compiled {
        CompiledProgram::Rustc(compiled) => {
            let runtime = Runtime::for_prism(compiled.needs_prism_runtime);
            let linkage = Linkage::Static;
            let profile = Profile::from_env_or(Profile::Release);
            ensure_runtime_built(profile, runtime, linkage)?;
            build_binary(
                &compiled.rust_source,
                output,
                profile,
                runtime,
                linkage,
                GenOpt::Optimized,
            )
        }
        CompiledProgram::Aot(compiled) => object::object_to_binary(&compiled.object, output),
    }
}
