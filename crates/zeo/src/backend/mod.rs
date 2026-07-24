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
//! This deliberately does NOT enumerate `zeo-rt`'s own transitive
//! dependencies (`indexmap`/`parking_lot`/`regex`/...) by hand: `rustc`
//! resolves those automatically via the `-L` search path, using the crate
//! metadata already embedded in the built `zeo-rt` itself -- the generated
//! program only ever references `zeo_rt` directly (`use zeo_rt::...`),
//! never its transitive deps by name, so only ONE `--extern` is ever needed.
//! Confirmed both correct (a real generated program links and runs) and
//! dramatically cheaper this way: a fresh-`cargo`-project build (the
//! previous approach here) recompiled `zeo-rt` and its whole dependency
//! graph from scratch on EVERY call (no target-dir reuse across throwaway
//! projects), costing ~3s per call; direct `rustc` against the already-built
//! artifacts costs ~0.2s. This is the same "compile one generated file
//! against already-built deps" shape tools like `trybuild`/`compiletest` use
//! for the identical reason.
//!
//! `build_binary` is PURE: it only links an already-built `zeo-rt` and errors
//! clearly if the artifact is missing. It never runs cargo and never mutates the
//! workspace, so parallel `zeo` subprocesses never contend on Cargo's
//! exclusive build-directory lock. Building the runtime is a separate, explicit
//! step (`ensure_runtime_built`) that an entrypoint which can't assume a prior
//! workspace build calls first -- the CLI (`main.rs`) on a fresh tree, and the
//! in-process test harness (`tests/support`), since `zeo`'s own `Cargo.toml`
//! has no dependency on `zeo-rt`. A driver that prebuilds the workspace (the
//! conformance harness's `prebuild`) needs neither.
//!
//! `ensure_runtime_built` lives here (not `main.rs`) so the CLI and the
//! in-process test harness call the exact same logic rather than re-derived
//! copies.
//!
//! The content-addressed binary CACHE that keeps the golden corpus from
//! recompiling identical programs lives in the `cache` submodule; this file is
//! the compile/link + runtime-build path the CLI actually needs.

mod cache;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

/// The workspace crates whose sources are inputs to the `zeo-rt` artifact --
/// its own crate plus its workspace path-dependency closure. `ensure_runtime_built`
/// stat-walks exactly these to decide whether a built runtime is stale, so this
/// list MUST track `zeo-rt`'s `path = "..."` dependencies (see its
/// `Cargo.toml`). Getting it wrong under-scopes the freshness check and lets a
/// stale runtime be linked silently -- the very bug the check exists to prevent.
const RUNTIME_CRATES: &[&str] = &["zeo-rt", "zeo-abi", "zeo-enc", "zeo-fiber"];

/// The workspace root -- two levels up from `crates/zeo` (this crate's
/// own `CARGO_MANIFEST_DIR`), i.e. wherever the top-level `Cargo.toml`/
/// `target/` actually live.
fn workspace_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// Respects `CARGO_TARGET_DIR` (the standard Cargo override) if set, else
/// the ordinary `<workspace_root>/target` default -- not a full `cargo
/// metadata` query (this project's build layout is simple enough that the
/// common cases are all that's needed).
fn target_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => workspace_root().join("target"),
    }
}

/// The target ROOT a runtime variant's artifacts live under (before the profile
/// subdir). The `Lean` (default, parser-free) runtime uses the ordinary
/// `target/`, exactly where `cargo build -p zeo-rt` puts it. The `Eval`
/// variant CANNOT share it: cargo writes every feature-combination to the same
/// `libzeo_rt.rlib` path, so a lean build and an `--features eval-vm` build
/// would clobber each other on every alternation. Giving `Eval` its own
/// `target/zeo-rt-eval/` (a `--target-dir` cargo redirect) lets both coexist,
/// still under `target/` so `cargo clean` reaps them. See `Runtime`.
fn variant_target_dir(runtime: Runtime) -> PathBuf {
    match runtime {
        Runtime::Lean => target_dir(),
        Runtime::Eval => target_dir().join("zeo-rt-eval"),
    }
}

/// Ensure the `zeo-rt` runtime artifact `build_binary` links against exists.
///
/// `build_binary` is pure -- it only LINKS an already-built runtime -- so an
/// entrypoint that can't assume a prior workspace build calls this first: the
/// CLI on a fresh tree (so `zeo foo.rb` just works), and the in-process e2e
/// harness (`cargo test -p zeo` doesn't build `zeo-rt`, since `zeo`
/// has no cargo dependency on it). It builds AT MOST ONCE per process, and only
/// when the artifact is missing OR STALE -- a cheap stat-only freshness gate
/// ([`runtime_artifact_is_stale`]) compares the runtime crates' sources against
/// the built artifact, so an edited `zeo-rt` is rebuilt rather than silently
/// linked stale (zeo has no cargo dependency edge that would catch it). When
/// the artifact is already current -- the common case under a driver that
/// prebuilt the workspace (the conformance harness) -- the gate is a handful of
/// `stat`s that skip the build, with none of the cargo build-lock contention that
/// made an unconditional per-call `cargo build` cost the conformance suite ~153s.
pub fn ensure_runtime_built(profile: Profile, runtime: Runtime) -> Result<(), String> {
    // Memoized PER (PROFILE, VARIANT): a single process almost always touches
    // one combination (the CLI's `-e` and the harness build `Debug`; only
    // `zeo foo.rb -o app` builds `Release`; and only a program that reaches
    // eval asks for `Eval`), but keying the once-cells by both axes keeps it
    // correct if several are exercised, and still collapses the e2e harness's
    // many `#[test]` threads to one build per combination.
    static DEBUG_LEAN: OnceLock<Result<(), String>> = OnceLock::new();
    static DEBUG_EVAL: OnceLock<Result<(), String>> = OnceLock::new();
    static RELEASE_LEAN: OnceLock<Result<(), String>> = OnceLock::new();
    static RELEASE_EVAL: OnceLock<Result<(), String>> = OnceLock::new();
    let cell = match (profile, runtime) {
        (Profile::Debug, Runtime::Lean) => &DEBUG_LEAN,
        (Profile::Debug, Runtime::Eval) => &DEBUG_EVAL,
        (Profile::Release, Runtime::Lean) => &RELEASE_LEAN,
        (Profile::Release, Runtime::Eval) => &RELEASE_EVAL,
    };
    cell.get_or_init(|| {
        // Fresh already -- the common case (harness prebuild, or a prior build in
        // this tree).
        //
        // Freshness, not bare existence: an entrypoint here can't assume a prior
        // `build_runtime`, and `zeo` has no cargo dependency on `zeo-rt`,
        // so nothing else would rebuild an edited runtime. The stat gate catches
        // that. When it reports fresh, cargo is never invoked -- so a driver that
        // prebuilt the workspace still gets the cheap path. When stale (or
        // missing), `build_runtime` shells `cargo build`, which does the real
        // incremental rebuild.
        if !runtime_artifact_is_stale(profile, runtime) {
            return Ok(());
        }
        build_runtime(profile, runtime)
    })
    .clone()
}

/// Whether the built `zeo-rt` artifact is missing or older than any source
/// cargo would treat as an input to it -- a `stat`-only gate so
/// [`ensure_runtime_built`] rebuilds a STALE runtime instead of linking it,
/// without paying a `cargo build` lock round-trip on the already-fresh path.
///
/// Scans exactly the runtime's workspace dependency closure ([`RUNTIME_CRATES`])
/// plus the resolved lockfile (external-dep bumps), comparing the newest source
/// mtime against the artifact's. Missing artifact -> stale (must build). This is
/// an mtime heuristic, not cargo's full fingerprint, but it only ever
/// over-reports staleness (a redundant `cargo build` that no-ops), never
/// under-reports for an in-tree edit -- so it cannot reintroduce the silent-stale
/// bug.
fn runtime_artifact_is_stale(profile: Profile, runtime: Runtime) -> bool {
    let Ok(artifact) = rlib_for("zeo-rt", profile, runtime) else {
        return true; // not built yet
    };
    let Some(artifact_mtime) = file_mtime(&artifact) else {
        return true;
    };
    let root = workspace_root();
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

/// Shell out to `cargo build` for one runtime variant, unconditionally (cargo
/// itself no-ops when the artifact is fresh and rebuilds it when stale). This is
/// the freshness-correct entry a prebuild step calls; `ensure_runtime_built`
/// wraps it behind an existence check for the pure-link entrypoints that can
/// assume a prior prebuild.
///
/// The `Eval` variant is `default + eval-vm` (so prism links in), built into its
/// OWN target dir so it never clobbers the lean `libzeo_rt` -- see
/// `variant_target_dir`.
pub fn build_runtime(profile: Profile, runtime: Runtime) -> Result<(), String> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("build").arg("--quiet");
    if let Some(flag) = profile.cargo_flag() {
        cmd.arg(flag);
    }
    cmd.args(["-p", "zeo-rt"]);
    if runtime == Runtime::Eval {
        cmd.arg("--features").arg("eval-vm");
        cmd.arg("--target-dir").arg(variant_target_dir(runtime));
    }
    cmd.current_dir(workspace_root());
    let label = build_label(profile, runtime);
    match cmd.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("`cargo build {label}` exited with {status}")),
        Err(e) => Err(format!("running `cargo build {label}`: {e}")),
    }
}

/// The human-readable `cargo build ...` invocation for an error message --
/// reflects the profile flag and the `Eval` variant's extra args.
fn build_label(profile: Profile, runtime: Runtime) -> String {
    let mut label = String::new();
    if let Some(flag) = profile.cargo_flag() {
        label.push_str(flag);
        label.push(' ');
    }
    label.push_str("-p zeo-rt");
    if runtime == Runtime::Eval {
        label.push_str(" --features eval-vm --target-dir ");
        label.push_str(&variant_target_dir(runtime).display().to_string());
    }
    label
}

/// Which cargo profile's `zeo-rt` a generated program links against.
///
/// `Debug` is the fast-iteration answer: the run-once `-e` path, the e2e harness,
/// and the conformance suite all use it so they never pay an optimized runtime
/// build. `Release` is for a SHIPPED artifact (`zeo foo.rb -o app`): it links
/// the release-profiled runtime (optimized + stripped, see `[profile.release]`),
/// so the produced binary is small and fast instead of embedding the ~10MB
/// unoptimized debug runtime. Keyed off intent (`-o` output vs `-e`/throwaway).
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

    /// Extra `rustc` flags for the GENERATED crate itself. Release optimizes
    /// the generated code, not just the linked runtime rlib: the generated
    /// main is where typed fast paths (inline Int arithmetic, direct calls)
    /// live, and at `-O0` those and every cross-crate `#[inline]` hint
    /// (`FrameGuard::push`, `set_line`) stay unoptimized calls. Debug keeps
    /// `-O0` for compile speed.
    ///
    /// BOTH strip symbols from the final binary. The bulk of a generated
    /// program's size is the statically-linked `zeo-rt` rlib's debug info (a
    /// Debug entry was ~15MB, mostly symbols) -- worthless here because zeo
    /// stamps its OWN Ruby backtraces (`stamp_backtrace`), never relying on
    /// Rust-level symbols. Stripping shrinks each cached binary several-fold,
    /// which matters across the thousands of entries the golden corpus builds.
    fn rustc_flags(self) -> &'static [&'static str] {
        match self {
            Profile::Debug => &["-C", "strip=symbols"],
            Profile::Release => &["-C", "opt-level=2", "-C", "strip=symbols"],
        }
    }
}

/// Which `zeo-rt` VARIANT a generated program links -- orthogonal to
/// `Profile`.
///
/// `Lean` is the default runtime built by a plain `cargo build -p zeo-rt`:
/// parser-free, no `ruby-prism`, so the vast majority of programs (which never
/// reach a runtime `eval`) ship a small binary. `Eval` adds the `eval-vm`
/// feature -- and with it prism, a C library pulled in via bindgen/cc -- for the
/// programs `zeo` detects can reach the runtime eval VM
/// (`CompileOutput::needs_eval_vm`). The two are built into SEPARATE target dirs
/// (see `variant_target_dir`) precisely because cargo cannot hold both feature
/// sets in one `target/<profile>/` at once.
///
/// Passed explicitly, like `Profile`: only the caller that compiled
/// the program knows whether it reached an eval site.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Lean,
    Eval,
}

impl Runtime {
    /// Map the compiler's `needs_eval_vm` verdict to a variant. The single
    /// place the boolean becomes a runtime choice, so every caller agrees.
    pub fn for_eval(needs_eval_vm: bool) -> Self {
        if needs_eval_vm {
            Runtime::Eval
        } else {
            Runtime::Lean
        }
    }

    fn tag(self) -> &'static [u8] {
        match self {
            Runtime::Lean => b"lean;",
            Runtime::Eval => b"eval;",
        }
    }
}

/// The built `rlib` a generated program links against -- every program links
/// the runtime statically, so the produced binary is self-contained.
fn rlib_for(crate_name: &str, profile: Profile, runtime: Runtime) -> Result<PathBuf, String> {
    let underscored = crate_name.replace('-', "_");
    let dir = variant_target_dir(runtime).join(profile.subdir());
    let rlib = dir.join(format!("lib{underscored}.rlib"));
    if !rlib.exists() {
        return Err(format!(
            "{crate_name} is not built: expected {} -- run `cargo build {}` \
             (or call `ensure_runtime_built` first)",
            rlib.display(),
            build_label(profile, runtime),
        ));
    }
    Ok(rlib)
}

pub fn build_binary(
    rust_source: &str,
    output: &Path,
    profile: Profile,
    runtime: Runtime,
) -> Result<(), String> {
    // Reclaim dead cache generations (old compiler/runtime) once per process,
    // off the hot path -- see `cache::maybe_sweep_stale_cache`.
    cache::maybe_sweep_stale_cache();
    // Pure: only LINKS the already-built runtime, never runs cargo or mutates
    // the workspace. Callers that can't assume a prior build (`zeo`'s own
    // CLI, the e2e harness) run `ensure_runtime_built` first; a driver that
    // prebuilds (the conformance harness) needs nothing here.
    let runtime_lib = rlib_for("zeo-rt", profile, runtime)?;
    let deps_dir = variant_target_dir(runtime)
        .join(profile.subdir())
        .join("deps");

    let cached = cache::cache_path(rust_source, profile, runtime)?;
    // A failed link is treated as a miss rather than an error: a concurrent
    // process pruning a stale generation can unlink an entry between the check
    // and the link, and rebuilding is always a correct answer.
    //
    // `is_usable_entry`, not a bare `exists()`: existence is not validity, and
    // treating it as validity makes any corrupt entry STICKY -- it is served
    // as a successful build forever, and the program silently does nothing.
    // That cost real debugging time (two examples "regressed" to empty output
    // with a green exit code). A miss is always safe; a false hit never is.
    if cache::is_usable_entry(&cached) && cache::link_or_copy(&cached, output).is_ok() {
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
    cmd.args(profile.rustc_flags());
    let status = cmd.status().map_err(|e| format!("running rustc: {e}"))?;

    if !status.success() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!(
            "rustc failed compiling the generated program (source at {})",
            src_path.display()
        ));
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
