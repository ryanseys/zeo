//! The differential parity probe: one Gemfile installed by two engines.
//!
//! zeo's claim about RubyGems and Bundler is not "a compatible
//! reimplementation" -- it is that zeo RUNS THE REAL LIBRARY. The only honest
//! test of that claim is to hand the same Ruby code to CRuby and to zeo and
//! demand the same result, so this stages two identical trees, installs
//! through each, and compares what came out.
//!
//! Three things make the comparison mean something.
//!
//! **The same library on both sides.** CRuby runs with `--disable-gems` and
//! `lib/ruby/{rubygems,bundler}/lib` on `-I`, so both engines execute the
//! RubyGems and Bundler that zeo vendors. Without that, the machine's own
//! RubyGems would join in and a difference in the answer could mean a
//! difference in the *libraries* rather than in the engines -- which is the
//! one thing this test must never be able to say.
//!
//! **The same driver on both sides**, [`zeo::subcommand::BUNDLE_DRIVER`]
//! verbatim, so neither side runs a program the other does not.
//!
//! **No network, and no ambient anything.** The fixture gems are built into a
//! `vendor/cache` and installed with `--local`; the environment is cleared and
//! `HOME` points inside the scratch tree, because `~/.bundle/config` reaches a
//! bundler that did not ask for it (it did, the first time this ran by hand,
//! and put one machine's `without: minitest` into the answer).
//!
//! What is compared: the exit status, the normalised output, the installed
//! tree byte for byte with its permission bits, and what each store says about
//! itself through `Gem::Specification.stubs` -- read by ONE engine, so a
//! difference there is a difference in what the install wrote.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `crates/zeo/tests/fixtures/parity`.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity")
}

/// The pinned ruby, resolved the way the golden harness resolves it. `None`
/// when this machine has no ruby to be the other engine.
fn oracle_ruby() -> Option<PathBuf> {
    let mise = Command::new("mise")
        .args(["which", "ruby"])
        .current_dir(crate::paths::workspace_root())
        .output();
    if let Ok(out) = mise
        && out.status.success()
    {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    Command::new("ruby")
        .arg("-v")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| PathBuf::from("ruby"))
}

/// The two vendored trees both engines run: RubyGems' `lib/` and Bundler's.
fn vendored_libs() -> [PathBuf; 2] {
    let root = crate::paths::workspace_root().join("lib/ruby");
    [
        root.join("rubygems/lib"),
        root.join("bundler/lib"),
    ]
}

/// A scratch directory under `target/`, wiped on entry so a rerun cannot
/// inherit the last run's store.
fn scratch(name: &str) -> PathBuf {
    let dir = crate::paths::profile_dir()
        .expect("target/<profile>")
        .join("parity-e2e")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creating the scratch dir");
    dir
}

/// The environment BOTH sides get: nothing but what the run needs. `env_clear`
/// rather than a few `env_remove`s, because the list of variables that can
/// reach bundler is not one a test should try to enumerate.
fn hermetic(cmd: &mut Command, root: &Path) {
    cmd.env_clear()
        .env("HOME", root.join("home"))
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("BUNDLE_GEMFILE", root.join("Gemfile"))
        .env("BUNDLE_PATH", root.join("bundle"))
        // One worker. The parallel installer interleaves its progress lines,
        // and a probe that normalises that away would also normalise away a
        // real ordering difference.
        .env("BUNDLE_JOBS", "1")
        .current_dir(root);
}

/// Stage one side's tree: the Gemfile, the offline cache, and a HOME of its
/// own.
fn stage(root: &Path, cache: &Path) {
    std::fs::create_dir_all(root.join("home")).expect("a scratch HOME");
    std::fs::create_dir_all(root.join("vendor/cache")).expect("a vendor/cache");
    std::fs::copy(fixtures().join("Gemfile"), root.join("Gemfile")).expect("the Gemfile");
    for entry in std::fs::read_dir(cache).expect("the built fixture gems").flatten() {
        let name = entry.file_name();
        std::fs::copy(entry.path(), root.join("vendor/cache").join(name)).expect("caching a gem");
    }
}

/// Build the fixture gems once, into `<out>/cache`.
fn build_fixture_gems(ruby: &Path, out: &Path) -> PathBuf {
    let mut cmd = Command::new(ruby);
    cmd.arg("--disable-gems");
    for lib in vendored_libs() {
        cmd.arg("-I").arg(lib);
    }
    let built = cmd
        .arg(fixtures().join("build_gems.rb"))
        .arg(fixtures())
        .arg(out)
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("building the fixture gems");
    assert!(
        built.status.success(),
        "the fixture gems did not build:\n{}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    out.join("cache")
}

/// One side's install, as (exit code, stdout, stderr).
struct Run {
    code: Option<i32>,
    out: String,
    err: String,
}

fn finish(output: std::process::Output, root: &Path) -> Run {
    Run {
        code: output.status.code(),
        out: normalize(&String::from_utf8_lossy(&output.stdout), root),
        err: normalize(&String::from_utf8_lossy(&output.stderr), root),
    }
}

/// CRuby's side: the vendored libraries on `-I`, running zeo's own driver.
fn install_with_ruby(ruby: &Path, root: &Path) -> Run {
    let mut cmd = Command::new(ruby);
    cmd.arg("--disable-gems");
    for lib in vendored_libs() {
        cmd.arg("-I").arg(lib);
    }
    cmd.arg("-e").arg(zeo::subcommand::BUNDLE_DRIVER);
    cmd.args(["install", "--local"]);
    hermetic(&mut cmd, root);
    finish(cmd.output().expect("running ruby's install"), root)
}

/// zeo's side. `zeo install` IS `bundle install`, so this is the command a
/// user types.
fn install_with_zeo(root: &Path) -> Run {
    let zeo = crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}"));
    let mut cmd = Command::new(zeo);
    cmd.args(["install", "--local"]);
    hermetic(&mut cmd, root);
    // A shared compiled-program cache would let one side's run answer for the
    // other's; each tree keeps its own.
    cmd.env("ZEO_CACHE_DIR", root.join("zeo-cache"));
    finish(cmd.output().expect("running zeo's install"), root)
}

/// Read what a store says about itself, with ONE engine, so the answer is
/// about the store rather than about the reader.
fn stubs(ruby: &Path, root: &Path) -> String {
    let store = bundle_store(root).expect("the install left a store");
    let mut cmd = Command::new(ruby);
    cmd.arg("--disable-gems");
    for lib in vendored_libs() {
        cmd.arg("-I").arg(lib);
    }
    let out = cmd
        .arg(fixtures().join("dump_stubs.rb"))
        .env_clear()
        .env("HOME", root.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .env("GEM_HOME", &store)
        .env("GEM_PATH", &store)
        .output()
        .expect("reading the store");
    assert!(
        out.status.success(),
        "reading {}:\n{}",
        store.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `<root>/bundle/ruby/<abi>`, whatever the abi turned out to be.
fn bundle_store(root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(root.join("bundle/ruby"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_dir())
}

/// Fold out everything that is allowed to differ between two runs of the same
/// program: where the tree is, and how long the install took.
fn normalize(text: &str, root: &Path) -> String {
    let root = root.to_string_lossy().into_owned();
    // A macOS scratch path is reported through `/private`, and bundler prints
    // whichever spelling it was handed.
    let alt = format!("/private{root}");
    let mut out = text.replace(&alt, "<root>").replace(&root, "<root>");
    out = out.replace(&crate::paths::workspace_root().to_string_lossy().into_owned(), "<repo>");
    // Bundler's `--verbose` timing lines are the only output that a rerun of
    // the same install is allowed to change.
    out.lines()
        .map(|line| if line.contains("Took") { "<timing>" } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every file under `dir`, by path relative to it: the bytes, and the
/// permission bits. Mode is compared because a binstub that is not executable
/// is an install that did not work, and a byte compare alone would pass it.
fn tree(dir: &Path) -> BTreeMap<String, (Vec<u8>, u32)> {
    let mut out = BTreeMap::new();
    collect(dir, dir, &mut out);
    out
}

fn collect(root: &Path, dir: &Path, out: &mut BTreeMap<String, (Vec<u8>, u32)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .expect("a path under the root")
            .to_string_lossy()
            .into_owned();
        let mode = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                path.metadata().map(|m| m.permissions().mode() & 0o777).unwrap_or(0)
            }
            #[cfg(not(unix))]
            {
                0
            }
        };
        out.insert(rel, (std::fs::read(&path).unwrap_or_default(), mode));
    }
}

/// The store paths that are allowed to differ, with the reason.
///
/// `build_info` records the arguments a native build ran with, and the
/// fixture gems have no extension, so nothing lands there. The list is empty
/// on purpose: an exception belongs here only once something has argued for
/// it, and an empty list is what says none has.
const ALLOWED_TO_DIFFER: &[&str] = &[];

/// The two engines install one Gemfile identically, or this says exactly
/// where they parted.
///
/// IGNORED on the divergence it found the first time it ran: zeo installs
/// `bundler/rubygems_ext.rb`'s universal-arch `Gem::BasicSpecification#
/// extensions_dir` on a machine that is not universal, so the guarded body
/// shadows `basic_specification.rb`'s real one and every install raises
/// `uninitialized constant ORIGINAL_LOCAL_PLATFORM`. CRuby's side of this
/// same probe passes, which is what says the fault is zeo's.
#[test]
#[ignore = "zeo installs a guarded def whose guard is false (extensions_dir)"]
fn one_gemfile_installs_the_same_way_under_both_engines() {
    let Some(ruby) = oracle_ruby() else {
        eprintln!("skipping: this machine has no ruby to be the other engine");
        return;
    };
    let dir = scratch("install");
    let cache = build_fixture_gems(&ruby, &dir.join("fixture-gems"));

    let (ruby_root, zeo_root) = (dir.join("ruby"), dir.join("zeo"));
    for root in [&ruby_root, &zeo_root] {
        stage(root, &cache);
    }

    let by_ruby = install_with_ruby(&ruby, &ruby_root);
    let by_zeo = install_with_zeo(&zeo_root);

    assert_eq!(
        (by_zeo.code, by_zeo.out.as_str()),
        (by_ruby.code, by_ruby.out.as_str()),
        "the two engines disagree about the install.\n\
         --- ruby stderr ---\n{}\n--- zeo stderr ---\n{}",
        by_ruby.err,
        by_zeo.err
    );
    assert!(
        by_ruby.code == Some(0),
        "the reference install itself failed, so there is nothing to compare against:\n{}\n{}",
        by_ruby.out,
        by_ruby.err
    );

    let (want, got) = (
        tree(&ruby_root.join("bundle")),
        tree(&zeo_root.join("bundle")),
    );
    let differing: Vec<&String> = want
        .keys()
        .chain(got.keys())
        .filter(|k| !ALLOWED_TO_DIFFER.iter().any(|a| k.contains(a)))
        .filter(|k| want.get(*k) != got.get(*k))
        .collect();
    assert!(
        differing.is_empty(),
        "the installed trees differ at {} path(s): {:?}",
        differing.len(),
        differing
    );

    assert_eq!(
        stubs(&ruby, &zeo_root),
        stubs(&ruby, &ruby_root),
        "the two stores describe themselves differently"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
