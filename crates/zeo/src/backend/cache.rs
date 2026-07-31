//! The content-addressed cache of compiled generated programs, plus the small
//! filesystem primitives it needs.
//!
//! Split out of `backend` so the compile/link path (`super`) reads as the
//! CLI's concern, and this layer -- which exists almost entirely to keep the
//! thousands-of-cases golden corpus from recompiling identical programs --
//! stays quarantined behind a handful of `pub(super)` entry points
//! (`cache_path`, `is_usable_entry`, `link_or_copy`, `seal`,
//! `maybe_sweep_stale_cache`, and the source-staging helpers).
//!
//! Every entry is keyed by `<generation>/<source hash>`: the source hash makes
//! the compiled binary a pure function of the generated Rust, and the
//! generation folds in everything ELSE that shapes the bytes (the runtime rlib,
//! the compiler itself, profile/variant, rustc flags) so a rebuild of any of
//! those retires the old entries wholesale rather than serving a stale mix.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::{GenOpt, Linkage, Profile, Runtime, runtime_artifact_fingerprint, target_dir};

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
/// the golden corpus (measured 2026-07: ~5.5MB per release entry, ~15MB
/// per debug entry), so a flat keyspace grew by that much per runtime rebuild
/// and never shrank. `sweep_stale_cache_generations` reclaims dead generations
/// when tooling invokes it between runs.
///
/// The rlibs are keyed by len+mtime rather than content: `zeo` runs as a
/// fresh process per case under the golden harness, so a content hash of
/// the 32MB rlib could not be amortized and would cost more than it saves.
/// Cargo does not touch mtimes on a no-op rebuild, so this only
/// over-invalidates when the runtime genuinely got rebuilt.
pub(super) fn cache_path(
    rust_source: &str,
    profile: Profile,
    runtime: Runtime,
    linkage: Linkage,
    gen_opt: GenOpt,
) -> Result<PathBuf, String> {
    let generation = generation_hash(profile, runtime, linkage, gen_opt)?;
    let root = cache_dir();
    std::fs::create_dir_all(&root).map_err(|e| format!("creating {}: {e}", root.display()))?;
    let dir = root.join(format!("{generation:016x}"));
    // Idempotent: many processes publish into the same generation concurrently,
    // and each just ensures the directory exists. Stale generations are NOT
    // swept here -- a `remove_dir_all` in the build hot path could delete a
    // directory a sibling process is still writing rustc output into (seen as
    // spurious FAIL_RUSTC across the corpus). Reclamation happens between runs
    // instead, via `sweep_stale_cache_generations` (or by wiping the cache dir
    // wholesale).
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    Ok(dir.join(format!("{:016x}", fnv1a64(rust_source.as_bytes()))))
}

/// The cache generation for one (profile, runtime, linkage) combination --
/// shared by `cache_path` and `sweep_stale_cache_generations` so the writer and
/// the sweeper can never disagree on a generation's name.
///
/// Errors when that combination's runtime artifact isn't built -- in which case
/// no process can compute (and so write into) that generation either.
fn generation_hash(
    profile: Profile,
    runtime: Runtime,
    linkage: Linkage,
    gen_opt: GenOpt,
) -> Result<u64, String> {
    // Profile, runtime variant, AND linkage are part of the generation, not just
    // details: the same source compiles to a debug or release binary (profile),
    // to a lean or prism-carrying binary (runtime), and to a self-contained or
    // dylib-linked binary (linkage) -- handing a caller the wrong kind would
    // bloat its output, link an `eval` stub, or hand it a binary that can't find
    // its shared runtime. Each combination's own artifact is also fingerprinted
    // below (they live in different target dirs), so their mtimes already
    // distinguish them; the tags make the intent explicit and collision-proof.
    let mut generation = fnv1a64_with(0xcbf2_9ce4_8422_2325, profile.tag());
    generation = fnv1a64_with(generation, runtime.tag());
    generation = fnv1a64_with(generation, linkage.tag());
    // The generated crate's own opt level is part of the generation: a
    // binary built at a different opt-level is a different artifact, and
    // serving a stale one would silently undo (or fake) the optimization.
    generation = fnv1a64_with(generation, gen_opt.tag());
    // The runtime artifact's len+mtime: generated programs bind to the exact
    // rlib/dylib they were built against, so a rebuilt runtime must retire the
    // old entries (a dynamic binary linked to a stale dylib ABI would fail to
    // load outright). Cargo leaves mtimes untouched on a no-op rebuild, so this
    // only rolls when the runtime genuinely changed.
    generation = fnv1a64_with(
        generation,
        runtime_artifact_fingerprint(profile, runtime, linkage)?.as_bytes(),
    );
    // The COMPILER's own fingerprint: the generated program's typed fast
    // paths and its whole codegen come from `zeo`, so a CHANGED compiler
    // yields different binaries even against an unchanged runtime rlib.
    // A build.rs content hash over every code-shaping source (this crate,
    // zeo-dsl/zeo-abi, the zeo-rt headers the surface projection reads, the
    // lockfile) -- NOT the executable's len+mtime -- so a mere rebuild of
    // identical source keeps the generation (and a restored CI bin-cache)
    // live, while any real compiler change still rolls it and
    // `sweep_stale_cache_generations` reclaims the dead generation. The CLI
    // and the test binaries embed the same env, so they share generations.
    generation = fnv1a64_with(
        generation,
        concat!("zeo:", env!("ZEO_COMPILER_FINGERPRINT"), ";").as_bytes(),
    );
    Ok(generation)
}

/// Runs `sweep_stale_cache_generations` at most ONCE per process, on a detached
/// background thread so a multi-GB `remove_dir_all` never blocks (or times out)
/// the build/test that triggered it. Safe to race with concurrent builds: the
/// sweep only removes NON-live generations, which no live build ever writes
/// into; concurrent sweeps racing on the same dead dir is idempotent
/// (best-effort removal). Called from `build_binary`'s hot path -- cheap after
/// the first process cleans up (a bare readdir once nothing is stale).
pub(super) fn maybe_sweep_stale_cache() {
    // Under `ZEO_ASSUME_RUNTIME_FRESH` (CI, after its explicit prebuild) the
    // cache dir was just restored/validated; skip the per-process read_dir +
    // per-generation artifact stats nextest's process-per-test model repeats.
    if std::env::var_os("ZEO_ASSUME_RUNTIME_FRESH").is_some() {
        return;
    }
    static SWEPT: OnceLock<()> = OnceLock::new();
    SWEPT.get_or_init(|| {
        std::thread::spawn(sweep_stale_cache_generations);
    });
}

/// Removes every cache generation no current runtime artifact can produce,
/// keeping the cache bounded to the live generations instead of growing by one
/// orphaned multi-GB generation per runtime rebuild.
///
/// The keep-set is every (profile, runtime) combination whose rlib exists RIGHT
/// NOW -- not just the caller's own combination -- so a concurrent build using a
/// different profile (e.g. an e2e run's Debug generation during a Release golden
/// sweep) keeps its entries. A combination whose rlib is missing has an
/// uncomputable generation name, so its old dirs are dead weight by construction.
///
/// A caller must exclude concurrent sweeps/builds of the SAME generations
/// (`maybe_sweep_stale_cache` does, running this once per process). Best-effort
/// throughout: a failed removal costs disk, never a build.
fn sweep_stale_cache_generations() {
    let root = cache_dir();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return; // no cache yet -- nothing to sweep
    };
    let mut live = Vec::new();
    for profile in [Profile::Debug, Profile::Release] {
        for runtime in [Runtime::Lean, Runtime::Eval] {
            for linkage in [Linkage::Static, Linkage::Dynamic] {
                for gen_opt in [GenOpt::Optimized, GenOpt::Unoptimized] {
                    if let Ok(generation) = generation_hash(profile, runtime, linkage, gen_opt) {
                        live.push(format!("{generation:016x}"));
                    }
                }
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
pub(super) fn seal(path: &Path) {
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
pub(super) fn is_usable_entry(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// Hard-links the cached binary to where the caller wanted it, falling back to
/// a copy across filesystems.
///
/// A link rather than a copy so that re-running the same program reuses one
/// inode -- the OS malware scan is per-file, so a fresh copy can be treated as
/// never-before-seen content and re-scanned. Callers that `remove_file` their
/// output only drop their own link; the cache entry survives.
pub(super) fn link_or_copy(from: &Path, to: &Path) -> Result<(), String> {
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
/// on the e2e suite). It would also buy little: per-program codegen is small
/// -- the exception prelude lives in the prebuilt runtime (`main()` calls
/// `zeo_rt::ClassRegistry::with_core()`; `puts 1` emits ~74 lines, not
/// thousands), so the emitted crates share little to dedup.
/// The remaining per-program build cost is the link +
/// codesign of the runtime artifact, not codegen -- so compile-time work
/// belongs in `zeo-rt`, not here.
pub(super) const GENERATED_CRATE_NAME: &str = "zeo_gen";

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
pub(super) fn generated_source_path(rust_source: &str) -> PathBuf {
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
pub(super) fn write_atomically(path: &Path, contents: &str) -> Result<(), String> {
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
pub(super) fn thread_unique_suffix() -> String {
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
