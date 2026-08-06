//! `cargo xtask dist [--target <triple>] [--no-vendor] [--no-smoke]
//! [--stage-only] [-o <dir>]`: assemble the relocatable distribution.
//!
//! One staged tree feeds every install channel -- the GitHub Release
//! tarball, Homebrew, and the rubygems platform gems all carry exactly this:
//!
//! ```text
//! zeo-<version>-<triple>/
//!   bin/zeo                          # release build
//!   share/zeo/
//!     dist-manifest.json             # {schema, version, target, vendored}
//!     gems/                          # the repo's gems/, verbatim
//!     runtime/                       # a REAL mini-workspace: the four
//!       Cargo.toml                   #   runtime crates + a pruned root
//!       Cargo.lock                   #   manifest, lock, and (default) a
//!       .cargo/config.toml           #   crates.io vendor tree so the
//!       vendor/                      #   first compile builds offline
//!       crates/{zeo-rt,zeo-abi,zeo-dsl,zeo-macros}/
//!   share/doc/zeo/                   # README + licenses
//! ```
//!
//! The runtime workspace's root manifest is REWRITTEN from the live root
//! `Cargo.toml` (members pruned to the four crates, the `zeo` workspace-dep
//! entry dropped, everything else -- `[workspace.package]`,
//! `[workspace.dependencies]`, `[workspace.lints]`, and the artifact-shaping
//! `[profile.*]` sections -- carried verbatim), so it cannot drift from what
//! the dev tree builds.
//!
//! The smoke test runs the staged `bin/zeo` on a hello program with
//! `ZEO_CACHE_DIR` pointed at a temp dir and no `ZEO_HOME`: it proves the
//! exe-relative payload resolution, the offline `--locked` runtime build,
//! and the bundled gems, end to end. `--stage-only` skips the binary,
//! smoke, and tarball and instead validates the staged runtime workspace
//! with `cargo metadata --locked --offline` (seconds, not minutes).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const RUNTIME_CRATES: &[&str] = &["zeo-rt", "zeo-abi", "zeo-dsl", "zeo-macros"];

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let mut target: Option<String> = None;
    let mut vendor = true;
    let mut smoke = true;
    let mut stage_only = false;
    let mut out_dir: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--target" => target = it.next().cloned(),
            "--no-vendor" => vendor = false,
            "--no-smoke" => smoke = false,
            "--stage-only" => stage_only = true,
            "-o" => out_dir = it.next().map(PathBuf::from),
            other => {
                eprintln!("dist: unknown argument `{other}`");
                return ExitCode::FAILURE;
            }
        }
    }
    match run(root, target.as_deref(), vendor, smoke, stage_only, out_dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dist: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    root: &Path,
    target: Option<&str>,
    vendor: bool,
    smoke: bool,
    stage_only: bool,
    out_dir: Option<PathBuf>,
) -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let triple = match target {
        Some(t) => t.to_string(),
        None => host_triple()?,
    };
    let dist_name = format!("zeo-{version}-{triple}");
    let out_dir = out_dir.unwrap_or_else(|| root.join("target/dist"));
    let stage = out_dir.join(&dist_name);
    refuse_to_delete_our_own_cwd(&stage)?;
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| format!("creating {}: {e}", stage.display()))?;

    let payload = stage.join("share/zeo");

    // -- share/zeo/gems ---------------------------------------------------
    copy_tree(&root.join("gems"), &payload.join("gems"), &|p| {
        p.file_name().is_some_and(|n| n == ".DS_Store")
    })?;

    // -- share/zeo/runtime ------------------------------------------------
    let runtime = payload.join("runtime");
    for name in RUNTIME_CRATES {
        let src = root.join("crates").join(name);
        let dst = runtime.join("crates").join(name);
        copy_tree(&src, &dst, &|p| {
            p.file_name()
                .is_some_and(|n| n == "target" || n == ".DS_Store")
        })?;
    }
    std::fs::write(runtime.join("Cargo.toml"), runtime_manifest(root)?)
        .map_err(|e| format!("writing the runtime manifest: {e}"))?;
    // The lock: seed with the root's (the versions the dev tree tested),
    // then let cargo prune the compiler/xtask-only entries.
    std::fs::copy(root.join("Cargo.lock"), runtime.join("Cargo.lock"))
        .map_err(|e| format!("seeding the runtime lockfile: {e}"))?;
    run_cargo(&runtime, &["generate-lockfile"], "generate-lockfile")?;
    if vendor {
        run_cargo(&runtime, &["vendor", "--locked", "vendor"], "vendor")?;
        let cargo_dir = runtime.join(".cargo");
        std::fs::create_dir_all(&cargo_dir).map_err(|e| e.to_string())?;
        std::fs::write(
            cargo_dir.join("config.toml"),
            "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n\
             [source.vendored-sources]\ndirectory = \"vendor\"\n",
        )
        .map_err(|e| format!("writing .cargo/config.toml: {e}"))?;
    }

    // -- share/zeo/dist-manifest.json -------------------------------------
    std::fs::write(
        payload.join("dist-manifest.json"),
        format!(
            "{{\n  \"schema\": 1,\n  \"version\": \"{version}\",\n  \
             \"target\": \"{triple}\",\n  \"vendored\": {vendor}\n}}\n"
        ),
    )
    .map_err(|e| format!("writing dist-manifest.json: {e}"))?;

    // -- share/doc/zeo -----------------------------------------------------
    let doc = stage.join("share/doc/zeo");
    std::fs::create_dir_all(&doc).map_err(|e| e.to_string())?;
    for name in [
        "README.md",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "THIRD-PARTY-NOTICES.md",
    ] {
        let src = root.join(name);
        if src.is_file() {
            std::fs::copy(&src, doc.join(name))
                .map_err(|e| format!("copying {name}: {e}"))?;
        }
    }

    if stage_only {
        // Coherence gate: the staged workspace must resolve with exactly its
        // shipped lock (and vendor tree, when present) -- what an installed
        // zeo's `--locked --offline` build will demand.
        let mut check = vec!["metadata", "--format-version", "1", "--locked"];
        if vendor {
            check.push("--offline");
        }
        run_cargo_quiet(&runtime, &check, "metadata --locked")?;
        println!("dist: staged {} (stage-only, coherence OK)", stage.display());
        return Ok(());
    }

    // -- bin/zeo -----------------------------------------------------------
    let mut build = vec!["build", "--release", "-p", "zeo"];
    if let Some(t) = target {
        build.extend(["--target", t]);
    }
    run_cargo(root, &build, "build --release -p zeo")?;
    let built = match target {
        Some(t) => root.join("target").join(t).join("release/zeo"),
        None => root.join("target/release/zeo"),
    };
    let bin_dir = stage.join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    std::fs::copy(&built, bin_dir.join("zeo"))
        .map_err(|e| format!("copying {}: {e}", built.display()))?;

    // -- smoke -------------------------------------------------------------
    if smoke {
        smoke_test(&stage)?;
    }

    // -- tarball -----------------------------------------------------------
    let tarball = out_dir.join(format!("{dist_name}.tar.gz"));
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&tarball)
        .arg("-C")
        .arg(&out_dir)
        .arg(&dist_name)
        .status()
        .map_err(|e| format!("running tar: {e}"))?;
    if !status.success() {
        return Err(format!("tar exited with {status}"));
    }
    let digest = sha256_file(&tarball)?;
    std::fs::write(
        out_dir.join(format!("{dist_name}.tar.gz.sha256")),
        format!("{digest}  {dist_name}.tar.gz\n"),
    )
    .map_err(|e| format!("writing the checksum: {e}"))?;
    println!("dist: {} ({digest})", tarball.display());
    Ok(())
}

/// Staging wipes and recreates its output directory. If this process is
/// RUNNING inside that directory (a shell left there by an earlier build, or
/// `-o` pointed at the cwd), the wipe pulls the working directory out from
/// under us and every later `current_dir()` fails with a baffling "could not
/// locate working directory" -- from cargo, from the staged binary, from
/// anything downstream. Refuse up front with an actionable message instead.
fn refuse_to_delete_our_own_cwd(stage: &Path) -> Result<(), String> {
    let (Ok(cwd), Ok(stage_abs)) = (std::env::current_dir(), absolutize(stage)) else {
        return Ok(()); // Nothing resolvable to compare; the wipe is no riskier.
    };
    if cwd.starts_with(&stage_abs) {
        return Err(format!(
            "refusing to stage into {} -- the current directory ({}) is inside it, \
             and staging starts by deleting that tree. Run dist from elsewhere \
             (the repo root is the usual choice).",
            stage_abs.display(),
            cwd.display()
        ));
    }
    Ok(())
}

/// `canonicalize` needs the path to exist; a not-yet-created staging dir does
/// not. Fall back to the nearest existing ancestor plus the remainder, which
/// is enough to compare against the cwd.
fn absolutize(path: &Path) -> std::io::Result<PathBuf> {
    if let Ok(real) = std::fs::canonicalize(path) {
        return Ok(real);
    }
    let mut ancestor = path;
    while let Some(parent) = ancestor.parent() {
        if let Ok(real) = std::fs::canonicalize(parent) {
            let rest = path.strip_prefix(parent).unwrap_or(path);
            return Ok(real.join(rest));
        }
        ancestor = parent;
    }
    std::env::current_dir().map(|cwd| cwd.join(path))
}

/// The staged tree must work with no help from the environment: a temp cache,
/// no ZEO_HOME, no ambient CARGO_TARGET_DIR. `-e` exercises the Debug-profile
/// runtime build (offline when vendored), the gems dir, and a real
/// compile+run.
fn smoke_test(stage: &Path) -> Result<(), String> {
    let cache = std::env::temp_dir().join(format!("zeo-dist-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    println!("dist: smoke test (cold runtime build -- takes a few minutes)...");
    let out = Command::new(stage.join("bin/zeo"))
        .args(["-e", "require \"json\"; puts JSON.generate({smoke: \"ok\"})"])
        // An explicit cwd, so the test never depends on where dist was
        // launched from -- and never inherits a directory staging deletes.
        .current_dir(std::env::temp_dir())
        .env_remove("ZEO_HOME")
        .env_remove("CARGO_TARGET_DIR")
        .env("ZEO_CACHE_DIR", &cache)
        .output()
        .map_err(|e| format!("running the staged zeo: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ok = out.status.success() && stdout.contains("{\"smoke\":\"ok\"}");
    let _ = std::fs::remove_dir_all(&cache);
    if ok {
        println!("dist: smoke test passed");
        Ok(())
    } else {
        Err(format!(
            "smoke test FAILED (exit {}):\nstdout: {stdout}\nstderr: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// The payload's runtime-workspace manifest, rewritten from the LIVE root
/// manifest so the two cannot drift: members pruned to the runtime crates,
/// the `zeo` workspace-dependency entry (whose path does not exist in the
/// payload) dropped, all other tables carried through verbatim.
fn runtime_manifest(root: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("reading the root manifest: {e}"))?;
    let mut doc: toml_edit::DocumentMut = source
        .parse()
        .map_err(|e| format!("parsing the root manifest: {e}"))?;
    let members = RUNTIME_CRATES
        .iter()
        .map(|n| format!("crates/{n}"))
        .collect::<toml_edit::Array>();
    doc["workspace"]["members"] = toml_edit::value(members);
    if let Some(deps) = doc["workspace"]["dependencies"].as_table_mut() {
        deps.remove("zeo");
    }
    Ok(format!(
        "# @generated by `cargo xtask dist` from the repo's root Cargo.toml.\n{doc}"
    ))
}

fn host_triple() -> Result<String, String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| format!("running rustc -vV: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
        .ok_or_else(|| "rustc -vV printed no host line".to_string())
}

fn run_cargo(dir: &Path, args: &[&str], label: &str) -> Result<(), String> {
    let status = Command::new("cargo")
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| format!("running cargo {label}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo {label} exited with {status}"))
    }
}

/// Like [`run_cargo`] but swallowing stdout (metadata dumps megabytes).
fn run_cargo_quiet(dir: &Path, args: &[&str], label: &str) -> Result<(), String> {
    let out = Command::new("cargo")
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("running cargo {label}: {e}"))?;
    if out.success() {
        Ok(())
    } else {
        Err(format!("cargo {label} exited with {out}"))
    }
}

fn copy_tree(src: &Path, dst: &Path, skip: &dyn Fn(&Path) -> bool) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("creating {}: {e}", dst.display()))?;
    let entries =
        std::fs::read_dir(src).map_err(|e| format!("reading {}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let from = entry.path();
        if skip(&from) {
            continue;
        }
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to, skip)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("copying {}: {e}", from.display()))?;
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::Digest as _;
    let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}
