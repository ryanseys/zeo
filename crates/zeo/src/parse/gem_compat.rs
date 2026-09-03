//! How each gem in a lockfile fares against zeo: a LAYOUT classification that
//! never runs the compiler. `gems_require.rs` is the consumer.
//!
//! Not lowering -- this classifies a gem STORE against a lockfile and is
//! consumed by one e2e test. It sits beside the
//! store/lockfile/gemspec readers it calls, rather than in `parse/mod.rs`,
//! which it has nothing to do with.

use super::gem_store;
use zeo_gem::LockedGem;

/// How one lockfile gem fares against zeo.
#[derive(Debug, Clone, PartialEq)]
pub struct GemCompatEntry {
    pub name: String,
    pub version: String,
    pub outcome: GemCompatOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GemCompatOutcome {
    /// Pure Ruby -- zeo resolved a require-path root and would compile it.
    Compiled,
    /// A name zeo provides via a built-in; the store copy is ignored.
    /// `diverges` when zeo's implementation is not the upstream gem.
    Builtin {
        diverges: bool,
        note: Option<String>,
    },
    /// The gem ships its C as SOURCE, and zeo compiles it. Distinguished
    /// from [`GemCompatOutcome::Compiled`] because it needs a C toolchain and
    /// the vendored headers, so a machine without either turns this row into
    /// a build failure and the pure-Ruby row into nothing.
    NativeSource { extensions: Vec<String> },
    /// A native gem zeo can't provide -- the detected layout and why.
    NativeUnsupported { kind: String, reason: String },
    /// A GIT/PATH-source lockfile gem, not drawn from the RubyGems store.
    ExternalSource,
    /// Resolvable in principle but contributed no root (e.g. a default gem's
    /// empty placeholder dir) -- zeo provides it as stdlib, not from here.
    Skipped { reason: String },
}

/// Classify every gem in `lockfile` against an installed `store` (`gem env
/// gemdir`), reusing the same provider. The out-of-the-box resolvability
/// matrix.
pub fn gem_compat(
    store: &std::path::Path,
    lockfile: &std::path::Path,
) -> Result<Vec<GemCompatEntry>, String> {
    let parsed = super::read_lockfile(lockfile).map_err(|e| e.message().to_string())?;
    classify(store, &parsed.resolved())
}

/// Like [`gem_compat`], but over EVERY gem installed in the store rather than a
/// lockfile's subset -- the broad out-of-the-box sample. Builds a synthetic gem
/// set from the store's own `specifications/`.
pub fn gem_compat_installed(store: &std::path::Path) -> Result<Vec<GemCompatEntry>, String> {
    let parsed = gem_store::installed_gems(store).map_err(|e| e.message().to_string())?;
    classify(store, &parsed)
}

fn classify(store: &std::path::Path, parsed: &[LockedGem]) -> Result<Vec<GemCompatEntry>, String> {
    use zeo_gem::lockfile::GemSource;
    let resolution = gem_store::resolve(&[store.to_path_buf()], parsed)?;

    let compiled: std::collections::HashSet<&str> =
        resolution.roots.iter().map(|(n, _)| n.as_str()).collect();
    let native: std::collections::HashMap<&str, &[String]> = resolution
        .native_exts
        .iter()
        .map(|e| (e.name.as_str(), e.extconfs.as_slice()))
        .collect();
    let disclosed: std::collections::HashMap<&str, &crate::gem_report::SatisfiedBy> = resolution
        .disclosures
        .iter()
        .map(|r| (r.name.as_str(), &r.by))
        .collect();

    let mut out = Vec::with_capacity(parsed.len());
    for gem in parsed {
        let outcome = if gem.source != GemSource::Rubygems {
            GemCompatOutcome::ExternalSource
        } else if let Some(extconfs) = native.get(gem.name.as_str()) {
            GemCompatOutcome::NativeSource {
                extensions: extconfs.to_vec(),
            }
        } else if compiled.contains(gem.name.as_str()) {
            GemCompatOutcome::Compiled
        } else {
            match disclosed.get(gem.name.as_str()) {
                Some(crate::gem_report::SatisfiedBy::Excluded { kind, reason }) => {
                    GemCompatOutcome::NativeUnsupported {
                        kind: kind.clone(),
                        reason: reason.clone(),
                    }
                }
                Some(_) => GemCompatOutcome::Builtin {
                    diverges: crate::gem_report::substitution_note(&gem.name).is_some(),
                    note: crate::gem_report::substitution_note(&gem.name).map(str::to_string),
                },
                None => GemCompatOutcome::Skipped {
                    reason: "no require-path root (default-gem placeholder or empty)".to_string(),
                },
            }
        };
        out.push(GemCompatEntry {
            name: gem.name.clone(),
            version: gem.version.clone(),
            outcome,
        });
    }
    Ok(out)
}
