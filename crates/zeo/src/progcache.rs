//! The compiled-program cache: `zeo prog.rb` links once and execs after.
//!
//! zeo recompiled a program on every invocation. That is invisible for a
//! script and ruinous for a tool: `zeo gem --version` and `zeo bundle
//! --version` each spent seconds rebuilding RubyGems and Bundler, and a
//! `gem install` re-enters zeo several times, so one install paid it over
//! and over.
//!
//! So the default run mode compiles ahead of time, links a real binary into
//! this cache, and `exec`s it -- and on the next run with the same inputs it
//! skips straight to the `exec`. A cache MISS costs a link that the in-process
//! JIT did not; every run after it costs nothing at all.
//!
//! # What makes a hit
//!
//! Two halves, because the whole input set is not known until a compile has
//! already happened:
//!
//! * The KEY names the entry -- the source or its path, the compile options,
//!   zeo's own identity, and the environment a compile reads. It is
//!   computable before compiling, so it selects a candidate directory.
//! * The MANIFEST records every source file that compile actually read, with
//!   a hash of the exact text used, plus the modification time of every
//!   directory it read from. A hit re-checks all of it.
//!
//! # What it does not catch
//!
//! A NEW file appearing in a directory zeo searches but never read from --
//! an empty `-I` root that later gains the feature a `require` wanted. The
//! directory stamps cover every directory a compile read, which is where a
//! shadowing file realistically lands. `ZEO_CACHE=0` turns the cache off for
//! a run that needs certainty.

use std::path::{Path, PathBuf};

/// One source file a compile read: the name it was registered under, and the
/// exact text that was compiled. The text is shared, so collecting these
/// costs an `Arc` bump per file rather than a copy.
pub struct Input {
    pub name: String,
    pub source: std::sync::Arc<str>,
}

/// Whether the cache is in use. `ZEO_CACHE=0` turns it off.
pub fn enabled() -> bool {
    std::env::var("ZEO_CACHE").as_deref() != Ok("0")
}

/// `<build root>/programs`, beside the other things zeo builds.
/// `ZEO_PROGRAM_CACHE` puts it somewhere else -- what a test that must not
/// share the developer's cache sets.
fn root() -> PathBuf {
    match std::env::var_os("ZEO_PROGRAM_CACHE") {
        Some(dir) => PathBuf::from(dir),
        None => crate::home::build_root().join("programs"),
    }
}

/// The identity of a cached program, as a hex string that names its
/// directory.
///
/// `opts` is hashed WHOLE through its derived `Hash`, which is the point of
/// that derive: a new field that changes what zeo emits joins the key without
/// anyone remembering to add it, and a field that cannot be hashed fails the
/// build instead of silently serving a stale program.
pub fn key(source: &str, opts: &crate::CompileOptions) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "zeo-progcache-1".hash(&mut h);
    source.hash(&mut h);
    opts.hash(&mut h);
    zeo_identity().hash(&mut h);
    for (name, value) in compile_env() {
        name.hash(&mut h);
        value.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// One file's identity for the key: path, length and modification time.
type FileId = (String, u64, Option<std::time::SystemTime>);

fn file_identity(path: &Path) -> FileId {
    let meta = std::fs::metadata(path).ok();
    (
        path.to_string_lossy().into_owned(),
        meta.as_ref().map_or(0, std::fs::Metadata::len),
        meta.and_then(|m| m.modified().ok()),
    )
}

/// What tells one zeo build from another: the executable AND the runtime
/// archive its programs link, each by path, length and modification time.
///
/// Both halves matter. A rebuilt compiler emits different code, and in a dev
/// tree that happens many times a day. A rebuilt ARCHIVE changes every
/// program's runtime WITHOUT touching the compiler binary -- a zeo-rt body
/// fix leaves the projected class surface identical, so cargo never relinks
/// `zeo` -- and a key without it kept serving programs linked against the
/// old runtime. `runtime_archive` in a dev tree also brings the archive up
/// to date first, so the identity hashed here is the one a fresh compile
/// would link.
fn zeo_identity() -> (FileId, FileId) {
    let exe = std::env::current_exe().unwrap_or_default();
    let archive = crate::backend::link::runtime_archive().unwrap_or_default();
    (file_identity(&exe), file_identity(&archive))
}

/// Environment a COMPILE reads, sorted so the key is stable.
///
/// By prefix rather than by name: `RUBYOPT`, `RUBYLIB`, `GEM_PATH`,
/// `BUNDLE_GEMFILE` and every `ZEO_*` dial can change what zeo emits, and a
/// list of exact names would go stale the first time a dial is added. An
/// unrelated variable under one of these prefixes only costs an extra cache
/// entry.
fn compile_env() -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = std::env::vars()
        .filter(|(name, _)| {
            name.starts_with("ZEO_")
                || name.starts_with("RUBY")
                || name.starts_with("GEM_")
                || name.starts_with("BUNDLE_")
        })
        .collect();
    rows.sort();
    rows
}

/// The cached binary for `key`, if one is there and every input it was built
/// from is unchanged.
pub fn lookup(key: &str) -> Option<PathBuf> {
    let dir = root().join(key);
    let bin = dir.join("bin");
    if !bin.is_file() {
        return None;
    }
    let manifest = std::fs::read_to_string(dir.join("manifest")).ok()?;
    verify(&manifest).then_some(bin)
}

// ---- The package cache: the same mechanism, holding `.zeopkg`s. ---------
//
// A separately compiled package caches machine-wide exactly the way a
// program does: the key hashes the entry, the options and zeo's identity;
// the manifest re-reads every source the compile used. `ZEO_CACHE=0`
// turns both caches off together, and `ZEO_PACKAGE_CACHE` relocates this
// one the way `ZEO_PROGRAM_CACHE` relocates the other.

/// `<build root>/packages`, beside `programs`.
fn pkg_root() -> PathBuf {
    match std::env::var_os("ZEO_PACKAGE_CACHE") {
        Some(dir) => PathBuf::from(dir),
        None => crate::home::build_root().join("packages"),
    }
}

/// The package cache's key: [`key`], with the OUTPUT spelling neutralized
/// first -- `manifest_out` names a destination, not an input, so one
/// cache entry serves every `-o`.
pub fn pkg_key(source: &str, opts: &crate::CompileOptions) -> String {
    let mut keyed = opts.clone();
    if let Some(pb) = &mut keyed.package_build {
        pb.manifest_out = PathBuf::new();
    }
    key(source, &keyed)
}

/// The cached artifact for `key`, when its manifest still vouches for it.
pub fn pkg_lookup(key: &str) -> Option<PathBuf> {
    let dir = pkg_root().join(key);
    let artifact = dir.join("pkg.zeopkg");
    if !artifact.is_file() {
        return None;
    }
    let manifest = std::fs::read_to_string(dir.join("manifest")).ok()?;
    verify(&manifest).then_some(artifact)
}

/// Where a fresh package build should land; the caller writes the artifact,
/// then calls [`pkg_commit`]. An artifact with no manifest never reads as a
/// hit.
pub fn pkg_reserve(key: &str) -> std::io::Result<PathBuf> {
    let dir = pkg_root().join(key);
    std::fs::create_dir_all(&dir)?;
    let _ = std::fs::remove_file(dir.join("manifest"));
    Ok(dir.join("pkg.zeopkg"))
}

/// Record what the cached artifact at `key` was built from.
pub fn pkg_commit(key: &str, inputs: &[Input]) -> std::io::Result<()> {
    std::fs::write(pkg_root().join(key).join("manifest"), rows_for(inputs)?)
}

/// A `.zeopkg`'s embedded object, landed where a link line can name it:
/// `<pkg root>/objects/<digest>.o`. Content-addressed, so an existing file
/// is already the right bytes; a read-only cache degrades to the system
/// temp directory rather than failing the compile.
pub fn pkg_object_file(digest: u64, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let place = |dir: PathBuf| -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{digest:016x}.o"));
        if !path.is_file() {
            let tmp = dir.join(format!("{digest:016x}.{}.tmp", std::process::id()));
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, &path)?;
        }
        Ok(path)
    };
    place(pkg_root().join("objects"))
        .or_else(|_| place(std::env::temp_dir().join("zeo-pkg-objects")))
}

/// Where a fresh build should be linked. The caller links here, then calls
/// [`commit`]; a binary with no manifest beside it never reads as a hit.
pub fn reserve(key: &str) -> std::io::Result<PathBuf> {
    let dir = root().join(key);
    std::fs::create_dir_all(&dir)?;
    // A leftover manifest must not vouch for a binary that is about to be
    // overwritten: if the link fails, the entry has to read as a miss.
    let _ = std::fs::remove_file(dir.join("manifest"));
    Ok(dir.join("bin"))
}

/// Record what the linked binary at `key` was built from.
pub fn commit(key: &str, inputs: &[Input]) -> std::io::Result<()> {
    std::fs::write(root().join(key).join("manifest"), rows_for(inputs)?)
}

/// The manifest text for `inputs`: one `F` row per source file with a hash of
/// the text that was compiled, then one `D` row per directory those files
/// came from.
fn rows_for(inputs: &[Input]) -> std::io::Result<String> {
    let mut rows = String::from("zeo-progcache 1\n");
    let mut dirs: Vec<PathBuf> = Vec::new();
    for input in inputs {
        let path = Path::new(&input.name);
        // Synthetic names -- `-e`, `(eval at f.rb:3)`, a spliced shim -- are
        // not files, so there is nothing to re-check. Their text is part of
        // the KEY or of a file that is checked.
        if !path.is_file() {
            continue;
        }
        if input.name.contains('\n') {
            // The manifest is line-based. A path with a newline is legal on
            // unix and absurd; refuse to cache rather than write a file the
            // reader would misparse.
            return Err(std::io::Error::other("a source path contains a newline"));
        }
        rows.push_str(&format!("F {:016x} {}\n", hash(input.source.as_bytes()), input.name));
        if let Some(parent) = path.parent()
            && !dirs.contains(&parent.to_path_buf())
        {
            dirs.push(parent.to_path_buf());
        }
    }
    for dir in dirs {
        let Some(stamp) = dir_stamp(&dir) else {
            continue;
        };
        rows.push_str(&format!("D {stamp} {}\n", dir.display()));
    }
    Ok(rows)
}

/// Does every row still hold?
fn verify(manifest: &str) -> bool {
    let mut lines = manifest.lines();
    if lines.next() != Some("zeo-progcache 1") {
        return false;
    }
    for line in lines {
        let Some((kind, rest)) = line.split_once(' ') else {
            return false;
        };
        let Some((stamp, path)) = rest.split_once(' ') else {
            return false;
        };
        match kind {
            "F" => {
                let Ok(bytes) = std::fs::read(path) else {
                    return false;
                };
                if format!("{:016x}", hash(&bytes)) != stamp {
                    return false;
                }
            }
            "D" => {
                if dir_stamp(Path::new(path)).map(|s| s.to_string()).as_deref() != Some(stamp) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// A directory's modification time in nanoseconds since the epoch, which
/// changes when an entry is added or removed. `None` for a directory that is
/// gone -- which a caller reads as "cannot vouch for this".
fn dir_stamp(dir: &Path) -> Option<u128> {
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_nanos())
}

use crate::package::fnv64 as hash;

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zeo-progcache-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch dir");
        dir
    }

    fn input(path: &Path, text: &str) -> Input {
        std::fs::write(path, text).expect("write the input");
        Input {
            name: path.to_string_lossy().into_owned(),
            source: text.into(),
        }
    }

    /// The manifest a compile writes vouches for the files it read.
    #[test]
    fn an_unchanged_input_verifies() {
        let dir = scratch("unchanged");
        let f = dir.join("a.rb");
        let manifest = manifest_for(&[input(&f, "p 1\n")]);
        assert!(verify(&manifest), "{manifest}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An EDITED source is the case the whole cache turns on. A hit here
    /// would run the previous version of the user's program.
    #[test]
    fn an_edited_input_fails_verification() {
        let dir = scratch("edited");
        let f = dir.join("a.rb");
        let manifest = manifest_for(&[input(&f, "p 1\n")]);
        std::fs::write(&f, "p 2\n").expect("edit the input");
        assert!(!verify(&manifest));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A deleted source cannot be vouched for either.
    #[test]
    fn a_deleted_input_fails_verification() {
        let dir = scratch("deleted");
        let f = dir.join("a.rb");
        let manifest = manifest_for(&[input(&f, "p 1\n")]);
        std::fs::remove_file(&f).expect("remove the input");
        assert!(!verify(&manifest));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file ADDED beside one the compile read changes the directory, and a
    /// new file is how a `require` silently resolves somewhere else.
    #[test]
    fn a_new_file_in_a_read_directory_fails_verification() {
        let dir = scratch("added");
        let f = dir.join("a.rb");
        let manifest = manifest_for(&[input(&f, "p 1\n")]);
        std::fs::write(dir.join("b.rb"), "p 2\n").expect("add a sibling");
        assert!(!verify(&manifest));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A name that is not a file on disk -- `-e`, an eval's label -- carries
    /// no row, because there is nothing to re-read.
    #[test]
    fn a_synthetic_name_carries_no_row() {
        let manifest = manifest_for(&[Input {
            name: "-e".to_string(),
            source: "p 1\n".into(),
        }]);
        assert_eq!(manifest, "zeo-progcache 1\n");
        assert!(verify(&manifest));
    }

    #[test]
    fn a_manifest_zeo_did_not_write_is_refused() {
        assert!(!verify(""));
        assert!(!verify("zeo-progcache 2\n"));
        assert!(!verify("zeo-progcache 1\nX 0 /tmp\n"));
    }

    /// The writer under test, without `commit`'s write into the real cache
    /// directory.
    fn manifest_for(inputs: &[Input]) -> String {
        rows_for(inputs).expect("the rows build")
    }
}
