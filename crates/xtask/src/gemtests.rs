//! `cargo run -p xtask -- gemtests sync`: fetch the WHOLE source trees of the
//! gems whose test suites the `gemtests` golden suite runs end-to-end
//! (`crates/zeo-tests/tests/gemtests.rs` over `tests/gemtests/<gem>/*.rb`).
//!
//! `.gem` archives don't ship `test/` (gemspec `files=` excludes it), so the
//! trees come from the upstream git repos, pinned in
//! `conformance/gemtests.toml` -- the same block format and fetch machinery as
//! `gems.toml`/`xtask gem`, but the vendor copies the FULL checkout (`lib/`,
//! `test/`, fixtures and all) into `vendor/gemtests/<name>/`, which is
//! gitignored: fetch-on-demand, the suite skips gracefully when absent.
//!
//! A stub gemspec is written so `vendor/gemtests/` doubles as a `--gems`
//! package dir -- the driver's `require "rack"` resolves against the vendored
//! `lib/` on the zeo side exactly as `-I .../lib` does for the CRuby oracle.

use std::path::Path;
use std::process::ExitCode;

use crate::gem::{GemEntry, copy_tree, fetch_checkout, read_manifest};

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let result = match args.first().map(String::as_str) {
        Some("sync") => sync(root),
        _ => Err("usage: cargo run -p xtask -- gemtests sync".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask gemtests: {e}");
            ExitCode::FAILURE
        }
    }
}

fn sync(root: &Path) -> Result<(), String> {
    let manifest = root.join("conformance/gemtests.toml");
    let entries = read_manifest(&manifest)?;
    if entries.is_empty() {
        return Err(format!("{} lists no gems", manifest.display()));
    }
    for entry in &entries {
        let rev = entry.rev.as_deref().ok_or_else(|| {
            format!(
                "{}: entry {} has no rev pin (add one like gems.toml's)",
                manifest.display(),
                entry.name
            )
        })?;
        let url = format!("https://github.com/{}", entry.github);
        let checkout = fetch_checkout(&entry.name, &url, &entry.tag, rev)?;
        let src = match &entry.subdir {
            Some(sub) => checkout.join(sub),
            None => checkout.clone(),
        };
        let dest = root.join("vendor/gemtests").join(&entry.name);
        let _ = std::fs::remove_dir_all(&dest);
        copy_tree(&src, &dest)?;
        write_stub_gemspec(&dest, entry)?;
        println!("vendored {} test tree @ {}", entry.name, entry.tag);
    }
    Ok(())
}

fn write_stub_gemspec(dest: &Path, entry: &GemEntry) -> Result<(), String> {
    let version = entry.tag.strip_prefix('v').unwrap_or(&entry.tag);
    // The loader requires spec.name == directory name; upstream gemspecs are
    // often non-literal (spec.version = X::VERSION), so the stub replaces
    // whatever the checkout carried.
    for existing in std::fs::read_dir(dest)
        .map_err(|e| format!("reading {}: {e}", dest.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "gemspec"))
    {
        let _ = std::fs::remove_file(existing);
    }
    let gemspec = format!(
        "# Test-tree vendor of https://github.com/{} @ {}; managed by `xtask gemtests`.\n\
         Gem::Specification.new do |s|\n  \
         s.name = {:?}\n  \
         s.version = {:?}\n  \
         s.require_paths = [\"lib\"]\n\
         end\n",
        entry.github, entry.tag, entry.name, version
    );
    std::fs::write(dest.join(format!("{}.gemspec", entry.name)), gemspec)
        .map_err(|e| format!("writing stub gemspec: {e}"))
}
