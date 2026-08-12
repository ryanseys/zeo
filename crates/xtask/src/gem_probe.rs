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
//! default rung is `codegen`: zeo produced Rust. That is deliberately the
//! weakest useful claim. No rustc runs, no binary exists, and the gem's own
//! code may not have been compiled at all -- zeo can decline a unit and defer
//! it to a runtime `LoadError`, which only `--run` can see. `--build` and
//! `--run` climb the rungs above, both off by default.
//!
//! Four steps, and only the first touches the network:
//!
//!   resolve  name [version]     -> an exact version
//!   fetch    the .gem           -> vendor/gems/<name>/   (cached, gitignored)
//!   probe    require "<entry>"  -> a Stage and an Outcome
//!   record   the verdict        -> conformance/gem-probe.tsv       (committed)
//!
//! The emitted Rust is kept, gzipped, under `vendor/.probe-rs/`, so a later
//! sweep can climb to `build` for the whole corpus without paying for
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
//! A panic, a timeout and a memory kill are each recorded as their own outcome
//! rather than folded into `lowering-gap`: a gap is a limit zeo reported, a
//! panic is a bug it did not, a timeout is no verdict at all, and an
//! out-of-memory says what this MACHINE could hold rather than anything about
//! the gem. How wide the sweep runs and how much each compile may hold are one
//! decision, made in `crate::jobs` -- see there for why it is a memory budget
//! and not a job count.
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
conformance/gem-probe.tsv (plus gem-probe.md). Two columns carry the verdict
and are read together: `stage` is the rung, `outcome` is what happened there.

selection (at least one, they add up):
  <name> [version]        one gem; the newest version unless one is given
  --corpus                every name in conformance/rubygems-names.txt
  --popular <n>           the n most-downloaded gems in the index
  --index                 every gem in the index
  --all                   re-probe every gem already in the ledger
  --failing               re-probe every ledger row that isn't `ok`, except
                          the frontier and the rows no compiler change can
                          move (no-lib-dir, no-entry-point, meta-gem,
                          ext-only, fetch-failed -- yanked gems and registry
                          drift -- and invalid-ruby decided at parse);
                          --refresh includes those too
  --unprobed              probe ledger rows that have no verdict yet
  --matching <text>       re-probe rows whose outcome detail contains <text>
  --outcome <tag>         re-probe rows with exactly this outcome tag
                          (`no-entry-point`, `no-lib-dir`, ...) -- the
                          selector for buckets whose detail is empty
                          -- how a landed fix is measured
  --matching-name <text>  re-probe rows whose gem NAME contains <text>, for a
                          stale band no detail substring can select

stages (fetch -> unpack -> parse -> lower -> analyze -> codegen -> build ->
run; the four middle rungs are zeo's own front-end passes, and a rejection is
recorded at the pass that made it):
  (default)               stop after codegen: generate Rust, keep the .rs,
                          build nothing
  --build                 also compile the generated Rust to a binary
  --run                   also EXECUTE it. This runs code downloaded from
                          rubygems, so it needs --allow-running-untrusted-gem-
                          code, named gems only, a sandbox, and a prompt
  --allow-running-untrusted-gem-code
                          the second half of --run's consent

options:
  --check                 exit non-zero if any probed gem regressed, or if the
                          README stats block disagrees with the ledger
  --sync-readme           rewrite gem-probe.md and the README stats block from
                          the ledger on disk, probing nothing
  --refresh               re-probe even names already in the ledger
  --refresh-index         fetch a fresh copy of the rubygems index
  --seed-index            record every gem in the index that has no row yet,
                          as `queued unprobed` -- makes the ledger say what is
                          left to measure, not just what has been
  --no-deps               don't resolve or fetch the gem's dependencies
  --limit <n>             stop after n gems
  --jobs <n>              gems in flight at once (default: derived from RAM,
                          not cores -- each one is a whole compile). More jobs
                          split the same memory budget, they do not add to it
  --timeout <seconds>     kill a gem that outruns it (default: 600)
  --zeo <path>            probe with exactly this binary (build nothing)
  --rebuild-zeo           force a fresh build + snapshot for HEAD; the default
                          reuses target/probe-bin/zeo-<sha> when it exists, so
                          a sweep never touches cargo while you keep building.
                          Run a side-by-side sweep at --jobs 3: concurrent
                          probe + dev rustc is the memory-pressure pattern
                          that froze this machine, and the spare slot is the
                          headroom
  -h, --help              this message
";

/// Long enough that a rails-scale require graph finishes -- those take minutes
/// in the front end alone -- and short enough that a gem which will never
/// finish cannot hold a sweep open.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// How far up the pipeline a verdict got.
///
/// The rungs are ordered and each is a strictly harder claim about the gem
/// than the one below: unpacking says the archive had a `lib/`, the four
/// front-end rungs say how far zeo's own passes got, `build` says rustc
/// accepted the Rust, `run` says the binary executed. They are a separate
/// column rather than more outcome tags so that adding a rung costs one value
/// instead of a schema change, and so a failure can say WHERE it stopped -- a
/// `timeout` in codegen and a `timeout` in rustc are not the same row.
///
/// The column is meaningless alone and is always read beside `outcome`:
/// `codegen ok` and `codegen lowering-gap` are the same rung with opposite
/// results.
///
/// `codegen` is the default and the last rung a sweep reaches on its own.
/// Reaching it is deliberately the weakest useful claim: it does NOT mean a
/// binary exists, and it does not mean the gem's own code was compiled -- zeo
/// may have declined a unit and deferred it to a runtime `LoadError`, which
/// only `run` can see.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Stage {
    /// No rung reached: the gem is KNOWN (the registry names it) and has never
    /// been probed. Ordered below `Fetch` so every `stage >= ...` test treats
    /// it as the bottom.
    Queued,
    Fetch,
    Unpack,
    /// The four FRONT-END rungs, in the order zeo runs them. A rejection names
    /// the pass that made it, which zeo already prints as the diagnostic's
    /// code (`zeo::analyze`, `zeo::codegen`, ...) and the probe used to throw
    /// away -- so every front-end failure landed in one undifferentiated
    /// bucket though the compiler had said which pass it was.
    ///
    /// They are rungs rather than columns because the ladder TERMINATES: the
    /// first rejection stops the compile, so the pass a row names implies
    /// `ok` at every pass before it and "never attempted" at every pass after.
    /// A column per pass would restate that.
    Parse,
    Lower,
    Analyze,
    Codegen,
    BuildsBin,
    Runs,
}

impl Stage {
    fn tag(self) -> &'static str {
        match self {
            Stage::Queued => "queued",
            Stage::Fetch => "fetch",
            Stage::Unpack => "unpack",
            Stage::Parse => "parse",
            Stage::Lower => "lower",
            Stage::Analyze => "analyze",
            Stage::Codegen => "codegen",
            Stage::BuildsBin => "build",
            Stage::Runs => "run",
        }
    }

    fn from_tag(tag: &str) -> Option<Stage> {
        match tag {
            "queued" => Some(Stage::Queued),
            "fetch" => Some(Stage::Fetch),
            "unpack" => Some(Stage::Unpack),
            "parse" => Some(Stage::Parse),
            "lower" => Some(Stage::Lower),
            "analyze" => Some(Stage::Analyze),
            // `emits-rs` was the single front-end rung these four replace, and
            // it is what every committed row said before the split. It names
            // the rung a SUCCESS reaches, which is the last of them.
            "codegen" | "emits-rs" => Some(Stage::Codegen),
            "build" | "builds-bin" => Some(Stage::BuildsBin),
            "run" | "runs" => Some(Stage::Runs),
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
    /// A `require` that MORE than one gem on the probe's load path provides
    /// -- usually the subject gem vendoring a file a bundled gem also ships
    /// (`openssl`, `English`). A fact about the probe's view, not the gem: no
    /// Bundler resolution puts both copies on one load path.
    AmbiguousRequire(String),
    /// None of the gem's declared `require_paths` exists in the archive, and
    /// Ruby files sit outside every one of them. The gem's own load path is
    /// empty as packaged, so no Ruby can require what it ships either.
    NoLibDir,
    /// The declared roots hold no file at all that a `require` could name.
    /// Probing would compile an unresolvable require, which says nothing
    /// about the gem.
    NoEntryPoint,
    /// The archive ships no Ruby at all -- a gemspec-only gem that exists to
    /// name dependencies. `rails` is the canonical one. Nothing to compile,
    /// and never a failure.
    MetaGem,
    /// The gem's code is its C extension: declared `extensions`, or an `ext/`
    /// tree, with no Ruby load path beside it. See docs/EXTENSIONS.md.
    ExtOnly,
    /// The compiler panicked. Distinct from a lowering gap on purpose: a gap
    /// is a known limit reported through the error path, a panic is a bug.
    CompilerPanic(String),
    /// The compiler was still running after `--timeout` seconds and was killed.
    /// Its own outcome, never a lowering gap: a stall is the absence of a
    /// verdict, and recording it as one would put a diagnosis in the ledger
    /// that zeo never made.
    Timeout,
    /// The compiler reached the memory ceiling this sweep handed it
    /// (`zeo::memguard`) and gave up. Kept apart from `Timeout` for the same
    /// reason `Timeout` is kept apart from a gap, one step further out: a
    /// stall says zeo never reached a verdict, and this says the MACHINE never
    /// let it -- the row is a fact about the host and the sweep's width, and a
    /// narrower sweep or a leaner compiler flips it back without anything
    /// about the gem changing. The payload is the phase it died in.
    OutOfMemory(String),
    /// Named by the registry and never probed. Not a failure and not a
    /// verdict -- it is the FRONTIER, the work still to do, and it is in the
    /// ledger so that "how much of rubygems have we measured" is a question
    /// the file answers rather than one that needs the index beside it.
    Unprobed,
    FetchFailed(String),
    /// The registry carries this version only as prebuilt PLATFORM artifacts
    /// (`x86_64-linux`, `java`, ...) -- there is no `ruby` platform gem to
    /// compile. The payload names the platforms. A fact about the release:
    /// zeo needs the pure-Ruby artifact, the same rule as Bundler's
    /// `force_ruby_platform`.
    PlatformGem(String),
    /// The gem's source is on disk, but the HARNESS could not carry out the
    /// probe on this machine: building the isolated load-path view failed, or
    /// the zeo process could not even be spawned. NOT a `fetch-failed`:
    /// nothing was downloaded, the registry was never asked, and re-running
    /// the fetch cannot help. Its own outcome so a sweep's fetch column keeps
    /// meaning "the registry or the network", which is what anyone reading it
    /// goes on to check.
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
            Outcome::AmbiguousRequire(_) => "ambiguous-require",
            Outcome::NoLibDir => "no-lib-dir",
            Outcome::NoEntryPoint => "no-entry-point",
            Outcome::MetaGem => "meta-gem",
            Outcome::ExtOnly => "ext-only",
            Outcome::CompilerPanic(_) => "compiler-panic",
            Outcome::Timeout => "timeout",
            Outcome::OutOfMemory(_) => "out-of-memory",
            Outcome::Unprobed => "unprobed",
            Outcome::FetchFailed(_) => "fetch-failed",
            Outcome::PlatformGem(_) => "platform-gem",
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
            | Outcome::AmbiguousRequire(d)
            | Outcome::FetchFailed(d)
            | Outcome::ViewFailed(d)
            | Outcome::CompilerPanic(d)
            | Outcome::RustcError(d)
            | Outcome::RunFailed(d)
            | Outcome::PlatformGem(d)
            | Outcome::OutOfMemory(d) => d,
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
            "meta-gem" => Outcome::MetaGem,
            "ext-only" => Outcome::ExtOnly,
            "timeout" => Outcome::Timeout,
            "out-of-memory" => Outcome::OutOfMemory(detail.to_string()),
            "missing-dependency" => Outcome::MissingDependency(detail.to_string()),
            "ambiguous-require" => Outcome::AmbiguousRequire(detail.to_string()),
            "unprobed" => Outcome::Unprobed,
            "fetch-failed" => Outcome::FetchFailed(detail.to_string()),
            "platform-gem" => Outcome::PlatformGem(detail.to_string()),
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
            Outcome::FetchFailed(_) | Outcome::PlatformGem(_) => Stage::Fetch,
            Outcome::ViewFailed(_)
            | Outcome::NoLibDir
            | Outcome::NoEntryPoint
            | Outcome::MetaGem
            | Outcome::ExtOnly => Stage::Unpack,
            Outcome::RustcError(_) => Stage::BuildsBin,
            Outcome::RunFailed(_) => Stage::Runs,
            // A legacy row said only `emits-rs`, which is the rung a SUCCESS
            // reaches; a failing legacy row cannot say which pass rejected it
            // until it is re-probed.
            _ => Stage::Codegen,
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
    ///
    /// UNITS CHANGED when the probe moved from `--dump=rust` to `--emit-rust`.
    /// Every row written before that measured PRETTYPLEASE output, which no
    /// build ever compiles and which runs about 2.5x the real thing
    /// (actionmailer: 305 MB pretty, 125 MB compact). The new number is what
    /// rustc is actually handed. Rows re-probed since carry it; the rest still
    /// carry the old one, so a cross-row byte comparison is only meaningful
    /// within one sweep until the corpus is swept through.
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

/// Records what the `.gem`'s own `metadata.gz` said about the gem, captured at
/// unpack time. The registry's spec is authoritative where the shipped
/// gemspec is not even present -- 42% of unpacked gems carry none -- and
/// where it is present it is usually dynamic Ruby a static parse refuses.
fn spec_stamp_path(dir: &Path) -> PathBuf {
    dir.join(".zeo-probe-spec")
}

/// What the probe keeps from `metadata.gz`: where the load path roots are,
/// which platform the artifact is for, and whether it declares C extensions.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GemMeta {
    require_paths: Vec<String>,
    platform: String,
    extensions: Vec<String>,
}

impl Default for GemMeta {
    fn default() -> GemMeta {
        GemMeta {
            require_paths: vec!["lib".to_string()],
            platform: "ruby".to_string(),
            extensions: Vec::new(),
        }
    }
}

/// Reads the three fields out of a `Gem::Specification#to_yaml` document.
///
/// Not a YAML parser. The document is machine-written by rubygems with a
/// fixed shape -- top-level `key:` lines, list items as `- value` -- and the
/// probe needs three keys from it. A hand parse of that shape beats a YAML
/// dependency that would still need the `!ruby/object` tags taught to it.
fn parse_gem_metadata_yaml(text: &str) -> GemMeta {
    let unquote = |s: &str| {
        let s = s.trim();
        s.strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(s)
            .to_string()
    };
    let mut meta = GemMeta {
        require_paths: Vec::new(),
        ..GemMeta::default()
    };
    let mut list: Option<&mut Vec<String>> = None;
    for line in text.lines() {
        // A list item belongs to the key above it; anything else ends the list.
        if line.starts_with("- ") {
            if let Some(items) = list.as_deref_mut() {
                items.push(unquote(&line[2..]));
            }
            continue;
        }
        list = None;
        if let Some(v) = line.strip_prefix("platform:") {
            meta.platform = unquote(v);
        } else if let Some(v) = line.strip_prefix("require_paths:") {
            if v.trim() != "[]" {
                list = Some(&mut meta.require_paths);
            }
        } else if let Some(v) = line.strip_prefix("extensions:") {
            if v.trim() != "[]" {
                list = Some(&mut meta.extensions);
            }
        }
    }
    if meta.require_paths.is_empty() {
        meta.require_paths = GemMeta::default().require_paths;
    }
    meta
}

fn write_spec_stamp(dir: &Path, meta: &GemMeta) -> Result<(), String> {
    let json = serde_json::json!({
        "require_paths": meta.require_paths,
        "platform": meta.platform,
        "extensions": meta.extensions,
    });
    std::fs::write(spec_stamp_path(dir), json.to_string()).map_err(|e| e.to_string())
}

/// The captured spec for an unpacked gem. `None` for a cache written before
/// the stamp existed -- `fetch` treats that as stale, so the answer heals on
/// the next probe.
fn read_spec_stamp(dir: &Path) -> Option<GemMeta> {
    let text = std::fs::read_to_string(spec_stamp_path(dir)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let strings = |key: &str| -> Vec<String> {
        v.get(key)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut meta = GemMeta {
        require_paths: strings("require_paths"),
        platform: v
            .get("platform")
            .and_then(|x| x.as_str())
            .unwrap_or("ruby")
            .to_string(),
        extensions: strings("extensions"),
    };
    if meta.require_paths.is_empty() {
        meta.require_paths = GemMeta::default().require_paths;
    }
    Some(meta)
}

fn unpack_gem(bytes: &[u8], dest: &Path) -> Result<(), String> {
    // A .gem is a tar of metadata.gz, data.tar.gz and checksums.yaml.gz. The
    // gem's own files are the middle one; the first is the registry's own
    // serialized spec, which carries the `require_paths` the shipped gemspec
    // usually cannot give up statically.
    let mut outer = tar::Archive::new(bytes);
    let mut data = Vec::new();
    let mut metadata = Vec::new();
    for entry in outer.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        match entry.path().map(|p| p.as_os_str().to_owned()) {
            Ok(p) if p == "data.tar.gz" => {
                entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
            }
            Ok(p) if p == "metadata.gz" => {
                entry
                    .read_to_end(&mut metadata)
                    .map_err(|e| e.to_string())?;
            }
            _ => {}
        }
        if !data.is_empty() && !metadata.is_empty() {
            break;
        }
    }
    if data.is_empty() {
        return Err("no data.tar.gz inside the .gem".into());
    }
    let gz = flate2::read::GzDecoder::new(&data[..]);
    tar::Archive::new(gz)
        .unpack(dest)
        .map_err(|e| format!("unpacking: {e}"))?;
    let meta = if metadata.is_empty() {
        GemMeta::default()
    } else {
        let mut yaml = String::new();
        flate2::read::GzDecoder::new(&metadata[..])
            .read_to_string(&mut yaml)
            .map_err(|e| format!("reading metadata.gz: {e}"))?;
        parse_gem_metadata_yaml(&yaml)
    };
    write_spec_stamp(dest, &meta)
}

fn write_stub_gemspec(
    dir: &Path,
    name: &str,
    version: &str,
    require_paths: &[String],
) -> Result<(), String> {
    let paths = require_paths
        .iter()
        .map(|p| format!("{p:?}.freeze"))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        dir.join(format!("{name}.gemspec")),
        format!(
            "Gem::Specification.new do |s|\n  \
             s.name = {name:?}.freeze\n  \
             s.version = {version:?}.freeze\n  \
             s.require_paths = [{paths}]\nend\n"
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
        // The probe's own stamps describe the cache, not the gem.
        if base.to_string_lossy().starts_with(".zeo-probe-") {
            continue;
        }
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
    // The stub carries the REAL require_paths from the captured spec. The one
    // it used to hard-code (`["lib"]`) probed every gem with a different
    // layout against a load path that did not exist -- concurrent-ruby's
    // `lib/concurrent-ruby` among them.
    let meta = read_spec_stamp(src).unwrap_or_default();
    write_stub_gemspec(&dest, gem, &unpacked_version(src), &meta.require_paths)
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
    // A cache without the spec stamp predates it, so its `require_paths` were
    // never captured -- re-fetching is what fills them in.
    if std::fs::read_to_string(stamp_path(&dir)).is_ok_and(|s| s.trim() == version)
        && spec_stamp_path(&dir).is_file()
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
///
/// `roots` are the gem's real load-path roots -- its declared
/// `require_paths`, filtered to the directories that exist. Every root goes
/// on the load path, so a feature found under any of them resolves.
fn entry_point(roots: &[PathBuf], name: &str) -> Option<String> {
    roots.iter().find_map(|r| entry_point_under(r, name))
}

fn entry_point_under(lib: &Path, name: &str) -> Option<String> {
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
    let mut tops: Vec<String> = std::fs::read_dir(lib)
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
    for sub in std::fs::read_dir(lib).ok()?.filter_map(Result::ok) {
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

/// The ruby programs a gem ships as EXECUTABLES: files directly under
/// `bin/` or `exe/` whose first line is a shebang naming ruby. A CLI-only
/// gem (darb, the whole dorian-* family) publishes nothing else -- no `.rb`
/// anywhere -- and read as a meta-gem before this looked. Sorted for a
/// deterministic program.
fn ruby_executables(dir: &Path) -> Vec<PathBuf> {
    use std::io::Read;
    let mut scripts: Vec<PathBuf> = ["bin", "exe"]
        .iter()
        .filter_map(|b| std::fs::read_dir(dir.join(b)).ok())
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            let Ok(mut f) = std::fs::File::open(p) else {
                return false;
            };
            let mut head = [0u8; 128];
            let n = f.read(&mut head).unwrap_or(0);
            let head = String::from_utf8_lossy(&head[..n]);
            let first = head.lines().next().unwrap_or("");
            first.starts_with("#!") && first.contains("ruby")
        })
        .collect();
    scripts.sort();
    scripts
}

/// Load-path roots DISCOVERED from the archive when every declared
/// `require_path` is missing. In order: the gem directory itself when bare
/// `.rb` files sit at its top (the archive root IS the load path), a nested
/// `<sub>/lib` (a gem packed one directory too deep), and any code-shaped
/// first-level directory (`gem/`, `ruby/`, `src/`). Conventional non-code
/// directories never become roots, so a tests-only archive still reports
/// `no-lib-dir` honestly.
fn discovered_roots(dir: &Path) -> Vec<PathBuf> {
    const NON_CODE: &[&str] = &[
        "spec",
        "test",
        "tests",
        "features",
        "benchmark",
        "benchmarks",
        "bin",
        "exe",
        "doc",
        "docs",
        "example",
        "examples",
        "sample",
        "samples",
        "vendor",
        "tasks",
        "rakelib",
        "script",
        "scripts",
        "man",
        "data",
        "assets",
    ];
    let mut roots = Vec::new();
    let has_top_rb = std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .any(|p| p.is_file() && p.extension().is_some_and(|x| x == "rb"))
    });
    if has_top_rb {
        roots.push(dir.to_path_buf());
    }
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .is_some_and(|n| !n.starts_with('.') && !NON_CODE.contains(&n.as_str()))
        })
        .collect();
    subs.sort();
    for sub in subs {
        let nested_lib = sub.join("lib");
        if nested_lib.is_dir() && ships_ruby(&nested_lib) {
            roots.push(nested_lib);
        } else if ships_ruby(&sub) {
            roots.push(sub);
        }
    }
    roots
}

/// The honest verdict for a gem whose declared load path does not exist in
/// its archive. Three different facts used to share the `no-lib-dir` tag, and
/// only one of them ever had Ruby a compiler could reach.
fn rootless_outcome(dir: &Path, meta: &GemMeta) -> Outcome {
    if !meta.extensions.is_empty() || dir.join("ext").is_dir() || dir.join("extconf.rb").is_file() {
        return Outcome::ExtOnly;
    }
    if !ships_ruby(dir) {
        return Outcome::MetaGem;
    }
    Outcome::NoLibDir
}

/// Whether any `.rb` exists anywhere under `dir`. Symlinked directories are
/// not followed and depth is capped, for the reason `collect_rb_features`
/// gives: an archive can carry a symlink cycle.
fn ships_ruby(dir: &Path) -> bool {
    fn walk(dir: &Path, depth: u32) -> bool {
        if depth > 32 {
            return false;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            let p = entry.path();
            if ft.is_file() && p.extension().is_some_and(|x| x == "rb") {
                return true;
            }
            if ft.is_dir() && walk(&p, depth + 1) {
                return true;
            }
        }
        false
    }
    walk(dir, 0)
}

/// Every feature a top-level file under the roots provides, sorted and
/// deduplicated -- the program the probe falls back to when no file carries
/// the gem's name. Top level only: requiring a gem's internal files directly
/// is not how any user loads it, but its top-level files are exactly the
/// features it publishes.
fn top_level_features(roots: &[PathBuf]) -> Vec<String> {
    let mut features: Vec<String> = roots
        .iter()
        .filter_map(|r| std::fs::read_dir(r).ok())
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "rb"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    features.sort();
    features.dedup();
    features
}

/// The features the gem's own REQUIRE GRAPH publishes -- the tier below
/// `top_level_features`, for the pre-convention layout that ships
/// `lib/<dir>/...` with no top-level file at all (`360_services` ships
/// `lib/sorenson/`, and nothing anywhere carries the gem's name).
///
/// Among the gem's own files, an entry point is a file no sibling requires:
/// in-degree zero in the graph of `require`/`require_relative` edges that
/// resolve to files INSIDE the gem. Of those roots, the ones whose transitive
/// closure reaches the most files are the published surface -- the real entry
/// dominates the stray helper that neither requires nor is required, while
/// several equal independent roots are all published, exactly as several
/// top-level files are. Still a resolution rule and not a guess, which is the
/// distinction `entry_point`'s doc exists to protect: every answer names a
/// file that exists, and the edges come from the gem's own source.
///
/// Empty when the roots hold no Ruby at all, or when every file sits in one
/// require cycle (no root to stand on).
fn require_graph_root_features(roots: &[PathBuf]) -> Vec<String> {
    // Every `.rb` under the roots, keyed by root-relative feature path.
    // Sorted for determinism; a feature shipped under two roots keeps the
    // first (the load path would resolve it the same way).
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for root in roots {
        collect_rb_features(root, root, 0, &mut files);
    }
    files.sort();
    files.dedup_by(|a, b| a.0 == b.0);
    if files.is_empty() {
        return Vec::new();
    }
    let index: std::collections::HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, (f, _))| (f.as_str(), i))
        .collect();
    let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); files.len()];
    let mut in_degree = vec![0usize; files.len()];
    for i in 0..files.len() {
        let Ok(src) = std::fs::read_to_string(&files[i].1) else {
            continue;
        };
        for (relative, target) in literal_requires(&src) {
            let feature = if relative {
                let dir = match files[i].0.rfind('/') {
                    Some(cut) => &files[i].0[..cut],
                    None => "",
                };
                match normalize_feature(&format!("{dir}/{target}")) {
                    Some(f) => f,
                    None => continue,
                }
            } else {
                target
            };
            // An edge only when the required feature is one of the gem's own
            // files -- a dependency's feature resolves elsewhere and says
            // nothing about which of THESE files is the entry.
            if let Some(&j) = index.get(feature.as_str())
                && j != i
                && !out_edges[i].contains(&j)
            {
                out_edges[i].push(j);
                in_degree[j] += 1;
            }
        }
    }
    let root_ixs: Vec<usize> = (0..files.len()).filter(|&i| in_degree[i] == 0).collect();
    if root_ixs.is_empty() {
        return Vec::new();
    }
    let coverage = |start: usize| -> usize {
        let mut seen = vec![false; files.len()];
        let mut stack = vec![start];
        let mut n = 0;
        while let Some(i) = stack.pop() {
            if std::mem::replace(&mut seen[i], true) {
                continue;
            }
            n += 1;
            stack.extend(out_edges[i].iter().copied());
        }
        n
    };
    let covs: Vec<usize> = root_ixs.iter().map(|&i| coverage(i)).collect();
    let max = *covs.iter().max().expect("root_ixs is non-empty");
    root_ixs
        .iter()
        .zip(&covs)
        .filter(|&(_, &c)| c == max)
        .map(|(&i, _)| files[i].0.clone())
        .collect()
}

/// Every `.rb` under `dir` (recursively) as a `(feature, path)` pair, the
/// feature root-relative with the extension dropped. Symlinked directories
/// are NOT followed -- a gem archive can carry a symlink cycle (one spun the
/// probe forever on the last gem of the first entry-point sweep), and a
/// feature reached only through a symlink has a real spelling elsewhere. The
/// depth cap is the backstop for a cycle spelled without symlinks.
fn collect_rb_features(root: &Path, dir: &Path, depth: u32, out: &mut Vec<(String, PathBuf)>) {
    if depth > 32 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let p = entry.path();
        // `file_type()` reads the entry itself and never follows a symlink,
        // unlike `Path::is_dir`.
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            collect_rb_features(root, &p, depth + 1, out);
        } else if ft.is_file()
            && p.extension().is_some_and(|x| x == "rb")
            && let Ok(rel) = p.strip_prefix(root)
        {
            let feature = rel.with_extension("");
            let feature = feature.to_string_lossy().replace('\\', "/");
            out.push((feature, p));
        }
    }
}

/// The `require "x"` / `require_relative "y"` targets a source spells as a
/// single literal string -- the only forms that name a file this side of
/// execution. `(relative, feature)` pairs; a computed or interpolated
/// argument contributes no edge.
fn literal_requires(src: &str) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim_start();
        let (relative, rest) = if let Some(r) = line.strip_prefix("require_relative") {
            (true, r)
        } else if let Some(r) = line.strip_prefix("require") {
            (false, r)
        } else {
            continue;
        };
        let rest = rest.trim_start_matches(['(', ' ', '\t']);
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let body = &rest[1..];
        let Some(end) = body.find(quote) else {
            continue;
        };
        let target = &body[..end];
        if target.is_empty() || target.contains("#{") {
            continue;
        }
        let target = target.strip_suffix(".rb").unwrap_or(target);
        out.push((relative, target.to_string()));
    }
    out
}

/// Resolves `.` and `..` segments in a feature path textually; `None` when
/// `..` escapes the root (the file lives outside the load path).
fn normalize_feature(feature: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in feature.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
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
fn probe(
    root: &Path,
    zeo: &Path,
    gem: &Ready,
    timeout: Duration,
    tiers: Tiers,
    budget: crate::jobs::Budget,
) -> Verdict {
    let Ready {
        name,
        version,
        dir,
        deps,
        ..
    } = gem;
    let meta = read_spec_stamp(dir).unwrap_or_default();
    // The gem's real load path: its declared `require_paths`, kept to the
    // directories the archive actually ships. RubyGems filters the same way.
    let mut roots: Vec<PathBuf> = meta
        .require_paths
        .iter()
        .map(|rp| dir.join(rp))
        .filter(|p| p.is_dir())
        .collect();
    // Every declared root missing: discover where the Ruby actually lives
    // before giving up (`gem/`, `ruby/`, a nested `<name>/lib/`, bare files
    // at the archive root). The entry ladder below is unchanged; only the
    // load path is recovered. Discovered roots ALSO ride `-I` on the compile
    // -- zeo's own loader honours the gemspec's (missing) require_paths, so
    // without the flag the feature would defer to a runtime require and the
    // compile would measure nothing (the exact failure `entry_point`'s doc
    // guards against).
    let mut discovered: Vec<PathBuf> = Vec::new();
    if roots.is_empty() {
        roots = discovered_roots(dir);
        discovered = roots.clone();
    }
    // A gem with no load path anywhere can still publish EXECUTABLES; those
    // proceed to the program builder below, which `load`s them.
    if roots.is_empty() && ruby_executables(dir).is_empty() {
        return Verdict::stopped(Stage::Unpack, rootless_outcome(dir, &meta));
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
    // The name ladder first; when no file carries the gem's name, require
    // every top-level file the roots ship; when there is no top-level file
    // at all (the pre-convention `lib/<dir>/` layout), the gem's own require
    // graph names its roots; and when the gem publishes NO library at all,
    // its ruby-shebang executables are the surface (`load`ed by absolute
    // path, exactly as a RubyGems binstub runs them -- `resolve_load`
    // splices a literal absolute path statically). All four keep the rule
    // the doc on `entry_point` protects: every feature the program names is
    // a file that exists, so the compile measures the gem and not a guess.
    let features = match entry_point(&roots, name) {
        Some(feature) => vec![feature],
        None => {
            let mut all = top_level_features(&roots);
            if all.is_empty() {
                all = require_graph_root_features(&roots);
            }
            all
        }
    };
    let program = if features.is_empty() {
        let scripts = ruby_executables(dir);
        if scripts.is_empty() {
            return Verdict::stopped(Stage::Unpack, Outcome::NoEntryPoint);
        }
        scripts
            .iter()
            .map(|p| format!("load {:?}\n", p.to_string_lossy()))
            .collect::<String>()
    } else {
        features
            .iter()
            .map(|f| format!("require {f:?}\n"))
            .collect::<String>()
    };
    // Per-gem, so concurrent workers never share one. Removed on every exit
    // path below except the one that stores it.
    let emitted_path = std::env::temp_dir().join(format!("zeo-gem-probe-{name}-{version}.rs"));

    let mut cmd = std::process::Command::new(zeo);
    for r in &discovered {
        cmd.arg("-I").arg(r);
    }
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
        // The subject is the distinguished root (Bundler-root semantics): a
        // feature it provides resolves to IT, never to an alphabetically
        // earlier dependency or bundled copy squatting the same path.
        .arg("--root-gem")
        .arg(name)
        // `--emit-rust`, not `--dump=rust`. The two compile the same program;
        // what differs is who holds it. `--dump=rust` renders through `syn`
        // and prettyplease for a person to read -- two more whole-program
        // copies in the child -- and then writes it to a pipe this process
        // reads into memory, which for the largest gems was a gigabyte in each
        // of them. Streamed to a file, the child's peak drops by a third and
        // this side holds nothing.
        .arg(format!("--emit-rust={}", emitted_path.display()));
    budget.apply(&mut cmd);
    let started = std::time::Instant::now();
    let emitted = crate::exec::run_with_timeout(cmd, None, timeout);
    let codegen_ms = started.elapsed().as_millis();

    // Every early return below abandons the emitted file, so it is cleaned up
    // once here rather than at each of them.
    let discard = |v: Verdict| {
        let _ = std::fs::remove_file(&emitted_path);
        v
    };
    match emitted {
        // A spawn failure is a fact about this machine, not the registry:
        // recording it as `fetch-failed` sent readers to check the network
        // for a binary that would not start.
        Err(e) => {
            return discard(Verdict::stopped(
                Stage::Unpack,
                Outcome::ViewFailed(format!("running zeo: {e}")),
            ));
        }
        Ok(ex) if ex.timed_out => {
            // A stall names no pass -- it never got to say one -- so it is
            // recorded at the first front-end rung rather than a guessed one.
            return discard(Verdict::stopped(Stage::Parse, Outcome::Timeout).timed(codegen_ms));
        }
        // The ceiling exits with its own status so this needs no stderr
        // parsing, and the phase it names is the one useful thing to keep.
        Ok(ex) if hit_memory_limit(&ex) => {
            return discard(
                Verdict::stopped(Stage::Parse, Outcome::OutOfMemory(memory_phase(&ex.stderr)))
                    .timed(codegen_ms),
            );
        }
        // A successful compile still writes to stderr -- every builtin
        // substitution warns there -- so the exit status is the verdict and
        // stderr is only read once it is non-zero.
        Ok(ex) if ex.success() => {}
        Ok(ex) => {
            let text = String::from_utf8_lossy(&ex.stderr);
            let mut v = Verdict::stopped(front_end_stage(&text), classify_stderr(&ex.stderr, root));
            v.site = site_of(&text, root);
            return discard(v.timed(codegen_ms));
        }
    }

    let mut verdict = Verdict::stopped(Stage::Codegen, Outcome::Ok).timed(codegen_ms);
    // From the filesystem, not from a capture: the child streamed the program
    // to a file and neither process ever held it whole.
    verdict.rust_bytes = std::fs::metadata(&emitted_path).ok().map(|m| m.len());
    // The Rust is kept so a later sweep can climb the next rung without paying
    // for codegen again -- emit once for the whole corpus, then build. gzip
    // because the uncompressed corpus does not fit: the generated source is
    // large and repetitive, and a disk that cannot hold the store is a store
    // nobody keeps.
    if let Err(e) = store_rust(root, name, version, &emitted_path) {
        eprintln!("gem-probe: {name}: keeping the generated Rust: {e}");
    }
    let _ = std::fs::remove_file(&emitted_path);
    if !tiers.build {
        return verdict;
    }

    // The binary tier re-runs zeo with `-o` rather than handing the stored
    // Rust to rustc: the flags that pick the runtime profile and linkage live
    // in zeo, and reproducing them here would be a second source of truth for
    // how a zeo program is built.
    let out = std::env::temp_dir().join(format!("zeo-gem-probe-{name}-{version}"));
    let mut cmd = std::process::Command::new(zeo);
    for r in &discovered {
        cmd.arg("-I").arg(r);
    }
    cmd.arg("-e")
        .arg(&program)
        .arg("--gems")
        .arg(&view)
        .arg("--gems")
        .arg(root.join("gems"))
        .arg("--root-gem")
        .arg(name)
        .arg("-o")
        .arg(&out);
    budget.apply(&mut cmd);
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
        // Checked ahead of the generic failure arm: a ceiling breach is not
        // rustc rejecting the emitted Rust, and calling it one would put a
        // zeo bug in the ledger that nobody can reproduce.
        Ok(ex) if hit_memory_limit(&ex) => {
            verdict.stage = Stage::BuildsBin;
            verdict.outcome = Outcome::OutOfMemory(memory_phase(&ex.stderr));
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
/// the front end is all that runs no code and builds no artifact.
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

/// Whether the compiler gave up on its memory ceiling rather than on the gem.
///
/// The exit status is the signal, not the message: a child killed by the OS
/// reports no status of its own, so a status that IS reported and IS
/// `EXIT_MEMORY_LIMIT` can only have come from zeo saying so deliberately.
fn hit_memory_limit(ex: &crate::exec::Execution) -> bool {
    ex.status.and_then(|s| s.code()) == Some(zeo::memguard::EXIT_MEMORY_LIMIT)
}

/// The compile phase a ceiling breach named, for the ledger's detail column.
/// `"unknown"` if the marker line is not there -- the verdict stands on the
/// exit status either way.
fn memory_phase(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let Some(line) = text
        .lines()
        .find(|l| l.contains(zeo::memguard::BREACH_MARKER))
    else {
        return "unknown".to_string();
    };
    // `... reached <n> MiB in <phase>, over the ...`
    let Some(rest) = line.split(" in ").nth(1) else {
        return "unknown".to_string();
    };
    rest.split(',')
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string()
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

/// Whether a verdict is one no change to zeo can move, so `--failing` leaves
/// it alone.
///
/// `no-lib-dir`, `no-entry-point`, `meta-gem` and `ext-only` are facts about
/// what the gem's archive contains: the compiler never ran, and running a
/// newer one changes nothing. An `invalid-ruby` decided at `parse` is a fact
/// about the gem too -- prism is ruby's own parser, so those gems load under
/// no ruby either -- but one recorded at any OTHER stage is a legacy row that
/// never said which pass refused, and a re-probe still has to reach it.
///
/// `fetch-failed` is a fact about the REGISTRY: a 403 is a yanked gem and a
/// checksum mismatch is registry drift (both verified identical across sweeps
/// days apart), so every sweep that retried them spent a registry request per
/// row to re-learn the same answer -- 400+ dead downloads per `--failing` run.
/// A genuinely transient failure is re-reachable via `--refresh`, or by
/// naming the gem.
fn terminal_for_a_compiler_change(row: &Row) -> bool {
    matches!(
        row.outcome,
        Outcome::NoLibDir
            | Outcome::NoEntryPoint
            | Outcome::MetaGem
            | Outcome::ExtOnly
            | Outcome::PlatformGem(_)
            | Outcome::FetchFailed(_)
    ) || (matches!(row.outcome, Outcome::InvalidRuby(_)) && row.stage == Stage::Parse)
}

/// Which front-end pass rejected the gem, read off the diagnostic CODE zeo
/// already prints on the first line of a rejection.
///
/// The compiler names its own pass -- `zeo::parse`, `zeo::lower`,
/// `zeo::analyze`, `zeo::codegen` -- and the probe used to skip that line, so every front-end
/// failure landed in one bucket though the answer was sitting in the output.
/// The passes fail differently and are fixed differently: a lowering gap is a
/// construct the front end will not translate, an analyze rejection is a
/// definition it will not register, a codegen rejection is a position it will
/// not emit into.
///
/// A message with no code is a compile that died without a diagnostic (a
/// panic, a kill). `Codegen` -- the last rung -- is the honest answer there:
/// it got as far as anything can without saying otherwise, and the OUTCOME
/// column is what records that it died.
fn front_end_stage(stderr: &str) -> Stage {
    for line in stderr.lines().map(str::trim) {
        match line {
            "zeo::parse" => return Stage::Parse,
            "zeo::lower" => return Stage::Lower,
            "zeo::analyze" => return Stage::Analyze,
            "zeo::codegen" => return Stage::Codegen,
            _ => {}
        }
    }
    Stage::Codegen
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
    // The `require_relative "x.so"` spelling of the same verdict: an
    // UNPROTECTED native require (no `rescue LoadError`), i.e. a gem whose
    // pure-Ruby half insists on its native half. The rescued spelling never
    // errors (it defers to a catchable runtime LoadError), so every gem that
    // reaches this line is one zeo cannot run until an ext exists -- a fact
    // about the gem, not a lowering gap.
    if msg.contains("native (.so/.bundle) features aren't supported") {
        return Outcome::NativeExtension;
    }
    if let Some(rest) = msg.split("cannot load such file -- ").nth(1) {
        let feature = rest.split_whitespace().next().unwrap_or(rest);
        return Outcome::MissingDependency(feature.trim_matches(['`', ':', '.']).to_string());
    }
    // An ambiguity is the probe's own artifact -- the subject gem vendors a
    // file a bundled gem also provides, and only the probe puts both on one
    // load path. Counting it as a lowering gap overstated the backlog by 642
    // rows.
    if msg.contains("is ambiguous: found in multiple gems") {
        return Outcome::AmbiguousRequire(truncate(&msg));
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

/// Where the emitted Rust is kept, so climbing to `build` later does not
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
///
/// Copied from `emitted` a block at a time rather than from a `&[u8]`: the
/// program is up to a gigabyte, and the point of streaming it to a file was
/// that no process has to hold it whole.
fn store_rust(root: &Path, name: &str, version: &str, emitted: &Path) -> Result<(), String> {
    let dir = rust_store(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut src = std::fs::File::open(emitted).map_err(|e| e.to_string())?;
    let file = std::fs::File::create(dir.join(format!("{name}-{version}.rs.gz")))
        .map_err(|e| e.to_string())?;
    let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    std::io::copy(&mut src, &mut gz).map_err(|e| e.to_string())?;
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
///   - the `stage` column read as a claim, because a reader in a file called
///     `fails` infers pass/fail from the FILE and reads the stage on its own.
///     Beside `outcome` it is unambiguous: `codegen ok` against `codegen
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

/// The corpus counts every published number derives from, computed once per
/// ledger write so the TSV, `gem-probe.md` and the README block cannot drift
/// from each other.
///
/// The buckets partition the probed rows. Each one answers a different
/// question, and the split keeps the headline honest: `no-entry-point` is a
/// fact about the HARNESS, `invalid-ruby` is a fact about the GEM, and only
/// `lowering-gap`/`compiler-panic`/`rustc-error` count against zeo.
struct LedgerStats {
    /// Every name the ledger knows, frontier included.
    names: usize,
    /// Rows with a verdict -- everything but `unprobed`.
    probed: usize,
    ok: usize,
    /// The harness never carried the gem to a compiler verdict:
    /// `no-lib-dir`, `no-entry-point`, `fetch-failed`, `view-failed`,
    /// `ambiguous-require`.
    harness: usize,
    /// Nothing a Ruby compiler can compile: `invalid-ruby`,
    /// `native-extension`, `ext-only`, `meta-gem`.
    not_ruby: usize,
    /// zeo's to fix: `lowering-gap`, `compiler-panic`, `rustc-error`.
    compiler: usize,
    /// A `require` the probe's view did not satisfy.
    missing_dep: usize,
    /// No verdict was reached: `timeout`, `out-of-memory`.
    no_verdict: usize,
    /// `run-failed` -- the opt-in build/run tier.
    run_failed: usize,
    /// `(stage tag, outcome tag) -> count` over the probed rows.
    by_stage_outcome: BTreeMap<(&'static str, &'static str), usize>,
}

impl LedgerStats {
    fn from_rows(rows: &BTreeMap<String, Row>) -> LedgerStats {
        let mut s = LedgerStats {
            names: rows.len(),
            probed: 0,
            ok: 0,
            harness: 0,
            not_ruby: 0,
            compiler: 0,
            missing_dep: 0,
            no_verdict: 0,
            run_failed: 0,
            by_stage_outcome: BTreeMap::new(),
        };
        for r in rows.values() {
            match &r.outcome {
                Outcome::Unprobed => continue,
                Outcome::Ok => s.ok += 1,
                Outcome::NoLibDir
                | Outcome::NoEntryPoint
                | Outcome::FetchFailed(_)
                | Outcome::ViewFailed(_)
                | Outcome::AmbiguousRequire(_) => s.harness += 1,
                Outcome::InvalidRuby(_)
                | Outcome::NativeExtension
                | Outcome::MetaGem
                | Outcome::ExtOnly
                | Outcome::PlatformGem(_) => s.not_ruby += 1,
                Outcome::LoweringGap(_) | Outcome::CompilerPanic(_) | Outcome::RustcError(_) => {
                    s.compiler += 1
                }
                Outcome::MissingDependency(_) => s.missing_dep += 1,
                Outcome::Timeout | Outcome::OutOfMemory(_) => s.no_verdict += 1,
                Outcome::RunFailed(_) => s.run_failed += 1,
            }
            s.probed += 1;
            *s.by_stage_outcome
                .entry((r.stage.tag(), r.outcome.tag()))
                .or_default() += 1;
        }
        s
    }

    /// The rows zeo could attempt: probed, minus the rows the harness never
    /// carried to the compiler, minus the gems no Ruby loads.
    fn attempted(&self) -> usize {
        self.probed - self.harness - self.not_ruby
    }
}

/// `1234567` -> `1,234,567`. The README quotes corpus-scale numbers, and six
/// undelimited digits misread by a factor of ten.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "\u{2014}".to_string();
    }
    format!("{:.1}%", part as f64 * 100.0 / whole as f64)
}

/// The markdown the published numbers live in. One renderer serves the README
/// block and `gem-probe.md`, so the two files always agree.
fn render_stats_block(s: &LedgerStats) -> String {
    let row = |label: &str, n: usize, share: String| {
        format!("| {label} | {} | {share} |\n", thousands(n))
    };
    let mut md = String::new();
    md.push_str(&format!(
        "**{} of {} probed gems compile to Rust ({}).** The probe runs zeo's full \
         front end (parse, lower, analyze, codegen) on the newest release of every gem \
         on rubygems.org. `ok` means zeo produced Rust; no rustc ran. One row per gem \
         in [`conformance/gem-probe.tsv`](conformance/gem-probe.tsv).\n\n",
        thousands(s.ok),
        thousands(s.probed),
        percent(s.ok, s.probed),
    ));
    md.push_str("| Verdict | Gems | Share of probed |\n|---|---|---|\n");
    md.push_str(&row(
        "Compile to Rust (`ok`)",
        s.ok,
        percent(s.ok, s.probed),
    ));
    md.push_str(&row(
        "Compiler gaps, zeo's to fix (`lowering-gap`, `compiler-panic`, `rustc-error`)",
        s.compiler,
        percent(s.compiler, s.probed),
    ));
    md.push_str(&row(
        "Unresolved dependency in the probe's view (`missing-dependency`)",
        s.missing_dep,
        percent(s.missing_dep, s.probed),
    ));
    md.push_str(&row(
        "Harness limits, not compiler verdicts (`no-entry-point`, `no-lib-dir`, `fetch-failed`, `view-failed`, `ambiguous-require`)",
        s.harness,
        percent(s.harness, s.probed),
    ));
    md.push_str(&row(
        "Nothing to compile (`invalid-ruby`, `native-extension`, `ext-only`, `meta-gem`, `platform-gem`)",
        s.not_ruby,
        percent(s.not_ruby, s.probed),
    ));
    md.push_str(&row(
        "No verdict reached (`timeout`, `out-of-memory`)",
        s.no_verdict,
        percent(s.no_verdict, s.probed),
    ));
    if s.run_failed > 0 {
        md.push_str(&row(
            "Built but did not run (`run-failed`)",
            s.run_failed,
            percent(s.run_failed, s.probed),
        ));
    }
    md.push_str(&format!(
        "\nOf the {} gems zeo can attempt -- the probed set minus the harness limits and \
         the gems no Ruby loads -- **{} compile ({})**.\n",
        thousands(s.attempted()),
        thousands(s.ok),
        percent(s.ok, s.attempted()),
    ));
    md
}

const README_STATS_BEGIN: &str = "<!-- gem-probe-stats:begin -->";
const README_STATS_END: &str = "<!-- gem-probe-stats:end -->";

/// Replaces the marked block in `text` with `block`, or answers `None` when
/// the markers are absent or out of order. Pure, so the tests can pin it.
fn splice_readme_stats(text: &str, block: &str) -> Option<String> {
    let begin = text.find(README_STATS_BEGIN)?;
    let end_at = text[begin..].find(README_STATS_END)? + begin;
    let mut out = String::with_capacity(text.len() + block.len());
    out.push_str(&text[..begin + README_STATS_BEGIN.len()]);
    out.push('\n');
    out.push_str(block);
    out.push_str(&text[end_at..]);
    Some(out)
}

/// The `--check` half of the README contract: fails when the stats block and
/// the ledger disagree -- a hand-edit, or a README that lost its markers. The
/// rows are filtered the same way `write_ledger` filters them, so the two
/// sides compare the same corpus.
fn check_readme_fresh(root: &Path, rows: &BTreeMap<String, Row>) -> Result<(), String> {
    let ignored = read_ignored(root);
    let published: BTreeMap<String, Row> = rows
        .iter()
        .filter(|(name, _)| !ignored.contains_key(*name))
        .map(|(n, r)| (n.clone(), r.clone()))
        .collect();
    let block = render_stats_block(&LedgerStats::from_rows(&published));
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap_or_default();
    match splice_readme_stats(&readme, &block) {
        None => Err(format!(
            "README.md has no `{README_STATS_BEGIN}` block to carry the ledger's numbers"
        )),
        Some(updated) if updated != readme => {
            Err("the README stats block disagrees with the ledger -- run \
             `cargo xtask gem-probe --sync-readme` to regenerate it"
                .to_string())
        }
        Some(_) => Ok(()),
    }
}

/// Rewrites the README's stats block from the rows just written. A README
/// without the markers is left alone -- the block is opt-in per checkout, and
/// a scratch root in the tests has no README at all.
fn write_readme_stats(root: &Path, rows: &BTreeMap<String, Row>) -> Result<(), String> {
    let path = root.join("README.md");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let block = render_stats_block(&LedgerStats::from_rows(rows));
    let Some(updated) = splice_readme_stats(&text, &block) else {
        return Ok(());
    };
    if updated != text {
        std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    }
    Ok(())
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

    let stats = LedgerStats::from_rows(rows);
    let mut md = String::from("# Gem probe results\n\nGenerated by `cargo xtask gem-probe`.\n\n");
    md.push_str(
        "`gem-probe.tsv` beside this file holds ONE row per gem. Two columns carry the \
         verdict and they must be read together: `stage` is the RUNG the row is about, and \
         `outcome` is what happened there. `codegen ok` and `codegen lowering-gap` are the \
         same rung with opposite results -- the stage alone claims nothing.\n\n\
         The ladder is `queued -> fetch -> unpack -> parse -> lower -> analyze -> codegen \
         -> build -> run`. The four middle rungs are zeo's own front-end passes, and a \
         rejection is recorded at the pass that MADE it -- zeo prints that as the \
         diagnostic's code, so a front-end failure says which pass refused rather than \
         landing in one bucket. They are rungs rather than columns because the ladder \
         terminates: the pass a row names implies success at every pass before it, and \
         that no pass after it was attempted.\n\n\
         **`codegen ok` means zeo produced Rust, and nothing more.** No rustc ran, no \
         binary exists, and the gem's own code may not have been compiled at all -- zeo can \
         decline a unit and defer it to a runtime `LoadError`, which only the `run` stage \
         sees. `build` and `run` are opt-in (`--build`, `--run`) and a sweep does not \
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
    md.push_str(&format!(
        "{} of {} names known have a verdict.\n\n",
        thousands(stats.probed),
        thousands(stats.names)
    ));
    md.push_str(&render_stats_block(&stats));
    md.push_str("\n| Stage | Outcome | Gems |\n|---|---|---|\n");
    for ((stage, tag), n) in &stats.by_stage_outcome {
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
    std::fs::write(root.join("conformance/gem-probe.md"), md).map_err(|e| e.to_string())?;
    write_readme_stats(root, rows)
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
        Err(e) => {
            // The CDN refuses `<name>-<version>.gem` with a 403 when that
            // version shipped only prebuilt platform artifacts -- there is no
            // pure-Ruby gem behind the plain name at all. Ask the versions
            // list before writing `fetch-failed`, so the row names the real
            // fact about the release rather than sending readers to check the
            // network.
            let outcome = match e.contains("status: 403") {
                true => match ruby_platform_absent(name, &version) {
                    Some(platforms) => Outcome::PlatformGem(platforms),
                    None => Outcome::FetchFailed(e),
                },
                false => Outcome::FetchFailed(e),
            };
            Err(Box::new(Row::stopped(
                &version,
                Stage::Fetch,
                outcome,
                digest,
            )))
        }
    }
}

/// When rubygems carries `version` only under non-`ruby` platforms, answers
/// the platform list; otherwise `None` (including on any network failure, so
/// a flaky request never upgrades a `fetch-failed` into a claim).
fn ruby_platform_absent(name: &str, version: &str) -> Option<String> {
    let v = json(&format!("{REGISTRY}/api/v1/versions/{name}.json")).ok()?;
    let platforms: Vec<String> = v
        .as_array()?
        .iter()
        .filter(|e| e.get("number").and_then(|n| n.as_str()) == Some(version))
        .filter_map(|e| e.get("platform").and_then(|p| p.as_str()))
        .map(String::from)
        .collect();
    if platforms.is_empty() || platforms.iter().any(|p| p == "ruby") {
        return None;
    }
    let mut platforms = platforms;
    platforms.sort();
    platforms.dedup();
    Some(platforms.join(", "))
}

/// `95` -> `"1m35s"`, `4230` -> `"1h10m"` -- the sweep's remaining-time
/// estimate, coarse on purpose (it is an average over gems whose individual
/// compile times spread across three orders of magnitude).
fn human_duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60),
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
    let mut sync_readme = false;
    let (mut index, mut refresh, mut refresh_index) = (false, false, false);
    let (mut limit, mut popular): (Option<usize>, Option<usize>) = (None, None);
    let mut matching: Option<String> = None;
    let mut matching_name: Option<String> = None;
    let mut outcome_filter: Option<String> = None;
    let mut failing = false;
    let (mut unprobed, mut seed_index) = (false, false);
    let mut positional: Vec<String> = Vec::new();
    let mut jobs: Option<usize> = None;
    let mut timeout = DEFAULT_TIMEOUT;
    let (mut build, mut run, mut allow_run) = (false, false, false);
    let mut zeo_override: Option<PathBuf> = None;
    let mut rebuild_zeo = false;

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
            "--sync-readme" => sync_readme = true,
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
                Some(n) if n >= 1 => jobs = Some(n),
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
            "--outcome" => match it.next() {
                Some(v) => outcome_filter = Some(v.clone()),
                None => {
                    eprintln!("gem-probe: --outcome needs an outcome tag (e.g. no-entry-point)");
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
            "--zeo" => match it.next() {
                Some(v) => zeo_override = Some(PathBuf::from(v)),
                None => {
                    eprintln!("gem-probe: --zeo needs a path to a zeo binary");
                    return ExitCode::FAILURE;
                }
            },
            "--rebuild-zeo" => rebuild_zeo = true,
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
            ("--outcome", outcome_filter.is_some()),
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
    // Rewrites every file the ledger derives -- `gem-probe.md` and the README
    // stats block -- from the rows already on disk, probing nothing. This is
    // how the derived files catch up after a generator change.
    if sync_readme {
        if let Err(e) = write_ledger(root, &before) {
            eprintln!("gem-probe: syncing the derived files: {e}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }
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
                //
                // A row the COMPILER cannot change is skipped too, unless
                // `--refresh` asks for it: `no-lib-dir` and `no-entry-point`
                // are facts about what the archive contains, and re-probing
                // them spends a registry request each to learn what the row
                // already says. They are 10,780 of the 16,238 non-`ok` rows.
                .filter(|(_, r)| {
                    r.outcome != Outcome::Ok
                        && r.outcome != Outcome::Unprobed
                        && (refresh || !terminal_for_a_compiler_change(r))
                })
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
    // The bucket twin of `--matching`, for outcomes whose detail is empty
    // (`no-entry-point`, `no-lib-dir`) -- reachable after a HARNESS change
    // where `--failing` deliberately skips them as compiler-terminal.
    if let Some(tag) = &outcome_filter {
        names.extend(
            before
                .iter()
                .filter(|(_, r)| r.outcome.tag() == tag.as_str())
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
        // A bare `--check` still gates the derived files: nothing probed
        // means the ledger is exactly what `before` read.
        if check {
            return match check_readme_fresh(root, &before) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("gem-probe --check: {e}");
                    ExitCode::FAILURE
                }
            };
        }
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

    // The classifier is a SNAPSHOT of the `zeo` binary, not the live
    // `target/debug/zeo`: a dev rebuild mid-sweep would otherwise hand later
    // verdicts to a different compiler than earlier ones, and the cargo
    // invocation itself contended with dev builds. Every verdict in a sweep
    // comes from ONE binary, and the sweep says which.
    //
    //   --zeo <path>     probe with exactly that binary, build nothing
    //   (default)        reuse target/probe-bin/zeo-<HEAD sha> if present;
    //                    build + snapshot it if not
    //   --rebuild-zeo    force a fresh build + snapshot for HEAD
    //
    // A sweep started mid-implementation therefore reuses the last snapshot
    // and never touches cargo at all.
    let zeo = match zeo_override {
        Some(path) => {
            if !path.is_file() {
                eprintln!("gem-probe: --zeo {}: no such binary", path.display());
                return ExitCode::FAILURE;
            }
            path
        }
        None => {
            let sha = std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .current_dir(root)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|| "unversioned".to_string());
            let bin_dir = root.join("target").join("probe-bin");
            let snapshot = bin_dir.join(format!("zeo-{sha}"));
            if rebuild_zeo || !snapshot.is_file() {
                eprintln!(
                    "gem-probe: building zeo for snapshot {}...",
                    snapshot.display()
                );
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
                if let Err(e) = std::fs::create_dir_all(&bin_dir) {
                    eprintln!("gem-probe: creating {}: {e}", bin_dir.display());
                    return ExitCode::FAILURE;
                }
                // Prune older snapshots first (a debug zeo is large, and the
                // target/ tree is already on a wipe cadence) -- then copy.
                if let Ok(entries) = std::fs::read_dir(&bin_dir) {
                    for entry in entries.flatten() {
                        if entry.path() != snapshot {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
                let live = root.join("target").join("debug").join("zeo");
                if let Err(e) = std::fs::copy(&live, &snapshot) {
                    eprintln!("gem-probe: snapshotting {}: {e}", snapshot.display());
                    return ExitCode::FAILURE;
                }
            }
            snapshot
        }
    };
    eprintln!("gem-probe: probing with {}", zeo.display());

    let mut rows = before.clone();
    if skipped > 0 {
        println!("{skipped} gem(s) already recorded; --refresh re-probes them");
    }
    let total = names.len();
    let budget = crate::jobs::Budget::derive(jobs);
    println!("probing {total} gem(s) with {}:", budget.describe());

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

        for _ in 0..budget.jobs {
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
                    let verdict = probe(root, zeo, &ready, timeout, tiers, budget);
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
        let started = std::time::Instant::now();
        while let Ok((name, row, verdict)) = done_rx.recv() {
            seen += 1;
            // Rate and remaining-time estimate ride on every line. The
            // first few verdicts land in a burst (the prep thread's head
            // start), so the estimate holds back until the rate means
            // something.
            let eta = match seen {
                0..=9 => String::new(),
                _ => {
                    let per = started.elapsed().as_secs_f64() / seen as f64;
                    let left = (per * (total - seen) as f64) as u64;
                    format!(" ~{} left", human_duration(left))
                }
            };
            print!("[{seen}/{total}{eta}]");
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
        reached(Stage::Codegen),
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
        // The README quotes the ledger's numbers from a generated block, and a
        // number quoted by hand goes stale the day after it is written.
        if let Err(e) = check_readme_fresh(root, &rows) {
            eprintln!("gem-probe --check: {e}");
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
        write_spec_stamp(&dir, &GemMeta::default()).unwrap();
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

    /// `--failing` is "re-check what a fix moved", so it must not spend a
    /// registry request per gem on rows a compiler change cannot reach. Those
    /// are the archive facts, plus a parse rejection the parser already
    /// placed -- a gap, a panic, a missing dependency and a LEGACY parse
    /// rejection (no recorded pass) all still have to be measured.
    #[test]
    fn only_the_archive_facts_are_terminal() {
        for archive_fact in [
            Outcome::NoLibDir,
            Outcome::NoEntryPoint,
            Outcome::MetaGem,
            Outcome::ExtOnly,
            Outcome::PlatformGem("x86_64-linux".into()),
        ] {
            assert!(terminal_for_a_compiler_change(&row_with(
                Stage::Unpack,
                archive_fact
            )));
        }
        assert!(terminal_for_a_compiler_change(&row_with(
            Stage::Parse,
            Outcome::InvalidRuby("parse error".into())
        )));
        // A registry fact: a 403 is a yanked gem, a checksum mismatch is
        // registry drift. Only `--refresh` (or naming the gem) retries one.
        assert!(terminal_for_a_compiler_change(&row_with(
            Stage::Fetch,
            Outcome::FetchFailed("403".into())
        )));
        for movable in [
            Outcome::LoweringGap("a gap".into()),
            Outcome::MissingDependency("x".into()),
            // A legacy row: `invalid-ruby` with no pass recorded.
            Outcome::InvalidRuby("parse error".into()),
            Outcome::CompilerPanic("boom".into()),
            Outcome::NativeExtension,
            Outcome::Timeout,
            Outcome::OutOfMemory("codegen".into()),
            Outcome::AmbiguousRequire("`require \"x\"` is ambiguous".into()),
        ] {
            assert!(
                !terminal_for_a_compiler_change(&row_with(Stage::Codegen, movable.clone())),
                "{movable:?} must still be re-probed"
            );
        }
    }

    /// zeo names the pass that rejected on the first line of every diagnostic,
    /// and the probe used to skip that line -- so every front-end failure
    /// landed in one bucket though the compiler had already said which pass it
    /// was. These are the four codes it emits, verbatim.
    #[test]
    fn the_front_end_rung_comes_from_the_compilers_own_code() {
        let boxed = |code: &str| {
            format!("{code}\n\n  \u{d7} something it refused\n   \u{256d}\u{2500}[-e:1:1]\n")
        };
        assert_eq!(front_end_stage(&boxed("zeo::parse")), Stage::Parse);
        assert_eq!(front_end_stage(&boxed("zeo::lower")), Stage::Lower);
        assert_eq!(front_end_stage(&boxed("zeo::analyze")), Stage::Analyze);
        assert_eq!(front_end_stage(&boxed("zeo::codegen")), Stage::Codegen);
        // No code at all: a compile that died without a diagnostic. The last
        // rung is the honest answer -- the OUTCOME is what records the death.
        assert_eq!(
            front_end_stage("thread 'main' panicked at src/lib.rs:1:1:\nboom\n"),
            Stage::Codegen
        );
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
            Row::stopped("1.0.0", Stage::Codegen, Outcome::Ok, None),
        );
        write_ledger(&root, &rows).unwrap();

        let back = read_ledger(&root).unwrap();
        assert_eq!(back["waiting"].outcome, Outcome::Unprobed);
        assert_eq!(back["waiting"].stage, Stage::Queued);
        assert_eq!(back["waiting"].version, "");
        assert!(Stage::Queued < Stage::Fetch, "the frontier is the bottom");
        assert!(back["waiting"].stage < Stage::Codegen);
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
                Row::stopped("1.0.0", Stage::Codegen, Outcome::Ok, None),
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

    /// An ambiguity is the probe's own artifact -- the subject vendors a file
    /// a bundled gem also ships, and only the probe's view holds both.
    /// Filing it as a lowering gap put 642 harness rows in zeo's backlog.
    #[test]
    fn an_ambiguous_require_is_not_a_lowering_gap() {
        let root = Path::new("/tmp/none");
        let msg = "`require \"openssl\"` is ambiguous: found in multiple gems \
                   (jruby-openssl, openssl)";
        match classify(msg, root) {
            Outcome::AmbiguousRequire(d) => assert!(d.contains("jruby-openssl"), "{d}"),
            other => panic!("expected an ambiguity, got {other:?}"),
        }
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
        assert_eq!(back["alpha"].stage, Stage::Codegen);
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
        assert!(Stage::Unpack < Stage::Codegen);
        // The front end in the order zeo runs it, so a gem that used to stop
        // at `analyze` and now stops at `codegen` reads as the progress it is.
        assert!(Stage::Unpack < Stage::Parse);
        assert!(Stage::Parse < Stage::Lower);
        assert!(Stage::Lower < Stage::Analyze);
        assert!(Stage::Analyze < Stage::Codegen);
        assert!(Stage::Codegen < Stage::BuildsBin);
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

    /// The same rule one step out: a memory kill is a fact about the host, and
    /// decaying it into `LoweringGap` would put a diagnosis zeo never made in
    /// the ledger AND hide that the row is re-probeable on a leaner compiler.
    #[test]
    fn an_out_of_memory_round_trips_as_itself() {
        let it = Outcome::OutOfMemory("codegen".into());
        assert_eq!(Outcome::from_ledger(it.tag(), it.detail()), it);
    }

    /// The phase comes off the compiler's own marker line, so a change to
    /// either side is caught here rather than in a sweep's detail column.
    #[test]
    fn the_memory_phase_comes_from_the_breach_line() {
        let line = format!(
            "{} -e: reached 9812.0 MiB in codegen, over the 8192.0 MiB ceiling\n",
            zeo::memguard::BREACH_MARKER
        );
        assert_eq!(memory_phase(line.as_bytes()), "codegen");
        assert_eq!(memory_phase(b"something else entirely\n"), "unknown");
    }

    fn lib_with(root: &Path, name: &str, files: &[&str]) -> Vec<PathBuf> {
        let dir = vendor_dir(root).join(name);
        for f in files {
            let p = dir.join("lib").join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "# x\n").unwrap();
        }
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        vec![dir.join("lib")]
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

    /// A CLI-only gem's published surface is its ruby-shebang executables;
    /// a shell script or a binary in `bin/` is nobody's Ruby.
    #[test]
    fn ruby_executables_read_the_shebang() {
        let root = scratch("exe-gems");
        let dir = vendor_dir(&root).join("cli-only");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/tool"), "#!/usr/bin/env ruby\nputs 1\n").unwrap();
        std::fs::write(dir.join("bin/helper.sh"), "#!/bin/sh\necho 1\n").unwrap();
        std::fs::create_dir_all(dir.join("exe")).unwrap();
        std::fs::write(dir.join("exe/other"), "#!/usr/bin/ruby -w\nputs 2\n").unwrap();
        assert_eq!(
            ruby_executables(&dir),
            vec![dir.join("bin/tool"), dir.join("exe/other")]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A symlink cycle in the archive must not spin the walk -- one did, on
    /// the last gem of the first entry-point sweep.
    #[test]
    #[cfg(unix)]
    fn a_symlink_cycle_does_not_spin_the_feature_walk() {
        let root = scratch("symlink-cycle");
        let libs = lib_with(&root, "loopy", &["ns/real.rb"]);
        std::os::unix::fs::symlink(libs[0].join("ns"), libs[0].join("ns/back")).unwrap();
        assert_eq!(
            require_graph_root_features(&libs),
            vec!["ns/real".to_string()]
        );
        assert!(ships_ruby(&libs[0]));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A gem whose declared require_paths don't exist in the archive: the
    /// roots are discovered from where the Ruby actually lives.
    #[test]
    fn discovered_roots_recover_the_load_path_a_gemspec_lost() {
        let root = scratch("rootless");
        // Bare files at the archive root: the gem dir IS the load path.
        let bare = vendor_dir(&root).join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("bare.rb"), "# x\n").unwrap();
        assert_eq!(discovered_roots(&bare), vec![bare.clone()]);

        // Packed one directory too deep: `<name>/lib` is the root.
        let deep = vendor_dir(&root).join("deep");
        std::fs::create_dir_all(deep.join("deep/lib")).unwrap();
        std::fs::write(deep.join("deep/lib/deep.rb"), "# x\n").unwrap();
        assert_eq!(discovered_roots(&deep), vec![deep.join("deep/lib")]);

        // A code-shaped directory under an unconventional name.
        let odd = vendor_dir(&root).join("odd");
        std::fs::create_dir_all(odd.join("gem")).unwrap();
        std::fs::write(odd.join("gem/odd.rb"), "# x\n").unwrap();
        assert_eq!(discovered_roots(&odd), vec![odd.join("gem")]);

        // Tests-only ships nothing loadable: no root, honestly.
        let tests = vendor_dir(&root).join("tests-only");
        std::fs::create_dir_all(tests.join("spec")).unwrap();
        std::fs::write(tests.join("spec/x_spec.rb"), "# x\n").unwrap();
        assert_eq!(discovered_roots(&tests), Vec::<PathBuf>::new());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The pre-convention layout: `lib/<dir>/...` with no top-level file and
    /// no name match anywhere. The gem's own require graph names the entry:
    /// the file no sibling requires that reaches the rest.
    #[test]
    fn the_require_graph_names_the_entry_when_no_convention_does() {
        let root = scratch("entry-graph");
        let libs = lib_with(
            &root,
            "360_services",
            &["sorenson/base.rb", "sorenson/client.rb", "sorenson/util.rb"],
        );
        std::fs::write(
            libs[0].join("sorenson/base.rb"),
            "require \"sorenson/util\"\nrequire_relative \"client\"\n",
        )
        .unwrap();
        assert_eq!(
            require_graph_root_features(&libs),
            vec!["sorenson/base".to_string()]
        );

        // Two independent roots tie on coverage: both are published, the
        // same answer several top-level files get.
        let twin = lib_with(&root, "twin", &["ns/alpha.rb", "ns/beta.rb"]);
        assert_eq!(
            require_graph_root_features(&twin),
            vec!["ns/alpha".to_string(), "ns/beta".to_string()]
        );

        // The dominant root wins over a stray helper that neither requires
        // nor is required... unless the helper ties, which `twin` covers.
        let dom = lib_with(
            &root,
            "dom",
            &["ns/main.rb", "ns/a.rb", "ns/b.rb", "ns/stray.rb"],
        );
        std::fs::write(
            dom[0].join("ns/main.rb"),
            "require \"ns/a\"\nrequire \"ns/b\"\n",
        )
        .unwrap();
        assert_eq!(
            require_graph_root_features(&dom),
            vec!["ns/main".to_string()]
        );

        // A dependency's feature contributes no edge; `..` escaping the root
        // contributes none either; a full cycle has no root and declines.
        let cyc = lib_with(&root, "cyc", &["ns/a.rb", "ns/b.rb"]);
        std::fs::write(cyc[0].join("ns/a.rb"), "require \"ns/b\"\n").unwrap();
        std::fs::write(cyc[0].join("ns/b.rb"), "require \"ns/a\"\n").unwrap();
        assert_eq!(require_graph_root_features(&cyc), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn literal_requires_reads_only_literal_single_arguments() {
        let src = r#"
require "plain"
require 'single'
require("parens")
require_relative "sibling"
require_relative '../up'
require "interp#{x}"
require variable
requires_grid "not_a_require"
        "#;
        assert_eq!(
            literal_requires(src),
            vec![
                (false, "plain".to_string()),
                (false, "single".to_string()),
                (false, "parens".to_string()),
                (true, "sibling".to_string()),
                (true, "../up".to_string()),
            ]
        );
        assert_eq!(normalize_feature("ns/../up"), Some("up".to_string()));
        assert_eq!(normalize_feature("../escape"), None);
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
        // A real serialized spec, the shape rubygems writes.
        let mut metadata = Vec::new();
        {
            use std::io::Write;
            let mut enc =
                flate2::write::GzEncoder::new(&mut metadata, flate2::Compression::default());
            enc.write_all(
                b"--- !ruby/object:Gem::Specification\nname: thing\nplatform: ruby\n\
                  require_paths:\n- lib\nextensions: []\n",
            )
            .unwrap();
            enc.finish().unwrap();
        }
        let mut outer = Vec::new();
        {
            let mut b = tar::Builder::new(&mut outer);
            for (name, body) in [
                ("metadata.gz", metadata.as_slice()),
                ("data.tar.gz", &inner),
            ] {
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
        // The registry's own spec rides along: `metadata.gz` is captured at
        // unpack time, so `require_paths` never has to be guessed again.
        assert_eq!(read_spec_stamp(&dest), Some(GemMeta::default()));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `metadata.gz` is machine-written YAML with a fixed shape; the three
    /// fields the probe keeps come out of it without a YAML parser.
    #[test]
    fn the_metadata_yaml_gives_up_its_three_fields() {
        let meta = parse_gem_metadata_yaml(
            "--- !ruby/object:Gem::Specification\nname: concurrent-ruby\n\
             version: !ruby/object:Gem::Version\n  version: 1.3.4\n\
             platform: ruby\nauthors:\n- Jerry D'Antonio\n\
             require_paths:\n- lib/concurrent-ruby\n- 'ext'\n\
             extensions:\n- ext/foo/extconf.rb\nlicenses:\n- MIT\n",
        );
        assert_eq!(meta.require_paths, vec!["lib/concurrent-ruby", "ext"]);
        assert_eq!(meta.platform, "ruby");
        assert_eq!(meta.extensions, vec!["ext/foo/extconf.rb"]);

        // Empty inline lists and absent keys fall back to rubygems' defaults.
        let bare = parse_gem_metadata_yaml("name: tiny\nextensions: []\n");
        assert_eq!(bare, GemMeta::default());
    }

    /// Three facts used to share the `no-lib-dir` tag. Only one of them ever
    /// had Ruby the compiler could reach.
    #[test]
    fn a_rootless_gem_names_which_fact_stopped_it() {
        let root = scratch("rootless");
        let dir = root.join("g");
        std::fs::create_dir_all(&dir).unwrap();

        // No files at all: a metagem.
        assert_eq!(
            rootless_outcome(&dir, &GemMeta::default()),
            Outcome::MetaGem
        );
        // Declared extensions make it ext-only, whatever else it ships.
        let ext = GemMeta {
            extensions: vec!["extconf.rb".into()],
            ..GemMeta::default()
        };
        assert_eq!(rootless_outcome(&dir, &ext), Outcome::ExtOnly);
        // An `ext/` tree says the same without the declaration.
        std::fs::create_dir_all(dir.join("ext")).unwrap();
        assert_eq!(
            rootless_outcome(&dir, &GemMeta::default()),
            Outcome::ExtOnly
        );
        std::fs::remove_dir(dir.join("ext")).unwrap();
        // Ruby outside every declared root: the load path is empty as
        // packaged, which is what `no-lib-dir` now means.
        std::fs::write(dir.join("loose.rb"), "# x\n").unwrap();
        assert_eq!(
            rootless_outcome(&dir, &GemMeta::default()),
            Outcome::NoLibDir
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A gem whose files match no name ladder is probed through everything
    /// its roots publish, not declined.
    #[test]
    fn the_fallback_program_requires_every_top_level_file() {
        let root = scratch("fallback");
        let roots = lib_with(&root, "mystery", &["alpha.rb", "beta.rb", "sub/inner.rb"]);
        assert_eq!(entry_point(&roots, "mystery"), None);
        assert_eq!(top_level_features(&roots), vec!["alpha", "beta"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `require_paths` other than `lib` root the same ladder: the entry point
    /// and the stub both follow the gem's declared load path.
    #[test]
    fn a_declared_root_other_than_lib_is_honoured() {
        let root = scratch("roots");
        let dir = vendor_dir(&root).join("nested");
        let real_root = dir.join("lib/concurrent-ruby");
        std::fs::create_dir_all(&real_root).unwrap();
        std::fs::write(real_root.join("nested.rb"), "# x\n").unwrap();
        assert_eq!(
            entry_point(&[real_root], "nested").as_deref(),
            Some("nested")
        );
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
        write_stub_gemspec(&stubbed, "a", "1.0.0", &["lib".to_string()]).unwrap();
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
        write_spec_stamp(
            &src,
            &GemMeta {
                require_paths: vec!["lib".into(), "lib/thing".into()],
                ..GemMeta::default()
            },
        )
        .unwrap();

        let view = isolate(&root, "thing", &[]).unwrap();
        let seen = view.join("thing");

        // The view: a stub carrying the version AND the captured
        // require_paths, and no trace of the computed-version original.
        let stub = std::fs::read_to_string(seen.join("thing.gemspec")).unwrap();
        assert!(stub.contains(r#"s.version = "4.5.6".freeze"#), "{stub}");
        assert!(stub.contains(r#"s.name = "thing".freeze"#), "{stub}");
        assert!(
            stub.contains(r#"s.require_paths = ["lib".freeze, "lib/thing".freeze]"#),
            "{stub}"
        );
        assert!(!seen.join("real.gemspec").exists());
        // Everything else is still reachable through it, except the probe's
        // own stamps, which describe the cache and not the gem.
        assert!(seen.join("lib/thing.rb").exists());
        assert!(!seen.join(".zeo-probe-spec").exists());
        assert!(!seen.join(".zeo-probe-version").exists());

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

    fn row_with(stage: Stage, outcome: Outcome) -> Row {
        Row {
            version: "1.0.0".into(),
            stage,
            outcome,
            rust_bytes: None,
            binary_bytes: None,
            site: None,
            digest: None,
        }
    }

    fn stats_fixture() -> BTreeMap<String, Row> {
        let mut rows = BTreeMap::new();
        let mut put = |name: &str, stage, outcome| {
            rows.insert(name.to_string(), row_with(stage, outcome));
        };
        put("a", Stage::Codegen, Outcome::Ok);
        put("b", Stage::Codegen, Outcome::Ok);
        put("c", Stage::Lower, Outcome::LoweringGap("x".into()));
        put("d", Stage::Unpack, Outcome::NoEntryPoint);
        put("e", Stage::Parse, Outcome::InvalidRuby("y".into()));
        put("f", Stage::Lower, Outcome::MissingDependency("z".into()));
        put("g", Stage::Parse, Outcome::Timeout);
        put("h", Stage::Queued, Outcome::Unprobed);
        rows
    }

    /// The buckets partition the probed rows: every verdict lands in exactly
    /// one, and the frontier lands in none.
    #[test]
    fn the_stats_buckets_partition_the_probed_rows() {
        let s = LedgerStats::from_rows(&stats_fixture());
        assert_eq!(s.names, 8);
        assert_eq!(s.probed, 7);
        assert_eq!(
            s.ok + s.compiler
                + s.harness
                + s.not_ruby
                + s.missing_dep
                + s.no_verdict
                + s.run_failed,
            s.probed
        );
        // `attempted` excludes what the harness never carried to the compiler
        // and what no Ruby loads: 7 - 1 (no-entry-point) - 1 (invalid-ruby).
        assert_eq!(s.attempted(), 5);
    }

    #[test]
    fn the_stats_block_quotes_the_headline_ratio() {
        let block = render_stats_block(&LedgerStats::from_rows(&stats_fixture()));
        assert!(block.starts_with("**2 of 7 probed gems compile to Rust (28.6%).**"));
        assert!(block.contains("| Compile to Rust (`ok`) | 2 | 28.6% |"));
        // 2 of 5 attempted.
        assert!(block.contains("**2 compile (40.0%)**"));
    }

    #[test]
    fn thousands_delimits_from_four_digits_up() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(195_778), "195,778");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    /// The splice replaces only what sits between the markers, keeps the
    /// markers, and is idempotent -- so every ledger write may run it.
    #[test]
    fn the_readme_splice_replaces_only_the_marked_block() {
        let readme = format!(
            "# Title\n\nprose above\n\n{README_STATS_BEGIN}\nold numbers\n{README_STATS_END}\n\nprose below\n"
        );
        let once = splice_readme_stats(&readme, "new numbers\n").unwrap();
        assert!(once.contains("prose above"));
        assert!(once.contains("prose below"));
        assert!(once.contains("new numbers"));
        assert!(!once.contains("old numbers"));
        let twice = splice_readme_stats(&once, "new numbers\n").unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn a_readme_without_markers_is_left_alone() {
        assert!(splice_readme_stats("# Title\n\nno markers here\n", "block").is_none());
        // End before begin is malformed, not a partial match.
        let backwards = format!("{README_STATS_END}\n{README_STATS_BEGIN}\n");
        assert!(splice_readme_stats(&backwards, "block").is_none());
    }

    /// `write_ledger` carries the ledger's numbers into a README that opts in
    /// with the markers, and leaves a scratch root without one untouched.
    #[test]
    fn writing_the_ledger_refreshes_the_readme_block() {
        let root = scratch("readme");
        std::fs::write(
            root.join("README.md"),
            format!("intro\n\n{README_STATS_BEGIN}\nstale\n{README_STATS_END}\nend\n"),
        )
        .unwrap();
        write_ledger(&root, &stats_fixture()).unwrap();
        let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
        assert!(readme.contains("**2 of 7 probed gems compile to Rust (28.6%).**"));
        assert!(!readme.contains("stale"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
