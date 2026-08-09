//! `cargo xtask gem-probe` -- fetch a gem from rubygems.org and try to compile
//! it, then record the verdict.
//!
//! This answers a question `gem-compat` cannot. That command classifies a gem
//! by its LAYOUT: "pure Ruby, so zeo would compile it". It never runs the
//! compiler, so a gem using a construct zeo cannot lower still reports as
//! resolvable. concurrent-ruby is the standing example. `gem-probe` runs the
//! front end for real.
//!
//! What it runs, and how far, is the `stage` column -- see [`Stage`]. The
//! default rung is `emits-rs`: zeo produced Rust. That is deliberately the
//! weakest useful claim and it used to be spelled `compiles`, which read as a
//! much stronger one. No rustc runs, no binary exists, and the gem's own code
//! may not have been compiled at all -- zeo can decline a unit and defer it to
//! a runtime `LoadError`, which only `--run` can see. `--build` and `--run`
//! climb the rungs above, both off by default.
//!
//! Four steps, and only the first touches the network:
//!
//!   resolve  name [version]     -> an exact version
//!   fetch    the .gem           -> vendor/gems/<name>/   (cached, gitignored)
//!   probe    require "<entry>"  -> a Stage and an Outcome
//!   record   the verdict        -> conformance/gem-probe-*.tsv     (committed)
//!
//! The emitted Rust is kept, gzipped, under `vendor/.probe-rs/`, so a later
//! sweep can climb to `builds-bin` for the whole corpus without paying for
//! codegen twice.
//!
//! Probing an unpacked tree needs no network and is deterministic, so a ledger
//! row reproduces from its recorded version alone.
//!
//! `probe` runs the `zeo` BINARY, and the sweep runs `--jobs` of them at once
//! against a shared work queue while a single thread ahead of them does the
//! resolving and fetching. Both halves of that are load-bearing. One compile is
//! seconds to minutes and there are hundreds, so a sequential sweep is hours --
//! long enough that the last two were abandoned partway, which is how the
//! ledger came to carry rows measured against a compiler two fixes old. And a
//! subprocess contains every way a compile can die, not just the unwinding
//! panic `parser` raised: an abort, a stack overflow and a run that never
//! finishes all end the same way, the last of them via `--timeout`.
//!
//! A panic and a timeout are each recorded as their own outcome rather than
//! folded into `lowering-gap`: a gap is a limit zeo reported, a panic is a bug
//! it did not, and a timeout is no verdict at all.
//!
//! Only the recorder thread inserts and writes, so the ledger on disk stays the
//! truth so far and an interrupted sweep keeps its work. Verdicts arrive out of
//! order; the ledger does not, because it is a `BTreeMap` written whole.
//!
//! A gem is unpacked to `vendor/gems/<name>/`, named for the gem rather than
//! `<name>-<version>`, and its gemspec is REPLACED with a stub. Both are
//! required, not stylistic: zeo checks a gemspec's name against its directory
//! name, and it parses gemspecs statically, so the computed `s.version` most
//! real gems use ("spec.version = Colorator::VERSION") is rejected. The stub
//! carries the version the registry reported. This is the layout `gems/`
//! already uses, so a probe exercises the same loader path bundled gems do.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const REGISTRY: &str = "https://rubygems.org";

/// `--help`. Selection comes first because that is the choice a run starts
/// with, and the stage flags say plainly what is OFF by default: nothing here
/// builds a binary or executes gem code unless asked twice.
const HELP: &str = "\
usage: cargo xtask gem-probe [<name> [version]] [selection] [options]

Compiles real rubygems with zeo and records how far each one got in
conformance/gem-probe-{compiles,fails}.tsv (plus gem-probe.md).

selection (at least one, they add up):
  <name> [version]        one gem; the newest version unless one is given
  --corpus                every name in conformance/rubygems-names.txt
  --popular <n>           the n most-downloaded gems in the index
  --index                 every gem in the index
  --all                   re-probe every gem already in the ledger
  --failing               re-probe every ledger row that isn't `ok` (the
                          frontier is excluded -- it is not a failure)
  --unprobed              probe ledger rows that have no verdict yet
  --matching <text>       re-probe rows whose outcome detail contains <text>
                          -- how a landed fix is measured
  --matching-name <text>  re-probe rows whose gem NAME contains <text>, for a
                          stale band no detail substring can select

stages (the ladder is fetch -> unpack -> emits-rs -> builds-bin -> runs):
  (default)               stop at emits-rs: generate Rust, keep the .rs, build
                          nothing
  --build                 also compile the generated Rust to a binary
  --run                   also EXECUTE it. This runs code downloaded from
                          rubygems, so it needs --allow-running-untrusted-gem-
                          code, named gems only, a sandbox, and a prompt
  --allow-running-untrusted-gem-code
                          the second half of --run's consent

options:
  --check                 exit non-zero if any probed gem regressed
  --refresh               re-probe even names already in the ledger
  --refresh-index         fetch a fresh copy of the rubygems index
  --seed-index            record every gem in the index that has no row yet,
                          as `queued unprobed` -- makes the ledger say what is
                          left to measure, not just what has been
  --no-deps               don't resolve or fetch the gem's dependencies
  --limit <n>             stop after n gems
  --jobs <n>              gems in flight at once (default: cpu count)
  --timeout <seconds>     kill a gem that outruns it (default: 600)
  -h, --help              this message
";

/// Long enough that a rails-scale require graph finishes -- those take minutes
/// in the front end alone -- and short enough that a gem which will never
/// finish cannot hold a sweep open.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// How far up the pipeline a verdict got.
///
/// The rungs are ordered and each is a strictly harder claim about the gem
/// than the one below: unpacking says the archive had a `lib/`, `emits-rs`
/// says zeo's front end produced Rust, `builds-bin` says rustc accepted that
/// Rust, `runs` says the binary executed. They are a separate column rather
/// than more outcome tags so that adding a rung costs one value instead of a
/// schema change, and so a failure can say WHERE it stopped -- a `timeout` in
/// codegen and a `timeout` in rustc are not the same row.
///
/// `emits-rs` is the default and the only rung a sweep reaches on its own.
/// It is deliberately the weakest useful claim: it does NOT mean a binary
/// exists, and it does not mean the gem's own code was compiled -- zeo may
/// have declined a unit and deferred it to a runtime `LoadError`, which only
/// `runs` can see.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Stage {
    /// No rung reached: the gem is KNOWN (the registry names it) and has never
    /// been probed. Ordered below `Fetch` so every `stage >= ...` test treats
    /// it as the bottom.
    Queued,
    Fetch,
    Unpack,
    EmitsRs,
    BuildsBin,
    Runs,
}

impl Stage {
    fn tag(self) -> &'static str {
        match self {
            Stage::Queued => "queued",
            Stage::Fetch => "fetch",
            Stage::Unpack => "unpack",
            Stage::EmitsRs => "emits-rs",
            Stage::BuildsBin => "builds-bin",
            Stage::Runs => "runs",
        }
    }

    fn from_tag(tag: &str) -> Option<Stage> {
        match tag {
            "queued" => Some(Stage::Queued),
            "fetch" => Some(Stage::Fetch),
            "unpack" => Some(Stage::Unpack),
            "emits-rs" => Some(Stage::EmitsRs),
            "builds-bin" => Some(Stage::BuildsBin),
            "runs" => Some(Stage::Runs),
            _ => None,
        }
    }
}

/// What a probe concluded at its [`Stage`]. `Ok` means that rung was reached;
/// every other variant says why the climb stopped there.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Outcome {
    Ok,
    /// zeo reached the gem's source and could not lower it. The payload is the
    /// compiler's own first line -- the actionable half of the verdict.
    LoweringGap(String),
    /// The gem's source is not Ruby any release still parses. Its own outcome,
    /// never a lowering gap: zeo parses with prism, CRuby's own parser, so a
    /// parse error here is one `ruby -c` gives too -- 1.8-era `when x: y`,
    /// `class Foo::Bar` with an undefined `Foo`, a stray `\r`. Nothing to fix,
    /// and counting these as gaps overstated the ledger's backlog.
    InvalidRuby(String),
    /// Ships C sources. zeo cannot build these at all; see docs/EXTENSIONS.md.
    NativeExtension,
    /// A `require` reached outside the gem and its fetched dependencies.
    MissingDependency(String),
    /// No `lib/` to put on the load path.
    NoLibDir,
    /// A `lib/` with no file this gem's name could name. Probing would compile
    /// an unresolvable require, which says nothing about the gem.
    NoEntryPoint,
    /// The compiler panicked. Distinct from a lowering gap on purpose: a gap
    /// is a known limit reported through the error path, a panic is a bug.
    CompilerPanic(String),
    /// The compiler was still running after `--timeout` seconds and was killed.
    /// Its own outcome, never a lowering gap: a stall is the absence of a
    /// verdict, and recording it as one would put a diagnosis in the ledger
    /// that zeo never made.
    Timeout,
    /// Named by the registry and never probed. Not a failure and not a
    /// verdict -- it is the FRONTIER, the work still to do, and it is in the
    /// ledger so that "how much of rubygems have we measured" is a question
    /// the file answers rather than one that needs the index beside it.
    Unprobed,
    FetchFailed(String),
    /// The gem's source is on disk, but building its isolated load-path view
    /// failed. NOT a `fetch-failed`: nothing was downloaded, the registry was
    /// never asked, and re-running the fetch cannot help. Its own outcome so a
    /// sweep's fetch column keeps meaning "the registry or the network", which
    /// is what anyone reading it goes on to check.
    ViewFailed(String),
    /// rustc rejected the Rust zeo emitted. zeo's to fix, and a sharper bug
    /// than a lowering gap: the front end believed it had produced a program.
    RustcError(String),
    /// The binary was built and did not exit 0. The common shape is a runtime
    /// `LoadError` from a unit zeo declined at compile time, which every rung
    /// below this one reports as success.
    RunFailed(String),
}

impl Outcome {
    fn tag(&self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::LoweringGap(_) => "lowering-gap",
            Outcome::InvalidRuby(_) => "invalid-ruby",
            Outcome::NativeExtension => "native-extension",
            Outcome::MissingDependency(_) => "missing-dependency",
            Outcome::NoLibDir => "no-lib-dir",
            Outcome::NoEntryPoint => "no-entry-point",
            Outcome::CompilerPanic(_) => "compiler-panic",
            Outcome::Timeout => "timeout",
            Outcome::Unprobed => "unprobed",
            Outcome::FetchFailed(_) => "fetch-failed",
            Outcome::ViewFailed(_) => "view-failed",
            Outcome::RustcError(_) => "rustc-error",
            Outcome::RunFailed(_) => "run-failed",
        }
    }

    fn detail(&self) -> &str {
        match self {
            Outcome::LoweringGap(d)
            | Outcome::InvalidRuby(d)
            | Outcome::MissingDependency(d)
            | Outcome::FetchFailed(d)
            | Outcome::ViewFailed(d)
            | Outcome::CompilerPanic(d)
            | Outcome::RustcError(d)
            | Outcome::RunFailed(d) => d,
            _ => "",
        }
    }

    fn from_ledger(tag: &str, detail: &str) -> Outcome {
        match tag {
            // `compiles` is the pre-`stage` spelling of this rung's success.
            // It read as "this gem compiles", which was never what the probe
            // measured -- no rustc ran and no binary existed.
            "ok" | "compiles" => Outcome::Ok,
            "native-extension" => Outcome::NativeExtension,
            "no-lib-dir" => Outcome::NoLibDir,
            "no-entry-point" => Outcome::NoEntryPoint,
            "timeout" => Outcome::Timeout,
            "missing-dependency" => Outcome::MissingDependency(detail.to_string()),
            "unprobed" => Outcome::Unprobed,
            "fetch-failed" => Outcome::FetchFailed(detail.to_string()),
            "view-failed" => Outcome::ViewFailed(detail.to_string()),
            "compiler-panic" => Outcome::CompilerPanic(detail.to_string()),
            "rustc-error" => Outcome::RustcError(detail.to_string()),
            "run-failed" => Outcome::RunFailed(detail.to_string()),
            "invalid-ruby" => Outcome::InvalidRuby(detail.to_string()),
            _ => Outcome::LoweringGap(detail.to_string()),
        }
    }

    /// The rung a legacy row's outcome must have been decided at, for a
    /// ledger written before the `stage` column existed.
    fn implied_stage(&self) -> Stage {
        match self {
            Outcome::Unprobed => Stage::Queued,
            Outcome::FetchFailed(_) => Stage::Fetch,
            Outcome::ViewFailed(_) | Outcome::NoLibDir | Outcome::NoEntryPoint => Stage::Unpack,
            Outcome::RustcError(_) => Stage::BuildsBin,
            Outcome::RunFailed(_) => Stage::Runs,
            _ => Stage::EmitsRs,
        }
    }
}

#[derive(Clone, Debug)]
struct Row {
    version: String,
    /// The rung this verdict is about -- see [`Stage`]. Held rather than
    /// derived because `Ok` is reachable at three different rungs and the
    /// outcome alone cannot say which one a sweep asked for.
    stage: Stage,
    outcome: Outcome,
    /// Bytes of Rust zeo emitted. Deterministic for a given gem and compiler,
    /// so it belongs in the committed ledger: it diffs when codegen changes
    /// and is silent otherwise. Absent when the front end never got there.
    rust_bytes: Option<u64>,
    /// Bytes of the linked binary, when `--build` ran. Deterministic for a
    /// given toolchain; a rustc upgrade moves every row at once, which is
    /// rare and worth seeing. The binary itself is deleted once measured --
    /// one per gem across the corpus is tens of gigabytes.
    binary_bytes: Option<u64>,
    /// The Ruby the verdict points at, `<repo-relative path>:<line>`, when zeo
    /// named one.
    ///
    /// The detail alone says what zeo refused, not where: `subclassing the
    /// built-in type \`Module\`` names a construct that appears in dozens of
    /// files across a gem's dependency tree, and finding it meant grepping.
    /// Repo-relative because this is committed and must not carry one
    /// machine's directory layout.
    site: Option<String>,
    /// The registry's sha256 of the `.gem` this verdict was measured against.
    ///
    /// The version alone names a release; this names the bytes. It is what
    /// makes a disagreeing re-probe attributable -- zeo changed, rather than
    /// the artifact did -- which matters exactly because this ledger's first
    /// job was distinguishing a stale row from a real gap. Absent for
    /// `--no-deps` runs, which never ask the registry anything.
    digest: Option<String>,
}

impl Row {
    /// A gem that never reached the compiler, so has no metrics to record.
    fn stopped(version: &str, stage: Stage, outcome: Outcome, digest: Option<String>) -> Row {
        Row {
            version: version.to_string(),
            stage,
            outcome,
            rust_bytes: None,
            binary_bytes: None,
            site: None,
            digest,
        }
    }

    fn from_verdict(version: String, digest: Option<String>, v: &Verdict) -> Row {
        Row {
            version,
            stage: v.stage,
            outcome: v.outcome.clone(),
            rust_bytes: v.rust_bytes,
            binary_bytes: v.binary_bytes,
            site: v.site.clone(),
            digest,
        }
    }
}

// ---------------------------------------------------------------- registry

/// The registry client, with the timeouts that keep a stalled socket from
/// stopping the whole run.
///
/// `ureq::get` on its own has no deadline, and a probe of the full corpus makes
/// thousands of requests -- so "rare" is a certainty. One did: a `.gem` download
/// from rubygems' CDN went quiet mid-body, the worker sat in `read` forever, and
/// with the other 11 workers parked waiting for it the run stopped dead at
/// 2275/3850 with no output and no CPU. There is a `--timeout` for the COMPILE
/// step; the fetch had none.
///
/// `timeout_global` is the one that matters, because it bounds the whole call
/// including the body read. The others are tighter bounds on the phases that
/// should be fast, so a dead host is noticed in seconds rather than a minute.
/// A gem that trips these is a `fetch-failed` row like any other -- the run
/// carries on, and the row says what happened.
fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .build()
            .into()
    })
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    agent()
        .get(url)
        .call()
        .map_err(|e| format!("{e}"))?
        .into_body()
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|e| format!("{e}"))?;
    Ok(body)
}

fn json(url: &str) -> Result<serde_json::Value, String> {
    serde_json::from_slice(&get(url)?).map_err(|e| format!("{url}: {e}"))
}

fn latest_version(name: &str) -> Result<String, String> {
    let v = json(&format!("{REGISTRY}/api/v1/versions/{name}/latest.json"))?;
    match v.get("version").and_then(|v| v.as_str()) {
        // The registry answers "unknown" for a name it does not carry.
        None | Some("unknown") => Err(format!("no such gem on rubygems.org: {name}")),
        Some(v) => Ok(v.to_string()),
    }
}

/// What one EXACT version declares.
struct VersionMeta {
    /// The registry's own sha256 of the `.gem`. Recorded in the ledger so a row
    /// names the artifact it was measured against, and checked against the
    /// bytes on a real download.
    sha: Option<String>,
    runtime: Vec<String>,
}

/// Metadata for one EXACT version. The v1 endpoint describes only the newest
/// release, which would silently mis-resolve a pinned probe.
///
/// The sha rides along on this request rather than being computed from the
/// bytes, so a row records one whether or not the gem was already unpacked --
/// hashing would have covered only the gems a sweep happened to re-download,
/// which is the minority and an arbitrary one.
fn version_meta(name: &str, version: &str) -> Result<VersionMeta, String> {
    let v = json(&format!(
        "{REGISTRY}/api/v2/rubygems/{name}/versions/{version}.json"
    ))?;
    Ok(VersionMeta {
        sha: v
            .get("sha")
            .and_then(|s| s.as_str())
            .map(str::to_ascii_lowercase),
        runtime: v
            .pointer("/dependencies/runtime")
            .and_then(|d| d.as_array())
            .map(|deps| {
                deps.iter()
                    .filter_map(|d| d.get("name").and_then(|n| n.as_str()))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

// ------------------------------------------------------------------ fetch

fn vendor_dir(root: &Path) -> PathBuf {
    root.join("vendor/gems")
}

/// Records which version is unpacked, so a fetch is idempotent and a version
/// change re-unpacks rather than probing stale source.
fn stamp_path(dir: &Path) -> PathBuf {
    dir.join(".zeo-probe-version")
}

fn unpack_gem(bytes: &[u8], dest: &Path) -> Result<(), String> {
    // A .gem is a tar of metadata.gz, data.tar.gz and checksums.yaml.gz. The
    // gem's own files are the middle one.
    let mut outer = tar::Archive::new(bytes);
    let mut data = Vec::new();
    for entry in outer.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let is_data = entry
            .path()
            .map(|p| p.as_os_str() == "data.tar.gz")
            .unwrap_or(false);
        if is_data {
            entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
            break;
        }
    }
    if data.is_empty() {
        return Err("no data.tar.gz inside the .gem".into());
    }
    let gz = flate2::read::GzDecoder::new(&data[..]);
    tar::Archive::new(gz)
        .unpack(dest)
        .map_err(|e| format!("unpacking: {e}"))
}

fn write_stub_gemspec(dir: &Path, name: &str, version: &str) -> Result<(), String> {
    std::fs::write(
        dir.join(format!("{name}.gemspec")),
        format!(
            "Gem::Specification.new do |s|\n  \
             s.name = {name:?}.freeze\n  \
             s.version = {version:?}.freeze\n  \
             s.require_paths = [\"lib\".freeze]\nend\n"
        ),
    )
    .map_err(|e| e.to_string())
}

/// Whether the CACHED gemspec is one of our own stubs rather than what
/// rubygems shipped.
///
/// The stub belongs in the view, and does go there now -- but an earlier
/// version wrote it over the cache, and the version stamp then kept those
/// gems from ever being re-fetched. A stub in the cache has lost the gem's
/// real `require_paths` (concurrent-ruby's is `lib/concurrent-ruby`, not
/// `lib`) and its `extensions`, so treating it as stale is what heals the
/// damage without anyone having to know which gems were affected.
fn cached_gemspec_is_a_stub(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|e| {
        let p = e.path();
        p.extension().is_some_and(|x| x == "gemspec")
            && std::fs::read_to_string(&p).is_ok_and(|t| {
                // The stub is exactly four lines and names nothing else.
                t.lines().count() == 5 && t.contains("s.require_paths") && !t.contains("s.summary")
            })
    })
}

/// The version `vendor/gems/<gem>/` was unpacked at, for stubbing its gemspec
/// in a view. Absent only if the cache was written by hand.
fn unpacked_version(dir: &Path) -> String {
    std::fs::read_to_string(stamp_path(dir))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

/// One gem inside a view: its own contents, symlinked, except that the gemspec
/// is replaced by a stub.
///
/// The stub is required, not stylistic -- zeo parses gemspecs statically, so
/// the computed `s.version` most real gems use (`spec.version = Colorator::VERSION`)
/// is rejected, and zeo also checks a gemspec's name against its directory
/// name. What is stylistic is WHERE it goes, and it goes here rather than over
/// the real file in `vendor/gems/`: overwriting made the cache no longer a copy
/// of what rubygems shipped, so a ledger row could not be reproduced from it,
/// the real gemspec could never be re-read, and changing the stub's format
/// meant re-downloading every gem.
fn link_gem_into_view(view: &Path, src: &Path, gem: &str) -> Result<(), String> {
    let dest = view.join(gem);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.extension().is_some_and(|x| x == "gemspec") {
            continue;
        }
        let Some(base) = p.file_name() else { continue };
        let link = dest.join(base);
        std::os::unix::fs::symlink(&p, &link).map_err(|e| {
            // On a case-insensitive filesystem two gems whose names differ only
            // in case share one directory, so the second one's links land on
            // the first one's. `Cartesian` depends on `cartesian`, and macOS
            // cannot hold both. Say that, rather than leaving `File exists` for
            // a reader to work out.
            match e.kind() {
                // No absolute path in the text: this goes to a committed
                // ledger and must not carry one machine's directory layout.
                std::io::ErrorKind::AlreadyExists => format!(
                    "`{gem}` is already in the view -- on a case-insensitive \
                     filesystem it cannot be told from a dependency spelled \
                     differently only in case"
                ),
                _ => e.to_string(),
            }
        })?;
    }
    write_stub_gemspec(&dest, gem, &unpacked_version(src))
}

/// Unpacks `name`-`version` into `vendor/gems/<name>/`, or confirms it is
/// already there.
///
/// `expect_sha` is the registry's own digest when one was resolved. A download
/// that doesn't match it is refused rather than probed: the whole point of
/// recording a digest is that a verdict names the artifact it was measured
/// against, and probing bytes the registry disowns would put a verdict in the
/// ledger under an artifact that never produced it.
fn fetch(
    root: &Path,
    name: &str,
    version: &str,
    expect_sha: Option<&str>,
) -> Result<PathBuf, String> {
    let dir = vendor_dir(root).join(name);
    if std::fs::read_to_string(stamp_path(&dir)).is_ok_and(|s| s.trim() == version)
        && !cached_gemspec_is_a_stub(&dir)
    {
        return Ok(dir);
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let url = format!("{REGISTRY}/downloads/{name}-{version}.gem");
    let bytes = get(&url).map_err(|e| format!("{url}: {e}"))?;
    if let Some(want) = expect_sha {
        let got = sha256_hex(&bytes);
        if got != want {
            return Err(format!(
                "{name}-{version}.gem is sha256 {got}, but the registry describes {want}"
            ));
        }
    }
    unpack_gem(&bytes, &dir)?;
    std::fs::write(stamp_path(&dir), version).map_err(|e| e.to_string())?;
    Ok(dir)
}

// ------------------------------------------------------------------ probe

/// The feature a gem's users require, resolved against the files the gem
/// actually ships.
///
/// Guessing from the name alone is not good enough, and failing quietly is the
/// reason: `require "activerecord"` names no file, zeo lowers an unresolvable
/// require to a RUNTIME `Kernel#require` rather than failing, and codegen then
/// trivially succeeds having compiled none of the gem. Every Rails gem
/// reported `compiles` that way while measuring nothing at all.
///
/// So the entry point is a file that exists, or the probe declines to answer.
fn entry_point(dir: &Path, name: &str) -> Option<String> {
    let lib = dir.join("lib");
    // `net-http` -> `net/http`, `ruby-progressbar` -> `ruby_progressbar`.
    for candidate in [
        name.replace('-', "/"),
        name.replace('-', "_"),
        name.to_string(),
    ] {
        if lib.join(format!("{candidate}.rb")).is_file() {
            return Some(candidate);
        }
    }
    // `activerecord` ships `active_record.rb`: the separators differ, so
    // compare with them removed.
    let squash = |s: &str| s.replace(['-', '_', '/'], "").to_lowercase();
    let target = squash(name);
    let mut tops: Vec<String> = std::fs::read_dir(&lib)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "rb"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    if let Some(hit) = tops.iter().find(|stem| squash(stem) == target) {
        return Some(hit.clone());
    }
    // A gem with exactly one top-level file has named its entry point.
    tops.sort();
    if tops.len() == 1 {
        return Some(tops.remove(0));
    }
    // ONE DIRECTORY DEEPER, for the gems that nest their entry: concurrent-ruby
    // ships `lib/concurrent-ruby/concurrent-ruby.rb`, json_pure ships
    // `lib/json/pure.rb`. Both are still name matches, just against a path
    // rather than a top-level stem -- so this stays a resolution rule and not a
    // guess, which is the distinction the doc above exists to protect.
    //
    // Either half can carry the name: `json/pure` squashes to the gem name as a
    // PATH, while `concurrent-ruby/concurrent-ruby` carries it in the STEM.
    let mut nested: Vec<String> = Vec::new();
    for sub in std::fs::read_dir(&lib).ok()?.filter_map(Result::ok) {
        let subdir = sub.path();
        if !subdir.is_dir() {
            continue;
        }
        let Some(sub_name) = subdir.file_name().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        for f in std::fs::read_dir(&subdir).ok()?.filter_map(Result::ok) {
            let p = f.path();
            if !p.is_file() || p.extension().is_none_or(|x| x != "rb") {
                continue;
            }
            let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            if squash(&stem) == target {
                return Some(format!("{sub_name}/{stem}"));
            }
            nested.push(format!("{sub_name}/{stem}"));
        }
    }
    nested.sort();
    nested.into_iter().find(|path| squash(path) == target)
}

/// A package directory holding ONLY this gem and its declared dependencies.
///
/// Pointing the probe at the whole of `vendor/gems` made a verdict depend on
/// which other gems happened to be cached: kramdown reported one gap alone,
/// and a different one once kramdown-parser-gfm had been fetched beside it. An
/// isolated view makes the result a function of the gem and its deps, which is
/// what the ledger claims to record.
fn isolate(root: &Path, name: &str, deps: &[String]) -> Result<PathBuf, String> {
    // Outside vendor/gems, not under it. A view is a directory of gem
    // directories, so nesting it inside the cache would make the cache
    // contain something shaped like a gem.
    let view = root.join("vendor/.probe").join(name);
    let _ = std::fs::remove_dir_all(&view);
    std::fs::create_dir_all(&view).map_err(|e| e.to_string())?;
    // DEDUPLICATED. A gem may list itself among its runtime dependencies --
    // jeweler-generated gemspecs do it routinely -- and the registry may name
    // one twice. Linking a name a second time re-symlinks entries that already
    // exist, which fails with EEXIST and lost the gem a verdict entirely: 257
    // corpus rows, `Authorizr` and its self-dependency among them.
    let mut linked = std::collections::HashSet::new();
    for gem in std::iter::once(&name.to_string()).chain(deps) {
        if !linked.insert(gem.as_str()) {
            continue;
        }
        let src = vendor_dir(root).join(gem);
        if src.is_dir() {
            link_gem_into_view(&view, &src, gem)?;
        }
    }
    Ok(view)
}

/// Compiles the gem's entry point by running the `zeo` BINARY, not the library.
///
/// A sweep must survive a gem that kills the compiler, and a subprocess is the
/// only containment that covers every way it can die. `catch_unwind` caught the
/// unwinding panic `parser` raised, but it cannot catch an abort, a stack
/// overflow, or a compile that simply never finishes -- and a run that never
/// finishes is what stopped the last two sweeps partway and left the ledger
/// carrying stale rows. The OS ends all four the same way, and `--timeout`
/// bounds the last one.
///
/// The verdict is unchanged by the move: `--dump=rust` is the CLI spelling of
/// `compile_to_rust_with`, and `message_of` already accepted either the plain
/// `CompileError` text or the CLI's boxed rendering.
fn probe(root: &Path, zeo: &Path, gem: &Ready, timeout: Duration, tiers: Tiers) -> Verdict {
    let Ready {
        name,
        version,
        dir,
        deps,
        ..
    } = gem;
    if !dir.join("lib").is_dir() {
        return Verdict::stopped(Stage::Unpack, Outcome::NoLibDir);
    }
    // A DECLARED extension is not decisive either, which is why nothing checks
    // for one before this point any more. Many gems ship an optional C
    // accelerator beside a pure-Ruby implementation and compile fine without
    // it -- erb, json, prism, bigdecimal, rbs and eight more were all reported
    // `native-extension` on the strength of the gemspec line alone, having
    // never been compiled. Try, then classify what actually failed: zeo says
    // `native (C) extension` itself when a require genuinely needs one, and
    // `classify` reads that.
    let view = match isolate(root, name, deps) {
        Ok(v) => v,
        Err(e) => return Verdict::stopped(Stage::Unpack, Outcome::ViewFailed(e)),
    };
    let Some(feature) = entry_point(dir, name) else {
        return Verdict::stopped(Stage::Unpack, Outcome::NoEntryPoint);
    };
    let program = format!("require {feature:?}\n");

    let mut cmd = std::process::Command::new(zeo);
    cmd.arg("-e")
        .arg(&program)
        // The repo's own gems/ come too: a probed gem may require a stdlib
        // feature, and answering that from zeo's bundled copy is what a real
        // compile would do. `-e` has no input path, so zeo adds no package dir
        // of its own and the isolated view stays the whole world.
        .arg("--gems")
        .arg(&view)
        .arg("--gems")
        .arg(root.join("gems"))
        .arg("--dump=rust");
    let started = std::time::Instant::now();
    let emitted = crate::exec::run_with_timeout(cmd, None, timeout);
    let codegen_ms = started.elapsed().as_millis();

    let rust = match emitted {
        Err(e) => {
            return Verdict::stopped(
                Stage::Fetch,
                Outcome::FetchFailed(format!("running zeo: {e}")),
            );
        }
        Ok(ex) if ex.timed_out => {
            return Verdict::stopped(Stage::EmitsRs, Outcome::Timeout).timed(codegen_ms);
        }
        // A successful compile still writes to stderr -- every builtin
        // substitution warns there -- so the exit status is the verdict and
        // stderr is only read once it is non-zero.
        Ok(ex) if ex.success() => ex.stdout,
        Ok(ex) => {
            let text = String::from_utf8_lossy(&ex.stderr);
            let mut v = Verdict::stopped(Stage::EmitsRs, classify_stderr(&ex.stderr, root));
            v.site = site_of(&text, root);
            return v.timed(codegen_ms);
        }
    };

    let mut verdict = Verdict::stopped(Stage::EmitsRs, Outcome::Ok).timed(codegen_ms);
    verdict.rust_bytes = Some(rust.len() as u64);
    // The Rust is kept so a later sweep can climb the next rung without paying
    // for codegen again -- emit once for the whole corpus, then build. gzip
    // because the uncompressed corpus does not fit: the generated source is
    // large and repetitive, and a disk that cannot hold the store is a store
    // nobody keeps.
    if let Err(e) = store_rust(root, name, version, &rust) {
        eprintln!("gem-probe: {name}: keeping the generated Rust: {e}");
    }
    if !tiers.build {
        return verdict;
    }

    // The binary tier re-runs zeo with `-o` rather than handing the stored
    // Rust to rustc: the flags that pick the runtime profile and linkage live
    // in zeo, and reproducing them here would be a second source of truth for
    // how a zeo program is built.
    let out = std::env::temp_dir().join(format!("zeo-gem-probe-{name}-{version}"));
    let mut cmd = std::process::Command::new(zeo);
    cmd.arg("-e")
        .arg(&program)
        .arg("--gems")
        .arg(&view)
        .arg("--gems")
        .arg(root.join("gems"))
        .arg("-o")
        .arg(&out);
    let started = std::time::Instant::now();
    let built = crate::exec::run_with_timeout(cmd, None, timeout);
    verdict.build_ms = Some(started.elapsed().as_millis());
    match built {
        Err(e) => {
            verdict.stage = Stage::BuildsBin;
            verdict.outcome = Outcome::RustcError(format!("running zeo -o: {e}"));
            return verdict;
        }
        Ok(ex) if ex.timed_out => {
            verdict.stage = Stage::BuildsBin;
            verdict.outcome = Outcome::Timeout;
            return verdict;
        }
        Ok(ex) if !ex.success() => {
            verdict.stage = Stage::BuildsBin;
            verdict.outcome = Outcome::RustcError(truncate(&classify_build(&ex.stderr, root)));
            return verdict;
        }
        Ok(_) => {}
    }
    verdict.stage = Stage::BuildsBin;
    verdict.binary_bytes = std::fs::metadata(&out).ok().map(|m| m.len());

    if tiers.run {
        let started = std::time::Instant::now();
        let ran = run_sandboxed(&out, timeout);
        verdict.run_ms = Some(started.elapsed().as_millis());
        verdict.stage = Stage::Runs;
        verdict.outcome = match ran {
            Err(e) => Outcome::RunFailed(e),
            Ok(ex) if ex.timed_out => Outcome::Timeout,
            Ok(ex) if ex.success() => Outcome::Ok,
            Ok(ex) => Outcome::RunFailed(truncate(&first_error_line(&ex.stderr))),
        };
    }
    // Measured, then removed. One binary per gem is 20-30 MB and the corpus
    // would be tens of gigabytes; `rust_bytes`/`binary_bytes` are what a later
    // reader wants, and the stored Rust is what a later BUILD wants.
    let _ = std::fs::remove_file(&out);
    verdict
}

/// Which rungs a sweep is allowed to climb. Both are off unless asked for:
/// `emits-rs` is the only rung that runs no code and builds no artifact.
#[derive(Clone, Copy, Default)]
struct Tiers {
    build: bool,
    run: bool,
}

/// One gem's result, at whatever rung it reached.
#[derive(Clone, Debug)]
struct Verdict {
    stage: Stage,
    outcome: Outcome,
    rust_bytes: Option<u64>,
    binary_bytes: Option<u64>,
    site: Option<String>,
    codegen_ms: Option<u128>,
    build_ms: Option<u128>,
    run_ms: Option<u128>,
}

impl Verdict {
    fn stopped(stage: Stage, outcome: Outcome) -> Verdict {
        Verdict {
            stage,
            outcome,
            rust_bytes: None,
            binary_bytes: None,
            site: None,
            codegen_ms: None,
            build_ms: None,
            run_ms: None,
        }
    }

    fn timed(mut self, codegen_ms: u128) -> Verdict {
        self.codegen_ms = Some(codegen_ms);
        self
    }
}

/// A failed `zeo` run's stderr, as an [`Outcome`].
///
/// A panic reaches stderr as `... panicked at <loc>:` with the message on the
/// NEXT line, which no amount of reading the diagnostic text would reveal, so
/// it is detected before `classify` gets a chance to call it a lowering gap.
/// The source location zeo pointed at, as `<repo-relative path>:<line>`.
///
/// miette's excerpt header is the one place the compiler prints a location in
/// a fixed shape -- `╭─[<path>:<line>:<col>]` -- and both error kinds render it
/// now that analyze stamps spans too. A gem outside the repo (nothing in a
/// sweep is) keeps its absolute path, so the caller drops it rather than
/// committing one machine's layout.
fn site_of(stderr: &str, root: &Path) -> Option<String> {
    let open = stderr.find("╭─[")? + "╭─[".len();
    let rest = &stderr[open..];
    let close = rest.find(']')?;
    let located = &rest[..close];
    // `path:line:col` -- keep the line, drop the column. A column is precise
    // about a token, and the ledger is read to find a file.
    let (path, line) = {
        let mut parts = located.rsplitn(3, ':');
        let _col = parts.next()?;
        let line = parts.next()?;
        (parts.next()?, line)
    };
    let prefix = format!("{}/", root.display());
    let path = path.strip_prefix(&prefix)?;
    Some(format!("{path}:{line}"))
}

fn classify_stderr(stderr: &[u8], root: &Path) -> Outcome {
    let text = String::from_utf8_lossy(stderr);
    if let Some(i) = text.lines().position(|l| l.contains("panicked at")) {
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let msg = lines
            .get(i + 1)
            .filter(|l| !l.is_empty())
            .copied()
            .unwrap_or(lines[i]);
        return Outcome::CompilerPanic(truncate(
            &message_of(msg).replace(&format!("{}/", root.display()), ""),
        ));
    }
    classify(&text, root)
}

/// The compiler's message, as one line and free of local paths.
///
/// zeo wraps a diagnostic across `│` continuation lines and prefixes it with
/// the require chain that reached the file, as absolute paths. Both have to go:
/// the wrap so the message survives, and the paths because this text is
/// committed to a ledger and must not carry one machine's directory layout.
fn message_of(err: &str) -> String {
    let boxed: Vec<&str> = err
        .lines()
        .map(str::trim)
        .take_while(|l| !l.starts_with('╭'))
        .filter(|l| l.starts_with('×') || l.starts_with('│'))
        .collect();
    // A CompileError stringifies to a plain message; only the CLI renders the
    // boxed form. Accept either.
    let joined = if boxed.is_empty() {
        err.to_string()
    } else {
        boxed
            .iter()
            .map(|l| l.trim_start_matches(['×', '│']).trim())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut msg = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    // Peel `<path>.rb: ` prefixes, one per file in the require chain.
    while let Some(i) = msg.find(".rb: ") {
        msg = msg[i + ".rb: ".len()..].to_string();
    }
    msg
}

/// Strips this machine's layout out of a message bound for the ledger.
///
/// A diagnostic can carry an absolute path anywhere in it -- inside a
/// `Some((..))` span, not only as a leading prefix -- so the repository root is
/// rewritten away wholesale rather than peeled, and any surviving home-rooted
/// token goes with it.
fn scrub_paths(msg: &str, root: &Path) -> String {
    let msg = msg.replace(&format!("{}/", root.display()), "");
    if msg.contains("/Users/") || msg.contains("/home/") {
        msg.split_whitespace()
            .filter(|w| !w.contains("/Users/") && !w.contains("/home/"))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        msg
    }
}

fn classify(err: &str, root: &Path) -> Outcome {
    let msg = scrub_paths(&message_of(err), root);
    let msg = if msg.is_empty() {
        "compile failed".to_string()
    } else {
        msg
    };
    // NATIVE EXTENSION FIRST. zeo says so INSIDE a `cannot load such file`
    // message ("cannot load such file -- nokogiri: this gem has a native (C)
    // extension ..."), so testing the load-failure prefix first swallowed
    // every one of them into `missing-dependency` and left this arm dead. The
    // two are different verdicts: a missing dependency is a gem the sweep
    // failed to put on the load path, and re-running with it there changes the
    // answer; a native extension is a gem zeo cannot compile at all until an
    // ext exists or the gem is reached through FFI.
    if msg.contains("native (C) extension") {
        return Outcome::NativeExtension;
    }
    if let Some(rest) = msg.split("cannot load such file -- ").nth(1) {
        let feature = rest.split_whitespace().next().unwrap_or(rest);
        return Outcome::MissingDependency(feature.trim_matches(['`', ':', '.']).to_string());
    }
    // zeo parses with prism, which IS CRuby's parser, so its parse errors are
    // the ones `ruby -c` gives. Those gems do not load under any current ruby
    // either -- see `Outcome::InvalidRuby`.
    if msg.starts_with("parse error: ") {
        return Outcome::InvalidRuby(truncate(&msg));
    }
    Outcome::LoweringGap(truncate(&msg))
}

/// rustc's first complaint about the Rust zeo emitted.
///
/// Its diagnostics lead with `error[E0308]: ...` and then quote the source, so
/// the first `error` line is the claim and everything after it is context.
fn classify_build(stderr: &[u8], root: &Path) -> String {
    let text = String::from_utf8_lossy(stderr);
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("error"))
        .or_else(|| text.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("the build failed without saying why");
    scrub_paths(first, root)
}

/// A run's first stderr line -- for a `LoadError` that is the whole message.
fn first_error_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("exited non-zero with no stderr")
        .to_string()
}

/// Whether `--run` may proceed, and if not, why.
///
/// Split out from `main` so the fence can be tested: this is the one code path
/// in the probe that executes code from rubygems, and its guard failing open
/// is not something a reader should have to take on trust. Every check is a
/// refusal, never a warning -- there is no "run anyway" branch to slip into.
fn run_is_permitted(
    named: usize,
    bulk: &[(&str, bool)],
    allow_run: bool,
    sandboxed_platform: bool,
) -> Result<(), String> {
    if let Some((flag, _)) = bulk.iter().find(|(_, on)| *on) {
        return Err(format!(
            "--run takes named gems only, and {flag} selects in bulk.\n  \
             Running a gem executes code from rubygems; name the ones you have read."
        ));
    }
    if named == 0 {
        return Err("--run needs a gem name".into());
    }
    if !allow_run {
        return Err(
            "--run executes code from rubygems in this gem and its dependencies.\n  \
                    Refusing without --allow-running-untrusted-gem-code."
                .into(),
        );
    }
    if !sandboxed_platform {
        return Err(
            "--run confines the binary with sandbox-exec, which is macOS-only.\n  \
             Refusing to run unconfined."
                .into(),
        );
    }
    Ok(())
}

/// An interactive `y` and nothing else.
///
/// A non-tty stdin answers no rather than reading a line: a pipe cannot make
/// this decision, and treating EOF as consent is how a confirmation prompt
/// becomes decoration in a script.
fn confirmed(question: &str) -> bool {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        eprintln!("gem-probe: stdin is not a terminal, so {question:?} cannot be answered");
        return false;
    }
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).is_ok() && answer.trim().eq_ignore_ascii_case("y")
}

/// Where the emitted Rust is kept, so climbing to `builds-bin` later does not
/// pay for codegen again. Under `vendor/`, which is gitignored.
fn rust_store(root: &Path) -> PathBuf {
    root.join("vendor/.probe-rs")
}

/// Keeps one gem's generated Rust, gzipped.
///
/// Compressed because the uncompressed corpus does not fit: generated Rust is
/// large and extremely repetitive, and a store that fills the disk is a store
/// that gets deleted before it is ever used. `.rs.gz` is read back by the
/// build tier and by hand with `gunzip -c`.
fn store_rust(root: &Path, name: &str, version: &str, rust: &[u8]) -> Result<(), String> {
    let dir = rust_store(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = std::fs::File::create(dir.join(format!("{name}-{version}.rs.gz")))
        .map_err(|e| e.to_string())?;
    let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    gz.write_all(rust).map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Runs a probed gem's binary with the OS holding the leash.
///
/// This is the only rung that executes code from rubygems, so confinement is
/// not left to the flag logic that guards it: the binary gets no network and
/// can write nowhere but a scratch directory. `sandbox-exec` is deprecated and
/// still the only thing on macOS that does this without a helper.
#[cfg(target_os = "macos")]
fn run_sandboxed(bin: &Path, timeout: Duration) -> Result<crate::exec::Execution, String> {
    let scratch = std::env::temp_dir().join("zeo-gem-probe-run");
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let profile = format!(
        "(version 1)\
         (deny default)\
         (allow process-exec process-fork)\
         (allow file-read*)\
         (allow sysctl-read mach-lookup signal)\
         (deny network*)\
         (allow file-write* (subpath {:?}))\
         (allow file-write-data (literal \"/dev/null\") (literal \"/dev/stdout\") \
         (literal \"/dev/stderr\"))",
        scratch.to_string_lossy()
    );
    let mut cmd = std::process::Command::new("sandbox-exec");
    cmd.arg("-p").arg(profile).arg(bin).current_dir(&scratch);
    crate::exec::run_with_timeout(cmd, None, timeout)
}

/// No sandbox, no run. The gate refuses before reaching here, and this keeps
/// that true if a future edit lets it through.
#[cfg(not(target_os = "macos"))]
fn run_sandboxed(_bin: &Path, _timeout: Duration) -> Result<crate::exec::Execution, String> {
    Err("no sandbox on this platform; --run is macOS-only".into())
}

/// The diagnostic reaches the ledger whole.
///
/// The cap used to be 160 characters, which cut real messages mid-path -- the
/// gem name, the require_paths entry and the directory it was looked for under
/// all sat past it, so the row said less than the compiler did and `--matching`
/// could not select on the part that was missing. The limit that remains is a
/// runaway guard for a message no diagnostic should produce, not a display
/// width: nothing zeo emits comes close to it.
fn truncate(s: &str) -> String {
    const LIMIT: usize = 2000;
    match s.char_indices().nth(LIMIT) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => s.to_string(),
    }
}

// ----------------------------------------------------------------- ledger

/// Gems the sweep declines to probe at all, each with the reason, read from
/// `conformance/gem-probe-ignored.tsv`.
///
/// An ignored gem is not a verdict -- it is a statement that no verdict is
/// worth measuring, so it carries a reason and never reaches the ledger. The
/// case that named this: `Cartesian` is obsolete, renamed to `cartesian`, and
/// depends on the gem that replaced it. On a case-insensitive filesystem the
/// two cannot both sit in one load-path view, so the probe can only ever
/// report a `view-failed` that says nothing about zeo.
///
/// Deliberately NOT a way to hide a failure: anything here is a fact about the
/// gem or the platform, never about the compiler. A row whose reason is "zeo
/// cannot compile it" belongs in the ledger, where it counts against us.
fn read_ignored(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(root.join("conformance/gem-probe-ignored.tsv")) else {
        return out;
    };
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("gem\t"))
    {
        let mut f = line.split('\t');
        if let (Some(gem), Some(reason)) = (f.next(), f.next()) {
            out.insert(gem.to_string(), reason.to_string());
        }
    }
    out
}

/// The ledger is ONE file: one row per gem, whatever the verdict.
///
/// It was two, split on whether the outcome was `ok`. Both carried the same
/// nine columns and one parser and one writer served both, so the split was a
/// filter frozen into the filesystem -- and it cost more than it gave:
///
///   - the `stage` column read as a claim. A failing row saying `emits-rs`
///     looks like it emitted Rust, because a reader in a file called `fails`
///     infers pass/fail from the FILE and reads the stage on its own. Beside
///     `outcome` it is unambiguous: `emits-rs ok` against `emits-rs
///     lowering-gap` is the rung, then what happened at it;
///   - a fix showed up as a deletion in one file and an insertion in another,
///     so a review could not see the two halves as one moved row;
///   - a gem could appear in both, which is a corruption `read_ledger` still
///     has to refuse.
///
/// `gem-probe.md` beside it carries the counts, since `wc -l` no longer
/// answers "how many compile".
fn ledger_path(root: &Path) -> PathBuf {
    root.join("conformance/gem-probe.tsv")
}

/// Every path a row may be read FROM: the merged file, plus the two it
/// replaced so an older checkout, a branch, or a half-finished migration still
/// parses. Only [`ledger_path`] is ever written.
fn ledger_read_paths(root: &Path) -> [PathBuf; 3] {
    [
        ledger_path(root),
        root.join("conformance/gem-probe-compiles.tsv"),
        root.join("conformance/gem-probe-fails.tsv"),
    ]
}

/// Reads every ledger file into one map.
///
/// A gem in two of them is a corruption -- a hand-edit, or a merge that kept
/// both sides of a moved row -- and the copies disagree about the verdict.
/// There is no safe way to pick one, so this refuses rather than guesses.
fn read_ledger(root: &Path) -> Result<BTreeMap<String, Row>, String> {
    let mut out: BTreeMap<String, Row> = BTreeMap::new();
    for path in ledger_read_paths(root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // `#` lines are the pre-header shape, kept readable so a checkout that
        // predates the header row still parses. `gem\t...` is the header row
        // itself, and no gem is named `gem` at version `version`.
        for line in text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .filter(|l| !l.starts_with("gem\tversion\t"))
        {
            let mut f = line.split('\t');
            let (Some(name), Some(version), Some(third)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            // Column 3 tells the two shapes apart without a version stamp: it
            // is a stage tag in the current format and an outcome tag in every
            // older one, and the two vocabularies do not overlap. Four columns
            // is the pre-`site` shape and six the pre-`stage` shape; both are
            // read as rows that simply never recorded the later fields, rather
            // than invalidating the whole ledger.
            let num = |s: Option<&str>| s.filter(|v| !v.is_empty()).and_then(|v| v.parse().ok());
            let text = |s: Option<&str>| s.filter(|v| !v.is_empty()).map(str::to_string);
            let row = match Stage::from_tag(third) {
                Some(stage) => {
                    let tag = f.next().unwrap_or("");
                    let rust_bytes = num(f.next());
                    let binary_bytes = num(f.next());
                    let outcome = Outcome::from_ledger(tag, &tsv_unfield(f.next().unwrap_or("")));
                    Row {
                        version: version.to_string(),
                        stage,
                        outcome,
                        rust_bytes,
                        binary_bytes,
                        site: text(f.next()),
                        digest: text(f.next()),
                    }
                }
                None => {
                    let outcome = Outcome::from_ledger(third, &tsv_unfield(f.next().unwrap_or("")));
                    Row {
                        version: version.to_string(),
                        stage: outcome.implied_stage(),
                        outcome,
                        rust_bytes: None,
                        binary_bytes: None,
                        site: text(f.next()),
                        digest: text(f.next()),
                    }
                }
            };
            if out.insert(name.to_string(), row).is_some() {
                return Err(format!(
                    "{name} is recorded twice (second copy in {}); each gem belongs \
                     to exactly one ledger -- delete the wrong row, then re-probe it",
                    path.display()
                ));
            }
        }
    }
    Ok(out)
}

/// One field, quoted when it has to be.
///
/// A compiler diagnostic quotes the source it rejected (`unsupported syntax at
/// "..."`), so a detail routinely carries `"` -- and a tab-separated file is
/// still read by a CSV parser, which sees an unbalanced quote and gives up on
/// the whole file. RFC 4180's rule: wrap the field and double the quotes
/// inside it.
/// A tab or a newline inside a field would end the column or the row, and
/// `read_ledger` is line-based, so quoting cannot rescue either -- they become
/// spaces. `message_of` already collapses whitespace, but a `fetch-failed`
/// detail is an io error that never went through it.
fn tsv_field(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_control() || c == '\t' { ' ' } else { c })
        .collect();
    if s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// [`tsv_field`]'s inverse.
fn tsv_unfield(s: &str) -> String {
    match s.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
        Some(inner) => inner.replace("\"\"", "\""),
        None => s.to_string(),
    }
}

/// How long each rung took, in a file nobody commits.
///
/// Wall-clock timing is not reproducible, and the ledger is committed and read
/// as a diff: putting it there would rewrite ~2000 values on every sweep and
/// make a real change impossible to see. It is appended to rather than
/// rewritten so a scoped re-probe adds to the history instead of erasing the
/// rows it did not measure.
fn timings_path(root: &Path) -> PathBuf {
    root.join("conformance/gem-probe-timings.tsv")
}

fn write_timings(
    root: &Path,
    rows: &BTreeMap<String, Row>,
    timings: &BTreeMap<String, Verdict>,
) -> Result<(), String> {
    if timings.is_empty() {
        return Ok(());
    }
    let path = timings_path(root);
    let mut out = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => String::from(
            "gem\tversion\tstage\tcodegen_ms\tbuild_ms\trun_ms\trust_bytes\tbinary_bytes\n",
        ),
    };
    let ms = |v: Option<u128>| v.map(|n| n.to_string()).unwrap_or_default();
    let by = |v: Option<u64>| v.map(|n| n.to_string()).unwrap_or_default();
    for (name, v) in timings {
        let version = rows.get(name).map(|r| r.version.as_str()).unwrap_or("-");
        out.push_str(&format!(
            "{name}\t{version}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            v.stage.tag(),
            ms(v.codegen_ms),
            ms(v.build_ms),
            ms(v.run_ms),
            by(v.rust_bytes),
            by(v.binary_bytes),
        ));
    }
    std::fs::write(&path, out).map_err(|e| e.to_string())
}

fn write_ledger(root: &Path, rows: &BTreeMap<String, Row>) -> Result<(), String> {
    // An ignored gem has no verdict, so it has no row -- adding a name to
    // `gem-probe-ignored.tsv` prunes it here on the next run rather than
    // needing the ledger hand-edited.
    let ignored = read_ignored(root);
    let rows: BTreeMap<String, Row> = rows
        .iter()
        .filter(|(name, _)| !ignored.contains_key(*name))
        .map(|(n, r)| (n.clone(), r.clone()))
        .collect();
    let rows = &rows;
    let compiles = || rows.iter().filter(|(_, r)| r.outcome == Outcome::Ok);
    let fails = || rows.iter().filter(|(_, r)| r.outcome != Outcome::Ok);

    // A real header row, and every row the same width. The prose that used to
    // sit here in `#` comments now lives only in `gem-probe.md`: a TSV viewer
    // takes line 1 as the header, so a comment there made every data row read
    // as malformed and GitHub refused to render the file at all. Trailing
    // empty fields are written rather than dropped for the same reason.
    let render = |selected: &mut dyn Iterator<Item = (&String, &Row)>| {
        let mut tsv = String::from(
            "gem\tversion\tstage\toutcome\trust_bytes\tbinary_bytes\tdetail\twhere\tsha256\n",
        );
        for (name, r) in selected {
            let num = |v: Option<u64>| v.map(|n| n.to_string()).unwrap_or_default();
            let (rust_bytes, binary_bytes) = (num(r.rust_bytes), num(r.binary_bytes));
            let fields = [
                name.as_str(),
                r.version.as_str(),
                r.stage.tag(),
                r.outcome.tag(),
                rust_bytes.as_str(),
                binary_bytes.as_str(),
                r.outcome.detail(),
                r.site.as_deref().unwrap_or(""),
                r.digest.as_deref().unwrap_or(""),
            ];
            let row: Vec<String> = fields.iter().map(|f| tsv_field(f)).collect();
            tsv.push_str(&row.join("\t"));
            tsv.push('\n');
        }
        tsv
    };

    std::fs::write(ledger_path(root), render(&mut rows.iter())).map_err(|e| e.to_string())?;
    // The two files this replaced go, rather than being left to rot beside it
    // holding a copy of every row -- `read_ledger` would then refuse the next
    // run for exactly the duplication this removed.
    for old in ["gem-probe-compiles.tsv", "gem-probe-fails.tsv"] {
        let _ = std::fs::remove_file(root.join("conformance").join(old));
    }

    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for r in rows.values() {
        *counts.entry((r.stage.tag(), r.outcome.tag())).or_default() += 1;
    }
    let mut md = String::from("# Gem probe results\n\nGenerated by `cargo xtask gem-probe`.\n\n");
    md.push_str(
        "`gem-probe.tsv` beside this file holds ONE row per gem. Two columns carry the \
         verdict and they must be read together: `stage` is the RUNG the row is about, and \
         `outcome` is what happened there. `emits-rs ok` and `emits-rs lowering-gap` are the \
         same rung with opposite results -- the stage alone claims nothing.\n\n\
         **`emits-rs ok` means zeo produced Rust, and nothing more.** No rustc ran, no \
         binary exists, and the gem's own code may not have been compiled at all -- zeo can \
         decline a unit and defer it to a runtime `LoadError`, which only the `runs` stage \
         sees. `builds-bin` and `runs` are opt-in (`--build`, `--run`) and a sweep does not \
         reach them.\n\n\
         No `ok` row may regress: `gem-probe --check` gates both the outcome and the stage. \
         Of the failures only `lowering-gap`, `compiler-panic` and `rustc-error` are zeo's \
         to fix -- the rest are facts about the gem or the harness. The `where` column \
         points at the Ruby line the compiler rejected, relative to the repository root, \
         and `sha256` pins the `.gem` the row was measured against. \
         `rust_bytes`/`binary_bytes` are reproducible and so are committed; wall-clock \
         timings are not, and go to the gitignored `gem-probe-timings.tsv` instead.\n\n\
         `gem-probe-ignored.tsv` lists gems the sweep declines to probe, each with a \
         reason. Those are facts about the gem or the platform, never about zeo -- a gem \
         zeo cannot compile belongs in the ledger, where it counts against us.\n\n\
         A `queued unprobed` row is the FRONTIER: a gem the registry names that has never \
         been measured. It carries a name and nothing else, so it churns only when \
         rubygems gains a gem, and it keeps every ratio here honest -- the denominators \
         below count gems with a verdict, never the frontier.\n\n",
    );
    let measured = rows
        .values()
        .filter(|r| r.outcome != Outcome::Unprobed)
        .count();
    md.push_str(&format!(
        "{measured} gems probed, {} of {} names known.\n\n",
        measured,
        rows.len()
    ));
    md.push_str("| Stage | Outcome | Gems |\n|---|---|---|\n");
    for ((stage, tag), n) in &counts {
        md.push_str(&format!("| {stage} | {tag} | {n} |\n"));
    }
    let mut table = |title: &str, selected: &mut dyn Iterator<Item = (&String, &Row)>| {
        md.push_str(&format!(
            "\n## {title}\n\n| Gem | Version | Stage | Outcome | Rust bytes | Detail |\
             \n|---|---|---|---|---|---|\n"
        ));
        for (name, r) in selected {
            md.push_str(&format!(
                "| {name} | {} | {} | {} | {} | {} |\n",
                r.version,
                r.stage.tag(),
                r.outcome.tag(),
                r.rust_bytes.map(|n| n.to_string()).unwrap_or_default(),
                r.outcome.detail().replace('|', "\\|")
            ));
        }
    };
    table("Reached its stage", &mut compiles());
    table("Did not", &mut fails());
    std::fs::write(root.join("conformance/gem-probe.md"), md).map_err(|e| e.to_string())
}

const NAMES_URL: &str = "https://index.rubygems.org/names";

/// The cached copy of the registry's name index. Gitignored: it is ~3MB of
/// upstream data that changes daily, and the repository already refuses
/// tracked files that size.
fn names_cache(root: &Path) -> PathBuf {
    root.join("conformance/rubygems-names.txt")
}

/// Every gem name the registry knows, from the compact index.
///
/// Cached on first use, because a sweep of ~200k names is worked through in
/// slices across many runs and re-downloading the list each time is pointless.
/// `--refresh-index` takes a newer copy.
fn registry_names(root: &Path, refresh: bool) -> Result<Vec<String>, String> {
    let cache = names_cache(root);
    let text = match std::fs::read_to_string(&cache) {
        Ok(t) if !refresh && !t.is_empty() => t,
        _ => {
            let body = get(NAMES_URL)?;
            if let Some(parent) = cache.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&cache, &body).map_err(|e| e.to_string())?;
            String::from_utf8(body).map_err(|e| e.to_string())?
        }
    };
    Ok(text
        .lines()
        .map(str::trim)
        // The index opens with a `---` header line.
        .filter(|l| !l.is_empty() && *l != "---")
        .map(String::from)
        .collect())
}

/// How many results one page of the search API carries.
const POPULAR_PAGE: usize = 30;

fn popular_cache(root: &Path) -> PathBuf {
    root.join("conformance/rubygems-popular.txt")
}

/// The most-downloaded gems, in rank order.
///
/// The registry has no top-N endpoint -- `/api/v1/downloads/top.json` is gone,
/// and `rubygems.org/stats` stops at 100 -- while the compact index is
/// alphabetical, so a sweep of it meets popular gems only by chance. The
/// search API answers a `*` query with every gem sorted by download count,
/// which is the ranking neither of the others gives.
///
/// The cache is a prefix of that ranking, so asking for more extends it
/// instead of re-fetching what is already known.
fn popular_names(root: &Path, want: usize, refresh: bool) -> Result<Vec<String>, String> {
    let cache = popular_cache(root);
    let mut names: Vec<String> = match std::fs::read_to_string(&cache) {
        Ok(t) if !refresh => t
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    };

    while names.len() < want {
        // Resume on a page boundary: a half-page tail would misalign every
        // page number after it.
        names.truncate(names.len() / POPULAR_PAGE * POPULAR_PAGE);
        let page = names.len() / POPULAR_PAGE + 1;
        let body = json(&format!(
            "{REGISTRY}/api/v1/search.json?query=*&page={page}"
        ))?;
        let batch: Vec<String> = body
            .as_array()
            .map(|gems| {
                gems.iter()
                    .filter_map(|g| g.get("name")?.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let exhausted = batch.len() < POPULAR_PAGE;
        names.extend(batch);
        if exhausted {
            break;
        }
    }

    let mut seen = std::collections::BTreeSet::new();
    names.retain(|n| seen.insert(n.clone()));
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&cache, names.join("\n") + "\n").map_err(|e| e.to_string())?;
    names.truncate(want);
    Ok(names)
}

fn corpus_path(root: &Path) -> PathBuf {
    root.join("conformance/gem-probe-corpus.txt")
}

/// `<name>` or `<name> <version>`, one per line, `#` comments ignored.
fn read_corpus(root: &Path) -> Vec<(String, Option<String>)> {
    let Ok(text) = std::fs::read_to_string(corpus_path(root)) else {
        return Vec::new();
    };
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut f = l.split_whitespace();
            (
                f.next().unwrap_or_default().to_string(),
                f.next().map(String::from),
            )
        })
        .collect()
}

// -------------------------------------------------------------------- run

/// A gem resolved and unpacked on disk, ready for the compile step.
struct Ready {
    name: String,
    version: String,
    dir: PathBuf,
    deps: Vec<String>,
    digest: Option<String>,
}

/// Resolves `name` to a version and unpacks it and its runtime dependencies.
///
/// This is the half that touches the network and writes to the shared
/// `vendor/gems` cache, so it stays on one thread. Two gems routinely share a
/// dependency, and letting two workers unpack the same one into the same
/// directory would corrupt it.
///
/// `Err` is a row that already knows its verdict and needs no compile.
fn prepare(root: &Path, name: &str, want: Option<&str>, no_deps: bool) -> Result<Ready, Box<Row>> {
    let version = want
        .map(String::from)
        .map_or_else(|| latest_version(name), Ok)
        .map_err(|e| {
            Box::new(Row::stopped(
                want.unwrap_or("-"),
                Stage::Fetch,
                Outcome::FetchFailed(e),
                None,
            ))
        })?;

    // One request answers both questions. `--no-deps` skips it, so those rows
    // record no digest -- the flag's whole purpose is to not ask the registry.
    let meta = match no_deps {
        true => None,
        false => version_meta(name, &version).ok(),
    };
    let mut deps = Vec::new();
    for dep in meta.as_ref().map(|m| m.runtime.clone()).unwrap_or_default() {
        // A dependency is a load-path entry, not a subject: it is never
        // recorded, so its digest is never asked for.
        if let Ok(dv) = latest_version(&dep)
            && fetch(root, &dep, &dv, None).is_ok()
        {
            deps.push(dep);
        }
    }

    let digest = meta.and_then(|m| m.sha);
    match fetch(root, name, &version, digest.as_deref()) {
        Ok(dir) => Ok(Ready {
            name: name.to_string(),
            version,
            dir,
            deps,
            digest,
        }),
        Err(e) => Err(Box::new(Row::stopped(
            &version,
            Stage::Fetch,
            Outcome::FetchFailed(e),
            digest,
        ))),
    }
}

fn report(name: &str, row: &Row) {
    let detail = row.outcome.detail();
    // The stage rides on every line, including the successes. A sweep's
    // scrolling output is where "compiles" got read as more than it was.
    println!(
        "  {name} {}: {} {}{}",
        row.version,
        row.stage.tag(),
        row.outcome.tag(),
        if detail.is_empty() {
            String::new()
        } else {
            format!(" -- {detail}")
        }
    );
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let mut names: Vec<(String, Option<String>)> = Vec::new();
    let (mut corpus, mut all, mut check, mut no_deps) = (false, false, false, false);
    let (mut index, mut refresh, mut refresh_index) = (false, false, false);
    let (mut limit, mut popular): (Option<usize>, Option<usize>) = (None, None);
    let mut matching: Option<String> = None;
    let mut matching_name: Option<String> = None;
    let mut failing = false;
    let (mut unprobed, mut seed_index) = (false, false);
    let mut positional: Vec<String> = Vec::new();
    let mut jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut timeout = DEFAULT_TIMEOUT;
    let (mut build, mut run, mut allow_run) = (false, false, false);

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "--corpus" => corpus = true,
            "--all" => all = true,
            "--check" => check = true,
            "--no-deps" => no_deps = true,
            "--index" => index = true,
            "--unprobed" => unprobed = true,
            "--seed-index" => seed_index = true,
            "--refresh" => refresh = true,
            "--refresh-index" => refresh_index = true,
            "--limit" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => limit = Some(n),
                None => {
                    eprintln!("gem-probe: --limit needs a number");
                    return ExitCode::FAILURE;
                }
            },
            "--failing" => failing = true,
            "--build" => build = true,
            "--run" => run = true,
            "--allow-running-untrusted-gem-code" => allow_run = true,
            "--jobs" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) if n >= 1 => jobs = n,
                _ => {
                    eprintln!("gem-probe: --jobs needs a number of 1 or more");
                    return ExitCode::FAILURE;
                }
            },
            "--timeout" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) if n >= 1 => timeout = Duration::from_secs(n),
                _ => {
                    eprintln!("gem-probe: --timeout needs a number of seconds, 1 or more");
                    return ExitCode::FAILURE;
                }
            },
            "--matching" => match it.next() {
                Some(v) => matching = Some(v.clone()),
                None => {
                    eprintln!("gem-probe: --matching needs a substring of the outcome detail");
                    return ExitCode::FAILURE;
                }
            },
            "--matching-name" => match it.next() {
                Some(v) => matching_name = Some(v.clone()),
                None => {
                    eprintln!("gem-probe: --matching-name needs a substring of the gem name");
                    return ExitCode::FAILURE;
                }
            },
            "--popular" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => popular = Some(n),
                None => {
                    eprintln!("gem-probe: --popular needs a number");
                    return ExitCode::FAILURE;
                }
            },
            other if other.starts_with("--") => {
                eprintln!("gem-probe: unknown option {other:?}");
                return ExitCode::FAILURE;
            }
            other => positional.push(other.to_string()),
        }
    }
    match positional.len() {
        0 => {}
        1 => names.push((positional[0].clone(), None)),
        2 => names.push((positional[0].clone(), Some(positional[1].clone()))),
        _ => {
            eprintln!("usage: gem-probe [<name> [version]] [--corpus] [--all] [--check]");
            return ExitCode::FAILURE;
        }
    }

    // `--run` executes code downloaded from rubygems. Nothing about a sweep
    // should be able to reach it by accident, so it is fenced four ways before
    // any network call happens: a second explicit flag, named gems only, a
    // platform that can confine the process, and a prompt. Bulk selectors are
    // refused outright rather than confirmed -- a `y` covering 194k gems is not
    // a decision anyone can make, and that is exactly the typo worth stopping.
    if run {
        build = true;
        let bulk = [
            ("--corpus", corpus),
            ("--all", all),
            ("--index", index),
            ("--unprobed", unprobed),
            ("--failing", failing),
            ("--matching", matching.is_some()),
            ("--matching-name", matching_name.is_some()),
            ("--popular", popular.is_some()),
        ];
        if let Err(why) = run_is_permitted(names.len(), &bulk, allow_run, cfg!(target_os = "macos"))
        {
            eprintln!("gem-probe: {why}");
            return ExitCode::FAILURE;
        }
        println!("gem-probe: --run will BUILD and EXECUTE:");
        for (name, want) in &names {
            println!("  {name} {}", want.as_deref().unwrap_or("(latest)"));
        }
        println!(
            "  sandboxed: no network, no writes outside a scratch directory.\n\
             Their dependencies are compiled in and run too."
        );
        if !confirmed("proceed?") {
            eprintln!("gem-probe: not confirmed; nothing was run");
            return ExitCode::FAILURE;
        }
    }
    let tiers = Tiers { build, run };

    let before = match read_ledger(root) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("gem-probe: reading the ledger: {e}");
            return ExitCode::FAILURE;
        }
    };
    if corpus {
        names.extend(read_corpus(root));
    }
    if let Some(n) = popular {
        match popular_names(root, n, refresh_index) {
            Ok(ranked) => names.extend(ranked.into_iter().map(|n| (n, None))),
            Err(e) => {
                eprintln!("gem-probe: reading the download ranking: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    if index {
        match registry_names(root, refresh_index) {
            Ok(all_names) => names.extend(all_names.into_iter().map(|n| (n, None))),
            Err(e) => {
                eprintln!("gem-probe: reading the registry index: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    // Re-probe exactly the gems a fix targets. A fix lands against one
    // diagnostic, so re-running the whole ledger to see whether it worked
    // spends hours to learn about a few dozen rows.
    // Every row that is not `compiles`. After a batch of fixes this is the
    // whole question -- which of them moved -- without re-probing the gems
    // already known to compile.
    if failing {
        names.extend(
            before
                .iter()
                // `unprobed` is not a failure -- it is the frontier, and
                // sweeping it here would turn "re-check what a fix moved" into
                // a run over every gem rubygems has ever published.
                .filter(|(_, r)| r.outcome != Outcome::Ok && r.outcome != Outcome::Unprobed)
                .map(|(n, r)| (n.clone(), Some(r.version.clone()))),
        );
    }
    // The frontier itself, without needing the index cache beside it: the
    // ledger already names every gem waiting for a first verdict.
    if unprobed {
        names.extend(
            before
                .iter()
                .filter(|(_, r)| r.outcome == Outcome::Unprobed)
                .map(|(n, _)| (n.clone(), None)),
        );
    }
    if let Some(pat) = &matching {
        names.extend(
            before
                .iter()
                .filter(|(_, r)| r.outcome.detail().contains(pat.as_str()))
                .map(|(n, r)| (n.clone(), Some(r.version.clone()))),
        );
    }
    // The name twin of `--matching`. A fix usually lands against one
    // diagnostic, but a stale BAND is named by its gems rather than by what it
    // says -- the aws-sdk rows all report a diagnostic their own dependency no
    // longer produces, so no detail substring selects them.
    if let Some(pat) = &matching_name {
        names.extend(
            before
                .iter()
                .filter(|(n, _)| n.contains(pat.as_str()))
                .map(|(n, r)| (n.clone(), Some(r.version.clone()))),
        );
    }
    if all {
        names.extend(
            before
                .iter()
                // Every gem with a VERDICT. The frontier is `--unprobed`,
                // which is a different and much larger job.
                .filter(|(_, r)| r.outcome != Outcome::Unprobed)
                .map(|(n, r)| (n.clone(), Some(r.version.clone()))),
        );
    }
    // Seeding records the FRONTIER: a row per gem the registry names and the
    // ledger has never measured, so "how much of rubygems have we probed" is a
    // question the committed file answers rather than one that needs the
    // gitignored index cache beside it.
    //
    // The row is a name and nothing else -- no version, no digest, no detail.
    // Versions move daily and a version here would churn the file for gems
    // nobody has looked at; a bare name is stable until the gem is probed, so
    // a diff shows exactly what rubygems gained.
    if seed_index {
        match registry_names(root, refresh_index) {
            Ok(all_names) => {
                let mut rows = match read_ledger(root) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("gem-probe: reading the ledger: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                let fresh: Vec<String> = all_names
                    .into_iter()
                    .filter(|n| !rows.contains_key(n))
                    .collect();
                let added = fresh.len();
                for n in fresh {
                    rows.insert(n, Row::stopped("", Stage::Queued, Outcome::Unprobed, None));
                }
                if let Err(e) = write_ledger(root, &rows) {
                    eprintln!("gem-probe: writing the ledger: {e}");
                    return ExitCode::FAILURE;
                }
                let waiting = rows
                    .values()
                    .filter(|r| r.outcome == Outcome::Unprobed)
                    .count();
                println!(
                    "gem-probe: seeded {added} name(s); {waiting} of {} rows have no verdict yet",
                    rows.len()
                );
                if names.is_empty() {
                    return ExitCode::SUCCESS;
                }
            }
            Err(e) => {
                eprintln!("gem-probe: reading the registry index: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    if names.is_empty() {
        eprintln!(
            "gem-probe: nothing to probe (give a gem name, --corpus, --popular N, --failing, --unprobed, --matching <detail>, --matching-name <name>, --index, --seed-index or --all)"
        );
        return ExitCode::FAILURE;
    }

    // Ignored gems drop out of EVERY selection, including a name given
    // explicitly -- the reason is the answer to why it was asked for. Printed
    // rather than silently dropped, so a sweep never shrinks without saying so.
    let ignored = read_ignored(root);
    if !ignored.is_empty() {
        let before_len = names.len();
        names.retain(|(name, _)| !ignored.contains_key(name));
        if names.len() != before_len {
            for (gem, reason) in &ignored {
                println!("gem-probe: ignoring {gem} -- {reason}");
            }
        }
    }

    // Resume. A sweep of the registry is hours of work, so re-running must
    // continue rather than start over. `--all` is the explicit re-probe, and
    // `--refresh` forces it for any selection.
    let skipped = if all || refresh || failing || matching.is_some() || matching_name.is_some() {
        0
    } else {
        let n = names.len();
        // A row that is only the frontier does NOT count as already probed --
        // resume must still reach it, or seeding the index would make every
        // unprobed gem permanently invisible to a plain sweep.
        names.retain(|(name, _)| {
            !before
                .get(name)
                .is_some_and(|r| r.outcome != Outcome::Unprobed)
        });
        n - names.len()
    };
    if let Some(n) = limit {
        names.truncate(n);
    }
    if names.is_empty() {
        println!("gem-probe: nothing left to probe ({skipped} already in the ledger)");
        return ExitCode::SUCCESS;
    }

    if let Err(e) = std::fs::create_dir_all(vendor_dir(root)) {
        eprintln!("gem-probe: {e}");
        return ExitCode::FAILURE;
    }

    // The classifier is the `zeo` BINARY (see `probe`). Build it once up front
    // so the workers below don't race each other into cargo.
    eprintln!("gem-probe: building zeo...");
    let built = std::process::Command::new("cargo")
        .args(["build", "--quiet", "-p", "zeo"])
        .current_dir(root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !built {
        eprintln!("gem-probe: `cargo build -p zeo` failed");
        return ExitCode::FAILURE;
    }
    let zeo = root.join("target").join("debug").join("zeo");

    let mut rows = before.clone();
    if skipped > 0 {
        println!("{skipped} gem(s) already recorded; --refresh re-probes them");
    }
    let total = names.len();
    println!("probing {total} gem(s) with {jobs} job(s):");

    // Three roles, so the network and the compiles overlap without ever
    // sharing a writer:
    //
    //   prep thread  -- resolve + fetch, sequentially (see `prepare`)
    //   N workers    -- one `zeo` subprocess each, the long pole
    //   this thread  -- receives verdicts, inserts, writes the ledger
    //
    // Keeping every `insert` and `write_ledger` here preserves the invariant a
    // sequential sweep had: the ledger on disk is always the truth so far, so
    // an interrupted run keeps its work. Results arrive out of order, but
    // `rows` is a `BTreeMap` that `write_ledger` emits whole, so the file stays
    // byte-identical to what a sequential run would have produced.
    let (work_tx, work_rx) = std::sync::mpsc::channel::<Ready>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<(String, Row, Option<Verdict>)>();
    let work_rx = std::sync::Mutex::new(work_rx);
    let mut timings: BTreeMap<String, Verdict> = BTreeMap::new();

    std::thread::scope(|scope| {
        let done_for_prep = done_tx.clone();
        scope.spawn(move || {
            for (name, want) in &names {
                match prepare(root, name, want.as_deref(), no_deps) {
                    Ok(ready) => {
                        if work_tx.send(ready).is_err() {
                            break;
                        }
                    }
                    // Nothing to compile -- straight to the recorder.
                    Err(row) => {
                        if done_for_prep.send((name.clone(), *row, None)).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        for _ in 0..jobs {
            let done = done_tx.clone();
            let work_rx = &work_rx;
            let zeo = &zeo;
            scope.spawn(move || {
                loop {
                    // The lock is released before the compile, so the workers
                    // only serialize on taking the next item.
                    let Ok(ready) = work_rx.lock().unwrap().recv() else {
                        break;
                    };
                    let verdict = probe(root, zeo, &ready, timeout, tiers);
                    let row = Row::from_verdict(ready.version, ready.digest, &verdict);
                    if done.send((ready.name, row, Some(verdict))).is_err() {
                        break;
                    }
                }
            });
        }
        // Every live sender is now owned by a spawned thread; this one has to
        // go or the drain below never sees the channel close.
        drop(done_tx);

        let mut seen = 0usize;
        while let Ok((name, row, verdict)) = done_rx.recv() {
            seen += 1;
            print!("[{seen}/{total}]");
            report(&name, &row);
            if let Some(v) = verdict {
                timings.insert(name.clone(), v);
            }
            rows.insert(name, row);
            if let Err(e) = write_ledger(root, &rows) {
                eprintln!("gem-probe: writing the ledger: {e}");
            }
        }
    });
    if let Err(e) = write_timings(root, &rows, &timings) {
        eprintln!("gem-probe: writing the timings: {e}");
    }

    if let Err(e) = write_ledger(root, &rows) {
        eprintln!("gem-probe: writing the ledger: {e}");
        return ExitCode::FAILURE;
    }

    let reached = |stage: Stage| {
        rows.values()
            .filter(|r| r.outcome == Outcome::Ok && r.stage >= stage)
            .count()
    };
    // Named by the rung, not by "compile": the whole point of the stage column
    // is that this number is about emitting Rust and nothing further.
    //
    // The denominator is gems with a VERDICT. Seeding puts the frontier in the
    // same file, and counting those as probed would silently deflate every
    // ratio the ledger is read for.
    let measured = rows
        .values()
        .filter(|r| r.outcome != Outcome::Unprobed)
        .count();
    let waiting = rows.len() - measured;
    println!(
        "gem-probe: {}/{measured} probed gems emit Rust\n  {}",
        reached(Stage::EmitsRs),
        ledger_path(root).display()
    );
    if waiting > 0 {
        println!("           {waiting} named gem(s) still have no verdict");
    }
    if tiers.build {
        println!(
            "           {} of those build a binary",
            reached(Stage::BuildsBin)
        );
    }
    if tiers.run {
        println!("           {} of those run", reached(Stage::Runs));
    }

    // A gem that reached a rung and no longer does is a regression, whatever
    // the new outcome is -- and so is one that reached a LOWER rung than
    // before, which the outcome alone cannot see. Only `--check` fails on it,
    // so an exploratory probe of a fresh gem never breaks a build.
    if check {
        let lost: Vec<(&String, String)> = before
            .iter()
            .filter_map(|(n, old)| {
                let new = rows.get(n)?;
                let was_ok = old.outcome == Outcome::Ok;
                let regressed = (was_ok && new.outcome != Outcome::Ok)
                    || (was_ok && new.outcome == Outcome::Ok && new.stage < old.stage);
                regressed.then(|| {
                    (
                        n,
                        format!(
                            "{} {} -> {} {}",
                            old.stage.tag(),
                            old.outcome.tag(),
                            new.stage.tag(),
                            new.outcome.tag()
                        ),
                    )
                })
            })
            .collect();
        if !lost.is_empty() {
            eprintln!("gem-probe: {} gem(s) regressed:", lost.len());
            for (n, what) in lost {
                eprintln!("  {n}: {what}");
            }
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch root per test. `std::env::temp_dir` rather than a dev
    /// dependency, since xtask has none and needs none for this.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zeo-gem-probe-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("conformance")).unwrap();
        dir
    }

    fn gem_at(root: &Path, name: &str, version: &str, body: &str) -> PathBuf {
        let dir = vendor_dir(root).join(name);
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("lib").join(format!("{name}.rb")), body).unwrap();
        // What rubygems shipped, NOT our stub: a stub in the cache is the
        // broken state `cached_gemspec_is_a_stub` exists to re-fetch out of,
        // so writing one here would make every cached-gem test miss.
        std::fs::write(
            dir.join(format!("{name}.gemspec")),
            format!(
                "Gem::Specification.new do |s|\n  s.name = {name:?}.freeze\n  \
                 s.version = {version:?}.freeze\n  s.summary = \"a test gem\".freeze\n  \
                 s.require_paths = [\"lib\".freeze]\nend\n"
            ),
        )
        .unwrap();
        std::fs::write(stamp_path(&dir), version).unwrap();
        dir
    }

    // ---------------------------------------------------------- diagnostics

    #[test]
    fn the_require_chain_is_stripped_from_a_message() {
        let err = "/a/b/lib/x.rb: /a/b/lib/y.rb: unsupported statement in `class << self`";
        assert_eq!(message_of(err), "unsupported statement in `class << self`");
    }

    #[test]
    fn a_wrapped_boxed_diagnostic_is_rejoined() {
        let err = "zeo::lower\n\n  × first part of the\n  │ message continues here\n    ╭─[x.rb:1:1]\n 1 │ code";
        assert_eq!(message_of(err), "first part of the message continues here");
    }

    /// The ledger is committed, so no local path may survive into it -- not as
    /// a leading prefix, and not buried inside a span tuple.
    #[test]
    fn no_absolute_path_reaches_the_ledger() {
        let root = Path::new("/Users/someone/dev/zeo");
        let err = "/Users/someone/dev/zeo/vendor/gems/i18n/lib/a.rb: bad thing at Some((\"/Users/someone/dev/zeo/vendor/gems/i18n/lib/a.rb\", 9))";
        let detail = classify(err, root).detail().to_string();
        assert!(!detail.contains("/Users/"), "{detail}");
        assert!(detail.contains("bad thing"), "{detail}");
    }

    /// The two scrubbers do different jobs. A path UNDER the repository root
    /// becomes relative, which stays readable; anything else absolute is
    /// dropped outright as a last resort. Without the first, a useful location
    /// would be deleted rather than shortened.
    #[test]
    fn a_path_under_the_root_is_made_relative_not_deleted() {
        let root = Path::new("/Users/someone/dev/zeo");
        let err = "trouble in /Users/someone/dev/zeo/vendor/gems/i18n/lib/a.rb here";
        let detail = classify(err, root).detail().to_string();
        assert!(
            detail.contains("vendor/gems/i18n/lib/a.rb"),
            "the location should survive, relative: {detail}"
        );
        assert!(!detail.contains("/Users/"), "{detail}");
    }

    #[test]
    fn an_absolute_path_outside_the_root_is_dropped() {
        let root = Path::new("/Users/someone/dev/zeo");
        let detail = classify("trouble in /home/other/thing.rb here", root)
            .detail()
            .to_string();
        assert!(!detail.contains("/home/"), "{detail}");
        assert!(detail.contains("trouble in"), "{detail}");
    }

    /// A panicking gem must not be recorded as a lowering gap: one is a known
    /// limit, the other a bug, and conflating them hides the bug.
    #[test]
    fn a_compiler_panic_is_its_own_outcome() {
        let o = Outcome::CompilerPanic("internal error: boom".into());
        assert_eq!(o.tag(), "compiler-panic");
        assert_eq!(
            Outcome::from_ledger("compiler-panic", "internal error: boom"),
            o
        );
        assert_ne!(o, Outcome::LoweringGap("internal error: boom".into()));
    }

    #[test]
    fn classify_separates_the_outcomes() {
        let root = Path::new("/tmp/none");
        assert_eq!(
            classify("cannot load such file -- public_suffix", root),
            Outcome::MissingDependency("public_suffix".into())
        );
        assert_eq!(
            classify(
                "`msgpack` has a native (C) extension zeo has no built-in for",
                root
            ),
            Outcome::NativeExtension
        );
        // zeo's OTHER native-extension message is worded as a load failure,
        // and it is the one the loader actually emits. Testing only the
        // gem-store wording above is why the arm sat dead: the load-failure
        // prefix matched first and every one of these was filed as a missing
        // dependency on itself.
        assert_eq!(
            classify(
                "cannot load such file -- nokogiri: this gem has a native (C) extension \
                 zeo does not provide a built-in for. See docs/EXTENSIONS.md",
                root
            ),
            Outcome::NativeExtension
        );
        assert!(matches!(
            classify("define_method's second argument must be a block", root),
            Outcome::LoweringGap(_)
        ));
        // A parse error is not a gap: prism IS ruby's parser, so these gems do
        // not load under any current ruby either.
        assert!(matches!(
            classify(
                "parse error: expected a delimiter after the predicates of a `when` clause",
                root
            ),
            Outcome::InvalidRuby(_)
        ));
        // ... but a message that merely MENTIONS parsing still is one.
        assert!(matches!(
            classify(
                "a pattern can't bind a variable inside a `|` alternation",
                root
            ),
            Outcome::LoweringGap(_)
        ));
    }

    /// A seeded row is the frontier, not a verdict, and two things must hold
    /// or seeding makes the sweep worse rather than better: it round-trips as
    /// `Unprobed` at the bottom rung, and it is ordered below every rung a
    /// real probe reaches, so no `stage >= ...` test counts it as progress.
    #[test]
    fn an_unprobed_row_is_the_bottom_rung_and_round_trips() {
        let root = scratch("frontier");
        let mut rows = BTreeMap::new();
        rows.insert(
            "waiting".to_string(),
            Row::stopped("", Stage::Queued, Outcome::Unprobed, None),
        );
        rows.insert(
            "measured".to_string(),
            Row::stopped("1.0.0", Stage::EmitsRs, Outcome::Ok, None),
        );
        write_ledger(&root, &rows).unwrap();

        let back = read_ledger(&root).unwrap();
        assert_eq!(back["waiting"].outcome, Outcome::Unprobed);
        assert_eq!(back["waiting"].stage, Stage::Queued);
        assert_eq!(back["waiting"].version, "");
        assert!(Stage::Queued < Stage::Fetch, "the frontier is the bottom");
        assert!(back["waiting"].stage < Stage::EmitsRs);
        assert_eq!(Outcome::Unprobed.implied_stage(), Stage::Queued);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An ignored gem has no verdict, so it has no row: adding a name to the
    /// ignore file prunes it from the ledger on the next write rather than
    /// needing a hand-edit.
    #[test]
    fn an_ignored_gem_is_read_with_its_reason_and_kept_out_of_the_ledger() {
        let root = scratch("ignored");
        std::fs::write(
            root.join("conformance/gem-probe-ignored.tsv"),
            "gem\treason\nCartesian\tobsolete: renamed to `cartesian`\n",
        )
        .unwrap();
        let ignored = read_ignored(&root);
        assert_eq!(ignored.len(), 1);
        assert!(ignored["Cartesian"].starts_with("obsolete:"));

        let mut rows = BTreeMap::new();
        for name in ["Cartesian", "keeper"] {
            rows.insert(
                name.to_string(),
                Row::stopped("1.0.0", Stage::EmitsRs, Outcome::Ok, None),
            );
        }
        write_ledger(&root, &rows).unwrap();
        let tsv = std::fs::read_to_string(ledger_path(&root)).unwrap();
        assert_eq!(gem_names(&tsv), ["keeper"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A gem may name ITSELF among its runtime dependencies -- jeweler-
    /// generated gemspecs do it routinely, and `Authorizr` is one. Linking a
    /// name twice re-symlinks entries that already exist, which fails with
    /// EEXIST and cost the gem a verdict entirely: 257 corpus rows, all of
    /// them filed as `fetch-failed` though nothing was ever fetched.
    #[test]
    fn a_self_dependency_does_not_break_the_view() {
        let root = scratch("self-dep");
        let src = root.join("vendor/gems/Selfish");
        std::fs::create_dir_all(src.join("lib")).unwrap();
        std::fs::write(src.join("lib/selfish.rb"), "").unwrap();
        std::fs::write(src.join("Selfish.gemspec"), "").unwrap();

        let deps = vec!["Selfish".to_string(), "Selfish".to_string()];
        let view = isolate(&root, "Selfish", &deps).expect("a self-dependency is not an error");
        assert!(view.join("Selfish/lib").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A view that cannot be built is NOT a fetch failure: the source is
    /// already on disk, the registry was never asked, and re-running the fetch
    /// cannot change the answer.
    #[test]
    fn a_view_failure_is_not_a_fetch_failure() {
        assert_eq!(Outcome::ViewFailed("boom".into()).tag(), "view-failed");
        assert_eq!(
            Outcome::ViewFailed("boom".into()).implied_stage(),
            Stage::Unpack
        );
        assert_eq!(
            Outcome::from_ledger("view-failed", "boom"),
            Outcome::ViewFailed("boom".into())
        );
    }

    /// A panic reaches stderr with its message on the line AFTER `panicked at`,
    /// so reading the first line alone would file a compiler bug as a gap --
    /// exactly the distinction the ledger exists to keep.
    #[test]
    fn a_panic_on_stderr_is_not_a_lowering_gap() {
        let root = Path::new("/tmp/none");
        let stderr = b"thread 'main' panicked at crates/zeo/src/lower/mod.rs:12:5:\n\
             value payload root has a `new` constructor\n\
             note: run with `RUST_BACKTRACE=1` ...\n";
        match classify_stderr(stderr, root) {
            Outcome::CompilerPanic(msg) => {
                assert_eq!(msg, "value payload root has a `new` constructor");
            }
            other => panic!("expected a panic, got {other:?}"),
        }
    }

    /// Ordinary rejections still reach `classify` unchanged -- the panic check
    /// must not swallow the common path.
    #[test]
    fn a_rejection_on_stderr_still_classifies_normally() {
        let root = Path::new("/tmp/none");
        assert_eq!(
            classify_stderr(b"zeo: cannot load such file -- rack/test\n", root),
            Outcome::MissingDependency("rack/test".into())
        );
    }

    /// The `where` column is what turns a diagnostic into something you can
    /// open, so the shape of miette's excerpt header is load-bearing here.
    #[test]
    fn the_site_comes_from_the_diagnostic_excerpt() {
        let root = Path::new("/repo");
        let stderr = "  × unsupported syntax at \"...\"\n   \
             ╭─[/repo/vendor/gems/nokogiri/lib/nokogiri/xml.rb:22:20]\n \
             22 │         Reader.new(...)\n";
        assert_eq!(
            site_of(stderr, root).as_deref(),
            Some("vendor/gems/nokogiri/lib/nokogiri/xml.rb:22")
        );
    }

    /// A diagnostic with no excerpt (a pre-parse failure, an error raised
    /// outside any statement) simply has no site -- not a wrong one.
    #[test]
    fn a_diagnostic_without_an_excerpt_has_no_site() {
        assert_eq!(site_of("  × compile failed\n", Path::new("/repo")), None);
    }

    /// A path the root doesn't own would commit one machine's directory
    /// layout, which is the reason `classify` scrubs paths in the first place.
    #[test]
    fn a_site_outside_the_repo_is_dropped() {
        let stderr = "   ╭─[/elsewhere/lib/x.rb:3:1]\n";
        assert_eq!(site_of(stderr, Path::new("/repo")), None);
    }

    /// Every committed row predates the `where` column. Reading one as a row
    /// that never recorded a site keeps the ledger valid across the change.
    #[test]
    fn a_four_column_row_still_reads() {
        let root = scratch("legacy-ledger");
        let fails = root.join("conformance/gem-probe-fails.tsv");
        std::fs::create_dir_all(fails.parent().unwrap()).unwrap();
        std::fs::write(&fails, "# header\nalpha\t1.0.0\tlowering-gap\tsome gap\n").unwrap();

        let back = read_ledger(&root).unwrap();
        assert_eq!(
            back["alpha"].outcome,
            Outcome::LoweringGap("some gap".into())
        );
        assert_eq!(back["alpha"].site, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The pre-`stage` shape: six columns, and `compiles` where the stage tag
    /// now sits. Column 3 is what tells the two apart, so the reader must not
    /// mistake an outcome tag for a stage -- and `compiles` in particular has
    /// to land on `emits-rs`, since that is the only rung it ever measured.
    #[test]
    fn a_six_column_row_reads_as_the_stage_it_measured() {
        let root = scratch("legacy-stage");
        let compiles = root.join("conformance/gem-probe-compiles.tsv");
        let fails = root.join("conformance/gem-probe-fails.tsv");
        std::fs::create_dir_all(fails.parent().unwrap()).unwrap();
        std::fs::write(
            &compiles,
            "gem\tversion\toutcome\tdetail\twhere\tsha256\n\
             alpha\t1.0.0\tcompiles\t\t\tdeadbeef\n",
        )
        .unwrap();
        std::fs::write(
            &fails,
            "gem\tversion\toutcome\tdetail\twhere\tsha256\n\
             beta\t2.0.0\tno-lib-dir\t\t\t\n\
             gamma\t3.0.0\tfetch-failed\tsha256 mismatch\t\t\n",
        )
        .unwrap();

        let back = read_ledger(&root).unwrap();
        assert_eq!(back["alpha"].outcome, Outcome::Ok);
        assert_eq!(back["alpha"].stage, Stage::EmitsRs);
        assert_eq!(back["alpha"].digest.as_deref(), Some("deadbeef"));
        // A legacy row measured no bytes; recording 0 would claim it did.
        assert_eq!(back["alpha"].rust_bytes, None);
        assert_eq!(back["beta"].stage, Stage::Unpack);
        assert_eq!(back["gamma"].stage, Stage::Fetch);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The ladder is ordered, and `--check` leans on that to tell a promotion
    /// from a regression.
    #[test]
    fn the_stages_are_ordered_by_how_much_they_claim() {
        assert!(Stage::Fetch < Stage::Unpack);
        assert!(Stage::Unpack < Stage::EmitsRs);
        assert!(Stage::EmitsRs < Stage::BuildsBin);
        assert!(Stage::BuildsBin < Stage::Runs);
    }

    /// The fence around executing code from rubygems. Each clause is a
    /// separate refusal, so a mistake in one cannot be covered by another.
    #[test]
    fn running_gem_code_is_refused_unless_every_condition_holds() {
        let none: [(&str, bool); 1] = [("--corpus", false)];
        let bulk: [(&str, bool); 2] = [("--corpus", false), ("--failing", true)];

        // A bulk selector is refused outright -- not confirmed, not warned.
        let why = run_is_permitted(1, &bulk, true, true).unwrap_err();
        assert!(why.contains("--failing selects in bulk"), "{why}");
        // ... even with every other condition satisfied.
        assert!(run_is_permitted(9, &bulk, true, true).is_err());

        assert!(
            run_is_permitted(0, &none, true, true).is_err(),
            "no gem named"
        );
        let why = run_is_permitted(1, &none, false, true).unwrap_err();
        assert!(why.contains("--allow-running-untrusted-gem-code"), "{why}");
        let why = run_is_permitted(1, &none, true, false).unwrap_err();
        assert!(why.contains("Refusing to run unconfined"), "{why}");

        assert!(run_is_permitted(1, &none, true, true).is_ok());
    }

    /// A stall carries no diagnosis, so it must not borrow one: `timeout` has
    /// to survive a ledger round trip as itself rather than decaying into the
    /// `LoweringGap` catch-all `from_ledger` uses for unknown tags.
    #[test]
    fn a_timeout_round_trips_as_itself() {
        assert_eq!(
            Outcome::from_ledger(Outcome::Timeout.tag(), Outcome::Timeout.detail()),
            Outcome::Timeout
        );
    }

    fn lib_with(root: &Path, name: &str, files: &[&str]) -> PathBuf {
        let dir = vendor_dir(root).join(name);
        for f in files {
            let p = dir.join("lib").join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "# x\n").unwrap();
        }
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        dir
    }

    /// An entry point must name a file that exists. Guessing and hoping is how
    /// every Rails gem came back `compiles` while compiling none of itself:
    /// `require "activerecord"` resolves to nothing, zeo defers it to runtime,
    /// and codegen then succeeds trivially.
    #[test]
    fn the_entry_point_comes_from_the_files_the_gem_ships() {
        let root = scratch("entry");
        let plain = lib_with(&root, "colorator", &["colorator.rb"]);
        assert_eq!(
            entry_point(&plain, "colorator").as_deref(),
            Some("colorator")
        );

        let nested = lib_with(&root, "net-http", &["net/http.rb"]);
        assert_eq!(
            entry_point(&nested, "net-http").as_deref(),
            Some("net/http")
        );

        // The separator differs from the gem name entirely.
        let rails = lib_with(&root, "activerecord", &["active_record.rb", "arel.rb"]);
        assert_eq!(
            entry_point(&rails, "activerecord").as_deref(),
            Some("active_record")
        );

        let under = lib_with(&root, "ruby-progressbar", &["ruby_progressbar.rb"]);
        assert_eq!(
            entry_point(&under, "ruby-progressbar").as_deref(),
            Some("ruby_progressbar")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_lib_naming_nothing_recognisable_declines_to_answer() {
        let root = scratch("entry-none");
        // Two unrelated top-level files: neither matches, and picking one
        // would be a guess.
        let odd = lib_with(&root, "mystery", &["alpha.rb", "beta.rb"]);
        assert_eq!(entry_point(&odd, "mystery"), None);

        // A single file names itself, whatever it is called.
        let one = lib_with(&root, "solo", &["something_else.rb"]);
        assert_eq!(entry_point(&one, "solo").as_deref(), Some("something_else"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // --------------------------------------------------------------- ledger

    #[test]
    fn the_ledger_round_trips() {
        let root = scratch("ledger");
        let mut rows = BTreeMap::new();
        rows.insert(
            "alpha".to_string(),
            Row {
                version: "1.0.0".into(),
                stage: (Outcome::Ok).implied_stage(),
                outcome: Outcome::Ok,
                rust_bytes: None,
                binary_bytes: None,
                site: None,
                digest: None,
            },
        );
        rows.insert(
            "beta".to_string(),
            Row {
                version: "2.1.0".into(),
                stage: (Outcome::LoweringGap("some gap".into())).implied_stage(),
                outcome: Outcome::LoweringGap("some gap".into()),
                rust_bytes: None,
                binary_bytes: None,
                site: Some("vendor/gems/beta/lib/beta.rb:12".into()),
                digest: Some("d0d0cafe".into()),
            },
        );
        rows.insert(
            "gamma".to_string(),
            Row {
                version: "3.0.0".into(),
                stage: (Outcome::NativeExtension).implied_stage(),
                outcome: Outcome::NativeExtension,
                rust_bytes: None,
                binary_bytes: None,
                site: None,
                digest: None,
            },
        );
        write_ledger(&root, &rows).unwrap();

        let back = read_ledger(&root).unwrap();
        assert_eq!(back.len(), 3);
        assert_eq!(back["alpha"].outcome, Outcome::Ok);
        assert_eq!(back["beta"].version, "2.1.0");
        assert_eq!(
            back["beta"].outcome,
            Outcome::LoweringGap("some gap".into())
        );
        assert_eq!(back["gamma"].outcome, Outcome::NativeExtension);
        // The site rides alongside the detail rather than replacing it, and a
        // row that never had one still reads back as having none.
        assert_eq!(
            back["beta"].site.as_deref(),
            Some("vendor/gems/beta/lib/beta.rb:12")
        );
        assert_eq!(back["alpha"].site, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A diagnostic quotes the source it rejected, so a detail routinely
    /// carries `"`. Left raw, a CSV reader sees an unbalanced quote and
    /// abandons the file -- which is how GitHub stopped rendering it.
    #[test]
    fn a_detail_containing_quotes_survives_the_round_trip() {
        let root = scratch("ledger-quotes");
        let detail = r#"unsupported syntax at "'Resource' => resource""#;
        let mut rows = BTreeMap::new();
        rows.insert(
            "quoted".to_string(),
            Row {
                version: "1.0.0".into(),
                stage: (Outcome::LoweringGap(detail.into())).implied_stage(),
                outcome: Outcome::LoweringGap(detail.into()),
                rust_bytes: None,
                binary_bytes: None,
                site: None,
                digest: None,
            },
        );
        write_ledger(&root, &rows).unwrap();

        let text = std::fs::read_to_string(ledger_path(&root)).unwrap();
        let row = text.lines().nth(1).unwrap();
        assert_eq!(row.split('\t').count(), 9, "every row keeps nine fields");
        assert!(
            row.split('\t').nth(6).unwrap().starts_with('"'),
            "the detail is quoted: {row}"
        );
        assert_eq!(
            read_ledger(&root).unwrap()["quoted"].outcome.detail(),
            detail
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Writing the same rows twice must produce the same bytes, or a re-probe
    /// would show as a diff even when nothing changed.
    #[test]
    fn writing_the_ledger_is_deterministic() {
        let root = scratch("ledger-determinism");
        let mut rows = BTreeMap::new();
        for n in ["zeta", "alpha", "mu"] {
            rows.insert(
                n.to_string(),
                Row {
                    version: "1.0.0".into(),
                    stage: (Outcome::Ok).implied_stage(),
                    outcome: Outcome::Ok,
                    rust_bytes: None,
                    binary_bytes: None,
                    site: None,
                    digest: None,
                },
            );
        }
        write_ledger(&root, &rows).unwrap();
        let first = std::fs::read_to_string(ledger_path(&root)).unwrap();
        write_ledger(&root, &rows).unwrap();
        let second = std::fs::read_to_string(ledger_path(&root)).unwrap();
        assert_eq!(first, second);
        // BTreeMap ordering means the file is sorted, not insertion-ordered.
        assert_eq!(gem_names(&first), ["alpha", "mu", "zeta"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The `outcome` column of the row naming `gem`.
    fn outcomes<'a>(tsv: &'a str, gem: &str) -> &'a str {
        tsv.lines()
            .find(|l| l.starts_with(&format!("{gem}\t")))
            .and_then(|l| l.split('\t').nth(3))
            .unwrap_or("<missing>")
    }

    fn gem_names(tsv: &str) -> Vec<&str> {
        tsv.lines()
            .skip(1) // the header row
            .filter(|l| !l.is_empty())
            .filter_map(|l| l.split('\t').next())
            .collect()
    }

    /// The whole point of two files: a verdict decides which one a gem is in,
    /// and it is in exactly one.
    #[test]
    fn the_ledger_routes_each_gem_by_its_verdict() {
        let root = scratch("ledger-split");
        let mut rows = BTreeMap::new();
        for (name, outcome) in [
            ("alpha", Outcome::Ok),
            ("beta", Outcome::LoweringGap("a gap".into())),
            ("gamma", Outcome::Ok),
            ("delta", Outcome::NativeExtension),
        ] {
            rows.insert(
                name.to_string(),
                Row::stopped("1.0.0", outcome.implied_stage(), outcome, None),
            );
        }
        write_ledger(&root, &rows).unwrap();

        let tsv = std::fs::read_to_string(ledger_path(&root)).unwrap();
        // One file, every gem, sorted -- the verdict is the `outcome` column
        // rather than which file the row landed in.
        assert_eq!(gem_names(&tsv), ["alpha", "beta", "delta", "gamma"]);
        assert_eq!(outcomes(&tsv, "alpha"), "ok");
        assert_eq!(outcomes(&tsv, "gamma"), "ok");
        assert_eq!(outcomes(&tsv, "beta"), "lowering-gap");
        assert_eq!(outcomes(&tsv, "delta"), "native-extension");
        assert_eq!(read_ledger(&root).unwrap().len(), 4);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A fix rewrites the gem's row in place. It used to move the gem between
    /// two files, and the stale copy surviving in the file it left would have
    /// let resume read back the old verdict -- one file removes the hazard
    /// rather than guarding it.
    #[test]
    fn a_gem_that_starts_compiling_keeps_one_row() {
        let root = scratch("ledger-move");
        let mut rows = BTreeMap::new();
        rows.insert(
            "beta".to_string(),
            Row {
                version: "1.0.0".into(),
                stage: (Outcome::LoweringGap("a gap".into())).implied_stage(),
                outcome: Outcome::LoweringGap("a gap".into()),
                rust_bytes: None,
                binary_bytes: None,
                site: None,
                digest: None,
            },
        );
        write_ledger(&root, &rows).unwrap();
        rows.get_mut("beta").unwrap().outcome = Outcome::Ok;
        write_ledger(&root, &rows).unwrap();

        let tsv = std::fs::read_to_string(ledger_path(&root)).unwrap();
        assert_eq!(gem_names(&tsv), ["beta"], "one row, not two");
        assert_eq!(outcomes(&tsv, "beta"), "ok");
        assert_eq!(read_ledger(&root).unwrap()["beta"].outcome, Outcome::Ok);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The two files this ledger replaced can still be present -- an older
    /// checkout, a branch, a half-finished migration -- and they can disagree
    /// in a way one file never could. Guessing which copy is current would
    /// silently publish a wrong verdict.
    #[test]
    fn a_gem_recorded_in_two_ledgers_is_refused() {
        let root = scratch("ledger-dup");
        let compiles = root.join("conformance/gem-probe-compiles.tsv");
        let fails = root.join("conformance/gem-probe-fails.tsv");
        std::fs::create_dir_all(compiles.parent().unwrap()).unwrap();
        std::fs::write(&compiles, "beta\t1.0.0\tcompiles\n").unwrap();
        std::fs::write(&fails, "beta\t1.0.0\tlowering-gap\ta gap\n").unwrap();

        let err = read_ledger(&root).unwrap_err();
        assert!(err.contains("beta"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_corpus_ignores_comments_and_reads_pins() {
        let root = scratch("corpus");
        std::fs::write(
            corpus_path(&root),
            "# a comment\n\nrake\naddressable 2.9.0\nliquid   # trailing note\n",
        )
        .unwrap();
        let got = read_corpus(&root);
        assert_eq!(
            got,
            vec![
                ("rake".to_string(), None),
                ("addressable".to_string(), Some("2.9.0".to_string())),
                ("liquid".to_string(), None),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // ------------------------------------------------------------ unpacking

    fn synthetic_gem(files: &[(&str, &str)]) -> Vec<u8> {
        let mut inner = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut inner, flate2::Compression::default());
            let mut b = tar::Builder::new(enc);
            for (path, body) in files {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                b.append_data(&mut h, path, body.as_bytes()).unwrap();
            }
            b.into_inner().unwrap().finish().unwrap();
        }
        let mut outer = Vec::new();
        {
            let mut b = tar::Builder::new(&mut outer);
            for (name, body) in [("metadata.gz", b"x".as_slice()), ("data.tar.gz", &inner)] {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                b.append_data(&mut h, name, body).unwrap();
            }
            b.finish().unwrap();
        }
        outer
    }

    #[test]
    fn a_gem_unpacks_from_its_inner_data_archive() {
        let root = scratch("unpack");
        let gem = synthetic_gem(&[("lib/thing.rb", "module Thing; end\n"), ("README", "hi")]);
        let dest = root.join("out");
        unpack_gem(&gem, &dest).unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.join("lib/thing.rb")).unwrap(),
            "module Thing; end\n"
        );
        assert!(dest.join("README").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_gem_without_a_data_archive_is_an_error() {
        let root = scratch("unpack-bad");
        assert!(unpack_gem(b"not a tar at all", &root.join("out")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A stub gemspec in the CACHE means the gem was fetched before the stub
    /// moved into the view, so its real `require_paths`/`extensions` are gone
    /// and the entry must be re-fetched rather than trusted.
    #[test]
    fn a_stub_gemspec_in_the_cache_is_stale() {
        let root = scratch("stale-stub");
        let stubbed = root.join("stubbed");
        let real = root.join("real");
        std::fs::create_dir_all(&stubbed).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        write_stub_gemspec(&stubbed, "a", "1.0.0").unwrap();
        std::fs::write(
            real.join("b.gemspec"),
            "Gem::Specification.new do |s|\n  s.name = \"b\".freeze\n  \
             s.summary = \"real\".freeze\n  s.require_paths = [\"lib/b\".freeze]\nend\n",
        )
        .unwrap();
        assert!(cached_gemspec_is_a_stub(&stubbed));
        assert!(!cached_gemspec_is_a_stub(&real));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The stub goes in the VIEW and the cache keeps what rubygems shipped.
    ///
    /// Overwriting the real gemspec is what made a committed row
    /// irreproducible: the tree a verdict was measured against was no longer
    /// the gem. It also meant the real gemspec could only ever be read once,
    /// and that changing the stub's format required re-downloading everything.
    #[test]
    fn the_view_stubs_the_gemspec_and_the_cache_keeps_the_real_one() {
        let root = scratch("stub");
        let src = vendor_dir(&root).join("thing");
        std::fs::create_dir_all(src.join("lib")).unwrap();
        std::fs::write(src.join("lib/thing.rb"), "# x\n").unwrap();
        // The shape zeo rejects: a computed version.
        std::fs::write(
            src.join("real.gemspec"),
            "Gem::Specification.new { |s| s.version = Thing::VERSION }\n",
        )
        .unwrap();
        std::fs::write(stamp_path(&src), "4.5.6").unwrap();

        let view = isolate(&root, "thing", &[]).unwrap();
        let seen = view.join("thing");

        // The view: a stub, and no trace of the computed-version original.
        let stub = std::fs::read_to_string(seen.join("thing.gemspec")).unwrap();
        assert!(stub.contains(r#"s.version = "4.5.6".freeze"#), "{stub}");
        assert!(stub.contains(r#"s.name = "thing".freeze"#), "{stub}");
        assert!(!seen.join("real.gemspec").exists());
        // Everything else is still reachable through it.
        assert!(seen.join("lib/thing.rb").exists());

        // The cache: untouched.
        assert!(src.join("real.gemspec").exists());
        assert!(!src.join("thing.gemspec").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    // ------------------------------------------------- isolation, idempotency

    /// The property that makes a verdict reproducible: a probe sees its own
    /// gem and its declared dependencies, and nothing else that happens to be
    /// cached beside them.
    #[test]
    fn a_probe_sees_only_its_gem_and_its_dependencies() {
        let root = scratch("isolate");
        gem_at(&root, "target", "1.0.0", "module Target; end\n");
        gem_at(&root, "adep", "1.0.0", "module Adep; end\n");
        gem_at(&root, "unrelated", "9.9.9", "module Unrelated; end\n");

        let view = isolate(&root, "target", &["adep".to_string()]).unwrap();
        let mut seen: Vec<String> = std::fs::read_dir(&view)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            ["adep", "target"],
            "unrelated gems must not be visible"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Isolating twice must not accumulate. A stale link from an earlier probe
    /// would silently widen what the next one can see.
    #[test]
    fn isolating_twice_does_not_accumulate() {
        let root = scratch("isolate-twice");
        gem_at(&root, "target", "1.0.0", "module Target; end\n");
        gem_at(&root, "adep", "1.0.0", "module Adep; end\n");

        isolate(&root, "target", &["adep".to_string()]).unwrap();
        let view = isolate(&root, "target", &[]).unwrap();
        let seen: Vec<String> = std::fs::read_dir(&view)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(seen, ["target"], "the previous run's dep link survived");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The ledger on disk must be the truth so far, not the truth at the end.
    /// A registry-scale sweep is hours long, so an interrupted run that wrote
    /// nothing would throw away all of it.
    #[test]
    fn each_probe_persists_before_the_next_one_starts() {
        let root = scratch("incremental");
        let mut rows = BTreeMap::new();
        for (i, name) in ["alpha", "beta", "gamma"].iter().enumerate() {
            rows.insert(
                name.to_string(),
                Row {
                    version: "1.0.0".into(),
                    stage: (Outcome::Ok).implied_stage(),
                    outcome: Outcome::Ok,
                    rust_bytes: None,
                    binary_bytes: None,
                    site: None,
                    digest: None,
                },
            );
            write_ledger(&root, &rows).unwrap();
            // Whatever has been probed so far is readable right now.
            let ondisk = read_ledger(&root).unwrap();
            assert_eq!(ondisk.len(), i + 1);
            assert!(ondisk.contains_key(*name));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Resume is a set difference against the ledger, so re-running a sweep
    /// continues instead of starting over.
    #[test]
    fn a_rerun_skips_what_the_ledger_already_holds() {
        let root = scratch("resume");
        let mut rows = BTreeMap::new();
        rows.insert(
            "done".to_string(),
            Row {
                version: "1.0.0".into(),
                stage: (Outcome::Ok).implied_stage(),
                outcome: Outcome::Ok,
                rust_bytes: None,
                binary_bytes: None,
                site: None,
                digest: None,
            },
        );
        write_ledger(&root, &rows).unwrap();

        let before = read_ledger(&root).unwrap();
        let mut wanted: Vec<(String, Option<String>)> = vec![
            ("done".into(), None),
            ("fresh".into(), None),
            ("also-fresh".into(), None),
        ];
        wanted.retain(|(n, _)| !before.contains_key(n));
        assert_eq!(
            wanted.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["fresh", "also-fresh"]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A cached index is read from disk. No URL appears in this test, which is
    /// the point: if the cache were ignored, the test would need the network.
    #[test]
    fn the_registry_index_is_read_from_its_cache() {
        let root = scratch("names");
        std::fs::write(names_cache(&root), "---\nalpha\nbeta\n\ngamma\n").unwrap();
        assert_eq!(
            registry_names(&root, false).unwrap(),
            vec!["alpha", "beta", "gamma"]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A fetch whose stamp already records the wanted version must not go to
    /// the network. The absence of any URL here is the point: if `fetch` tried,
    /// this test would need one.
    #[test]
    fn fetch_is_idempotent_for_an_already_unpacked_version() {
        let root = scratch("idempotent");
        let dir = gem_at(&root, "cached", "2.0.0", "module Cached; end\n");
        let marker = dir.join("lib/cached.rb");
        let before = std::fs::read_to_string(&marker).unwrap();

        let got = fetch(&root, "cached", "2.0.0", None).unwrap();
        assert_eq!(got, dir);
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_version_change_invalidates_the_cache() {
        let root = scratch("stamp");
        gem_at(&root, "moving", "1.0.0", "module Moving; end\n");
        // A different version must not be served from the 1.0.0 tree; with no
        // network in a test this surfaces as an error rather than a stale hit.
        assert!(fetch(&root, "moving", "2.0.0", None).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
