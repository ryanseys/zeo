//! `cargo xtask gem-probe` -- fetch a gem from rubygems.org and try to compile
//! it, then record the verdict.
//!
//! This answers a question `gem-compat` cannot. That command classifies a gem
//! by its LAYOUT: "pure Ruby, so zeo would compile it". It never runs the
//! compiler, so a gem using a construct zeo cannot lower still reports as
//! resolvable. concurrent-ruby is the standing example. `gem-probe` runs the
//! front end for real, so `compiles` means compiles.
//!
//! Four stages, and only the first touches the network:
//!
//!   resolve  name [version]     -> an exact version
//!   fetch    the .gem           -> vendor/gems/<name>/   (cached, gitignored)
//!   probe    require "<entry>"  -> an Outcome
//!   record   the Outcome        -> conformance/gem-probe.{tsv,md}  (committed)
//!
//! Probing an unpacked tree needs no network and is deterministic, so a ledger
//! row reproduces from its recorded version alone.
//!
//! A gem is unpacked to `vendor/gems/<name>/`, named for the gem rather than
//! `<name>-<version>`, and its gemspec is REPLACED with a stub. Both are
//! required, not stylistic: zeo checks a gemspec's name against its directory
//! name, and it parses gemspecs statically, so the computed `s.version` most
//! real gems use ("spec.version = Colorator::VERSION") is rejected. The stub
//! carries the version the registry reported. This is the layout `gems/`
//! already uses, so a probe exercises the same loader path bundled gems do.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const REGISTRY: &str = "https://rubygems.org";

/// What a probe concluded. The first two are about zeo; the rest say the
/// question could not be put.
#[derive(Clone, PartialEq, Eq)]
enum Outcome {
    Compiles,
    /// zeo reached the gem's source and could not lower it. The payload is the
    /// compiler's own first line -- the actionable half of the verdict.
    LoweringGap(String),
    /// Ships C sources. zeo cannot build these at all; see docs/EXTENSIONS.md.
    NativeExtension,
    /// A `require` reached outside the gem and its fetched dependencies.
    MissingDependency(String),
    /// No `lib/` to put on the load path.
    NoLibDir,
    FetchFailed(String),
}

impl Outcome {
    fn tag(&self) -> &'static str {
        match self {
            Outcome::Compiles => "compiles",
            Outcome::LoweringGap(_) => "lowering-gap",
            Outcome::NativeExtension => "native-extension",
            Outcome::MissingDependency(_) => "missing-dependency",
            Outcome::NoLibDir => "no-lib-dir",
            Outcome::FetchFailed(_) => "fetch-failed",
        }
    }

    fn detail(&self) -> &str {
        match self {
            Outcome::LoweringGap(d) | Outcome::MissingDependency(d) | Outcome::FetchFailed(d) => d,
            _ => "",
        }
    }

    fn from_ledger(tag: &str, detail: &str) -> Outcome {
        match tag {
            "compiles" => Outcome::Compiles,
            "native-extension" => Outcome::NativeExtension,
            "no-lib-dir" => Outcome::NoLibDir,
            "missing-dependency" => Outcome::MissingDependency(detail.to_string()),
            "fetch-failed" => Outcome::FetchFailed(detail.to_string()),
            _ => Outcome::LoweringGap(detail.to_string()),
        }
    }
}

#[derive(Clone)]
struct Row {
    version: String,
    outcome: Outcome,
}

// ---------------------------------------------------------------- registry

fn get(url: &str) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    ureq::get(url)
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

/// Runtime dependencies of one EXACT version. The v1 endpoint describes only
/// the newest release, which would silently mis-resolve a pinned probe.
fn runtime_deps(name: &str, version: &str) -> Result<Vec<String>, String> {
    let v = json(&format!(
        "{REGISTRY}/api/v2/rubygems/{name}/versions/{version}.json"
    ))?;
    Ok(v.pointer("/dependencies/runtime")
        .and_then(|d| d.as_array())
        .map(|deps| {
            deps.iter()
                .filter_map(|d| d.get("name").and_then(|n| n.as_str()))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default())
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

/// Whether the gem's own gemspec declares C extensions -- the authoritative
/// signal, and the one that separates "must compile C" from "ships an optional
/// accelerator". concurrent-ruby has an `ext/` directory but declares nothing,
/// because its C half is the separate concurrent-ruby-ext gem.
fn declares_extensions(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|e| {
        let p = e.path();
        p.extension().is_some_and(|x| x == "gemspec")
            && std::fs::read_to_string(&p).is_ok_and(|t| {
                t.lines()
                    .map(str::trim)
                    .any(|l| l.contains(".extensions") && l.contains('=') && !l.contains("[]"))
            })
    })
}

fn write_stub_gemspec(dir: &Path, name: &str, version: &str) -> Result<(), String> {
    for existing in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let p = existing.map_err(|e| e.to_string())?.path();
        if p.extension().is_some_and(|x| x == "gemspec") {
            let _ = std::fs::remove_file(&p);
        }
    }
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

fn fetch(root: &Path, name: &str, version: &str) -> Result<PathBuf, String> {
    let dir = vendor_dir(root).join(name);
    if std::fs::read_to_string(stamp_path(&dir)).is_ok_and(|s| s.trim() == version) {
        return Ok(dir);
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let url = format!("{REGISTRY}/downloads/{name}-{version}.gem");
    let bytes = get(&url).map_err(|e| format!("{url}: {e}"))?;
    unpack_gem(&bytes, &dir)?;
    if declares_extensions(&dir) {
        std::fs::write(dir.join(".zeo-probe-native"), "1").map_err(|e| e.to_string())?;
    }
    write_stub_gemspec(&dir, name, version)?;
    std::fs::write(stamp_path(&dir), version).map_err(|e| e.to_string())?;
    Ok(dir)
}

// ------------------------------------------------------------------ probe

/// The feature a gem's users require, when it is not the gem's own name. A
/// hyphen is a path separator (`net-http` -> `net/http`).
fn entry_point(name: &str) -> String {
    name.replace('-', "/")
}

/// A package directory holding ONLY this gem and its declared dependencies.
///
/// Pointing the probe at the whole of `vendor/gems` made a verdict depend on
/// which other gems happened to be cached: kramdown reported one gap alone,
/// and a different one once kramdown-parser-gfm had been fetched beside it. An
/// isolated view makes the result a function of the gem and its deps, which is
/// what the ledger claims to record.
fn isolate(root: &Path, name: &str, deps: &[String]) -> Result<PathBuf, String> {
    let view = vendor_dir(root).join(".probe").join(name);
    let _ = std::fs::remove_dir_all(&view);
    std::fs::create_dir_all(&view).map_err(|e| e.to_string())?;
    for gem in std::iter::once(&name.to_string()).chain(deps) {
        let src = vendor_dir(root).join(gem);
        if src.is_dir() {
            std::os::unix::fs::symlink(&src, view.join(gem)).map_err(|e| e.to_string())?;
        }
    }
    Ok(view)
}

fn probe(root: &Path, name: &str, dir: &Path, deps: &[String]) -> Outcome {
    if dir.join(".zeo-probe-native").exists() {
        return Outcome::NativeExtension;
    }
    if !dir.join("lib").is_dir() {
        return Outcome::NoLibDir;
    }
    // An `ext/` directory is NOT decisive. Many gems ship an optional C
    // accelerator beside a pure-Ruby implementation -- concurrent-ruby is the
    // common case -- and compile fine without it. Try, then classify what
    // actually failed.
    let view = match isolate(root, name, deps) {
        Ok(v) => v,
        Err(e) => return Outcome::FetchFailed(e),
    };
    let opts = zeo::CompileOptions {
        // The repo's own gems/ come too: a probed gem may require a stdlib
        // feature, and answering that from zeo's bundled copy is what a real
        // compile would do.
        package_dirs: vec![view, root.join("gems")],
        ..Default::default()
    };
    let src = format!("require {:?}\n", entry_point(name));
    match zeo::compile_to_rust_with(&src, &opts) {
        Ok(_) => Outcome::Compiles,
        Err(e) => classify(&String::from(e), root),
    }
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

fn classify(err: &str, root: &Path) -> Outcome {
    let msg = message_of(err);
    // This text is committed. A diagnostic can carry an absolute path anywhere
    // in it -- inside a `Some((..))` span, not only as a leading prefix -- so
    // the repository root is rewritten away wholesale rather than peeled.
    let msg = msg.replace(&format!("{}/", root.display()), "");
    let msg = if msg.contains("/Users/") || msg.contains("/home/") {
        msg.split_whitespace()
            .filter(|w| !w.contains("/Users/") && !w.contains("/home/"))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        msg
    };
    let msg = if msg.is_empty() {
        "compile failed".to_string()
    } else {
        msg
    };
    if let Some(rest) = msg.split("cannot load such file -- ").nth(1) {
        let feature = rest.split_whitespace().next().unwrap_or(rest);
        return Outcome::MissingDependency(feature.trim_matches(['`', ':', '.']).to_string());
    }
    if msg.contains("native (C) extension") {
        return Outcome::NativeExtension;
    }
    Outcome::LoweringGap(truncate(&msg))
}

fn truncate(s: &str) -> String {
    const LIMIT: usize = 160;
    match s.char_indices().nth(LIMIT) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => s.to_string(),
    }
}

// ----------------------------------------------------------------- ledger

fn ledger_path(root: &Path) -> PathBuf {
    root.join("conformance/gem-probe.tsv")
}

fn read_ledger(root: &Path) -> BTreeMap<String, Row> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(ledger_path(root)) else {
        return out;
    };
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let mut f = line.split('\t');
        let (Some(name), Some(version), Some(tag)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let detail = f.next().unwrap_or("");
        out.insert(
            name.to_string(),
            Row {
                version: version.to_string(),
                outcome: Outcome::from_ledger(tag, detail),
            },
        );
    }
    out
}

fn write_ledger(root: &Path, rows: &BTreeMap<String, Row>) -> Result<(), String> {
    let mut tsv = String::from(
        "# Which real gems zeo compiles, measured by `cargo xtask gem-probe`.\n\
         # Columns: gem <TAB> version <TAB> outcome <TAB> detail\n\
         # A `compiles` row may not regress: `gem-probe --check` gates it.\n",
    );
    for (name, r) in rows {
        tsv.push_str(&format!(
            "{name}\t{}\t{}\t{}\n",
            r.version,
            r.outcome.tag(),
            r.outcome.detail()
        ));
    }
    std::fs::write(ledger_path(root), tsv).map_err(|e| e.to_string())?;

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for r in rows.values() {
        *counts.entry(r.outcome.tag()).or_default() += 1;
    }
    let mut md = String::from("# Gem probe results\n\nGenerated by `cargo xtask gem-probe`.\n\n");
    md.push_str(&format!("{} gems probed.\n\n", rows.len()));
    md.push_str("| Outcome | Gems |\n|---|---|\n");
    for (tag, n) in &counts {
        md.push_str(&format!("| {tag} | {n} |\n"));
    }
    md.push_str("\n| Gem | Version | Outcome | Detail |\n|---|---|---|---|\n");
    for (name, r) in rows {
        md.push_str(&format!(
            "| {name} | {} | {} | {} |\n",
            r.version,
            r.outcome.tag(),
            r.outcome.detail().replace('|', "\\|")
        ));
    }
    std::fs::write(root.join("conformance/gem-probe.md"), md).map_err(|e| e.to_string())
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

/// Probes `name`, fetching its runtime dependencies first unless `no_deps`.
fn probe_one(
    root: &Path,
    name: &str,
    want: Option<&str>,
    no_deps: bool,
    rows: &mut BTreeMap<String, Row>,
) {
    let version = match want
        .map(String::from)
        .map_or_else(|| latest_version(name), Ok)
    {
        Ok(v) => v,
        Err(e) => {
            println!("  {name}: fetch-failed ({e})");
            rows.insert(
                name.to_string(),
                Row {
                    version: want.unwrap_or("-").to_string(),
                    outcome: Outcome::FetchFailed(e),
                },
            );
            return;
        }
    };

    let mut deps = Vec::new();
    if !no_deps && let Ok(declared) = runtime_deps(name, &version) {
        for dep in declared {
            if let Ok(dv) = latest_version(&dep)
                && fetch(root, &dep, &dv).is_ok()
            {
                deps.push(dep);
            }
        }
    }

    let outcome = match fetch(root, name, &version) {
        Ok(dir) => probe(root, name, &dir, &deps),
        Err(e) => Outcome::FetchFailed(e),
    };
    let detail = outcome.detail();
    println!(
        "  {name} {version}: {}{}",
        outcome.tag(),
        if detail.is_empty() {
            String::new()
        } else {
            format!(" -- {detail}")
        }
    );
    rows.insert(name.to_string(), Row { version, outcome });
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let mut names: Vec<(String, Option<String>)> = Vec::new();
    let (mut corpus, mut all, mut check, mut no_deps) = (false, false, false, false);
    let mut positional: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--corpus" => corpus = true,
            "--all" => all = true,
            "--check" => check = true,
            "--no-deps" => no_deps = true,
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

    let before = read_ledger(root);
    if corpus {
        names.extend(read_corpus(root));
    }
    if all {
        names.extend(
            before
                .iter()
                .map(|(n, r)| (n.clone(), Some(r.version.clone()))),
        );
    }
    if names.is_empty() {
        eprintln!("gem-probe: nothing to probe (give a gem name, --corpus or --all)");
        return ExitCode::FAILURE;
    }

    if let Err(e) = std::fs::create_dir_all(vendor_dir(root)) {
        eprintln!("gem-probe: {e}");
        return ExitCode::FAILURE;
    }

    let mut rows = before.clone();
    println!("probing {} gem(s):", names.len());
    for (name, want) in &names {
        probe_one(root, name, want.as_deref(), no_deps, &mut rows);
    }

    if let Err(e) = write_ledger(root, &rows) {
        eprintln!("gem-probe: writing the ledger: {e}");
        return ExitCode::FAILURE;
    }

    let compiles = rows
        .values()
        .filter(|r| r.outcome == Outcome::Compiles)
        .count();
    println!(
        "gem-probe: {compiles}/{} probed gems compile; ledger at {}",
        rows.len(),
        ledger_path(root).display()
    );

    // A gem that compiled and no longer does is a regression, whatever the new
    // outcome is. Only `--check` fails on it, so an exploratory probe of a
    // fresh gem never breaks a build.
    if check {
        let lost: Vec<&String> = before
            .iter()
            .filter(|(n, r)| {
                r.outcome == Outcome::Compiles
                    && rows
                        .get(*n)
                        .is_some_and(|new| new.outcome != Outcome::Compiles)
            })
            .map(|(n, _)| n)
            .collect();
        if !lost.is_empty() {
            eprintln!("gem-probe: {} gem(s) stopped compiling:", lost.len());
            for n in lost {
                eprintln!("  {n}: {}", rows[n].outcome.tag());
            }
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
