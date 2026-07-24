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
//! A generated program links the runtime either STATICALLY (the whole `zeo-rt`
//! baked into a self-contained binary, for a shipped `zeo foo.rb -o app`) or
//! DYNAMICALLY (against a shared `libzeo_rt.dylib`, for the throwaway test
//! corpus). See [`Linkage`] -- dynamic linking is what keeps the golden/e2e
//! bin-cache from ballooning, since the static runtime can't be dead-stripped
//! (the builtin dispatch tables reference every method fn).
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

/// The target ROOT a (runtime, linkage) combination's artifacts live under
/// (before the profile subdir).
///
/// Each combination needs its OWN dir because cargo writes every build to the
/// same `libzeo_rt.*` path: a lean build and an `--features eval-vm` build would
/// clobber each other, and so would a static (rlib) build and a
/// `prefer-dynamic` dylib build. Only the lean-static default lands in the
/// ordinary `target/` (where a plain `cargo build -p zeo-rt` puts it); the other
/// three get a `--target-dir` redirect, all still under `target/` so `cargo
/// clean` reaps them. See [`Runtime`] and [`Linkage`].
fn variant_target_dir(runtime: Runtime, linkage: Linkage) -> PathBuf {
    let base = target_dir();
    match (linkage, runtime) {
        (Linkage::Static, Runtime::Lean) => base,
        (Linkage::Static, Runtime::Eval) => base.join("zeo-rt-eval"),
        (Linkage::Dynamic, Runtime::Lean) => base.join("zeo-rt-dyn"),
        (Linkage::Dynamic, Runtime::Eval) => base.join("zeo-rt-dyn-eval"),
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
/// build-lock contention that made an unconditional per-call `cargo build` cost
/// the conformance suite ~153s.
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

/// Shell out to cargo for one (profile, runtime, linkage) combination,
/// unconditionally (cargo itself no-ops when the artifact is fresh and rebuilds
/// it when stale). This is the freshness-correct entry a prebuild step calls;
/// `ensure_runtime_built` wraps it behind a stat gate for the pure-link
/// entrypoints that can assume a prior prebuild.
///
/// STATIC builds a plain rlib via `cargo build`. DYNAMIC builds a shared
/// `libzeo_rt.dylib` via `cargo rustc --crate-type dylib -- -C prefer-dynamic`
/// (so it links std as a shared dylib -- see [`std_libdir`]) with an `@rpath`
/// install name, so a consumer that adds the right rpaths finds it. The `Eval`
/// variant adds `--features eval-vm` (so prism links in). Every non-default
/// combination builds into its own target dir so it never clobbers another --
/// see [`variant_target_dir`].
pub fn build_runtime(profile: Profile, runtime: Runtime, linkage: Linkage) -> Result<(), String> {
    let mut cmd = std::process::Command::new("cargo");
    match linkage {
        Linkage::Static => {
            cmd.arg("build").arg("--quiet");
            if let Some(flag) = profile.cargo_flag() {
                cmd.arg(flag);
            }
            cmd.args(["-p", "zeo-rt"]);
            if runtime == Runtime::Eval {
                cmd.arg("--features").arg("eval-vm");
                cmd.arg("--target-dir").arg(variant_target_dir(runtime, linkage));
            }
        }
        Linkage::Dynamic => {
            // `cargo rustc` so we can force the dylib crate-type and pass
            // `prefer-dynamic` to the FINAL crate only (its rlib deps still link
            // normally). Always its own target dir -- a `prefer-dynamic` build
            // must not clobber the static rlib.
            cmd.arg("rustc").arg("--quiet");
            if let Some(flag) = profile.cargo_flag() {
                cmd.arg(flag);
            }
            cmd.args(["-p", "zeo-rt"]);
            if runtime == Runtime::Eval {
                cmd.arg("--features").arg("eval-vm");
            }
            cmd.arg("--crate-type").arg("dylib");
            cmd.arg("--target-dir").arg(variant_target_dir(runtime, linkage));
            cmd.arg("--");
            cmd.arg("-C").arg("prefer-dynamic");
            cmd.arg("-C").arg(install_name_arg());
        }
    }
    cmd.current_dir(workspace_root());
    let label = build_label(profile, runtime, linkage);
    match cmd.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("`{label}` exited with {status}")),
        Err(e) => Err(format!("running `{label}`: {e}")),
    }
}

/// The `-C link-arg=...` that stamps the dylib's own name so a consumer resolves
/// it through its rpath rather than a fixed absolute path. macOS uses an
/// `@rpath` install name; ELF platforms use a bare soname. (Only the macOS path
/// is exercised today; the ELF form mirrors the standard shared-object recipe.)
fn install_name_arg() -> String {
    let file = format!("{}zeo_rt{}", std::env::consts::DLL_PREFIX, std::env::consts::DLL_SUFFIX);
    if cfg!(target_os = "macos") {
        format!("link-arg=-Wl,-install_name,@rpath/{file}")
    } else {
        format!("link-arg=-Wl,-soname,{file}")
    }
}

/// The human-readable cargo invocation for an error message -- reflects the
/// static (`cargo build`) vs dynamic (`cargo rustc --crate-type dylib`) shape,
/// the profile flag, the `Eval` feature, and the redirected target dir.
fn build_label(profile: Profile, runtime: Runtime, linkage: Linkage) -> String {
    let mut label = String::from(match linkage {
        Linkage::Static => "cargo build",
        Linkage::Dynamic => "cargo rustc",
    });
    if let Some(flag) = profile.cargo_flag() {
        label.push(' ');
        label.push_str(flag);
    }
    label.push_str(" -p zeo-rt");
    if runtime == Runtime::Eval {
        label.push_str(" --features eval-vm");
    }
    if linkage == Linkage::Dynamic {
        label.push_str(" --crate-type dylib");
    }
    if runtime == Runtime::Eval || linkage == Linkage::Dynamic {
        label.push_str(" --target-dir ");
        label.push_str(&variant_target_dir(runtime, linkage).display().to_string());
    }
    if linkage == Linkage::Dynamic {
        label.push_str(" -- -C prefer-dynamic");
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

    /// Extra `rustc` flags for the GENERATED crate itself. Release optimizes
    /// the generated code, not just the linked runtime rlib: the generated
    /// main is where typed fast paths (inline Int arithmetic, direct calls)
    /// live, and at `-O0` those and every cross-crate `#[inline]` hint
    /// (`FrameGuard::push`, `set_line`) stay unoptimized calls. Debug keeps
    /// `-O0` for compile speed.
    ///
    /// BOTH strip symbols from the final binary. The bulk of a statically-linked
    /// generated program's size is the `zeo-rt` rlib's debug info (a Debug entry
    /// was ~15MB, mostly symbols) -- worthless here because zeo stamps its OWN
    /// Ruby backtraces (`stamp_backtrace`), never relying on Rust-level symbols.
    /// Stripping shrinks each cached binary several-fold; for dynamic linkage,
    /// where the runtime is a shared dylib and the binary is already tiny, it
    /// still trims the last of the per-program symbols.
    fn rustc_flags(self) -> &'static [&'static str] {
        match self {
            Profile::Debug => &["-C", "strip=symbols"],
            Profile::Release => &["-C", "opt-level=2", "-C", "strip=symbols"],
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
/// dependency. `Dynamic` is for the TEST corpus: the golden/e2e harness compiles
/// thousands of throwaway programs, and baking the ~6MB runtime into each --
/// which `-Wl,-dead_strip` can't shrink, since the builtin dispatch tables
/// reference every method fn -- cost ~14GB of bin-cache. Linking each against
/// ONE shared `libzeo_rt.dylib` drops it to ~100KB. Those binaries are only ever
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
    let dir = variant_target_dir(runtime, linkage).join(profile.subdir());
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

pub fn build_binary(
    rust_source: &str,
    output: &Path,
    profile: Profile,
    runtime: Runtime,
    linkage: Linkage,
) -> Result<(), String> {
    // Reclaim dead cache generations (old compiler/runtime) once per process,
    // off the hot path -- see `cache::maybe_sweep_stale_cache`.
    cache::maybe_sweep_stale_cache();
    // Pure: only LINKS the already-built runtime, never runs cargo or mutates
    // the workspace. Callers that can't assume a prior build (`zeo`'s own
    // CLI, the e2e harness) run `ensure_runtime_built` first; a driver that
    // prebuilds (the conformance harness) needs nothing here.
    let runtime_lib = runtime_artifact(profile, runtime, linkage)?;
    let variant_dir = variant_target_dir(runtime, linkage).join(profile.subdir());
    let deps_dir = variant_dir.join("deps");

    let cached = cache::cache_path(rust_source, profile, runtime, linkage)?;
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
                return Err(
                    "dynamic linkage needs the std lib dir, but `rustc --print \
                     target-libdir` failed"
                        .to_string(),
                );
            }
        }
    }
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
