//! `cargo xtask deps` -- the libraries zeo ships, fetched without Ruby.
//!
//! Two tiers come from the network, and neither needs a ruby on the machine:
//!
//! | Tier | Where | Source |
//! |---|---|---|
//! | bootstrap | `vendor/ruby/{rubygems,bundler}/` | the rubygems repo at the tag `rubygems.lock` pins |
//! | resolved | `vendor/gems/` | one `.gem` per `Gemfile.lock` row, cached in `vendor/cache/` |
//!
//! Ruby is needed for exactly one more thing, and `--oracle` is where it is
//! needed: `gem install --local` builds the native halves MRI wants and
//! writes the serialized gemspecs the oracle's `bundler/setup` reads. A
//! bless runs it; building, testing and packaging do not.
//!
//! Every download is verified against the lock's own `CHECKSUMS` row before
//! anything reads it, and a gem whose row is missing is refused rather than
//! trusted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zeo_gem::{Lockfile, Package, Store};

use crate::exec::{self, Capture};
use crate::{Error, root, root_join};

const HELP: &str = "\
usage: cargo xtask deps [--oracle] [--refresh]

Fetch the libraries `Gemfile.lock` names, and the rubygems/bundler pair
`crates/xtask/rubygems.lock` pins. Needs the network on the first run and
nothing but `curl` and `git`; a second run does nothing.

options:
  --oracle    also build the ruby oracle's own store (needs the pinned ruby)
  --refresh   re-unpack everything, even what is already in place
";

/// Where each tier lands, under the repo root.
const CACHE: &str = "vendor/cache";
const GEM_STORE: &str = "vendor/gems";
const BOOTSTRAP: &str = "vendor/ruby";
const ORACLE_STORE: &str = "vendor/bundle/ruby";
/// What the last successful run resolved. `checks::gem_versions` reads it.
const STAMP: &str = "vendor/gems/.zeo-deps-stamp";

pub fn run(args: &[String]) -> Result<(), Error> {
    let mut oracle = false;
    let mut refresh = false;
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--oracle" => oracle = true,
            "--refresh" => refresh = true,
            other => {
                return Err(Error::new(format!(
                    "deps: unknown option {other:?}\n\n{HELP}"
                )));
            }
        }
    }

    let lock = read_lock()?;
    let pin = RubygemsPin::read()?;
    pin.check_against(&lock)?;

    bootstrap(&pin, refresh)?;
    let cached = fetch_gems(&lock)?;
    unpack_gems(&cached, refresh)?;
    write_stamp(&pin)?;
    if oracle {
        install_for_oracle(&cached)?;
    }
    Ok(())
}

/// The repo's own lock, refusing one that cannot be verified.
fn read_lock() -> Result<Lockfile, Error> {
    let path = root_join("Gemfile.lock");
    let lock = Lockfile::parse_file(&path).map_err(|e| Error::new(e.message()))?;
    let unverifiable: Vec<String> = source_rows(&lock)
        .iter()
        .filter(|spec| {
            lock.checksum(&spec.full_name())
                .and_then(|c| c.sha256())
                .is_none()
        })
        .map(|spec| spec.full_name())
        .collect();
    if !unverifiable.is_empty() {
        return Err(Error::new(format!(
            "{} states no sha256 for {}. Re-resolve it with a bundler that \
             writes CHECKSUMS -- a download nothing can check is not one to unpack.",
            path.display(),
            unverifiable.join(", ")
        )));
    }
    Ok(lock)
}

/// The `ruby`-platform row of every `GEM` gem, name-sorted.
///
/// The source row, never a precompiled one: zeo compiles a gem's own
/// `ext/**/*.c` and can never load a binary built against MRI's ABI, and the
/// oracle's `gem install` builds the same source against its own ruby. A gem
/// with no source row at all would be a real refusal, and there is none.
fn source_rows(lock: &Lockfile) -> Vec<&zeo_gem::Spec> {
    let mut rows: BTreeMap<&str, &zeo_gem::Spec> = BTreeMap::new();
    for (source, spec) in lock.specs() {
        if source.kind != zeo_gem::lockfile::SectionKind::Gem || !spec.platform.is_ruby() {
            continue;
        }
        rows.insert(spec.name.as_str(), spec);
    }
    rows.into_values().collect()
}

/// `crates/xtask/rubygems.lock`: the bootstrap tier's one pin.
struct RubygemsPin {
    repo: String,
    tag: String,
    rev: String,
    version: String,
}

impl RubygemsPin {
    fn read() -> Result<RubygemsPin, Error> {
        let path = root_join("crates/xtask/rubygems.lock");
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
        let mut fields: BTreeMap<&str, &str> = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| Error::new(format!("{line:?} is not `key = value`")))?;
            fields.insert(key.trim(), value.trim());
        }
        let take = |key: &str| {
            fields
                .get(key)
                .map(|v| (*v).to_string())
                .ok_or_else(|| Error::new(format!("{}: names no {key}", path.display())))
        };
        Ok(RubygemsPin {
            repo: take("repo")?,
            tag: take("tag")?,
            rev: take("rev")?,
            version: take("version")?,
        })
    }

    /// One release ships rubygems and bundler both, and `Gemfile.lock` states
    /// it under `rubygems-update`. Two files naming two versions is the drift
    /// this check exists to stop.
    fn check_against(&self, lock: &Lockfile) -> Result<(), Error> {
        let locked = lock
            .specs()
            .map(|(_, spec)| spec)
            .find(|spec| spec.name == "rubygems-update")
            .ok_or_else(|| Error::new("Gemfile.lock names no `rubygems-update`"))?;
        if locked.version.as_str() != self.version {
            return Err(Error::new(format!(
                "Gemfile.lock says rubygems-update {}, crates/xtask/rubygems.lock says {} \
                 (tag {}). Bump the two together.",
                locked.version, self.version, self.tag
            )));
        }
        Ok(())
    }
}

/// Fetch the rubygems repo and lay its two trees out under `vendor/ruby/`.
///
/// `lib/` is the RubyGems tree and `bundler/{lib,exe}` is bundler, exactly
/// the split the two published gems have. Only what a `require` reaches is
/// copied: the repo's `bundler/spec/` alone is 32 MB of tests nothing here
/// loads. Each directory gets the stub gemspec that makes it a package zeo's
/// loader can resolve.
fn bootstrap(pin: &RubygemsPin, refresh: bool) -> Result<(), Error> {
    let dest = root_join(BOOTSTRAP);
    let stamp = dest.join(".zeo-rubygems-rev");
    if !refresh && std::fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == pin.rev) {
        return Ok(());
    }
    println!("deps: fetching rubygems {} ({})", pin.version, pin.tag);
    let checkout = crate::vendor::fetch_checkout(&crate::vendor::Pin {
        name: "rubygems".into(),
        repo: pin.repo.clone(),
        tag: pin.tag.clone(),
        rev: pin.rev.clone(),
        subdir: None,
    })?;

    crate::vendor::remove_dir_all(&dest)?;
    for (from, to) in [
        (checkout.join("lib"), dest.join("rubygems/lib")),
        (checkout.join("bundler/lib"), dest.join("bundler/lib")),
        (checkout.join("bundler/exe"), dest.join("bundler/exe")),
    ] {
        crate::vendor::copy_tree(&from, &to)?;
    }
    for (name, license) in [
        ("rubygems", checkout.join("LICENSE.txt")),
        ("bundler", checkout.join("bundler/LICENSE.md")),
    ] {
        let dir = dest.join(name);
        write_stub_gemspec(&dir, name, &pin.version)?;
        if license.is_file() {
            let to = dir.join(license.file_name().expect("a license file name"));
            std::fs::copy(&license, &to)
                .map_err(|e| Error::new(format!("copying {}: {e}", to.display())))?;
        }
    }
    check_bootstrap_versions(&dest, &pin.version)?;
    std::fs::write(&stamp, &pin.rev)
        .map_err(|e| Error::new(format!("writing {}: {e}", stamp.display())))?;
    Ok(())
}

fn write_stub_gemspec(dir: &Path, name: &str, version: &str) -> Result<(), Error> {
    let spec = zeo_gem::Gemspec {
        name: name.to_string(),
        version: Some(version.to_string()),
        require_paths: vec!["lib".to_string()],
        ..zeo_gem::Gemspec::default()
    };
    let path = dir.join(format!("{name}.gemspec"));
    std::fs::write(&path, spec.to_stub_source())
        .map_err(|e| Error::new(format!("writing {}: {e}", path.display())))
}

/// `Gem::VERSION` and `Bundler::VERSION` are the goldens' own anchors. A tag
/// that carries a different version than the lock names is worth catching
/// here, not as a mystery diff later.
fn check_bootstrap_versions(dest: &Path, want: &str) -> Result<(), Error> {
    for rel in ["rubygems/lib/rubygems.rb", "bundler/lib/bundler/version.rb"] {
        let path = dest.join(rel);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
        let found = text
            .lines()
            .find(|l| l.trim_start().starts_with("VERSION = "))
            .and_then(|l| l.split('"').nth(1))
            .ok_or_else(|| Error::new(format!("{}: no VERSION assignment", path.display())))?;
        if found != want {
            return Err(Error::new(format!(
                "{} states version {found}, and the lock names {want}",
                path.display()
            )));
        }
    }
    Ok(())
}

/// One cached `.gem`, already verified against the lock.
struct Cached {
    full_name: String,
    path: PathBuf,
}

/// Download every gem the lock resolves into `vendor/cache/`, verifying each
/// against its `CHECKSUMS` row. A file already there with the right digest is
/// left alone, so a second run touches the network for nothing.
fn fetch_gems(lock: &Lockfile) -> Result<Vec<Cached>, Error> {
    let remote = lock
        .gem_remote()
        .ok_or_else(|| Error::new("Gemfile.lock names no GEM remote to download from"))?
        .trim_end_matches('/')
        .to_string();
    let cache = root_join(CACHE);
    std::fs::create_dir_all(&cache)
        .map_err(|e| Error::new(format!("creating {}: {e}", cache.display())))?;

    let mut out = Vec::new();
    let mut fetched = 0usize;
    for spec in source_rows(lock) {
        let full_name = spec.full_name();
        let want = lock
            .checksum(&full_name)
            .and_then(|c| c.sha256())
            .expect("read_lock refused a row with no sha256");
        let path = cache.join(format!("{full_name}.gem"));
        if !holds(&path, want) {
            println!("deps: fetching {full_name}.gem");
            download(&format!("{remote}/downloads/{full_name}.gem"), &path)?;
            fetched += 1;
            if !holds(&path, want) {
                let _ = std::fs::remove_file(&path);
                return Err(Error::new(format!(
                    "{full_name}.gem does not match the sha256 Gemfile.lock states. \
                     The download was discarded."
                )));
            }
        }
        out.push(Cached { full_name, path });
    }
    if fetched > 0 {
        println!(
            "deps: {fetched} gem(s) downloaded, {} in the cache",
            out.len()
        );
    }
    Ok(out)
}

/// Whether the file is there AND hashes to what the lock states.
fn holds(path: &Path, want: &[u8]) -> bool {
    use sha2::Digest as _;
    std::fs::read(path).is_ok_and(|bytes| sha2::Sha256::digest(&bytes).as_slice() == want)
}

fn download(url: &str, to: &Path) -> Result<(), Error> {
    // A partial file must never look like a cache hit, so the download lands
    // beside the target and is renamed once curl says it finished.
    let temp = to.with_extension("gem.part");
    let out = exec::run(
        &[
            "curl",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--retry",
            "3",
            "--output",
            &temp.display().to_string(),
            url,
        ],
        root(),
        &[],
        Capture::Both,
    )?;
    if !out.success() {
        let _ = std::fs::remove_file(&temp);
        return Err(Error::new(format!(
            "fetching {url} failed: {}",
            out.stderr_text().trim()
        )));
    }
    std::fs::rename(&temp, to).map_err(|e| Error::new(format!("writing {}: {e}", to.display())))
}

/// Unpack each verified `.gem` into `vendor/gems/`, the store zeo's loader
/// reads. What is already installed is skipped unless `--refresh`.
fn unpack_gems(cached: &[Cached], refresh: bool) -> Result<(), Error> {
    let store = Store::new(root_join(GEM_STORE));
    let mut installed = 0usize;
    for gem in cached {
        if !refresh && store.holds(&gem.full_name) {
            continue;
        }
        let package = Package::open(&gem.path).map_err(|e| Error::new(e.message()))?;
        store
            .install(&package)
            .map_err(|e| Error::new(e.message()))?;
        installed += 1;
    }
    // A name in the store that the lock no longer resolves would keep serving
    // a require nothing pinned. The store is ours to state completely.
    let wanted: std::collections::BTreeSet<&str> =
        cached.iter().map(|g| g.full_name.as_str()).collect();
    for spec in store.installed().map_err(|e| Error::new(e.message()))? {
        let full_name = spec.full_name();
        if !wanted.contains(full_name.as_str()) {
            println!("deps: removing {full_name}, which the lock no longer names");
            store
                .remove(&full_name)
                .map_err(|e| Error::new(e.message()))?;
        }
    }
    if installed > 0 {
        println!("deps: {installed} gem(s) unpacked into {GEM_STORE}");
    }
    Ok(())
}

/// What this run resolved, for the checks that say "run `cargo xtask deps`".
fn write_stamp(pin: &RubygemsPin) -> Result<(), Error> {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    for rel in ["Gemfile.lock", "crates/xtask/rubygems.lock"] {
        let path = root_join(rel);
        let bytes =
            std::fs::read(&path).map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
        hasher.update(&bytes);
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let path = root_join(STAMP);
    std::fs::write(&path, format!("{digest}\nrubygems {}\n", pin.rev))
        .map_err(|e| Error::new(format!("writing {}: {e}", path.display())))
}

/// The oracle's own store: RubyGems' installer, over the same cached files.
///
/// This is the one step that needs ruby, and it needs it because only
/// RubyGems can build a gem's native half against the running interpreter and
/// write the serialized gemspecs `-rbundler/setup` reads.
fn install_for_oracle(cached: &[Cached]) -> Result<(), Error> {
    let oracle = crate::oracle::find(root()).map_err(Error::new)?;
    let abi = ruby_abi(&oracle)?;
    let dir = root_join(ORACLE_STORE).join(&abi);
    let store = Store::new(&dir);
    let missing: Vec<&Cached> = cached
        .iter()
        .filter(|gem| !store.holds(&gem.full_name))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    println!(
        "deps: installing {} gem(s) for the oracle into {}",
        missing.len(),
        dir.display()
    );
    for gem in missing {
        let out = exec::run(
            &[
                oracle.bin.display().to_string().as_str(),
                "-S",
                "gem",
                "install",
                "--local",
                "--ignore-dependencies",
                "--no-document",
                "--install-dir",
                &dir.display().to_string(),
                "--bindir",
                &dir.join("bin").display().to_string(),
                &gem.path.display().to_string(),
            ],
            root(),
            // RubyGems must not read whatever the developer has installed.
            &[
                ("GEM_HOME", None),
                ("GEM_PATH", None),
                ("RUBYOPT", None),
                ("BUNDLE_GEMFILE", None),
            ],
            Capture::Both,
        )?;
        if !out.success() {
            return Err(Error::new(format!(
                "installing {} for the oracle failed:\n{}",
                gem.full_name,
                out.stderr_text().trim()
            )));
        }
    }
    Ok(())
}

/// `RbConfig`'s ABI directory name -- what RubyGems itself installs under.
fn ruby_abi(oracle: &crate::oracle::Oracle) -> Result<String, Error> {
    let out = exec::run(
        &[
            oracle.bin.display().to_string().as_str(),
            "-rrbconfig",
            "-e",
            "print RbConfig::CONFIG['ruby_version']",
        ],
        root(),
        &[("RUBYOPT", None), ("BUNDLE_GEMFILE", None)],
        Capture::Both,
    )?;
    let abi = out.stdout_text().trim().to_string();
    if !out.success() || abi.is_empty() {
        return Err(Error::new(format!(
            "asking {} for its ABI version failed: {}",
            oracle.bin.display(),
            out.stderr_text().trim()
        )));
    }
    Ok(abi)
}
