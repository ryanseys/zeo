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
//!   record   the Outcome        -> conformance/gem-probe-*.tsv     (committed)
//!
//! Probing an unpacked tree needs no network and is deterministic, so a ledger
//! row reproduces from its recorded version alone.
//!
//! A sweep survives a gem that panics the compiler: `parser` did, and took
//! every verdict already gathered down with it. A panic is recorded as its own
//! outcome rather than folded into `lowering-gap`, because a gap is a limit
//! zeo reported and a panic is a bug it did not.
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
#[derive(Clone, PartialEq, Eq, Debug)]
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
    /// A `lib/` with no file this gem's name could name. Probing would compile
    /// an unresolvable require, which says nothing about the gem.
    NoEntryPoint,
    /// The compiler panicked. Distinct from a lowering gap on purpose: a gap
    /// is a known limit reported through the error path, a panic is a bug.
    CompilerPanic(String),
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
            Outcome::NoEntryPoint => "no-entry-point",
            Outcome::CompilerPanic(_) => "compiler-panic",
            Outcome::FetchFailed(_) => "fetch-failed",
        }
    }

    fn detail(&self) -> &str {
        match self {
            Outcome::LoweringGap(d)
            | Outcome::MissingDependency(d)
            | Outcome::FetchFailed(d)
            | Outcome::CompilerPanic(d) => d,
            _ => "",
        }
    }

    fn from_ledger(tag: &str, detail: &str) -> Outcome {
        match tag {
            "compiles" => Outcome::Compiles,
            "native-extension" => Outcome::NativeExtension,
            "no-lib-dir" => Outcome::NoLibDir,
            "no-entry-point" => Outcome::NoEntryPoint,
            "missing-dependency" => Outcome::MissingDependency(detail.to_string()),
            "fetch-failed" => Outcome::FetchFailed(detail.to_string()),
            "compiler-panic" => Outcome::CompilerPanic(detail.to_string()),
            _ => Outcome::LoweringGap(detail.to_string()),
        }
    }
}

#[derive(Clone, Debug)]
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
    match tops.len() {
        1 => Some(tops.remove(0)),
        _ => None,
    }
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
    let Some(feature) = entry_point(dir, name) else {
        return Outcome::NoEntryPoint;
    };
    let src = format!("require {feature:?}\n");

    // A sweep must survive a gem that panics the compiler. `parser` did
    // exactly that, and the whole run died with it, losing every verdict
    // already gathered. The panic message still reaches stderr; only the
    // process-ending part is caught.
    let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        zeo::compile_to_rust_with(&src, &opts)
    }));
    match compiled {
        Ok(Ok(_)) => Outcome::Compiles,
        Ok(Err(e)) => classify(&String::from(e), root),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("compiler panicked");
            Outcome::CompilerPanic(truncate(
                &message_of(msg).replace(&format!("{}/", root.display()), ""),
            ))
        }
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

/// The ledger is two files, one per verdict: a gem that compiles and a gem
/// that does not are read for different reasons, and interleaving them makes
/// each list something you have to filter for rather than open.
///
/// Both carry the same four columns, so one parser and one writer serve both
/// and a row keeps its meaning when a fix moves it between the files.
fn ledger_paths(root: &Path) -> [PathBuf; 2] {
    [
        root.join("conformance/gem-probe-compiles.tsv"),
        root.join("conformance/gem-probe-fails.tsv"),
    ]
}

/// Reads both files into one map.
///
/// A gem in both files is a corruption -- a hand-edit, or a merge that kept
/// two sides of a moved row -- and the two copies disagree about the verdict.
/// There is no safe way to pick one, so this refuses rather than guesses.
fn read_ledger(root: &Path) -> Result<BTreeMap<String, Row>, String> {
    let mut out: BTreeMap<String, Row> = BTreeMap::new();
    for path in ledger_paths(root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let mut f = line.split('\t');
            let (Some(name), Some(version), Some(tag)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            let row = Row {
                version: version.to_string(),
                outcome: Outcome::from_ledger(tag, f.next().unwrap_or("")),
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

fn write_ledger(root: &Path, rows: &BTreeMap<String, Row>) -> Result<(), String> {
    let [compiles_path, fails_path] = ledger_paths(root);
    let compiles = || rows.iter().filter(|(_, r)| r.outcome == Outcome::Compiles);
    let fails = || rows.iter().filter(|(_, r)| r.outcome != Outcome::Compiles);

    let render = |header: &str, selected: &mut dyn Iterator<Item = (&String, &Row)>| {
        let mut tsv = format!(
            "# {header}\n\
             # Written by `cargo xtask gem-probe`. Each gem is in this file or in \
             its sibling, never both.\n\
             # Columns: gem <TAB> version <TAB> outcome <TAB> detail\n"
        );
        for (name, r) in selected {
            tsv.push_str(&format!("{name}\t{}\t{}", r.version, r.outcome.tag()));
            match r.outcome.detail() {
                "" => tsv.push('\n'),
                d => tsv.push_str(&format!("\t{d}\n")),
            }
        }
        tsv
    };

    std::fs::write(
        &compiles_path,
        render(
            "Gems whose entry point zeo compiles. A row here may not regress: \
             `gem-probe --check` gates it.",
            &mut compiles(),
        ),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        &fails_path,
        render(
            "Gems zeo does not compile. The outcome says why; only `lowering-gap` \
             and `compiler-panic` are zeo's to fix.",
            &mut fails(),
        ),
    )
    .map_err(|e| e.to_string())?;

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
    let mut table = |title: &str, selected: &mut dyn Iterator<Item = (&String, &Row)>| {
        md.push_str(&format!(
            "\n## {title}\n\n| Gem | Version | Outcome | Detail |\n|---|---|---|---|\n"
        ));
        for (name, r) in selected {
            md.push_str(&format!(
                "| {name} | {} | {} | {} |\n",
                r.version,
                r.outcome.tag(),
                r.outcome.detail().replace('|', "\\|")
            ));
        }
    };
    table("Compiles", &mut compiles());
    table("Does not compile", &mut fails());
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

/// Probes `name`, fetching its runtime dependencies first unless `no_deps`.
/// Probes one gem and writes the ledger immediately.
///
/// Writing once at the end throws away everything when a long sweep is
/// interrupted -- and a sweep over the whole registry is measured in hours, so
/// interruption is the normal case, not the exceptional one. Rewriting the
/// whole file per gem is cheap next to fetching and compiling one, and it means
/// the ledger on disk is always the truth so far.
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
            let _ = write_ledger(root, rows);
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
    if let Err(e) = write_ledger(root, rows) {
        eprintln!("gem-probe: writing the ledger: {e}");
    }
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let mut names: Vec<(String, Option<String>)> = Vec::new();
    let (mut corpus, mut all, mut check, mut no_deps) = (false, false, false, false);
    let (mut index, mut refresh, mut refresh_index) = (false, false, false);
    let (mut limit, mut popular): (Option<usize>, Option<usize>) = (None, None);
    let mut matching: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--corpus" => corpus = true,
            "--all" => all = true,
            "--check" => check = true,
            "--no-deps" => no_deps = true,
            "--index" => index = true,
            "--refresh" => refresh = true,
            "--refresh-index" => refresh_index = true,
            "--limit" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => limit = Some(n),
                None => {
                    eprintln!("gem-probe: --limit needs a number");
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
    if let Some(pat) = &matching {
        names.extend(
            before
                .iter()
                .filter(|(_, r)| r.outcome.detail().contains(pat.as_str()))
                .map(|(n, r)| (n.clone(), Some(r.version.clone()))),
        );
    }
    if all {
        names.extend(
            before
                .iter()
                .map(|(n, r)| (n.clone(), Some(r.version.clone()))),
        );
    }
    if names.is_empty() {
        eprintln!(
            "gem-probe: nothing to probe (give a gem name, --corpus, --popular N, --matching <detail>, --index or --all)"
        );
        return ExitCode::FAILURE;
    }

    // Resume. A sweep of the registry is hours of work, so re-running must
    // continue rather than start over. `--all` is the explicit re-probe, and
    // `--refresh` forces it for any selection.
    let skipped = if all || refresh || matching.is_some() {
        0
    } else {
        let n = names.len();
        names.retain(|(name, _)| !before.contains_key(name));
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

    let mut rows = before.clone();
    if skipped > 0 {
        println!("{skipped} gem(s) already recorded; --refresh re-probes them");
    }
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
    let [compiles_path, fails_path] = ledger_paths(root);
    println!(
        "gem-probe: {compiles}/{} probed gems compile\n  {}\n  {}",
        rows.len(),
        compiles_path.display(),
        fails_path.display()
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
        write_stub_gemspec(&dir, name, version).unwrap();
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
        assert!(matches!(
            classify("define_method's second argument must be a block", root),
            Outcome::LoweringGap(_)
        ));
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
                outcome: Outcome::Compiles,
            },
        );
        rows.insert(
            "beta".to_string(),
            Row {
                version: "2.1.0".into(),
                outcome: Outcome::LoweringGap("some gap".into()),
            },
        );
        rows.insert(
            "gamma".to_string(),
            Row {
                version: "3.0.0".into(),
                outcome: Outcome::NativeExtension,
            },
        );
        write_ledger(&root, &rows).unwrap();

        let back = read_ledger(&root).unwrap();
        assert_eq!(back.len(), 3);
        assert_eq!(back["alpha"].outcome, Outcome::Compiles);
        assert_eq!(back["beta"].version, "2.1.0");
        assert_eq!(
            back["beta"].outcome,
            Outcome::LoweringGap("some gap".into())
        );
        assert_eq!(back["gamma"].outcome, Outcome::NativeExtension);
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
                    outcome: Outcome::Compiles,
                },
            );
        }
        write_ledger(&root, &rows).unwrap();
        let first = ledger_paths(&root).map(|p| std::fs::read_to_string(p).unwrap());
        write_ledger(&root, &rows).unwrap();
        let second = ledger_paths(&root).map(|p| std::fs::read_to_string(p).unwrap());
        assert_eq!(first, second);
        // BTreeMap ordering means the file is sorted, not insertion-ordered.
        assert_eq!(gem_names(&first[0]), ["alpha", "mu", "zeta"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn gem_names(tsv: &str) -> Vec<&str> {
        tsv.lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
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
            ("alpha", Outcome::Compiles),
            ("beta", Outcome::LoweringGap("a gap".into())),
            ("gamma", Outcome::Compiles),
            ("delta", Outcome::NativeExtension),
        ] {
            rows.insert(
                name.to_string(),
                Row {
                    version: "1.0.0".into(),
                    outcome,
                },
            );
        }
        write_ledger(&root, &rows).unwrap();

        let [compiles, fails] = ledger_paths(&root).map(|p| std::fs::read_to_string(p).unwrap());
        assert_eq!(gem_names(&compiles), ["alpha", "gamma"]);
        assert_eq!(gem_names(&fails), ["beta", "delta"]);
        // Reading them back reassembles the one map the prober works from.
        assert_eq!(read_ledger(&root).unwrap().len(), 4);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A fix moves a gem across the files. The stale row must not survive in
    /// the file the gem left, or resume would read back the old verdict.
    #[test]
    fn a_gem_that_starts_compiling_leaves_the_failing_ledger() {
        let root = scratch("ledger-move");
        let mut rows = BTreeMap::new();
        rows.insert(
            "beta".to_string(),
            Row {
                version: "1.0.0".into(),
                outcome: Outcome::LoweringGap("a gap".into()),
            },
        );
        write_ledger(&root, &rows).unwrap();
        rows.get_mut("beta").unwrap().outcome = Outcome::Compiles;
        write_ledger(&root, &rows).unwrap();

        let [compiles, fails] = ledger_paths(&root).map(|p| std::fs::read_to_string(p).unwrap());
        assert_eq!(gem_names(&compiles), ["beta"]);
        assert!(gem_names(&fails).is_empty());
        assert_eq!(
            read_ledger(&root).unwrap()["beta"].outcome,
            Outcome::Compiles
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Two files can disagree in a way one file never could. Guessing which
    /// copy is current would silently publish a wrong verdict.
    #[test]
    fn a_gem_recorded_in_both_ledgers_is_refused() {
        let root = scratch("ledger-dup");
        let [compiles, fails] = ledger_paths(&root);
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

    #[test]
    fn only_a_declared_extension_counts_as_native() {
        let root = scratch("ext");
        let with = root.join("with");
        let without = root.join("without");
        std::fs::create_dir_all(&with).unwrap();
        std::fs::create_dir_all(&without).unwrap();
        std::fs::write(
            with.join("a.gemspec"),
            "Gem::Specification.new do |s|\n  s.extensions = [\"ext/a/extconf.rb\"]\nend\n",
        )
        .unwrap();
        // concurrent-ruby's shape: an empty declaration is not a native gem.
        std::fs::write(
            without.join("b.gemspec"),
            "Gem::Specification.new do |s|\n  s.extensions = []\n  s.name = \"b\"\nend\n",
        )
        .unwrap();
        assert!(declares_extensions(&with));
        assert!(!declares_extensions(&without));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_stub_gemspec_replaces_the_original() {
        let root = scratch("stub");
        let dir = root.join("gem");
        std::fs::create_dir_all(&dir).unwrap();
        // The shape zeo rejects: a computed version.
        std::fs::write(
            dir.join("real.gemspec"),
            "Gem::Specification.new { |s| s.version = Thing::VERSION }\n",
        )
        .unwrap();
        write_stub_gemspec(&dir, "thing", "4.5.6").unwrap();
        assert!(!dir.join("real.gemspec").exists());
        let stub = std::fs::read_to_string(dir.join("thing.gemspec")).unwrap();
        assert!(stub.contains(r#"s.version = "4.5.6".freeze"#), "{stub}");
        assert!(stub.contains(r#"s.name = "thing".freeze"#), "{stub}");
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
                    outcome: Outcome::Compiles,
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
                outcome: Outcome::Compiles,
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

        let got = fetch(&root, "cached", "2.0.0").unwrap();
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
        assert!(fetch(&root, "moving", "2.0.0").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
