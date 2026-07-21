//! Compiles generated Rust source directly against the WORKSPACE's own
//! already-built `zeo-rt` artifacts -- a single `rustc` invocation with
//! `--extern zeo_rt=<the built lib>` and `-L dependency=target/debug/deps`,
//! not a fresh throwaway `cargo` project.
//!
//! Two things guard the identity of what comes out. `--crate-name` is pinned
//! (see `GENERATED_CRATE_NAME`) and the temp source is content-addressed (see
//! `generated_source_path`), because `rustc` otherwise derives the crate name
//! from the source FILENAME and embeds that path as panic-location metadata --
//! either one makes identical Ruby compile to different bytes. With both, the
//! output is a pure function of the generated source, which is what lets
//! `cache_path` hand back an earlier build instead of re-running `rustc`.
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

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

/// The workspace crates whose sources are inputs to the `zeo-rt` artifact --
/// its own crate plus its workspace path-dependency closure. `ensure_runtime_built`
/// stat-walks exactly these to decide whether a built runtime is stale, so this
/// list MUST track `zeo-rt`'s `path = "..."` dependencies (see its
/// `Cargo.toml`). Getting it wrong under-scopes the freshness check and lets a
/// stale runtime be linked silently -- the very bug the check exists to prevent.
const RUNTIME_CRATES: &[&str] = &["zeo-rt", "zeo-abi", "zeo-fiber"];

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
    // Pure: only LINKS the already-built runtime, never runs cargo or mutates
    // the workspace. Callers that can't assume a prior build (`zeo`'s own
    // CLI, the e2e harness) run `ensure_runtime_built` first; a driver that
    // prebuilds (the conformance harness) needs nothing here.
    let runtime_lib = rlib_for("zeo-rt", profile, runtime)?;
    let deps_dir = variant_target_dir(runtime)
        .join(profile.subdir())
        .join("deps");

    let cached = cache_path(rust_source, profile, runtime)?;
    // A failed link is treated as a miss rather than an error: a concurrent
    // process pruning a stale generation can unlink an entry between the check
    // and the link, and rebuilding is always a correct answer.
    //
    // `is_usable_entry`, not a bare `exists()`: existence is not validity, and
    // treating it as validity makes any corrupt entry STICKY -- it is served
    // as a successful build forever, and the program silently does nothing.
    // That cost real debugging time (two examples "regressed" to empty output
    // with a green exit code). A miss is always safe; a false hit never is.
    if is_usable_entry(&cached) && link_or_copy(&cached, output).is_ok() {
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
        thread_unique_suffix()
    ));
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("creating {}: {e}", staging.display()))?;
    let staged = staging.join(
        cached
            .file_name()
            .expect("cache_path always has a file name"),
    );

    let src_path = generated_source_path(rust_source);
    write_atomically(&src_path, rust_source)?;

    let mut cmd = std::process::Command::new("rustc");
    cmd.arg("--edition")
        .arg("2021")
        .arg("--crate-name")
        .arg(GENERATED_CRATE_NAME)
        .arg(&src_path)
        .arg("-o")
        .arg(&staged)
        .arg("--extern")
        .arg(format!("zeo_rt={}", runtime_lib.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps_dir.display()));
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
            seal(&cached);
            link_or_copy(&cached, output)
        }
        Err(_) => link_or_copy(&staged, output),
    };
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Marks a published cache entry read-only.
///
/// Not hygiene -- this closes a real corruption channel. `link_or_copy` hands
/// the caller a HARD LINK to this very inode, so the cache entry and the
/// caller's output file are the same bytes. Anything that then writes to that
/// output IN PLACE (a shell redirect, an interrupted writer, two concurrent
/// builds racing on one `-o` path) truncates the SHARED entry, and the cache
/// serves the wreckage to every later build of that program.
///
/// Reproduced exactly that way: `zeo x.rb -o x.bin` then `: > x.bin` left
/// the cache entry at 0 bytes, and every subsequent build of `x.rb` "succeeded"
/// instantly with an empty binary.
///
/// Read-only makes that write fail instead of silently succeeding. `0o555`
/// keeps the execute bit, which the caller's output still needs; replacing an
/// output is unaffected, since unlinking depends on the DIRECTORY's permissions
/// rather than the file's (`link_or_copy` unlinks before linking).
///
/// Best-effort: a filesystem that refuses the chmod costs us the hardening, not
/// the build -- and `is_usable_entry` still catches the damage on read.
fn seal(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o555));
    }
}

/// Whether a cache entry can be served as a finished build.
///
/// A zero-length file is never a valid executable, so it can only be damage --
/// see `seal` for the channel that produced one. Treating it as a miss makes
/// the cache self-healing: the entry is rebuilt and republished, and an already
/// poisoned cache recovers on the next build rather than needing a manual
/// sweep.
///
/// Length is the whole check on purpose. Anything stronger (a hash of the
/// contents) would re-read every cached binary on every hit -- megabytes per
/// lookup, which is precisely the cost the cache exists to avoid -- to defend
/// against corruption shapes that have never occurred. The cheap check covers
/// the one that has.
fn is_usable_entry(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// Where compiled generated programs are kept, keyed by their full input.
///
/// Lives under `target/` so `cargo clean` reaps it and it stays out of
/// `$TMPDIR`.
fn cache_dir() -> PathBuf {
    target_dir().join("zeo-bin-cache")
}

/// The cache slot for this exact program: `<rlib generation>/<source hash>`.
///
/// Split in two so a generation can be swept wholesale. Every entry under a
/// generation dies the moment the runtime is rebuilt -- generated programs link
/// `libzeo_rt.rlib` statically -- and a generation runs to several GB across
/// the conformance corpus (measured 2026-07: ~5.5MB per release entry, ~15MB
/// per debug entry), so a flat keyspace grew by that much per runtime rebuild
/// and never shrank. `sweep_stale_cache_generations` reclaims dead generations
/// at conformance-prebuild time.
///
/// The rlibs are keyed by len+mtime rather than content: `zeo` runs as a
/// fresh process per case under the conformance harness, so a content hash of
/// the 32MB rlib could not be amortized and would cost more than it saves.
/// Cargo does not touch mtimes on a no-op rebuild, so this only
/// over-invalidates when the runtime genuinely got rebuilt.
fn cache_path(rust_source: &str, profile: Profile, runtime: Runtime) -> Result<PathBuf, String> {
    let generation = generation_hash(profile, runtime)?;
    let root = cache_dir();
    std::fs::create_dir_all(&root).map_err(|e| format!("creating {}: {e}", root.display()))?;
    let dir = root.join(format!("{generation:016x}"));
    // Idempotent: many processes publish into the same generation concurrently,
    // and each just ensures the directory exists. Stale generations are NOT
    // swept here -- a `remove_dir_all` in the build hot path could delete a
    // directory a sibling process is still writing rustc output into (seen as
    // spurious FAIL_RUSTC across the corpus). Reclamation happens between runs
    // instead: automatically by `sweep_stale_cache_generations` (the
    // conformance prebuild, under its run lock), or wholesale by
    // `xtask conformance clean-cache`.
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    Ok(dir.join(format!("{:016x}", fnv1a64(rust_source.as_bytes()))))
}

/// The cache generation for one (profile, runtime) combination -- shared by
/// `cache_path` and `sweep_stale_cache_generations` so the writer and the
/// sweeper can never disagree on a generation's name.
///
/// Errors when that combination's rlib isn't built -- in which case no process
/// can compute (and so write into) that generation either.
fn generation_hash(profile: Profile, runtime: Runtime) -> Result<u64, String> {
    // Profile AND runtime variant are part of the generation, not just details:
    // the same source compiles to a debug or a release binary (profile), and to
    // a lean or a prism-carrying binary (runtime) -- handing a caller the wrong
    // kind would bloat their output or link a runtime whose `eval` is a
    // `NotImplementedError` stub. Each variant's own rlib is also stat'd below
    // (they live in different target dirs), so their mtimes already distinguish
    // them; the tags make the intent explicit and collision-proof.
    let mut generation = fnv1a64_with(0xcbf2_9ce4_8422_2325, profile.tag());
    generation = fnv1a64_with(generation, runtime.tag());
    let name = "zeo-rt";
    let lib = rlib_for(name, profile, runtime)?;
    let meta = std::fs::metadata(&lib).map_err(|e| format!("stat {}: {e}", lib.display()))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok(fnv1a64_with(
        generation,
        format!("{name}:{}:{mtime};", meta.len()).as_bytes(),
    ))
}

/// Removes every cache generation no current runtime artifact can produce,
/// keeping the cache bounded to the live generations instead of growing by one
/// orphaned multi-GB generation per runtime rebuild.
///
/// The keep-set is every (profile, runtime) combination whose rlib exists RIGHT
/// NOW -- not just the caller's own combination -- so a concurrent harness
/// using a different profile (e.g. an e2e run's Debug generation during a
/// Release conformance sweep) keeps its entries. A combination whose rlib is
/// missing has an uncomputable generation name, so its old dirs are dead weight
/// by construction.
///
/// Callers must hold whatever excludes concurrent sweeps/builds of the SAME
/// generations (the conformance harness's run lock). Best-effort throughout:
/// a failed removal costs disk, never a build.
pub fn sweep_stale_cache_generations() {
    let root = cache_dir();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return; // no cache yet -- nothing to sweep
    };
    let mut live = Vec::new();
    for profile in [Profile::Debug, Profile::Release] {
        for runtime in [Runtime::Lean, Runtime::Eval] {
            if let Ok(generation) = generation_hash(profile, runtime) {
                live.push(format!("{generation:016x}"));
            }
        }
    }
    for entry in entries.flatten() {
        let name = entry.file_name();
        if live.iter().any(|l| l.as_str() == name) {
            continue;
        }
        // Stray files (e.g. `.DS_Store`) are swept along with stale dirs.
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Hard-links the cached binary to where the caller wanted it, falling back to
/// a copy across filesystems.
///
/// A link rather than a copy so that re-running the same program reuses one
/// inode -- the OS malware scan is per-file, so a fresh copy can be treated as
/// never-before-seen content and re-scanned. Callers that `remove_file` their
/// output only drop their own link; the cache entry survives.
fn link_or_copy(from: &Path, to: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(to);
    match std::fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        Err(_) => std::fs::copy(from, to)
            .map(|_| ())
            .map_err(|e| format!("copying {} -> {}: {e}", from.display(), to.display())),
    }
}

/// Passed to `rustc --crate-name` so the generated program's identity never
/// depends on the temp file it happened to be written to. Without this,
/// `rustc` derives the crate name from the source FILENAME, so the old
/// pid+thread temp name leaked into every mangled symbol and identical Ruby
/// produced different bytes on every run (visible as
/// `<output>.zeo_gen_18719_ThreadId1...rcgu.o` intermediates).
///
/// Pinning it makes the output a pure function of the generated source, which
/// a content-addressed binary cache needs in order to hit, and which lets the
/// OS scan a given program exactly once instead of on every rebuild.
///
/// Note the output PATH still leaks into the ad-hoc code signature's
/// identifier (macOS signs every arm64 binary, and the identifier defaults to
/// the output filename), so byte-identical results also require callers to
/// pick a stable output path.
///
/// A fixed crate name also makes `-C incremental` theoretically useful for the
/// small residue two generated programs still share, letting `rustc` reuse one
/// program's codegen units for another. It is deliberately NOT enabled: `libtest`
/// runs each `#[test]` on its own thread, so any per-thread keying produces one
/// cold directory per test (measured: 449 directories, 7.7GB, and a 10s NET LOSS
/// on the e2e suite). It would also buy little now -- the large per-program
/// "exception prelude" this once referred to has since moved OUT of codegen into
/// the prebuilt runtime (`main()` calls `zeo_rt::ClassRegistry::with_core()`;
/// `puts 1` emits ~74 lines, not thousands), so the emitted crates no longer share
/// a big prelude to dedup. The remaining per-program build cost is the link +
/// codesign of the runtime artifact, not codegen -- so compile-time work
/// belongs in `zeo-rt`, not here.
const GENERATED_CRATE_NAME: &str = "zeo_gen";

/// Where the generated source is written, named by a hash of its own content.
///
/// The path is deliberately a pure function of `rust_source`, because `rustc`
/// embeds the source path in the binary as panic-location metadata (`file!()`
/// data, which is runtime state and so survives the default `debuginfo=0`).
/// The previous pid+thread name therefore leaked into the output, and two runs
/// of the same program produced different bytes -- which defeats a
/// content-addressed binary cache and makes the OS treat every rebuild as
/// never-before-seen content to re-scan.
///
/// Content-addressing also makes the path in a `rustc` failure stable and
/// findable, rather than naming a file a previous run had already deleted.
fn generated_source_path(rust_source: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "zeo-gen-{:016x}.rs",
        fnv1a64(rust_source.as_bytes())
    ))
}

/// Writes via a per-writer temp file + rename, so that concurrent compiles of
/// the same source (which content-addressing points at one path) can't observe
/// a partially-written file. Rename is atomic within a directory, and the bytes
/// are identical either way, so the loser of the race is harmless.
///
/// The temp name must be unique per THREAD, not just per process: two `#[test]`
/// threads compiling the same source share a pid, and a pid-only name let one
/// rename the other's half-written file into place for `rustc` to read.
///
/// The destination is left behind on purpose: it is content-addressed, so it
/// can only ever be rewritten with the same bytes, and deleting it would race a
/// concurrent `rustc` still reading it. `$TMPDIR` is reaped by the OS.
fn write_atomically(path: &Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        thread_unique_suffix()
    ));
    std::fs::write(&tmp, contents).map_err(|e| format!("writing {}: {e}", tmp.display()))?;

    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("renaming into {}: {e}", path.display())
    })
}

/// FNV-1a. Small, stable across processes and toolchain versions -- unlike
/// `DefaultHasher`, whose output is explicitly not guaranteed between releases,
/// which would silently break byte-identical rebuilds after a Rust upgrade.
fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_with(0xcbf2_9ce4_8422_2325, bytes)
}

/// `fnv1a64` continued from an existing hash, for mixing several inputs into
/// one key.
fn fnv1a64_with(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = seed;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Threads within one process stage their `rustc` output in separate
/// directories, since the test runner parallelizes `#[test]` functions.
fn thread_unique_suffix() -> String {
    format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zeo-build-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("entry")
    }

    /// A zero-length entry is damage, never a build -- see `seal` for the
    /// channel that produces one. Serving it would make the corruption
    /// STICKY: every later build of that program "succeeds" instantly with
    /// an empty binary. Rejecting it makes the cache self-healing.
    #[test]
    fn a_zero_length_entry_is_not_usable() {
        let p = tmp("zero");
        std::fs::write(&p, b"").unwrap();
        assert!(!is_usable_entry(&p));
    }

    #[test]
    fn a_nonempty_file_is_usable_and_a_missing_one_is_not() {
        let p = tmp("ok");
        std::fs::write(&p, b"\x7fELF").unwrap();
        assert!(is_usable_entry(&p));
        std::fs::remove_file(&p).unwrap();
        assert!(!is_usable_entry(&p));
    }

    /// A DIRECTORY at the entry's path is not a build either -- `exists()`
    /// would have said yes.
    #[test]
    fn a_directory_is_not_usable() {
        let p = tmp("dir");
        let _ = std::fs::remove_file(&p);
        std::fs::create_dir_all(&p).unwrap();
        assert!(!is_usable_entry(&p));
    }

    /// The corruption channel, end to end: `link_or_copy` hands the caller a
    /// HARD LINK to the cache's own inode, so an in-place write to the
    /// caller's output would truncate the shared entry. Sealing makes that
    /// write fail. Reproduced exactly this way before the fix.
    #[cfg(unix)]
    #[test]
    fn a_sealed_entry_survives_an_in_place_write_through_a_hard_link() {
        let entry = tmp("sealed");
        std::fs::write(&entry, b"a real binary's bytes").unwrap();
        seal(&entry);

        let output = entry.with_file_name("output");
        link_or_copy(&entry, &output).unwrap();

        // What poisoned the cache: truncating the OUTPUT, which is the same
        // inode as the entry.
        let truncate = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&output);
        assert!(
            truncate.is_err(),
            "an in-place write to a sealed entry must fail"
        );
        assert!(
            is_usable_entry(&entry),
            "the cache entry must survive intact"
        );

        // The output is still executable/readable -- sealing keeps 0o555.
        assert_eq!(std::fs::read(&output).unwrap(), b"a real binary's bytes");
    }

    /// Replacing an output is unaffected by the seal: unlinking depends on
    /// the DIRECTORY's permissions, not the file's, and `link_or_copy`
    /// unlinks first.
    #[cfg(unix)]
    #[test]
    fn a_sealed_entry_can_still_be_relinked_over_an_existing_output() {
        let entry = tmp("relink");
        std::fs::write(&entry, b"payload").unwrap();
        seal(&entry);
        let output = entry.with_file_name("relink-out");
        link_or_copy(&entry, &output).unwrap();
        // Second link over the existing (read-only) output must succeed.
        link_or_copy(&entry, &output).unwrap();
        assert_eq!(std::fs::read(&output).unwrap(), b"payload");
    }
}
