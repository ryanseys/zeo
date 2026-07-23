//! `cargo run -p xtask -- gem <add|sync|update>`: vendor pure-Ruby stdlib gems
//! into `gems/` from their upstream git repos.
//!
//! The model is **vendor-on-fetch**: a git URL + tag is the *source*, the
//! committed `gems/<name>/` (a stub gemspec + the verbatim `lib/`) is the
//! *storage* the compiler reads. A fresh `git clone` of zeo builds offline with
//! zero extra steps; only adding/updating a vendored gem runs the (network)
//! `git` fetch. **Vendoring needs only `git`** -- ruby/bundler are NOT required.
//!
//! The manifest is `gems.toml` at the repo root (this tool owns it; zeo's own
//! curated stdlib, NOT a user's app dependency graph -- so a plain TOML file,
//! not a Gemfile we'd never feed to bundler). Each git-sourced gem is:
//!
//! ```toml
//! [gems.fileutils]
//! github = "ruby/fileutils"
//! tag    = "v1.8.0"
//! rev    = "<full-sha>"   # resolved + written by `gem sync`; the immutable pin
//! ```
//!
//! `rev` is trusted as-is once written (the user chose "trust the tag" -- no
//! cross-check against the local install; `--check-oracle` is an optional extra).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Parsed CLI arguments for a `gem` subcommand.
struct ParsedArgs {
    positional: Vec<String>,
    /// The value of `--tag`/`--ref`/`--branch` (all resolve to a git ref).
    tag: Option<String>,
    /// Bare `--flag`s, e.g. `--check`, `--check-oracle`.
    flags: Vec<String>,
}

/// One git-sourced gem in `gems.toml`.
struct GemEntry {
    name: String,
    github: String,
    tag: String,
    /// The resolved commit SHA -- the reproducible pin. `None` until first sync.
    rev: Option<String>,
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let result = match args.first().map(String::as_str) {
        Some("add") => cmd_add(root, &args[1..]),
        Some("sync") => cmd_sync(root, &args[1..]),
        Some("update") => cmd_update(root, &args[1..]),
        _ => Err(
            "usage: cargo run -p xtask -- gem <add <owner/repo> [--tag <t>] | \
             sync [<name>] [--check] [--check-oracle] | update <name> [--tag <t>]>"
                .to_string(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask gem: {e}");
            ExitCode::FAILURE
        }
    }
}

// --- subcommands ------------------------------------------------------------

/// `gem add <owner/repo> --tag <t>`: append a manifest block (from CLI args --
/// never parsing Ruby) and vendor it.
fn cmd_add(root: &Path, args: &[String]) -> Result<(), String> {
    let parsed = parse_args(args)?;
    let owner_repo = parsed
        .positional
        .first()
        .ok_or("gem add needs <owner/repo>, e.g. ruby/fileutils")?;
    let tag = parsed
        .tag
        .ok_or("gem add needs --tag <tag>, e.g. --tag v1.8.0")?;
    let name = owner_repo
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("cannot derive a gem name from {owner_repo:?}"))?
        .to_string();

    let manifest_path = root.join("gems.toml");
    let mut entries = read_manifest(&manifest_path)?;
    if entries.iter().any(|e| e.name == name) {
        return Err(format!(
            "{name} is already in gems.toml -- use `gem update {name} --tag <t>` to change it"
        ));
    }
    entries.push(GemEntry {
        name: name.clone(),
        github: owner_repo.clone(),
        tag,
        rev: None,
    });
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    write_manifest(&manifest_path, &entries)?;
    println!("added {name} to gems.toml");
    sync_entries(root, &manifest_path, &mut entries, Some(&name), Mode::Write)
}

/// `gem sync [<name>] [--check]`: vendor every gem (or one) from its pinned
/// `rev` (resolving the tag on first sync). `--check` verifies the committed
/// `lib/` matches upstream without writing.
fn cmd_sync(root: &Path, args: &[String]) -> Result<(), String> {
    let parsed = parse_args(args)?;
    let name = parsed.positional.first().map(String::as_str);
    let mode = if parsed.flags.iter().any(|f| f == "--check") {
        Mode::Check
    } else {
        Mode::Write
    };
    let check_oracle = parsed.flags.iter().any(|f| f == "--check-oracle");

    let manifest_path = root.join("gems.toml");
    let mut entries = read_manifest(&manifest_path)?;
    sync_entries(root, &manifest_path, &mut entries, name, mode)?;
    if check_oracle {
        for entry in entries.iter().filter(|e| name.is_none_or(|n| n == e.name)) {
            check_against_oracle(root, entry)?;
        }
    }
    Ok(())
}

/// `gem update <name> [--tag <t>]`: bump the tag (optional) and re-resolve the
/// `rev`, then re-vendor.
fn cmd_update(root: &Path, args: &[String]) -> Result<(), String> {
    let parsed = parse_args(args)?;
    let name = parsed
        .positional
        .first()
        .ok_or("gem update needs <name>")?
        .clone();

    let manifest_path = root.join("gems.toml");
    let mut entries = read_manifest(&manifest_path)?;
    let entry = entries
        .iter_mut()
        .find(|e| e.name == name)
        .ok_or_else(|| format!("{name} is not in gems.toml"))?;
    if let Some(tag) = parsed.tag {
        entry.tag = tag;
    }
    entry.rev = None; // force re-resolution of the (possibly new) tag
    sync_entries(root, &manifest_path, &mut entries, Some(&name), Mode::Write)
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Materialize into `gems/<name>/`, persisting resolved revs to gems.toml.
    Write,
    /// Materialize into a temp dir and diff `lib/` against the committed copy.
    Check,
}

/// The core loop: fetch + vendor each selected gem. In `Write` mode, resolved
/// revs are written back to `gems.toml`.
fn sync_entries(
    root: &Path,
    manifest_path: &Path,
    entries: &mut [GemEntry],
    only: Option<&str>,
    mode: Mode,
) -> Result<(), String> {
    let mut manifest_dirty = false;
    let mut drift = Vec::new();
    for entry in entries.iter_mut() {
        if only.is_some_and(|n| n != entry.name) {
            continue;
        }
        let url = format!("https://github.com/{}", entry.github);
        // Resolve the tag to an immutable commit SHA once; the pin is reused
        // on every later sync so the vendor is reproducible.
        let rev = match &entry.rev {
            Some(rev) => rev.clone(),
            None => {
                let rev = resolve_tag(&url, &entry.tag)?;
                entry.rev = Some(rev.clone());
                manifest_dirty = true;
                rev
            }
        };
        let checkout = fetch_checkout(&entry.name, &url, &entry.tag, &rev)?;
        match mode {
            Mode::Write => {
                vendor(&checkout, &root.join("gems").join(&entry.name), entry)?;
                println!("vendored {} @ {} ({})", entry.name, entry.tag, short(&rev));
            }
            Mode::Check => {
                let tmp = std::env::temp_dir().join(format!("zeo-gemcheck-{}", entry.name));
                let _ = std::fs::remove_dir_all(&tmp);
                vendor(&checkout, &tmp, entry)?;
                let committed = root.join("gems").join(&entry.name).join("lib");
                if !dirs_equal(&tmp.join("lib"), &committed) {
                    drift.push(entry.name.clone());
                } else {
                    println!("ok {} @ {} ({})", entry.name, entry.tag, short(&rev));
                }
                let _ = std::fs::remove_dir_all(&tmp);
            }
        }
    }
    if manifest_dirty && mode == Mode::Write {
        write_manifest(manifest_path, entries)?;
    }
    if !drift.is_empty() {
        return Err(format!(
            "vendored lib/ drifted from upstream for: {} \
             (run `cargo run -p xtask -- gem sync` to re-vendor)",
            drift.join(", ")
        ));
    }
    Ok(())
}

// --- git --------------------------------------------------------------------

/// Resolve `refs/tags/<tag>` on `url` to a commit SHA via `git ls-remote`,
/// preferring the peeled (`^{}`) object so annotated tags yield the commit,
/// not the tag object.
fn resolve_tag(url: &str, tag: &str) -> Result<String, String> {
    let out = git(
        None,
        &[
            "ls-remote",
            url,
            &format!("refs/tags/{tag}^{{}}"),
            &format!("refs/tags/{tag}"),
        ],
    )?;
    let mut peeled = None;
    let mut direct = None;
    for line in out.lines() {
        let (sha, refname) = line.split_once('\t').unwrap_or(("", ""));
        if refname.ends_with("^{}") {
            peeled = Some(sha.to_string());
        } else if !refname.is_empty() {
            direct = Some(sha.to_string());
        }
    }
    peeled
        .or(direct)
        .ok_or_else(|| format!("tag {tag} not found in {url}"))
}

/// Fetch `<rev>` (via its tag) into a SHA-stamped cache dir and return the
/// checkout path. A stamped cache hit short-circuits the network.
fn fetch_checkout(name: &str, url: &str, tag: &str, rev: &str) -> Result<PathBuf, String> {
    let cache = cache_dir().join(format!("{name}-{rev}"));
    let stamp = cache.join(".zeo-vendor-stamp");
    if stamp.is_file() {
        return Ok(cache);
    }
    let _ = std::fs::remove_dir_all(&cache);
    std::fs::create_dir_all(&cache).map_err(|e| format!("creating {}: {e}", cache.display()))?;
    git(Some(&cache), &["init", "--quiet"])?;
    git(
        Some(&cache),
        &["fetch", "--depth", "1", url, &format!("refs/tags/{tag}")],
    )?;
    git(Some(&cache), &["checkout", "--quiet", "--detach", "FETCH_HEAD"])?;
    let head = git(Some(&cache), &["rev-parse", "HEAD"])?;
    let head = head.trim();
    if head != rev {
        return Err(format!(
            "{name}: tag {tag} resolved to {rev} but the fetched commit is {head} \
             (upstream tag moved -- run `gem update {name}`)"
        ));
    }
    // Strip the git metadata: the cache holds a plain, read-only source tree.
    let _ = std::fs::remove_dir_all(cache.join(".git"));
    std::fs::write(&stamp, rev).map_err(|e| format!("writing stamp: {e}"))?;
    Ok(cache)
}

fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    let out = cmd
        .args(args)
        .output()
        .map_err(|e| format!("running git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("zeo").join("gems")
}

// --- vendoring --------------------------------------------------------------

/// Copy the checkout's `lib/` (+ any license files) into `dest` and write a
/// stub gemspec. Only `lib/` is upstream source the drift check guards; the
/// gemspec is deterministic tool output.
fn vendor(checkout: &Path, dest: &Path, entry: &GemEntry) -> Result<(), String> {
    let src_lib = checkout.join("lib");
    if !src_lib.is_dir() {
        return Err(format!(
            "{}: upstream has no lib/ directory at {}",
            entry.name,
            src_lib.display()
        ));
    }
    let dest_lib = dest.join("lib");
    let _ = std::fs::remove_dir_all(&dest_lib);
    std::fs::create_dir_all(dest).map_err(|e| format!("creating {}: {e}", dest.display()))?;
    copy_tree(&src_lib, &dest_lib)?;

    // Carry any license the gem ships (kept per gems/UPSTREAM.md policy).
    for lic in ["COPYING", "BSDL", "LICENSE", "LICENSE.txt", "LICENSE.md", "MIT-LICENSE"] {
        let from = checkout.join(lic);
        if from.is_file() {
            std::fs::copy(&from, dest.join(lic))
                .map_err(|e| format!("copying {lic}: {e}"))?;
        }
    }

    let version = entry.tag.strip_prefix('v').unwrap_or(&entry.tag);
    let rev = entry.rev.as_deref().unwrap_or("");
    let gemspec = format!(
        "# Vendored from https://github.com/{} @ {} ({}); managed by `xtask gem`.\n\
         # Do not edit by hand -- see gems.toml.\n\
         Gem::Specification.new do |s|\n  \
         s.name = {:?}\n  \
         s.version = {:?}\n  \
         s.require_paths = [\"lib\"]\n\
         end\n",
        entry.github, entry.tag, rev, entry.name, version,
    );
    std::fs::write(dest.join(format!("{}.gemspec", entry.name)), gemspec)
        .map_err(|e| format!("writing gemspec: {e}"))?;
    Ok(())
}

/// Optional oracle cross-check: the vendored `lib/` should byte-match the copy
/// the local ruby install ships (what the conformance oracle actually runs).
fn check_against_oracle(root: &Path, entry: &GemEntry) -> Result<(), String> {
    let version = entry.tag.strip_prefix('v').unwrap_or(&entry.tag);
    let gemdir = git_free_command("gem", &["env", "gemdir"])?;
    let installed = PathBuf::from(gemdir.trim())
        .join("gems")
        .join(format!("{}-{version}", entry.name))
        .join("lib");
    if !installed.is_dir() {
        println!("skip oracle-check {}: not installed at {}", entry.name, installed.display());
        return Ok(());
    }
    let vendored = root.join("gems").join(&entry.name).join("lib");
    if dirs_equal(&vendored, &installed) {
        println!("oracle-ok {} @ {}", entry.name, version);
        Ok(())
    } else {
        Err(format!(
            "{} @ {}: vendored lib/ differs from the ruby install ({})",
            entry.name,
            version,
            installed.display()
        ))
    }
}

fn git_free_command(bin: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("running {bin}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{bin} {} failed", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// --- fs helpers -------------------------------------------------------------

fn copy_tree(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("creating {}: {e}", dest.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("reading {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("reading dir entry: {e}"))?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("copying {} -> {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Recursive byte-for-byte directory comparison (same file set, same contents).
fn dirs_equal(a: &Path, b: &Path) -> bool {
    let mut a_files = list_files(a);
    let mut b_files = list_files(b);
    a_files.sort();
    b_files.sort();
    if a_files != b_files {
        return false;
    }
    a_files.iter().all(|rel| {
        std::fs::read(a.join(rel)).ok() == std::fs::read(b.join(rel)).ok()
    })
}

/// Relative paths of every file under `dir` (empty if `dir` is missing).
fn list_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

// --- gems.toml (a tiny reader/writer; the schema is fixed and we own it) -----

fn read_manifest(path: &Path) -> Result<Vec<GemEntry>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    let mut entries: Vec<GemEntry> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix("[gems.").and_then(|s| s.strip_suffix(']')) {
            entries.push(GemEntry {
                name: name.to_string(),
                github: String::new(),
                tag: String::new(),
                rev: None,
            });
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("malformed gems.toml line: {raw:?}"))?;
        let value = value.trim().trim_matches('"').to_string();
        let entry = entries
            .last_mut()
            .ok_or_else(|| format!("gems.toml key outside any [gems.<name>] block: {raw:?}"))?;
        match key.trim() {
            "github" => entry.github = value,
            "tag" => entry.tag = value,
            "rev" => entry.rev = Some(value).filter(|v| !v.is_empty()),
            other => return Err(format!("unknown gems.toml key {other:?}")),
        }
    }
    Ok(entries)
}

fn write_manifest(path: &Path, entries: &[GemEntry]) -> Result<(), String> {
    let mut out = String::from(
        "# zeo's vendored batteries: git-sourced pure-Ruby stdlib gems.\n\
         # Managed by `cargo run -p xtask -- gem`. `rev` is the reproducible pin.\n",
    );
    for entry in entries {
        out.push_str(&format!("\n[gems.{}]\n", entry.name));
        out.push_str(&format!("github = {:?}\n", entry.github));
        out.push_str(&format!("tag    = {:?}\n", entry.tag));
        if let Some(rev) = &entry.rev {
            out.push_str(&format!("rev    = {rev:?}\n"));
        }
    }
    std::fs::write(path, out).map_err(|e| format!("writing {}: {e}", path.display()))
}

// --- arg parsing (positional + --tag/--ref/--branch + bare flags) -----------

fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    let mut parsed = ParsedArgs {
        positional: Vec::new(),
        tag: None,
        flags: Vec::new(),
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--tag" | "--ref" | "--branch" => {
                parsed.tag = Some(
                    iter.next()
                        .ok_or_else(|| format!("{arg} needs a value"))?
                        .clone(),
                );
            }
            flag if flag.starts_with("--") => parsed.flags.push(flag.to_string()),
            other => parsed.positional.push(other.to_string()),
        }
    }
    Ok(parsed)
}

fn short(rev: &str) -> &str {
    &rev[..rev.len().min(12)]
}
