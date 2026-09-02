//! MRI's C API headers, fetched once and finished in the cache.
//!
//! Nothing is vendored. The first extension build fetches upstream's
//! `include/` tree at the rev `zeo-capi` pins, applies zeo's edits to it and
//! writes the two files zeo adds (`zeo_capi::headers::finish`), and every
//! build after that finds the finished tree by its rev. So a header bump is
//! a change to the pin, and an installed zeo builds a native gem the same
//! way the dev tree does -- which a `cargo install`ed zeo could not before.
//!
//! The tree lives under [`crate::home::build_root`]: `target/` in the dev
//! tree, the per-user cache for an install. A release payload may carry a
//! finished tree pre-seeded by `cargo xtask dist`, which is used in place.
//!
//! Two overrides for a machine without the network: `ZEO_RUBY_HEADERS_DIR`
//! names upstream's pristine `include/` directory (a `ruby/ruby` checkout at
//! the pinned rev), and `ZEO_RUBY_HEADERS_TARBALL` names a local copy of the
//! GitHub archive. Either is finished into the cache like a fetch.

use std::io::Read;
use std::path::{Path, PathBuf};

use zeo_capi::headers::{self, Pin, Target};

use crate::home::ZeoHome;

/// The two include roots an extension build needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderDirs {
    /// Upstream's `include/` with zeo's edits: `-I` this.
    pub include: PathBuf,
    /// Holds `ruby/config.h`: `-I` this too (mkmf's `archhdrdir`).
    pub config: PathBuf,
}

impl HeaderDirs {
    fn under(root: &Path) -> HeaderDirs {
        HeaderDirs {
            include: root.join("include"),
            config: root.join("config"),
        }
    }
}

/// The file written last, so a tree that has it is whole.
const COMPLETE: &str = ".complete";

/// The three licence files the tree carries beside `include/`.
const LICENCES: &[&str] = &["BSDL", "COPYING", "LEGAL"];

/// Where the finished tree lives (or will), with no I/O. The rbconfig shim
/// names these paths for any program that reads `RbConfig::CONFIG`, and only
/// an extension build pays for the fetch.
pub fn expected() -> HeaderDirs {
    HeaderDirs::under(&cache_dir(&headers::pin()))
}

fn cache_dir(pin: &Pin) -> PathBuf {
    crate::home::build_root()
        .join("ruby-headers")
        .join(&pin.rev)
}

/// The finished tree, fetching it if this machine has none yet.
pub fn ensure() -> Result<HeaderDirs, String> {
    let pin = headers::pin();
    let cache = cache_dir(&pin);
    if cache.join(COMPLETE).is_file() {
        return Ok(HeaderDirs::under(&cache));
    }
    if let ZeoHome::Installed { payload, .. } = crate::home::zeo_home() {
        let seeded = payload.join("ruby-headers").join(&pin.rev);
        if seeded.join(COMPLETE).is_file() {
            return Ok(HeaderDirs::under(&seeded));
        }
    }
    if let Some(dir) = std::env::var_os("ZEO_RUBY_HEADERS_DIR") {
        return materialize_from(Path::new(&dir));
    }
    let tmp = Staging::new(&cache)?;
    let source = match std::env::var_os("ZEO_RUBY_HEADERS_TARBALL") {
        Some(path) => Source::Tarball(PathBuf::from(path)),
        None => download(&pin, tmp.path())?,
    };
    match source {
        Source::Tarball(tarball) => {
            verify_sha256(&tarball, &pin.sha256)?;
            unpack(&tarball, &pin, tmp.path())?;
            // The download, not a user's override, which stays theirs.
            if tarball.starts_with(tmp.path()) {
                let _ = std::fs::remove_file(&tarball);
            }
            tmp.finish()
        }
        Source::Checkout(checkout) => {
            drop(tmp);
            materialize_from(&checkout.join(&pin.subdir))
        }
    }
}

/// What a fetch produced.
enum Source {
    /// The GitHub archive, to verify and unpack.
    Tarball(PathBuf),
    /// A `git clone` at the pinned rev, to copy from.
    Checkout(PathBuf),
}

/// The finished tree, built from upstream's pristine `include/` directory
/// (`ZEO_RUBY_HEADERS_DIR`, or `cargo xtask`'s checkout).
pub fn materialize_from(pristine_include: &Path) -> Result<HeaderDirs, String> {
    let pin = headers::pin();
    if !pristine_include.join("ruby.h").is_file() {
        return Err(format!(
            "{} is not upstream's include/ tree (no ruby.h in it)",
            pristine_include.display()
        ));
    }
    let tmp = Staging::new(&cache_dir(&pin))?;
    copy_tree(pristine_include, &tmp.path().join("include"))?;
    if let Some(checkout) = pristine_include.parent() {
        for name in LICENCES {
            let src = checkout.join(name);
            if src.is_file() {
                std::fs::copy(&src, tmp.path().join(name))
                    .map_err(|e| format!("copying {}: {e}", src.display()))?;
            }
        }
    }
    tmp.finish()
}

/// A tree under construction beside its final place, renamed there whole.
/// Two processes racing to the same rev both finish; the second finds the
/// first's tree in place and discards its own.
struct Staging {
    dir: PathBuf,
    dest: PathBuf,
}

impl Staging {
    fn new(dest: &Path) -> Result<Staging, String> {
        let parent = dest.parent().ok_or("the header cache has no parent")?;
        let dir = parent.join(format!(
            ".{}-{}",
            dest.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config"))
            .map_err(|e| format!("creating {}: {e}", dir.display()))?;
        Ok(Staging {
            dir,
            dest: dest.to_path_buf(),
        })
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn finish(self) -> Result<HeaderDirs, String> {
        let dirs = HeaderDirs::under(&self.dir);
        headers::finish(&dirs.include, &dirs.config, Target::host()?)?;
        std::fs::write(self.dir.join(COMPLETE), headers::pin().rev.as_bytes())
            .map_err(|e| format!("writing {}: {e}", self.dir.display()))?;
        if std::fs::rename(&self.dir, &self.dest).is_err() {
            if !self.dest.join(COMPLETE).is_file() {
                return Err(format!(
                    "cannot move the finished headers into {}",
                    self.dest.display()
                ));
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
        Ok(HeaderDirs::under(&self.dest))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        // Only an abandoned build still has the directory; `finish` moved
        // a whole one away.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `curl` the archive into `into`, or `git` when `curl` cannot. Neither
/// tool is a build dependency: an HTTP client in the compiler would be a
/// large one for a single fetch, and every machine that builds a C
/// extension already has both.
fn download(pin: &Pin, into: &Path) -> Result<Source, String> {
    let url = pin.archive_url();
    let tarball = into.join("upstream.tar.gz");
    let curl = std::process::Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(&tarball)
        .arg(&url)
        .output();
    match curl {
        Ok(out) if out.status.success() => return Ok(Source::Tarball(tarball)),
        Ok(out) => tracing::warn!(
            url,
            "curl could not fetch the C API headers: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => tracing::warn!(url, "curl is not runnable: {e}"),
    }
    let checkout = into.join("checkout");
    let git = std::process::Command::new("git")
        .args(["clone", "--quiet", "--depth", "1", "--branch", &pin.tag])
        .arg(&pin.repo)
        .arg(&checkout)
        .output()
        .map_err(|e| fetch_error(pin, &format!("curl failed and git is not runnable: {e}")))?;
    if !git.status.success() {
        return Err(fetch_error(
            pin,
            &format!(
                "curl and git both failed:\n{}",
                String::from_utf8_lossy(&git.stderr).trim()
            ),
        ));
    }
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&checkout)
        .output()
        .map_err(|e| format!("git rev-parse: {e}"))?;
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    if head != pin.rev {
        return Err(format!(
            "tag {} is pinned at {} but the clone is at {head}: the upstream tag moved, and \
             the pin must be reviewed rather than followed",
            pin.tag, pin.rev
        ));
    }
    Ok(Source::Checkout(checkout))
}

fn fetch_error(pin: &Pin, why: &str) -> String {
    format!(
        "cannot fetch MRI's C API headers ({}): {why}\n\
         On a machine without the network, set ZEO_RUBY_HEADERS_TARBALL to a copy of that \
         archive, or ZEO_RUBY_HEADERS_DIR to the include/ directory of a ruby/ruby checkout \
         at {}.",
        pin.archive_url(),
        pin.rev
    )
}

fn verify_sha256(tarball: &Path, want: &str) -> Result<(), String> {
    use sha2::Digest;
    let bytes =
        std::fs::read(tarball).map_err(|e| format!("reading {}: {e}", tarball.display()))?;
    let have = format!("{:x}", sha2::Sha256::digest(&bytes));
    if have != want {
        return Err(format!(
            "{} is not the pinned archive: sha256 {have}, expected {want}",
            tarball.display()
        ));
    }
    Ok(())
}

/// Unpack `<root>/include/**` to `into/include/**`, and the licence files
/// beside it.
fn unpack(tarball: &Path, pin: &Pin, into: &Path) -> Result<(), String> {
    let file =
        std::fs::File::open(tarball).map_err(|e| format!("opening {}: {e}", tarball.display()))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let root = pin.archive_root();
    let mut headers = 0usize;
    for entry in archive
        .entries()
        .map_err(|e| format!("reading {}: {e}", tarball.display()))?
    {
        let mut entry = entry.map_err(|e| format!("reading {}: {e}", tarball.display()))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let Ok(rel) = path.strip_prefix(&root) else {
            continue;
        };
        let dest = if rel.starts_with(&pin.subdir) {
            headers += 1;
            into.join("include")
                .join(rel.strip_prefix(&pin.subdir).unwrap_or(rel))
        } else if LICENCES.iter().any(|l| Path::new(l) == rel) {
            into.join(rel)
        } else {
            continue;
        };
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        std::fs::write(&dest, bytes).map_err(|e| format!("writing {}: {e}", dest.display()))?;
    }
    if headers == 0 {
        return Err(format!(
            "{} holds no {root}/{}/ -- is it the pinned archive?",
            tarball.display(),
            pin.subdir
        ));
    }
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("creating {}: {e}", dest.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("reading {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("reading {}: {e}", src.display()))?;
        let (from, to) = (entry.path(), dest.join(entry.file_name()));
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("copying {}: {e}", from.display()))?;
        }
    }
    Ok(())
}
