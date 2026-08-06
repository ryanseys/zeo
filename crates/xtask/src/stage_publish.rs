//! `cargo xtask stage-publish [--check]`: stage the two artifacts the
//! published `zeo` crate ships but the repo does not commit.
//!
//! - `crates/zeo/src/class_surface.pregen.rs` -- the builtin class-surface
//!   projection. NOT regenerated here from scratch: a `cargo build -p zeo`
//!   runs (a no-op when fresh) and the build script's own `$OUT_DIR/
//!   class_surface.rs` is copied, so the staged file is byte-identical to
//!   what the dev tree compiles against, by construction.
//! - `crates/zeo/gems.pregen.tar.gz` -- the bundled `gems/` tree, embedded
//!   into a registry-installed binary and extracted on first run. Built
//!   deterministically (sorted walk, zeroed mtimes) so `--check` can compare
//!   content, not compression accidents.
//!
//! `--check` verifies staged copies that EXIST still match a fresh
//! generation and exits nonzero on drift; absent artifacts are fine (the dev
//! tree does not carry them). CI runs the check so a release can never ship
//! a stale staging.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");
    match run(root, check) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("stage-publish: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(root: &Path, check: bool) -> Result<(), String> {
    let surface_dest = root.join("crates/zeo/src/class_surface.pregen.rs");
    let gems_dest = root.join("crates/zeo/gems.pregen.tar.gz");

    let surface = generated_class_surface(root)?;
    let gems_tar = build_gems_tar(&root.join("gems"))?;

    if check {
        if surface_dest.is_file() {
            let staged = std::fs::read_to_string(&surface_dest)
                .map_err(|e| format!("reading {}: {e}", surface_dest.display()))?;
            if staged != surface {
                return Err(format!(
                    "{} is STALE relative to the live projection -- re-run \
                     `cargo xtask stage-publish`",
                    surface_dest.display()
                ));
            }
        }
        if gems_dest.is_file() {
            let staged = std::fs::read(&gems_dest)
                .map_err(|e| format!("reading {}: {e}", gems_dest.display()))?;
            // Compare DECOMPRESSED tar bytes: the tar stream is deterministic
            // (sorted, zeroed mtimes); the gzip envelope need not be.
            let mut staged_tar = Vec::new();
            flate2::read::GzDecoder::new(staged.as_slice())
                .read_to_end(&mut staged_tar)
                .map_err(|e| format!("decompressing {}: {e}", gems_dest.display()))?;
            if staged_tar != gems_tar {
                return Err(format!(
                    "{} is STALE relative to gems/ -- re-run `cargo xtask stage-publish`",
                    gems_dest.display()
                ));
            }
        }
        println!("stage-publish --check: staged artifacts match (or are absent).");
        return Ok(());
    }

    std::fs::write(&surface_dest, &surface)
        .map_err(|e| format!("writing {}: {e}", surface_dest.display()))?;
    let gz = gzip(&gems_tar)?;
    std::fs::write(&gems_dest, &gz).map_err(|e| format!("writing {}: {e}", gems_dest.display()))?;
    println!(
        "staged {} ({} bytes) and {} ({} bytes -- {} uncompressed)",
        surface_dest.display(),
        surface.len(),
        gems_dest.display(),
        gz.len(),
        gems_tar.len(),
    );
    Ok(())
}

/// Build `-p zeo` (no-op when fresh) and pull `class_surface.rs` out of the
/// build script's OUT_DIR, located via cargo's JSON message stream -- never
/// by globbing `target/`, where stale build dirs for older fingerprints
/// linger.
fn generated_class_surface(root: &Path) -> Result<String, String> {
    let out = std::process::Command::new("cargo")
        .args(["build", "-p", "zeo", "--message-format=json-render-diagnostics"])
        .current_dir(root)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("running cargo build -p zeo: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo build -p zeo exited with {}", out.status));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut out_dir = None;
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["reason"].as_str() != Some("build-script-executed") {
            continue;
        }
        let id = v["package_id"].as_str().unwrap_or_default();
        // Path package ids end `.../crates/zeo#<version>`; the separate
        // `#name@version` form appears when the dir name differs.
        if id.contains("/crates/zeo#") || id.contains("#zeo@") {
            out_dir = v["out_dir"].as_str().map(PathBuf::from);
        }
    }
    let out_dir = out_dir.ok_or("no build-script-executed message for zeo")?;
    let path = out_dir.join("class_surface.rs");
    std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))
}

/// A deterministic tar of `gems/`: entries named relative to the dir (so the
/// extraction dir IS the gems dir), sorted, mtime 0, mode reduced to
/// 0644/0755. `.DS_Store` and similar junk skipped.
fn build_gems_tar(gems_dir: &Path) -> Result<Vec<u8>, String> {
    if !gems_dir.is_dir() {
        return Err(format!("{} is not a directory", gems_dir.display()));
    }
    let mut files = Vec::new();
    collect_files(gems_dir, &mut files);
    files.sort();
    let mut builder = tar::Builder::new(Vec::new());
    for path in &files {
        let rel = path
            .strip_prefix(gems_dir)
            .expect("collected under gems_dir");
        let data = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        let executable = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            {
                false
            }
        };
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(if executable { 0o755 } else { 0o644 });
        header.set_mtime(0);
        builder
            .append_data(&mut header, rel, data.as_slice())
            .map_err(|e| format!("archiving {}: {e}", rel.display()))?;
    }
    builder
        .into_inner()
        .map_err(|e| format!("finishing the gems tar: {e}"))
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == ".DS_Store" {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write as _;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(bytes)
        .and_then(|_| enc.finish())
        .map_err(|e| format!("gzipping the gems archive: {e}"))
}
