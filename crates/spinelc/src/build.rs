//! Compiles generated Rust source directly against the WORKSPACE's own
//! already-built `spinel-rt` artifacts -- a single `rustc` invocation with
//! `--extern spinel_rt=<the built lib>` and `-L dependency=target/debug/deps`,
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
//! Whether the runtime is linked statically or dynamically is the caller's
//! choice (`Linkage`); it changes the output from a 9.8MB self-contained binary
//! to a 616K one that needs `target/` on disk beside it.
//!
//! This deliberately does NOT enumerate `spinel-rt`'s own transitive
//! dependencies (`indexmap`/`parking_lot`/`regex`/...) by hand: `rustc`
//! resolves those automatically via the `-L` search path, using the crate
//! metadata already embedded in the built `spinel-rt` itself -- the generated
//! program only ever references `spinel_rt` directly (`use spinel_rt::...`),
//! never its transitive deps by name, so only ONE `--extern` is ever needed.
//! Confirmed both correct (a real generated program links and runs) and
//! dramatically cheaper this way: a fresh-`cargo`-project build (the
//! previous approach here) recompiled `spinel-rt` and its whole dependency
//! graph from scratch on EVERY call (no target-dir reuse across throwaway
//! projects), costing ~3s per call; direct `rustc` against the already-built
//! artifacts costs ~0.2s. This is the same "compile one generated file
//! against already-built deps" shape tools like `trybuild`/`compiletest` use
//! for the identical reason.
//!
//! `ensure_spinel_rt_built` (a `std::sync::OnceLock`-guarded `cargo build -p
//! spinel-rt`) is what keeps this correct rather than merely fast: it
//! guarantees the rlib this reads is fresh relative to `spinel-rt`'s current
//! source, regardless of how this function's CALLER was invoked (the
//! in-process test harness has no other guarantee spinel-rt was built
//! first, since `spinelc`'s own `Cargo.toml` has no ordinary dependency on
//! it -- see that crate's dev-dependency addition). Run once per process
//! (cheap and idempotent every time after the first -- Cargo's own
//! fingerprinting makes a no-op rebuild check near-instant), not once per
//! call, so the hundreds of calls a test suite makes don't each pay a
//! `cargo` subprocess-startup cost just to confirm nothing changed.
//!
//! Lives in the library (not `main.rs`) so the in-process test harness
//! (`tests/support`) can call the exact same function the CLI uses, rather
//! than a re-derived copy of the same logic.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The workspace root -- two levels up from `crates/spinelc` (this crate's
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

static CRATES_BUILT: OnceLock<std::sync::Mutex<std::collections::HashMap<String, Result<(), String>>>> =
    OnceLock::new();

/// Builds one workspace crate (debug profile) into the shared workspace
/// `target/` exactly once per process per crate -- see this module's docs
/// for why this can't just be assumed already done. `spinel-rt` always goes
/// through here; a native package's `[native]` crate (Phase 14.3) does too,
/// the first time a program requiring it is built. Every subsequent call in
/// the same process (the common case: a test binary making hundreds of
/// `build_binary` calls) is a cached map hit.
fn ensure_crate_built(name: &str) -> Result<(), String> {
    // A harness that already built the workspace can say so, and skip this
    // entirely. The memoization below is per-PROCESS, which is worth nothing to
    // a driver that runs `spinelc` as a subprocess per case: the conformance
    // suite pays this 1,819 times at ~84ms each (~153s of CPU), and because
    // Cargo takes an exclusive lock on the whole build directory, those calls
    // serialize against each other instead of just being redundant.
    if std::env::var_os("SPINELC_ASSUME_BUILT").is_some() {
        return Ok(());
    }
    let map = CRATES_BUILT.get_or_init(Default::default);
    let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = map.get(name) {
        return cached.clone();
    }
    let result = match std::process::Command::new("cargo")
        .args(["build", "--quiet", "-p", name])
        .current_dir(workspace_root())
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("`cargo build -p {name}` exited with {status}")),
        Err(e) => Err(format!("running `cargo build -p {name}`: {e}")),
    };
    map.insert(name.to_string(), result.clone());
    result
}

/// How a generated program links the runtime.
///
/// `Static` is the only shippable answer and so the default. A `Dynamic`
/// program is NOT self-contained: it resolves `libspinel_rt.dylib` and
/// `libstd.dylib` at run time by absolute paths baked in at link time, so
/// `cargo clean` breaks every binary ever produced and it runs on no other
/// machine. `spinelc foo.rb -o app` must keep producing a real native binary
/// someone can just run.
///
/// The test harnesses choose `Dynamic`, because for them the tradeoff inverts:
/// their programs are compiled, executed once, and thrown away, always
/// alongside the `target/` that built them. It takes each one from 9.8MB to
/// 616K (measured), which is the difference between a ~21GB compiled-program
/// cache and a ~1.4GB one across the ~2,300 programs the suites build. It buys
/// almost no time (~7%) -- this is a disk tradeoff, not a speed one.
///
/// Passed explicitly rather than read from the environment down here: the e2e
/// harness calls this from a dozen `#[test]` threads at once, and a `set_var`
/// racing a `var_os` is a real data race. The CLI reads the environment once,
/// at startup, while still single-threaded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Linkage {
    Static,
    Dynamic,
}

impl Linkage {
    /// The CLI's choice: static unless a harness driving `spinelc` as a
    /// subprocess asked otherwise.
    pub fn from_env() -> Self {
        match std::env::var_os("SPINELC_LINK_DYNAMIC") {
            Some(_) => Linkage::Dynamic,
            None => Linkage::Static,
        }
    }

    fn tag(self) -> &'static [u8] {
        match self {
            Linkage::Static => b"static;",
            Linkage::Dynamic => b"dynamic;",
        }
    }
}

/// The built library a generated program should link against: the `dylib` when
/// linking dynamically and the crate publishes one, else the `rlib`.
///
/// A crate without a `dylib` (any `[native]` package that hasn't asked for one)
/// falls back to its rlib and links statically even in dynamic mode, which is
/// fine -- `-C prefer-dynamic` is a preference, not a requirement.
fn linkable_for(crate_name: &str, linkage: Linkage) -> Result<PathBuf, String> {
    let underscored = crate_name.replace('-', "_");
    let debug = target_dir().join("debug");
    if linkage == Linkage::Dynamic {
        let dylib = debug.join(format!("lib{underscored}.dylib"));
        if dylib.exists() {
            return Ok(dylib);
        }
    }
    let rlib = debug.join(format!("lib{underscored}.rlib"));
    if !rlib.exists() {
        return Err(format!(
            "expected {} to exist after building {crate_name} -- was it built into a different target directory?",
            rlib.display()
        ));
    }
    Ok(rlib)
}

pub fn build_binary(rust_source: &str, output: &Path, linkage: Linkage) -> Result<(), String> {
    build_binary_with_deps(rust_source, &[], output, linkage)
}

/// `build_binary` plus the extra workspace lib crates (`native_deps`, cargo
/// package names -- see `spinelc::CompileOutput`) the generated program
/// references: each is `cargo build`-built once and linked with its own
/// `--extern`, exactly the shape `spinel-rt` itself uses.
pub fn build_binary_with_deps(
    rust_source: &str,
    native_deps: &[String],
    output: &Path,
    linkage: Linkage,
) -> Result<(), String> {
    ensure_crate_built("spinel-rt")?;
    for dep in native_deps {
        ensure_crate_built(dep)?;
    }

    let runtime = linkable_for("spinel-rt", linkage)?;
    let deps_dir = target_dir().join("debug").join("deps");

    let cached = cache_path(rust_source, native_deps, linkage)?;
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
        "spinelc-staging-{}-{}",
        std::process::id(),
        thread_unique_suffix()
    ));
    std::fs::create_dir_all(&staging).map_err(|e| format!("creating {}: {e}", staging.display()))?;
    let staged = staging.join(cached.file_name().expect("cache_path always has a file name"));

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
        .arg(format!("spinel_rt={}", runtime.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps_dir.display()));
    for dep in native_deps {
        let dep_lib = linkable_for(dep, linkage)?;
        cmd.arg("--extern")
            .arg(format!("{}={}", dep.replace('-', "_"), dep_lib.display()));
    }
    if linkage == Linkage::Dynamic {
        // Matches how the workspace builds the dylib (`.cargo/config.toml`).
        // Required rather than cosmetic: a Rust `dylib` embeds its own `std`
        // unless built this way, and a program linking it would then carry a
        // second copy -- rustc rejects that outright ("cannot satisfy
        // dependencies so `std` only shows up once").
        cmd.arg("-C").arg("prefer-dynamic");
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
    std::fs::rename(&staged, &cached)
        .map_err(|e| format!("publishing {}: {e}", cached.display()))?;
    seal(&cached);
    let _ = std::fs::remove_dir_all(&staging);
    link_or_copy(&cached, output)
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
/// Reproduced exactly that way: `spinelc x.rb -o x.bin` then `: > x.bin` left
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
    target_dir().join("spinelc-bin-cache")
}

/// The cache slot for this exact program: `<rlib generation>/<source hash>`.
///
/// Split in two so a generation can be swept wholesale. Every entry under a
/// generation dies the moment the runtime is rebuilt -- generated programs link
/// `libspinel_rt.rlib` statically -- and each generation is ~4GB across the test
/// suite, so a flat keyspace grew by that much per commit and never shrank.
///
/// The rlibs are keyed by len+mtime rather than content: `spinelc` runs as a
/// fresh process per case under the conformance harness, so a content hash of
/// the 32MB rlib could not be amortized and would cost more than it saves.
/// Cargo does not touch mtimes on a no-op rebuild, so this only
/// over-invalidates when the runtime genuinely got rebuilt.
fn cache_path(
    rust_source: &str,
    native_deps: &[String],
    linkage: Linkage,
) -> Result<PathBuf, String> {
    // Linkage is part of the generation, not just a detail: the same source
    // compiles to a 9.8MB self-contained binary or a 616K one that needs the
    // dylib, and handing a caller the wrong kind would either bloat their
    // output or hand them something that dies in `dyld`.
    let mut generation = fnv1a64_with(0xcbf2_9ce4_8422_2325, linkage.tag());
    for name in std::iter::once("spinel-rt").chain(native_deps.iter().map(String::as_str)) {
        let lib = linkable_for(name, linkage)?;
        let meta = std::fs::metadata(&lib).map_err(|e| format!("stat {}: {e}", lib.display()))?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        generation = fnv1a64_with(generation, format!("{name}:{}:{mtime};", meta.len()).as_bytes());
    }
    let root = cache_dir();
    std::fs::create_dir_all(&root).map_err(|e| format!("creating {}: {e}", root.display()))?;
    let dir = root.join(format!("{generation:016x}"));
    // `create_dir`, not `create_dir_all`: the AlreadyExists error is the signal
    // for whether this process is the one that opened a new generation.
    let we_created = match std::fs::create_dir(&dir) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(format!("creating {}: {e}", dir.display())),
    };
    if we_created {
        sweep_stale_generations(&dir);
    }
    Ok(dir.join(format!("{:016x}", fnv1a64(rust_source.as_bytes()))))
}

/// Best-effort removal of cache generations that are provably not in use, run
/// only by whichever process first creates a new generation.
///
/// Two guards, because a sweep is a `remove_dir_all` and the harness runs a
/// dozen `spinelc` processes at once:
///
/// - only the process that actually created the new generation sweeps, so 1800
///   conformance cases don't each race to delete the same directories;
/// - a generation is only swept once nothing has touched it for an hour.
///   Writing an entry bumps the directory's mtime, so a generation any live run
///   is publishing into is never a candidate. Without this, a process whose
///   runtime rebuild landed a moment earlier would delete the generation its
///   siblings were still building into, and their `rustc` output would vanish
///   mid-write (seen as spurious FAIL_RUSTC across the conformance corpus).
///
/// A generation that stays warm is simply swept on some later run; the cost of
/// waiting is disk, and the cost of being wrong is a broken build.
fn sweep_stale_generations(live: &Path) {
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(60 * 60);
    let Ok(entries) = std::fs::read_dir(cache_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path() == live {
            continue;
        }
        let untouched_for = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().unwrap_or_default());
        if matches!(untouched_for, Ok(d) if d > STALE_AFTER) {
            let _ = std::fs::remove_dir_all(entry.path());
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
/// `<output>.spinelc_gen_18719_ThreadId1...rcgu.o` intermediates).
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
/// A fixed crate name also makes `-C incremental` theoretically useful here,
/// since ~99% of any two generated programs is the identical exception prelude
/// and a shared crate identity lets `rustc` reuse one program's codegen units
/// for another (measured in isolation: 514ms -> 246ms). It is deliberately NOT
/// enabled: `libtest` runs each `#[test]` on its own thread, so any per-thread
/// keying produces one cold directory per test (measured: 449 directories,
/// 7.7GB, and a 10s NET LOSS on the e2e suite). Reuse would need a fixed-size
/// pool of directories leased across tests, since `rustc` locks each one
/// exclusively -- the shared-prelude work in `docs/todo/runtime-exception-model.md`
/// addresses the same duplication structurally instead.
const GENERATED_CRATE_NAME: &str = "spinelc_gen";

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
    std::env::temp_dir().join(format!("spinelc-gen-{:016x}.rs", fnv1a64(rust_source.as_bytes())))
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
    let renamed = std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("renaming into {}: {e}", path.display())
    });
    renamed
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
        let d = std::env::temp_dir().join(format!("spinelc-build-test-{}-{name}", std::process::id()));
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
        let truncate = std::fs::OpenOptions::new().write(true).truncate(true).open(&output);
        assert!(truncate.is_err(), "an in-place write to a sealed entry must fail");
        assert!(is_usable_entry(&entry), "the cache entry must survive intact");

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
