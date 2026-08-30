//! Assemble the relocatable distribution.
//!
//! One staged tree feeds every install channel -- the GitHub Release
//! tarball, Homebrew, and the rubygems platform gems all carry exactly this:
//!
//! ```text
//! zeo-<version>-<triple>/
//!   bin/zeo                          # dist-profile build
//!   share/zeo/
//!     dist-manifest.json             # {schema, version, target}
//!     lib/ruby/                      # every bundled library (payload)
//!     lib/<triple>/libzeo.a          # what `zeo -o` links against
//!   share/doc/zeo/                   # README + licenses
//! ```
//!
//! `libzeo.a` is the payload: `zeo -o` links a compiled program against it,
//! so a tree without it can run programs (the JIT path) and compile none. It
//! is keyed by triple because a payload may one day carry two -- see the
//! mac<->mac cross note in the distribution plan.

use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};

use crate::exec::{self, Capture};
use crate::scratch::Scratch;
use crate::{Error, payload, root, root_join};

const USAGE: &str = "usage: cargo xtask dist [--target <triple>] [--pgo] [--no-smoke] \
                     [--stage-only] [-o <dir>]";

/// The docs every channel carries beside the binary.
const DOCS: &[&str] = &[
    "README.md",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "THIRD-PARTY-NOTICES.md",
];

struct Opts {
    target: Option<String>,
    pgo: bool,
    smoke: bool,
    stage_only: bool,
    out: Option<String>,
}

pub fn run(args: &[String]) -> Result<(), Error> {
    let Some(opts) = parse(args)? else {
        return Ok(());
    };

    let version = workspace_version()?;
    let triple = match &opts.target {
        Some(t) => t.clone(),
        None => host_triple()?,
    };
    let dist_name = format!("zeo-{version}-{triple}");
    let out_dir = match &opts.out {
        Some(dir) => PathBuf::from(dir),
        None => root_join("target/dist"),
    };
    let stage = out_dir.join(&dist_name);
    refuse_to_delete_our_own_cwd(&stage)?;
    match std::fs::remove_dir_all(&stage) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::new(format!("clearing {}: {e}", stage.display()))),
    }
    let payload_dir = stage.join("share/zeo");

    for (rel, src) in payload::files()? {
        copy(&src, &payload_dir.join("lib/ruby").join(rel))?;
    }
    write(
        &payload_dir.join("dist-manifest.json"),
        format!(
            "{{\n  \"schema\": 1,\n  \"version\": \"{version}\",\n  \"target\": \"{triple}\"\n}}\n"
        )
        .as_bytes(),
    )?;
    stage_docs(&stage)?;

    if opts.stage_only {
        println!(
            "dist: staged {} (stage-only, no binary and no archive)",
            stage.display()
        );
        return Ok(());
    }

    stage_binary(&opts, &stage, &payload_dir, &triple)?;
    if opts.smoke {
        smoke_test(&stage)?;
    }
    tarball(&out_dir, &dist_name)
}

fn parse(args: &[String]) -> Result<Option<Opts>, Error> {
    let mut opts = Opts {
        target: None,
        pgo: false,
        smoke: true,
        stage_only: false,
        out: None,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| Error::new(format!("{name} wants a value\n{USAGE}")))
        };
        match arg.as_str() {
            "--target" => opts.target = Some(value("--target")?),
            "-o" => opts.out = Some(value("-o")?),
            "--pgo" => opts.pgo = true,
            "--no-smoke" => opts.smoke = false,
            "--stage-only" => opts.stage_only = true,
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => return Err(Error::new(format!("unknown option {other:?}\n{USAGE}"))),
        }
    }
    Ok(Some(opts))
}

fn workspace_version() -> Result<String, Error> {
    let manifest = root_join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| Error::new(format!("reading {}: {e}", manifest.display())))?;
    text.lines()
        .find_map(|line| line.strip_prefix("version = "))
        .map(|v| v.trim().trim_matches('"').to_string())
        .ok_or_else(|| Error::new("the root manifest names no [workspace.package] version"))
}

/// Where cargo actually wrote. NOT `<root>/target`: the linux container
/// builds with `CARGO_TARGET_DIR=/target` (a named volume, so rebuilds stay
/// incremental) while the repo is mounted at /src, and a staged tree
/// assembled from the wrong root finds no binary at all.
fn cargo_target_root() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => root_join("target"),
    }
}

fn host_triple() -> Result<String, Error> {
    let out = exec::run(&["rustc", "-vV"], root(), &[], Capture::Both)?;
    out.stdout_text()
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|t| t.trim().to_string())
        .ok_or_else(|| Error::new("rustc -vV printed no host line"))
}

fn stage_docs(stage: &Path) -> Result<(), Error> {
    for name in DOCS {
        let src = root_join(name);
        if src.is_file() {
            copy(&src, &stage.join("share/doc/zeo").join(name))?;
        }
    }
    Ok(())
}

/// The `dist` profile: single-codegen-unit plus thin LTO, the
/// maximum-optimization shape the everyday `release` profile gives up for
/// build parallelism. Shipped artifacts pay it once per release.
///
/// One cargo invocation builds both halves: `zeo`'s crate-type is
/// `["rlib", "staticlib"]`, so the binary and `libzeo.a` come out of the same
/// profile directory and cannot be from different sources.
fn stage_binary(opts: &Opts, stage: &Path, payload_dir: &Path, triple: &str) -> Result<(), Error> {
    if opts.pgo {
        return stage_binary_pgo(opts, stage, payload_dir, triple);
    }
    let mut build = vec!["build", "--profile", "dist", "-p", "zeo"];
    if let Some(target) = &opts.target {
        build.push("--target");
        build.push(target);
    }
    cargo(&build, &[])?;
    let built = match &opts.target {
        Some(target) => cargo_target_root().join(target).join("dist"),
        None => cargo_target_root().join("dist"),
    };
    stage_built(stage, payload_dir, triple, &built)
}

fn stage_built(
    stage: &Path,
    payload_dir: &Path,
    triple: &str,
    built: &Path,
) -> Result<(), Error> {
    copy(&built.join("zeo"), &stage.join("bin/zeo"))?;
    let archive = built.join("libzeo.a");
    if !archive.is_file() {
        return Err(Error::new(format!(
            "cargo built no {} -- `zeo -o` cannot link without it",
            archive.display()
        )));
    }
    copy(
        &archive,
        &payload_dir.join("lib").join(triple).join("libzeo.a"),
    )
}

/// PGO, three phases: an instrumented dist build, a training pass over the
/// bench corpus, and a clean profile-use rebuild. RUSTFLAGS ride an EXPLICIT
/// --target so they never reach build scripts or proc-macros (cargo only
/// scopes RUSTFLAGS away from the host when a target is named). The flag
/// change alone re-fingerprints every crate, so no `cargo clean` is needed
/// between the phases.
fn stage_binary_pgo(
    opts: &Opts,
    stage: &Path,
    payload_dir: &Path,
    triple: &str,
) -> Result<(), Error> {
    if let Some(target) = &opts.target
        && *target != host_triple()?
    {
        return Err(Error::new(
            "--pgo trains on this machine; a cross build cannot run the corpus",
        ));
    }
    let prof_dir = cargo_target_root().join("pgo-profiles");
    match std::fs::remove_dir_all(&prof_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::new(format!("clearing {}: {e}", prof_dir.display()))),
    }
    std::fs::create_dir_all(&prof_dir)
        .map_err(|e| Error::new(format!("creating {}: {e}", prof_dir.display())))?;

    let built = cargo_target_root().join(triple).join("dist");
    let build = ["build", "--profile", "dist", "-p", "zeo", "--target", triple];

    println!("dist: pgo phase 1 -- instrumented build");
    let generate = format!("-Cprofile-generate={}", prof_dir.display());
    cargo(&build, &[("RUSTFLAGS", Some(&generate))])?;
    inject_profiler_runtime(&built)?;

    println!("dist: pgo phase 2 -- training on the bench corpus");
    train_pgo(&built.join("zeo"), &prof_dir)?;
    let merged = prof_dir.join("merged.profdata");
    merge_profiles(&prof_dir, &merged)?;

    println!("dist: pgo phase 3 -- profile-use rebuild");
    let use_profile = format!("-Cprofile-use={}", merged.display());
    cargo(&build, &[("RUSTFLAGS", Some(&use_profile))])?;
    stage_built(stage, payload_dir, triple, &built)
}

/// Compile and RUN every bench program with the instrumented toolchain. The
/// compiled programs link the instrumented libzeo.a, so their runs are what
/// teaches the profile the runtime's hot paths -- the compiler's own profile
/// falls out of the compiles for free. Output is checked against each
/// program's .expected: training on a wrong answer would bake a miscompile's
/// shape into the shipped profile.
fn train_pgo(zeo: &Path, prof_dir: &Path) -> Result<(), Error> {
    let benches = entries_matching(&root_join("bench"), "bm_", ".rb")?;
    if benches.is_empty() {
        return Err(Error::new("no bench corpus at bench/bm_*.rb"));
    }
    let raw = prof_dir.join("train-%p.profraw").display().to_string();
    let profile_env = [("LLVM_PROFILE_FILE", Some(raw.as_str()))];
    let work = Scratch::new("pgo-train")?;
    for rb in benches {
        let name = rb
            .file_stem()
            .expect("a matched file name")
            .to_string_lossy()
            .into_owned();
        let bin = work.path().join(&name);
        let out = exec::run(
            &[zeo, Path::new("-o"), &bin, &rb],
            work.path(),
            &profile_env,
            Capture::Both,
        )?;
        if !out.success() {
            return Err(Error::new(format!(
                "pgo training: compiling {name} failed:\n{}",
                out.stderr_text()
            )));
        }
        let out = exec::run(&[&bin], work.path(), &profile_env, Capture::Both)?;
        if !out.success() {
            return Err(Error::new(format!(
                "pgo training: {name} exited {:?}:\n{}",
                out.code, out.stderr_text()
            )));
        }
        let expected = rb.with_extension("rb.expected");
        if expected.is_file() {
            let want = std::fs::read(&expected)
                .map_err(|e| Error::new(format!("reading {}: {e}", expected.display())))?;
            if out.stdout != want {
                return Err(Error::new(format!(
                    "pgo training: {name} diverged from its .expected -- \
                     refusing to train on a wrong answer"
                )));
            }
        }
        println!("dist: pgo trained on {name}");
    }
    Ok(())
}

/// rustc bundles std into a staticlib but NOT profiler_builtins, so the
/// instrumented libzeo.a references `___llvm_profile_instrument_*` it cannot
/// resolve and every training link dies. Merge the TOOLCHAIN's own profiler
/// runtime objects into the instrumented archive -- the same LLVM the
/// .profraw format is locked to, where clang's -fprofile-generate runtime may
/// not be. The phase-3 profile-use rebuild regenerates the archive, so
/// nothing injected ships.
fn inject_profiler_runtime(built: &Path) -> Result<(), Error> {
    let rlib = in_each_rustlib("lib", "libprofiler_builtins-", ".rlib")?
        .ok_or_else(|| Error::new("the toolchain ships no profiler_builtins rlib"))?;
    let archive = built.join("libzeo.a");
    let work = Scratch::new("pgo-rt")?;
    let out = exec::run(
        &[Path::new("ar"), Path::new("x"), &rlib],
        work.path(),
        &[],
        Capture::Both,
    )?;
    if !out.success() {
        return Err(Error::new(format!(
            "ar x {} failed:\n{}",
            rlib.display(),
            out.stderr_text()
        )));
    }
    let objs = entries_matching(work.path(), "", ".o")?;
    if objs.is_empty() {
        return Err(Error::new(format!("{} held no objects", rlib.display())));
    }
    let mut argv = vec![
        PathBuf::from("ar"),
        PathBuf::from("qs"),
        archive.clone(),
    ];
    argv.extend(objs);
    let out = exec::run(&argv, work.path(), &[], Capture::Both)?;
    if !out.success() {
        return Err(Error::new(format!(
            "ar qs {} failed:\n{}",
            archive.display(),
            out.stderr_text()
        )));
    }
    Ok(())
}

/// The TOOLCHAIN's llvm-profdata (the llvm-tools component pinned in
/// rust-toolchain.toml), never a system one: profraw formats are
/// LLVM-version-locked.
fn merge_profiles(prof_dir: &Path, merged: &Path) -> Result<(), Error> {
    let tool = in_each_rustlib("bin", "llvm-profdata", "")?.ok_or_else(|| {
        Error::new(
            "the toolchain carries no llvm-profdata -- run `rustup component add llvm-tools`",
        )
    })?;
    let raws = entries_matching(prof_dir, "", ".profraw")?;
    if raws.is_empty() {
        return Err(Error::new("training produced no .profraw files"));
    }
    let mut argv = vec![tool, PathBuf::from("merge"), PathBuf::from("-o"), merged.to_path_buf()];
    argv.extend(raws);
    let out = exec::run(&argv, root(), &[], Capture::Both)?;
    if !out.success() {
        return Err(Error::new(format!(
            "llvm-profdata merge failed:\n{}",
            out.stderr_text()
        )));
    }
    Ok(())
}

fn toolchain_sysroot() -> Result<PathBuf, Error> {
    let out = exec::run(&["rustc", "--print", "sysroot"], root(), &[], Capture::Both)?;
    let sysroot = out.stdout_text().trim().to_string();
    if sysroot.is_empty() {
        return Err(Error::new("rustc --print sysroot answered nothing"));
    }
    Ok(PathBuf::from(sysroot))
}

/// The first match under `<sysroot>/lib/rustlib/*/<sub>/`. The middle
/// component is the toolchain's own target triple, which is not worth
/// deriving a second time.
fn in_each_rustlib(sub: &str, prefix: &str, suffix: &str) -> Result<Option<PathBuf>, Error> {
    let rustlib = toolchain_sysroot()?.join("lib/rustlib");
    for target in sorted_children(&rustlib)? {
        let found = entries_matching(&target.join(sub), prefix, suffix)?;
        if let Some(first) = found.into_iter().next() {
            return Ok(Some(first));
        }
    }
    Ok(None)
}

/// Sorted files directly in `dir` whose names match, empty when there is no
/// such directory -- these stand in for the globs the Ruby version used, and
/// a missing directory is one of the answers a glob gives.
fn entries_matching(dir: &Path, prefix: &str, suffix: &str) -> Result<Vec<PathBuf>, Error> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(prefix) && name.ends_with(suffix) {
            out.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}

fn sorted_children(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    entries_matching(dir, "", "")
}

/// The staged tree must work with no help from the environment: a temp
/// cache, no ZEO_HOME, no ambient CARGO_TARGET_DIR.
///
/// It COMPILES and runs, rather than `zeo -e`. `-e` takes the JIT path, which
/// needs no archive, so it passed for as long as the staged tree carried no
/// `libzeo.a` at all. Only `-o` proves the payload.
fn smoke_test(stage: &Path) -> Result<(), Error> {
    let work = Scratch::new("dist-smoke")?;
    let cache = work.path().join("cache");
    let src = work.path().join("smoke.rb");
    let bin = work.path().join("smoke");
    write(&src, b"require \"json\"\nputs JSON.generate({smoke: \"ok\"})\n")?;
    println!("dist: smoke test (compile and run)...");
    let cache = cache.to_string_lossy().into_owned();
    let env = [
        ("ZEO_CACHE_DIR", Some(cache.as_str())),
        ("ZEO_HOME", None),
        ("CARGO_TARGET_DIR", None),
    ];
    let zeo = stage.join("bin/zeo");
    // An explicit cwd, so the test never depends on where dist was launched
    // from -- and never inherits a directory staging deletes.
    let build = exec::run(
        &[&zeo, Path::new("-o"), &bin, &src],
        work.path(),
        &env,
        Capture::Both,
    )?;
    if !build.success() {
        return Err(smoke_failure("compile", &build));
    }
    let out = exec::run(&[&bin], work.path(), &env, Capture::Both)?;
    if !out.success() {
        return Err(smoke_failure("run", &out));
    }
    if !out.stdout_text().contains("{\"smoke\":\"ok\"}") {
        return Err(smoke_failure("output", &out));
    }
    println!("dist: smoke test passed");
    Ok(())
}

fn smoke_failure(stage: &str, out: &exec::Output) -> Error {
    Error::new(format!(
        "smoke test FAILED at {stage} (exit {:?}):\nstdout: {}\nstderr: {}",
        out.code,
        out.stdout_text(),
        out.stderr_text()
    ))
}

/// The release archive, plus the checksum file the install channels verify.
/// Written here rather than shelled out to `tar`, so a container image needs
/// no tar and the entry order is this walk's rather than a directory's.
fn tarball(out_dir: &Path, dist_name: &str) -> Result<(), Error> {
    let path = out_dir.join(format!("{dist_name}.tar.gz"));
    let file = std::fs::File::create(&path)
        .map_err(|e| Error::new(format!("creating {}: {e}", path.display())))?;
    let mut builder = tar::Builder::new(GzEncoder::new(file, Compression::default()));
    // Real metadata, so `bin/zeo` stays executable when the archive unpacks.
    builder.follow_symlinks(false);
    for entry in walk(&out_dir.join(dist_name))? {
        let rel = entry
            .strip_prefix(out_dir)
            .expect("walked from inside out_dir");
        builder
            .append_path_with_name(&entry, rel)
            .map_err(|e| Error::new(format!("adding {} to the archive: {e}", entry.display())))?;
    }
    builder
        .into_inner()
        .and_then(|gz| gz.finish())
        .map_err(|e| Error::new(format!("writing {}: {e}", path.display())))?;

    let bytes = std::fs::read(&path)
        .map_err(|e| Error::new(format!("reading {}: {e}", path.display())))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    write(
        &path.with_extension("gz.sha256"),
        format!("{digest}  {dist_name}.tar.gz\n").as_bytes(),
    )?;
    println!("dist: {} ({digest})", path.display());
    Ok(())
}

/// Every file under `dir`, sorted, deepest path last within a directory.
fn walk(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for path in sorted_children(&at)? {
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Staging wipes and recreates its output directory. If this process is
/// RUNNING inside that directory, the wipe pulls the working directory out
/// from under it and every later `chdir` fails with a baffling "could not
/// locate working directory" -- from cargo, from the staged binary, from
/// anything downstream.
fn refuse_to_delete_our_own_cwd(stage: &Path) -> Result<(), Error> {
    let cwd = std::env::current_dir()
        .map_err(|e| Error::new(format!("reading the current directory: {e}")))?;
    let abs = std::path::absolute(stage)
        .map_err(|e| Error::new(format!("resolving {}: {e}", stage.display())))?;
    if !cwd.starts_with(&abs) {
        return Ok(());
    }
    Err(Error::new(format!(
        "refusing to stage into {} -- the current directory ({}) is inside it, and staging \
         starts by deleting that tree. Run dist from elsewhere (the repo root is the usual \
         choice).",
        abs.display(),
        cwd.display()
    )))
}

/// cargo's own progress belongs on the terminal, so nothing is captured.
fn cargo(args: &[&str], env: &[(&str, Option<&str>)]) -> Result<(), Error> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut argv = vec![cargo.as_str()];
    argv.extend_from_slice(args);
    let out = exec::run(&argv, root(), env, Capture::Nothing)?;
    if !out.success() {
        return Err(Error::new(format!(
            "cargo {} exited with {:?}",
            args.join(" "),
            out.code
        )));
    }
    Ok(())
}

fn copy(src: &Path, dest: &Path) -> Result<(), Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::new(format!("creating {}: {e}", parent.display())))?;
    }
    std::fs::copy(src, dest).map_err(|e| {
        Error::new(format!(
            "copying {} to {}: {e}",
            src.display(),
            dest.display()
        ))
    })?;
    Ok(())
}

fn write(dest: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::new(format!("creating {}: {e}", parent.display())))?;
    }
    std::fs::write(dest, bytes).map_err(|e| Error::new(format!("writing {}: {e}", dest.display())))
}
