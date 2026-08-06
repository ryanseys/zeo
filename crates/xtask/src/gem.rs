//! `cargo run -p xtask -- gem <add|sync|update|outdated>`: vendor pure-Ruby
//! stdlib gems into `gems/` from their upstream git repos.
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
//! A repo that ships more than one gem (`rubygems/rubygems` carries bundler
//! under `bundler/`) gets one block per gem, distinguished by `subdir`.
//!
//! `rev` is trusted as-is once written (the user chose "trust the tag" -- no
//! cross-check against the local install; `--check-oracle` is an optional extra).
//!
//! Nothing here discovers a new version on its own: `update` re-resolves the
//! tag it is given. `outdated` is the discovery half -- it prints each pin
//! beside the version the ORACLE ruby installs and the newest upstream tag, so
//! a bump is a deliberate step. The target is the oracle's version, not the
//! newest one: vendoring ahead of the ruby every golden is blessed against
//! would manufacture divergences that are not bugs.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Parsed CLI arguments for a `gem` subcommand.
#[derive(Debug)]
struct ParsedArgs {
    positional: Vec<String>,
    /// The value of `--tag`/`--ref`/`--branch` (all resolve to a git ref).
    tag: Option<String>,
    /// `--name`: the vendored gem's name, when it isn't the repo's own.
    name: Option<String>,
    /// `--subdir`: see [`GemEntry::subdir`].
    subdir: Option<String>,
    /// Bare `--flag`s, e.g. `--check`, `--check-oracle`.
    flags: Vec<String>,
}

/// One git-sourced gem in `gems.toml`.
#[derive(Debug)]
struct GemEntry {
    name: String,
    github: String,
    tag: String,
    /// The resolved commit SHA -- the reproducible pin. `None` until first sync.
    rev: Option<String>,
    /// The directory INSIDE the checkout holding the gem's `lib/`, for a repo
    /// that ships more than one gem (`rubygems/rubygems` carries bundler under
    /// `bundler/`). `None` means the repo root, which is the usual shape.
    subdir: Option<String>,
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let result = match args.first().map(String::as_str) {
        Some("add") => cmd_add(root, &args[1..]),
        Some("sync") => cmd_sync(root, &args[1..]),
        Some("update") => cmd_update(root, &args[1..]),
        Some("outdated") => cmd_outdated(root, &args[1..]),
        _ => Err("usage: cargo run -p xtask -- gem \
             <add <owner/repo> [--tag <t>] [--name <n>] [--subdir <d>] | \
             sync [<name>] [--check] [--check-oracle] | update <name> [--tag <t>] | \
             outdated>"
            .to_string()),
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

/// `gem add <owner/repo> --tag <t> [--name <n>] [--subdir <d>]`: append a
/// manifest block (from CLI args -- never parsing Ruby) and vendor it.
fn cmd_add(root: &Path, args: &[String]) -> Result<(), String> {
    let parsed = parse_args(args)?;
    let owner_repo = parsed
        .positional
        .first()
        .ok_or("gem add needs <owner/repo>, e.g. ruby/fileutils")?;
    let tag = parsed
        .tag
        .ok_or("gem add needs --tag <tag>, e.g. --tag v1.8.0")?;
    // The repo's own name, unless a repo shipping several gems named one.
    let name = match parsed.name {
        Some(name) => name,
        None => owner_repo
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("cannot derive a gem name from {owner_repo:?}"))?
            .to_string(),
    };

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
        subdir: parsed.subdir,
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

/// `gem outdated`: one row per manifest entry -- the current pin, the version
/// the ORACLE RUBY installs, and the newest upstream tag.
///
/// The oracle column is the one that matters. zeo's conformance target is the
/// ruby in `mise.toml`, and every golden is blessed by running it, so a gem
/// vendored AHEAD of what that ruby ships manufactures divergences that are
/// not bugs. The upstream column is informational: it says how far the
/// ecosystem has moved past the oracle.
///
/// Read-only -- it never writes `gems.toml`. Feed its `oracle` column back in
/// via `gem update <name> --tag v<version>`.
fn cmd_outdated(root: &Path, args: &[String]) -> Result<(), String> {
    let parsed = parse_args(args)?;
    let only = parsed.positional.first().map(String::as_str);
    let entries = read_manifest(&root.join("gems.toml"))?;
    let installed = installed_gem_versions();

    println!(
        "{:<14} {:<12} {:<12} {:<12} {}",
        "gem", "pinned", "oracle", "upstream", "action"
    );
    let mut behind = Vec::new();
    for entry in entries.iter().filter(|e| only.is_none_or(|n| n == e.name)) {
        let pinned = entry.tag.strip_prefix('v').unwrap_or(&entry.tag).to_string();
        let oracle = installed.get(&entry.name).cloned();
        let upstream = newest_tag(&format!("https://github.com/{}", entry.github));
        let action = match &oracle {
            Some(v) if *v != pinned => {
                behind.push((entry.name.clone(), v.clone()));
                format!("gem update {} --tag v{v}", entry.name)
            }
            Some(_) => "-".to_string(),
            // A gem the oracle does not ship (bundler/rubygems live outside
            // the gem store's versioned layout) -- nothing to match against.
            None => "(not in the oracle install)".to_string(),
        };
        println!(
            "{:<14} {:<12} {:<12} {:<12} {}",
            entry.name,
            pinned,
            oracle.as_deref().unwrap_or("?"),
            upstream.as_deref().unwrap_or("?"),
            action
        );
    }
    if behind.is_empty() {
        println!("\nEvery pinned gem matches the oracle install.");
    } else {
        println!("\n{} gem(s) differ from the oracle:", behind.len());
        for (name, version) in &behind {
            println!("  cargo run -p xtask -- gem update {name} --tag v{version}");
        }
    }
    Ok(())
}

/// Every gem version the oracle ruby has installed, by name, from
/// `<gemdir>/gems/<name>-<version>/`. Same `mise which gem` resolution
/// [`check_against_oracle`] uses, for the same reason. Highest version wins
/// when several are installed side by side.
fn installed_gem_versions() -> std::collections::HashMap<String, String> {
    let mut found: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let gemdir = match git_free_command("mise", &["which", "gem"]) {
        Ok(path) if !path.trim().is_empty() => git_free_command(path.trim(), &["env", "gemdir"]),
        _ => git_free_command("gem", &["env", "gemdir"]),
    };
    let Ok(gemdir) = gemdir else {
        return found;
    };
    let Ok(entries) = std::fs::read_dir(PathBuf::from(gemdir.trim()).join("gems")) else {
        return found;
    };
    for entry in entries.flatten() {
        let dir = entry.file_name();
        let Some(dir) = dir.to_str() else { continue };
        // `<name>-<version>`, where the name itself may contain dashes
        // (`net-http-0.9.1`): split at the LAST dash that starts a digit.
        let Some(split) = dir
            .rmatch_indices('-')
            .find(|(i, _)| dir[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
            .map(|(i, _)| i)
        else {
            continue;
        };
        let (name, version) = (&dir[..split], &dir[split + 1..]);
        found
            .entry(name.to_string())
            .and_modify(|existing| {
                if version_lt(existing, version) {
                    *existing = version.to_string();
                }
            })
            .or_insert_with(|| version.to_string());
    }
    found
}

/// The newest `vX.Y.Z` tag on a remote, by numeric component order.
/// `None` when the remote is unreachable -- this column is informational, so
/// a network hiccup must not fail the command.
fn newest_tag(url: &str) -> Option<String> {
    let out = git(None, &["ls-remote", "--tags", "--refs", url]).ok()?;
    let mut newest: Option<String> = None;
    for line in out.lines() {
        let Some((_, refname)) = line.split_once('\t') else {
            continue;
        };
        let tag = refname.trim_start_matches("refs/tags/");
        let version = tag.strip_prefix('v').unwrap_or(tag);
        // Releases only: skip `1.2.3.pre1`, `v1.2.3-rc`, and similar.
        if version.is_empty()
            || !version
                .split('.')
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        if newest.as_deref().is_none_or(|n| version_lt(n, version)) {
            newest = Some(version.to_string());
        }
    }
    newest
}

/// `a < b` comparing dot-separated numeric components, shorter-is-lower on a
/// common prefix (`1.2` < `1.2.1`). Non-numeric components sort as 0, which is
/// fine because callers filter to all-numeric versions first.
fn version_lt(a: &str, b: &str) -> bool {
    let num = |s: &str| -> Vec<u64> { s.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    let (a, b) = (num(a), num(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x < y;
        }
    }
    false
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
    git(
        Some(&cache),
        &["checkout", "--quiet", "--detach", "FETCH_HEAD"],
    )?;
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
    let src_root = match &entry.subdir {
        Some(sub) => checkout.join(sub),
        None => checkout.to_path_buf(),
    };
    let src_lib = src_root.join("lib");
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
    for lic in [
        "COPYING",
        "BSDL",
        "LICENSE",
        "LICENSE.txt",
        "LICENSE.md",
        "MIT-LICENSE",
    ] {
        // A sub-gem carries its own license when it has one, else the repo's.
        let from = [src_root.join(lic), checkout.join(lic)]
            .into_iter()
            .find(|p| p.is_file())
            .unwrap_or_else(|| checkout.join(lic));
        if from.is_file() {
            std::fs::copy(&from, dest.join(lic)).map_err(|e| format!("copying {lic}: {e}"))?;
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

/// Optional oracle cross-check: how the vendored `lib/` compares to the copy the
/// `mise.toml`-pinned ruby ships (what the conformance oracle actually runs).
///
/// INFORMATIONAL, not a gate. A gem's upstream repo and ruby-core's bundled copy
/// of it are not the same tree: ruby-core patches several default gems in place
/// between releases, so `fileutils` 1.8.0 as installed is not byte-identical to
/// `ruby/fileutils` at v1.8.0. What the printed file list is good for is
/// spotting a vendored copy that has fallen a whole VERSION behind. The real
/// gate is `--check`, which verifies the vendored tree still matches its pinned
/// rev.
fn check_against_oracle(root: &Path, entry: &GemEntry) -> Result<(), String> {
    let version = entry.tag.strip_prefix('v').unwrap_or(&entry.tag);
    // The `mise.toml`-pinned ruby, not whatever `gem` is on PATH -- the point of
    // this check is to match what the CONFORMANCE ORACLE runs, and a bare `gem`
    // resolves to the system install (`mise which ruby` is the same resolution
    // the golden harness uses).
    let gemdir = match git_free_command("mise", &["which", "gem"]) {
        Ok(path) if !path.trim().is_empty() => git_free_command(path.trim(), &["env", "gemdir"])?,
        _ => git_free_command("gem", &["env", "gemdir"])?,
    };
    let vendored = root.join("gems").join(&entry.name).join("lib");
    let gems_root = PathBuf::from(gemdir.trim())
        .join("gems")
        .join(format!("{}-{version}", entry.name))
        .join("lib");
    // A DEFAULT gem isn't unpacked under `gems/` at all -- its files sit
    // directly in ruby's own stdlib dir, so that is the second place to look.
    // (Reported extras are suppressed there: "everything else in the stdlib"
    // isn't a useful diff.)
    let (installed, report_extras) = if gems_root.is_dir() {
        (gems_root, true)
    } else {
        let libdir = ruby_stdlib_dir();
        // "Any vendored file lands there" is enough to say this IS the gem's
        // install -- requiring all of them would skip a gem that ships a file
        // ruby-core drops (open3's `jruby_windows.rb`), which is precisely the
        // sort of thing worth reporting rather than hiding.
        match libdir.filter(|d| {
            list_files(&vendored)
                .iter()
                .any(|rel| d.join(rel).is_file())
        }) {
            Some(d) => (d, false),
            None => {
                println!(
                    "skip oracle-check {}: not installed at {}",
                    entry.name,
                    gems_root.display()
                );
                return Ok(());
            }
        }
    };
    match vendored_matches_install(&vendored, &installed) {
        OracleDiff::Same => {
            println!("oracle-ok {} @ {}", entry.name, version);
            Ok(())
        }
        // The install may ship files the upstream repo doesn't -- racc's
        // `parser-text.rb` is generated by its build step, not checked in. That
        // is not drift: every file zeo vendored is byte-identical, and the
        // extras are named so a genuinely new one gets noticed.
        OracleDiff::ExtraInInstall(_) if !report_extras => {
            println!("oracle-ok {} @ {} (default gem)", entry.name, version);
            Ok(())
        }
        OracleDiff::ExtraInInstall(extra) => {
            println!(
                "oracle-ok {} @ {} (install also ships: {})",
                entry.name,
                version,
                extra
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            Ok(())
        }
        OracleDiff::Differs(files) => {
            println!(
                "oracle-differs {} @ {}: {} (vs {})",
                entry.name,
                version,
                files
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                installed.display()
            );
            Ok(())
        }
    }
}

/// Ruby's own stdlib directory (`RbConfig::CONFIG["rubylibdir"]`) on the
/// `mise.toml`-pinned install -- where DEFAULT gems live, unpacked into the
/// same tree rather than under `gems/`.
fn ruby_stdlib_dir() -> Option<PathBuf> {
    let ruby = git_free_command("mise", &["which", "ruby"])
        .ok()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "ruby".to_string());
    let out = git_free_command(
        &ruby,
        &["-rrbconfig", "-e", "print RbConfig::CONFIG['rubylibdir']"],
    )
    .ok()?;
    let dir = PathBuf::from(out.trim());
    dir.is_dir().then_some(dir)
}

enum OracleDiff {
    Same,
    /// Every vendored file matches, but the install carries these extras.
    ExtraInInstall(Vec<PathBuf>),
    /// These vendored files are absent from, or differ from, the install.
    Differs(Vec<PathBuf>),
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
    a_files
        .iter()
        .all(|rel| std::fs::read(a.join(rel)).ok() == std::fs::read(b.join(rel)).ok())
}

/// How the vendored tree compares to the installed one: every vendored file
/// must be present and byte-identical, and anything EXTRA in the install is
/// reported rather than failed (see `check_against_oracle`).
fn vendored_matches_install(vendored: &Path, installed: &Path) -> OracleDiff {
    let mut ours = list_files(vendored);
    let mut theirs = list_files(installed);
    ours.sort();
    theirs.sort();
    let differing: Vec<PathBuf> = ours
        .iter()
        .filter(|rel| {
            std::fs::read(vendored.join(rel)).ok() != std::fs::read(installed.join(rel)).ok()
        })
        .cloned()
        .collect();
    if !differing.is_empty() {
        return OracleDiff::Differs(differing);
    }
    let extra: Vec<PathBuf> = theirs.into_iter().filter(|p| !ours.contains(p)).collect();
    match extra.is_empty() {
        true => OracleDiff::Same,
        false => OracleDiff::ExtraInInstall(extra),
    }
}

/// Relative paths of every file under `dir` (empty if `dir` is missing).
fn list_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
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
        if let Some(name) = line
            .strip_prefix("[gems.")
            .and_then(|s| s.strip_suffix(']'))
        {
            entries.push(GemEntry {
                name: name.to_string(),
                github: String::new(),
                tag: String::new(),
                rev: None,
                subdir: None,
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
            "subdir" => entry.subdir = Some(value).filter(|v| !v.is_empty()),
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
        if let Some(subdir) = &entry.subdir {
            out.push_str(&format!("subdir = {subdir:?}\n"));
        }
    }
    std::fs::write(path, out).map_err(|e| format!("writing {}: {e}", path.display()))
}

// --- arg parsing (positional + --tag/--ref/--branch + bare flags) -----------

fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    let mut parsed = ParsedArgs {
        positional: Vec::new(),
        tag: None,
        name: None,
        subdir: None,
        flags: Vec::new(),
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || {
            iter.next()
                .ok_or_else(|| format!("{arg} needs a value"))
                .cloned()
        };
        match arg.as_str() {
            "--tag" | "--ref" | "--branch" => parsed.tag = Some(value()?),
            "--name" => parsed.name = Some(value()?),
            "--subdir" => parsed.subdir = Some(value()?),
            flag if flag.starts_with("--") => parsed.flags.push(flag.to_string()),
            other => parsed.positional.push(other.to_string()),
        }
    }
    Ok(parsed)
}

fn short(rev: &str) -> &str {
    &rev[..rev.len().min(12)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_by_numeric_component_not_lexically() {
        assert!(version_lt("1.9.0", "1.10.0"), "10 > 9 numerically");
        assert!(version_lt("0.2.2", "0.3.0"));
        assert!(version_lt("1.2", "1.2.1"), "a prefix is lower");
        assert!(!version_lt("4.0.16", "4.0.16"));
        assert!(!version_lt("2.0.0", "1.99.99"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zeo-xtask-gem-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creating the scratch dir");
        dir
    }

    fn entry(name: &str, subdir: Option<&str>) -> GemEntry {
        GemEntry {
            name: name.to_string(),
            github: "rubygems/rubygems".to_string(),
            tag: "v4.0.16".to_string(),
            rev: Some("f".repeat(40)),
            subdir: subdir.map(str::to_string),
        }
    }

    /// The manifest is this tool's own format, read and written by hand -- so a
    /// key it can write but not read back is a silent data loss on the next
    /// `gem sync` (`subdir` vanishing re-vendors bundler as rubygems).
    #[test]
    fn every_written_key_reads_back() {
        let dir = scratch("roundtrip");
        let path = dir.join("gems.toml");
        let written = vec![entry("bundler", Some("bundler")), entry("rubygems", None)];
        write_manifest(&path, &written).expect("writes");
        let read = read_manifest(&path).expect("reads");

        assert_eq!(read.len(), 2);
        for (a, b) in written.iter().zip(&read) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.github, b.github);
            assert_eq!(a.tag, b.tag);
            assert_eq!(a.rev, b.rev);
            assert_eq!(a.subdir, b.subdir);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_manifest_is_an_empty_one_rather_than_an_error() {
        // `gem add` on a fresh tree has nothing to read yet.
        assert!(
            read_manifest(Path::new("/nonexistent/gems.toml"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_unknown_key_is_rejected_instead_of_ignored() {
        let dir = scratch("unknown-key");
        let path = dir.join("gems.toml");
        std::fs::write(&path, "[gems.x]\ngithub = \"a/b\"\nsubdirectory = \"c\"\n").unwrap();
        let err = read_manifest(&path).unwrap_err();
        assert!(err.contains("subdirectory"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `subdir` is the whole reason bundler can be vendored at all: one repo,
    /// two gems, two `lib/` trees. Vendoring the wrong one produces a plausible
    /// `gems/bundler/lib/` that is actually rubygems.
    #[test]
    fn subdir_selects_which_lib_tree_is_vendored() {
        let dir = scratch("subdir");
        let checkout = dir.join("checkout");
        std::fs::create_dir_all(checkout.join("lib")).unwrap();
        std::fs::write(checkout.join("lib/rubygems.rb"), "# root gem\n").unwrap();
        std::fs::create_dir_all(checkout.join("bundler/lib")).unwrap();
        std::fs::write(checkout.join("bundler/lib/bundler.rb"), "# sub gem\n").unwrap();
        // A license at the repo root and a more specific one in the sub-gem.
        std::fs::write(checkout.join("LICENSE.txt"), "root license\n").unwrap();
        std::fs::write(checkout.join("bundler/LICENSE.txt"), "sub license\n").unwrap();

        let dest = dir.join("gems/bundler");
        vendor(&checkout, &dest, &entry("bundler", Some("bundler"))).expect("vendors");
        assert!(dest.join("lib/bundler.rb").is_file());
        assert!(
            !dest.join("lib/rubygems.rb").exists(),
            "the repo root's lib/ must not be vendored for a subdir gem"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("LICENSE.txt")).unwrap(),
            "sub license\n",
            "a sub-gem's own license wins over the repo's"
        );
        let gemspec = std::fs::read_to_string(dest.join("bundler.gemspec")).unwrap();
        assert!(gemspec.contains("s.version = \"4.0.16\""), "{gemspec}");

        // ...and the same checkout with no `subdir` vendors the root tree.
        let dest = dir.join("gems/rubygems");
        vendor(&checkout, &dest, &entry("rubygems", None)).expect("vendors");
        assert!(dest.join("lib/rubygems.rb").is_file());
        assert!(!dest.join("lib/bundler.rb").exists());
        assert_eq!(
            std::fs::read_to_string(dest.join("LICENSE.txt")).unwrap(),
            "root license\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_lib_tree_is_a_named_error_not_an_empty_vendor() {
        let dir = scratch("no-lib");
        let checkout = dir.join("checkout");
        std::fs::create_dir_all(&checkout).unwrap();
        let err = vendor(
            &checkout,
            &dir.join("out"),
            &entry("bundler", Some("bundler")),
        )
        .unwrap_err();
        assert!(err.contains("bundler"), "{err}");
        assert!(err.contains("no lib/"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_takes_its_gem_name_and_subdir_from_the_command_line() {
        let parsed = parse_args(&[
            "rubygems/rubygems".into(),
            "--tag".into(),
            "v4.0.16".into(),
            "--name".into(),
            "bundler".into(),
            "--subdir".into(),
            "bundler".into(),
            "--check".into(),
        ])
        .expect("parses");
        assert_eq!(parsed.positional, ["rubygems/rubygems"]);
        assert_eq!(parsed.tag.as_deref(), Some("v4.0.16"));
        assert_eq!(parsed.name.as_deref(), Some("bundler"));
        assert_eq!(parsed.subdir.as_deref(), Some("bundler"));
        assert_eq!(parsed.flags, ["--check"]);
    }

    #[test]
    fn a_flag_missing_its_value_is_an_error() {
        let err = parse_args(&["repo".into(), "--tag".into()]).unwrap_err();
        assert!(err.contains("--tag"), "{err}");
    }
}
