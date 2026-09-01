//! Building a gem's C extension.
//!
//! An extension arrives as `ext/**/*.c` plus an `extconf.rb`. Three steps
//! turn that into something loadable, and zeo owns all three:
//!
//! 1. Run `extconf.rb` under zeo, with `RbConfig::CONFIG` supplied by
//!    `crates/zeo/src/parse/shims/rbconfig.rb.in` and `mkmf` vendored from
//!    the same `ruby/ruby` pin as the headers. It writes a Makefile.
//! 2. Read that Makefile's variables ([`makefile`]) and compile and link
//!    from them ([`build`]), in parallel and without `make`.
//! 3. Hand the shared object to the loader.
//!
//! Step 2 refuses any Makefile carrying a rule mkmf did not write and runs
//! real `make` instead -- see [`build`] for why that is the right way round.

pub mod build;
pub mod makefile;

use crate::home::ZeoHome;
use std::path::{Path, PathBuf};

/// Where the vendored MRI headers landed, as `(include, config)`.
///
/// This is a RUN-TIME fact. `crates/zeo/build.rs` bakes the dev tree's paths
/// into the rbconfig shim as a fallback, and an installed zeo's are under its
/// payload -- so a released binary that used the compile-time constant would
/// point every extension build at the build machine's source tree.
pub fn header_dirs() -> Result<(PathBuf, PathBuf), String> {
    let root = match crate::home::zeo_home() {
        ZeoHome::DevTree { root } => root.join("crates/zeo-rt/cext"),
        ZeoHome::Installed { payload, .. } => payload.join("cext"),
        // A `cargo install`ed zeo has no payload directory at all, so the
        // headers ride nowhere it can reach. Saying so beats handing an
        // extension an include path that does not exist.
        ZeoHome::Registry { .. } => {
            return Err("a cargo-installed zeo carries no C extension headers; \
                        install the release tarball to build native gems"
                .into());
        }
    };
    let (include, config) = (root.join("include"), root.join("config"));
    if !include.join("ruby.h").is_file() {
        return Err(format!(
            "the C extension headers are missing from {} -- this install is incomplete",
            root.display()
        ));
    }
    Ok((include, config))
}

/// Run `extconf.rb` in `dir` with `zeo`, so mkmf writes a Makefile.
///
/// zeo re-enters itself as a SUBPROCESS rather than compiling the file in
/// place, for the same reason rubygems does: an `extconf.rb` calls `exit`
/// freely, writes into the current directory, and installs constants and
/// globals a long-running process would then carry. A child gets its own of
/// each, and its exit status is the answer.
///
/// `zeo` is passed rather than read from `current_exe`, because the caller is
/// not always the binary -- a test is the harness, and spawning that with an
/// `extconf.rb` would run the test suite instead.
pub fn configure(zeo: &Path, dir: &Path, extconf: &Path, args: &[String]) -> Result<(), String> {
    let (include, config) = header_dirs()?;
    let out = std::process::Command::new(zeo)
        .arg(extconf)
        .args(args)
        .current_dir(dir)
        // The rbconfig shim reads these; without them it falls back to the
        // paths `build.rs` baked in, which only exist in the dev tree.
        .env("ZEO_CEXT_HDRDIR", &include)
        .env("ZEO_CEXT_ARCHHDRDIR", &config)
        .output()
        .map_err(|e| format!("running {}: {e}", extconf.display()))?;
    if out.status.success() && dir.join("Makefile").is_file() {
        return Ok(());
    }
    // extconf's own output is the whole diagnosis -- a `have_library` that
    // failed says which library, and nothing else can.
    Err(format!(
        "{} did not produce a Makefile\n{}{}",
        extconf.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

/// The `zeo` binary to run an `extconf.rb` with.
///
/// `current_exe` is right whenever zeo IS the running program, and wrong
/// whenever it is not -- under a test the running program is the harness, and
/// spawning that with an `extconf.rb` runs the test suite instead of
/// configuring anything. That is not a hypothetical: it is what this function
/// exists to have already gone wrong once.
///
/// So the exe is used only when it is actually named `zeo`. Otherwise the
/// binary is found beside it -- a test binary lives in
/// `target/<profile>/deps/`, and `target/<profile>/zeo` is the zeo that built
/// it -- and failing that, under the resolved home.
pub fn zeo_binary() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("finding the zeo binary: {e}"))?;
    if exe.file_stem().is_some_and(|s| s == "zeo") {
        return Ok(exe);
    }
    for up in [1usize, 2] {
        let mut dir = exe.clone();
        for _ in 0..up {
            dir.pop();
        }
        let cand = dir.join("zeo");
        if cand.is_file() {
            return Ok(cand);
        }
    }
    if let ZeoHome::Installed { payload, .. } = crate::home::zeo_home()
        && let Some(prefix) = payload.parent().and_then(std::path::Path::parent)
    {
        let cand = prefix.join("bin/zeo");
        if cand.is_file() {
            return Ok(cand);
        }
    }
    Err(format!(
        "cannot find the `zeo` binary to run an extconf.rb with (looked beside {})",
        exe.display()
    ))
}

/// Where an extension is BUILT, which is never the gem store.
///
/// mkmf and the compiler write a Makefile, a `.o` per source and the shared
/// object into the directory they run in. rubygems does that in place because
/// a gem directory is per-user and writable; a zeo gem store is neither -- it
/// is shared, often read-only, and a build that wrote into it would leave one
/// project's artifacts where another project reads them.
///
/// So the sources are COPIED into a cache directory keyed by the gem, its
/// version and the bytes of its `ext/` tree. A second compile of the same
/// gem finds the shared object already there and skips the build.
pub fn build_dir(gem: &str, key: &str) -> PathBuf {
    cache_root().join("cext").join(format!("{gem}-{key}"))
}

/// Copy `src` into `dst`, files and directories, leaving what is already
/// there. Used to stage a gem's `ext/` into the build directory.
fn stage(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        if from.is_dir() {
            stage(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// A content key for an `ext/` tree: every file's relative path and bytes.
///
/// A modification time would be cheaper and wrong -- a gem re-unpacked from
/// its `.gem` gets fresh mtimes and identical bytes, and rebuilding then is
/// pure waste. Hashing the bytes also means an edited source rebuilds, which
/// is what a developer working on a gem needs.
pub fn content_key(dir: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    // The key also names the platform: the dev tree's cache dir can be
    // shared across a container boundary (the Linux verification leg
    // mounts the repo), and a macOS `.bundle` served to a Linux process
    // is an "invalid ELF header" LoadError at require time.
    for byte in std::env::consts::OS
        .as_bytes()
        .iter()
        .chain(std::env::consts::ARCH.as_bytes())
    {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
    }
    let mut files = Vec::new();
    collect(dir, dir, &mut files);
    files.sort();
    for rel in files {
        for byte in rel.as_bytes() {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
        }
        if let Ok(bytes) = std::fs::read(dir.join(&rel)) {
            for byte in bytes {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
            }
        }
    }
    format!("{hash:016x}")
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
}

/// The user cache, which is where a build lands. Read from `zeo::home`'s own
/// resolution so an install and the dev tree agree.
fn cache_root() -> PathBuf {
    match crate::home::zeo_home() {
        ZeoHome::DevTree { root } => root.join("target/zeo-cext"),
        ZeoHome::Installed { cache, .. } | ZeoHome::Registry { cache } => cache.clone(),
    }
}

/// Configure and build `gem`'s extensions out of tree, and answer the
/// shared objects in `extconfs` order.
///
/// `gem_dir` is the gem's whole unpacked tree and `extconfs` are the
/// `extconf.rb` paths relative to it. The WHOLE tree is copied into the
/// cache and built there: an extconf freely reads its siblings (json's
/// loads `../simd/conf.rb`, prism's reads the gem's `include/` and
/// `src/`), so staging one extension directory starved those reads --
/// and the gem store is never written to.
pub fn build_out_of_tree(
    zeo: &Path,
    gem: &str,
    gem_dir: &Path,
    extconfs: &[String],
) -> Result<Vec<PathBuf>, String> {
    let key = content_key(gem_dir);
    let dir = build_dir(gem, &key);
    let products_in = |root: &Path| -> Option<Vec<PathBuf>> {
        extconfs
            .iter()
            .map(|e| product_of(&root.join(Path::new(e).parent()?)))
            .collect()
    };
    // An already-built product set with the same content key is the same
    // set, which is the whole reason the key hashes bytes.
    if let Some(found) = products_in(&dir) {
        return Ok(found);
    }
    // Two compiles may want the same extension at once -- parallel tests,
    // or two zeo processes sharing one cache. Each builds in a private
    // sibling and renames the finished directory into place, so `dir`
    // either holds a COMPLETE product set or nothing; a loser's rename
    // fails and it uses the winner's products. (Building in `dir` directly
    // let one process delete it out from under another mid-configure.)
    let scratch = dir.with_file_name(format!(
        "{}.build.{}",
        dir.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    stage(gem_dir, &scratch)
        .map_err(|e| format!("staging {gem} into {}: {e}", scratch.display()))?;
    for extconf in extconfs {
        let rel = Path::new(extconf);
        let ext_dir = scratch.join(rel.parent().unwrap_or_else(|| Path::new("")));
        let name = rel.file_name().unwrap_or_default();
        configure(zeo, &ext_dir, Path::new(name), &[])?;
        build_extension(&ext_dir, jobs())?;
    }
    if std::fs::rename(&scratch, &dir).is_err() {
        // The winner's directory is already there; ours is redundant.
        let _ = std::fs::remove_dir_all(&scratch);
    }
    products_in(&dir).ok_or_else(|| {
        format!(
            "built {gem}'s extensions but no shared object appeared in {}",
            dir.display()
        )
    })
}

/// How many compiles to run at once.
///
/// The same width the rest of zeo's build uses, and bounded the same way: a
/// C compile is memory-hungry, and a 16-core machine running 16 of them
/// against a large `ext/` is how a build gets OOM-killed rather than fast.
pub fn jobs() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get().min(8))
}

/// Build the extension whose Makefile is in `dir`, and answer the shared
/// object.
///
/// `jobs` bounds the parallel compiles. The `make` fallback gets the same
/// number through `-j`.
pub fn build_extension(dir: &Path, jobs: usize) -> Result<PathBuf, String> {
    let plan =
        build::Plan::of(dir).map_err(|e| format!("reading {}/Makefile: {e}", dir.display()))?;
    match plan {
        Ok(plan) => plan.run(jobs).map_err(|e| e.to_string()),
        Err(why) => {
            tracing::debug!("cext: running make in {} -- {why}", dir.display());
            build::run_make(dir, jobs).map_err(|e| e.to_string())?;
            product_of(dir).ok_or_else(|| {
                format!(
                    "make ran in {} and produced no shared object",
                    dir.display()
                )
            })
        }
    }
}

/// The shared object a `make` run left behind. The Makefile names it in
/// `TARGET_SO`, so that is what is read rather than a directory scan -- a
/// scan would pick up a `.bundle` from an earlier build of a different gem.
pub fn product_of(dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dir.join("Makefile")).ok()?;
    let name = makefile::Makefile::parse(&text).get("TARGET_SO");
    let path = dir.join(name.trim());
    path.is_file().then_some(path)
}
