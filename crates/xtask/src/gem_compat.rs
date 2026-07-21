//! `cargo xtask gem-compat <Gemfile.lock> [--gem-path <dir>]` -- the
//! out-of-the-box gem-compatibility matrix.
//!
//! Reuses zeo's own Phase-3 store provider (`zeo::gem_compat`) to
//! classify every gem a `Gemfile.lock` locked against an installed RubyGems
//! store: pure Ruby (compiled), satisfied by a zeo built-in (with the
//! version divergence noted), or a native gem zeo can't provide (with the
//! detected layout). Prints a per-gem table plus the headline resolvability
//! number, and writes `conformance/gem-compat.{tsv,md}` -- the same shape
//! `stdlib-status` uses to measure stdlib coverage.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use zeo::{GemCompatEntry, GemCompatOutcome};

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let mut lockfile: Option<PathBuf> = None;
    let mut gem_path: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--gem-path" => match it.next() {
                Some(p) => gem_path = Some(PathBuf::from(p)),
                None => {
                    eprintln!("gem-compat: --gem-path requires a directory");
                    return ExitCode::FAILURE;
                }
            },
            other if lockfile.is_none() => lockfile = Some(PathBuf::from(other)),
            other => {
                eprintln!("gem-compat: unexpected argument {other:?}");
                return ExitCode::FAILURE;
            }
        }
    }

    // Default the store to the installed Ruby's own gem home, the way
    // stdlib-status defaults the lib dir to its rubylibdir.
    let store = match gem_path {
        Some(p) => p,
        None => match ruby_query("print Gem.dir") {
            Ok(p) => PathBuf::from(p),
            Err(e) => {
                eprintln!("gem-compat: locating the gem store: {e}");
                eprintln!(
                    "  (pass one explicitly: `gem-compat <lock> --gem-path $(gem env gemdir)`)"
                );
                return ExitCode::FAILURE;
            }
        },
    };
    if !store.is_dir() {
        eprintln!("gem-compat: not a gem store directory: {}", store.display());
        return ExitCode::FAILURE;
    }

    // No lockfile -> classify the entire installed store (the broad sample);
    // with one -> just its locked subset.
    let source = match &lockfile {
        Some(p) => p.display().to_string(),
        None => "(entire installed store)".to_string(),
    };
    let result = match &lockfile {
        Some(p) => zeo::gem_compat(&store, p),
        None => zeo::gem_compat_installed(&store),
    };
    let entries = match result {
        Ok(e) => e,
        Err(e) => {
            eprintln!("gem-compat: {e}");
            return ExitCode::FAILURE;
        }
    };
    if entries.is_empty() {
        eprintln!("gem-compat: {source} has no gems to classify");
        return ExitCode::FAILURE;
    }

    for entry in &entries {
        eprintln!(
            "{:>18}  {}  {}",
            tag(&entry.outcome),
            entry.name,
            detail(&entry.outcome)
        );
    }

    match write_artifacts(root, &source, &store, &entries) {
        Ok((tsv, md)) => {
            print_summary(&entries);
            eprintln!("gem-compat: wrote {} and {}", tsv.display(), md.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gem-compat: writing artifacts: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The short status tag for the per-gem line and the TSV column.
fn tag(outcome: &GemCompatOutcome) -> &'static str {
    match outcome {
        GemCompatOutcome::Compiled => "pure-ruby",
        GemCompatOutcome::Builtin { diverges: true, .. } => "builtin (diverges)",
        GemCompatOutcome::Builtin {
            diverges: false, ..
        } => "builtin",
        GemCompatOutcome::NativeUnsupported { .. } => "native (unsupported)",
        GemCompatOutcome::ExternalSource => "external-source",
        GemCompatOutcome::Skipped { .. } => "skipped",
    }
}

/// The reason/detail column: empty for a clean pass, else the note or reason.
fn detail(outcome: &GemCompatOutcome) -> String {
    match outcome {
        GemCompatOutcome::Compiled | GemCompatOutcome::ExternalSource => String::new(),
        GemCompatOutcome::Builtin { note, .. } => note.clone().unwrap_or_default(),
        GemCompatOutcome::NativeUnsupported { kind, .. } => kind.clone(),
        GemCompatOutcome::Skipped { reason } => reason.clone(),
    }
}

fn write_artifacts(
    root: &Path,
    source: &str,
    store: &Path,
    entries: &[GemCompatEntry],
) -> Result<(PathBuf, PathBuf), String> {
    let dir = root.join("conformance");
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    // `root` is `CARGO_MANIFEST_DIR/../..`, so canonicalize before we build and
    // report paths -- otherwise they read `crates/xtask/../../conformance/...`.
    let dir = dir.canonicalize().unwrap_or(dir);

    let mut tsv = String::from("name\tversion\tstatus\tdetail\n");
    for e in entries {
        tsv.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            e.name,
            e.version,
            tag(&e.outcome),
            detail(&e.outcome)
        ));
    }
    let tsv_path = dir.join("gem-compat.tsv");
    std::fs::write(&tsv_path, tsv).map_err(|e| format!("writing {}: {e}", tsv_path.display()))?;

    let counts = Counts::of(entries);
    let mut md = String::new();
    md.push_str("# Gem compatibility\n\n");
    md.push_str(&format!("- Source: `{source}`\n"));
    md.push_str(&format!("- Store: `{}`\n", store.display()));
    md.push_str(&format!(
        "- Out of the box: **{}/{} store gems ({:.0}%)** are pure Ruby or satisfied by a built-in\n",
        counts.usable(),
        counts.store_gems(),
        counts.usable_pct(),
    ));
    md.push_str(
        "- `pure-ruby` = zeo resolves the gem and would attempt to compile it; this is a \
         static classification, NOT a verified compile.\n\n",
    );
    md.push_str("| status | count |\n|---|---|\n");
    md.push_str(&format!(
        "| pure-ruby (resolvable) | {} |\n",
        counts.compiled
    ));
    md.push_str(&format!(
        "| built-in (zeo provides) | {} |\n",
        counts.builtin
    ));
    md.push_str(&format!("| native (unsupported) | {} |\n", counts.native));
    md.push_str(&format!(
        "| external source (git/path) | {} |\n",
        counts.external
    ));
    md.push_str(&format!("| skipped | {} |\n\n", counts.skipped));

    // The native gems, grouped by detected layout -- the FFI (#161) work-list.
    let mut by_kind: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in entries {
        if let GemCompatOutcome::NativeUnsupported { kind, .. } = &e.outcome {
            by_kind
                .entry(kind.as_str())
                .or_default()
                .push(e.name.as_str());
        }
    }
    if !by_kind.is_empty() {
        md.push_str("## Native gems, by layout\n\n");
        for (kind, names) in by_kind {
            md.push_str(&format!("- **{kind}**: {}\n", names.join(", ")));
        }
    }
    let md_path = dir.join("gem-compat.md");
    std::fs::write(&md_path, md).map_err(|e| format!("writing {}: {e}", md_path.display()))?;
    Ok((tsv_path, md_path))
}

struct Counts {
    compiled: usize,
    builtin: usize,
    native: usize,
    external: usize,
    skipped: usize,
}

impl Counts {
    fn of(entries: &[GemCompatEntry]) -> Self {
        let mut c = Counts {
            compiled: 0,
            builtin: 0,
            native: 0,
            external: 0,
            skipped: 0,
        };
        for e in entries {
            match e.outcome {
                GemCompatOutcome::Compiled => c.compiled += 1,
                GemCompatOutcome::Builtin { .. } => c.builtin += 1,
                GemCompatOutcome::NativeUnsupported { .. } => c.native += 1,
                GemCompatOutcome::ExternalSource => c.external += 1,
                GemCompatOutcome::Skipped { .. } => c.skipped += 1,
            }
        }
        c
    }
    /// Gems drawn from the RubyGems store (the denominator -- git/path and
    /// skipped default gems aren't zeo's to compile or reject).
    fn store_gems(&self) -> usize {
        self.compiled + self.builtin + self.native
    }
    fn usable(&self) -> usize {
        self.compiled + self.builtin
    }
    fn usable_pct(&self) -> f64 {
        let denom = self.store_gems();
        if denom == 0 {
            100.0
        } else {
            self.usable() as f64 / denom as f64 * 100.0
        }
    }
}

fn print_summary(entries: &[GemCompatEntry]) {
    let c = Counts::of(entries);
    eprintln!(
        "gem-compat: {}/{} store gems resolvable ({:.0}%) -- {} pure-ruby, {} built-in, {} native unsupported ({} external, {} skipped)",
        c.usable(),
        c.store_gems(),
        c.usable_pct(),
        c.compiled,
        c.builtin,
        c.native,
        c.external,
        c.skipped,
    );
    eprintln!(
        "gem-compat: note -- `pure-ruby` is a static classification (zeo would attempt to \
         compile it), not a verified compile."
    );
}

fn ruby_query(expr: &str) -> Result<String, String> {
    let out = Command::new("ruby")
        .arg("-e")
        .arg(expr)
        .output()
        .map_err(|e| format!("running ruby: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ruby -e exited non-zero: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
