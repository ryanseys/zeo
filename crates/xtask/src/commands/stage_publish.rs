//! Stage the two artifacts the published `zeo` crate ships but the repo does
//! not commit.
//!
//! - `crates/zeo/src/class_surface.pregen.rs` -- the builtin class-surface
//!   projection. NOT regenerated from scratch here: a `cargo build -p zeo`
//!   runs (a no-op when fresh) and the build script's own
//!   `$OUT_DIR/class_surface.rs` is copied, so the staged file is
//!   byte-identical to what the dev tree compiles against, by construction.
//! - `crates/zeo/gems.pregen.tar.gz` -- every bundled library, embedded into
//!   a registry-installed binary and extracted on first run. Built
//!   deterministically (sorted walk, zeroed mtimes) so `--check` compares
//!   content rather than compression accidents.
//!
//! `--check` verifies staged copies that EXIST still match a fresh
//! generation and exits nonzero on drift; absent artifacts are fine, since
//! the dev tree does not carry them. CI runs the check so a release can never
//! ship a stale staging.

use std::io::Read;
use std::path::Path;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::exec::{self, Capture};
use crate::{Error, payload, root, root_join};

const SURFACE: &str = "crates/zeo/src/class_surface.pregen.rs";
const GEMS_TAR: &str = "crates/zeo/gems.pregen.tar.gz";

const USAGE: &str = "usage: cargo xtask stage-publish [--check]";

pub fn run(args: &[String]) -> Result<(), Error> {
    let mut check = false;
    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(Error::new(format!("unknown option {other:?}\n{USAGE}"))),
        }
    }

    let surface = generated_class_surface()?;
    let gems_tar = build_gems_tar()?;
    let surface_dest = root_join(SURFACE);
    let gems_dest = root_join(GEMS_TAR);

    if check {
        return check_staged(&surface_dest, &surface, &gems_dest, &gems_tar);
    }

    std::fs::write(&surface_dest, &surface)
        .map_err(|e| Error::new(format!("writing {}: {e}", surface_dest.display())))?;
    let gz = gzip(&gems_tar)?;
    std::fs::write(&gems_dest, &gz)
        .map_err(|e| Error::new(format!("writing {}: {e}", gems_dest.display())))?;
    println!(
        "staged {} ({} bytes) and {} ({} bytes -- {} uncompressed)",
        surface_dest.display(),
        surface.len(),
        gems_dest.display(),
        gz.len(),
        gems_tar.len()
    );
    Ok(())
}

fn check_staged(
    surface_dest: &Path,
    surface: &[u8],
    gems_dest: &Path,
    gems_tar: &[u8],
) -> Result<(), Error> {
    if surface_dest.is_file() {
        let staged = std::fs::read(surface_dest)
            .map_err(|e| Error::new(format!("reading {}: {e}", surface_dest.display())))?;
        if staged != surface {
            return Err(Error::new(format!(
                "{SURFACE} is STALE relative to the live projection -- re-run \
                 `cargo xtask stage-publish`"
            )));
        }
    }
    if gems_dest.is_file() {
        // Compare DECOMPRESSED tar bytes: the tar stream is deterministic
        // (sorted, zeroed mtimes); the gzip envelope need not be.
        let packed = std::fs::read(gems_dest)
            .map_err(|e| Error::new(format!("reading {}: {e}", gems_dest.display())))?;
        let mut staged = Vec::new();
        GzDecoder::new(&packed[..])
            .read_to_end(&mut staged)
            .map_err(|e| Error::new(format!("decompressing {}: {e}", gems_dest.display())))?;
        if staged != gems_tar {
            return Err(Error::new(format!(
                "{GEMS_TAR} is STALE relative to the bundled libraries -- re-run \
                 `cargo xtask stage-publish`"
            )));
        }
    }
    println!("stage-publish --check: staged artifacts match (or are absent).");
    Ok(())
}

/// Build `-p zeo` (a no-op when fresh) and pull `class_surface.rs` out of the
/// build script's OUT_DIR, located through cargo's JSON message stream --
/// never by globbing `target/`, where stale build dirs for older fingerprints
/// linger.
fn generated_class_surface() -> Result<Vec<u8>, Error> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = exec::run(
        &[
            cargo.as_str(),
            "build",
            "-p",
            "zeo",
            "--message-format=json-render-diagnostics",
        ],
        root(),
        &[],
        Capture::Stdout,
    )?;
    if !out.success() {
        return Err(Error::new(format!(
            "cargo build -p zeo exited with {:?}",
            out.code
        )));
    }
    let mut out_dir = None;
    for line in out.stdout_text().lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["reason"] != "build-script-executed" {
            continue;
        }
        let id = v["package_id"].as_str().unwrap_or_default();
        // A path package id ends `.../crates/zeo#<version>`; the separate
        // `#name@version` form appears when the dir name differs.
        if id.contains("/crates/zeo#") || id.contains("#zeo@") {
            out_dir = v["out_dir"].as_str().map(str::to_string);
        }
    }
    let out_dir = out_dir.ok_or_else(|| Error::new("no build-script-executed message for zeo"))?;
    let path = Path::new(&out_dir).join("class_surface.rs");
    std::fs::read(&path).map_err(|e| Error::new(format!("reading {}: {e}", path.display())))
}

/// A deterministic tar of every bundled library: entries named `<name>/...`,
/// so the extraction dir IS the gems dir; sorted; mtime 0; mode reduced to
/// 0644 or 0755.
fn build_gems_tar() -> Result<Vec<u8>, Error> {
    let files = payload::files()?;
    if files.is_empty() {
        return Err(Error::new("no bundled libraries found"));
    }
    let mut builder = tar::Builder::new(Vec::new());
    for (name, path) in &files {
        let data = std::fs::read(path)
            .map_err(|e| Error::new(format!("reading {}: {e}", path.display())))?;
        let mut header = tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(if is_executable(path) { 0o755 } else { 0o644 });
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_entry_type(tar::EntryType::Regular);
        builder
            .append_data(&mut header, name, &data[..])
            .map_err(|e| Error::new(format!("adding {name} to the tar: {e}")))?;
    }
    builder
        .into_inner()
        .map_err(|e| Error::new(format!("finishing the tar: {e}")))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
}

/// Best compression, and mtime 0 so the envelope carries no clock.
fn gzip(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::best());
    std::io::Write::write_all(&mut gz, bytes).map_err(|e| Error::new(format!("gzip: {e}")))?;
    gz.finish().map_err(|e| Error::new(format!("gzip: {e}")))
}
