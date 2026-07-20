//! `cargo xtask gem-compat`: how each gem in a lockfile fares against spinel.
//!
//! Not lowering -- this classifies a gem STORE against a lockfile and is
//! consumed by the xtask compat matrix (plus one e2e test). It lived at the
//! top of `parse/mod.rs`, ahead of 4700 lines of prism-to-HIR lowering it has
//! nothing to do with; it belongs beside the store/lockfile/gemspec readers it
//! actually calls.

use super::{gem_store, lockfile};

/// How one lockfile gem fares against spinel, for `cargo xtask gem-compat`.
#[derive(Debug, Clone, PartialEq)]
pub struct GemCompatEntry {
    pub name: String,
    pub version: String,
    pub outcome: GemCompatOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GemCompatOutcome {
    /// Pure Ruby -- spinel resolved a require-path root and would compile it.
    Compiled,
    /// A name spinel provides via a built-in; the store copy is ignored.
    /// `diverges` when spinel's implementation is not the upstream gem.
    Builtin { diverges: bool, note: Option<String> },
    /// A native gem spinel can't provide -- the detected layout and why.
    NativeUnsupported { kind: String, reason: String },
    /// A GIT/PATH-source lockfile gem, not drawn from the RubyGems store.
    ExternalSource,
    /// Resolvable in principle but contributed no root (e.g. a default gem's
    /// empty placeholder dir) -- spinel provides it as stdlib, not from here.
    Skipped { reason: String },
}

/// Classify every gem in `lockfile` against an installed `store` (`gem env
/// gemdir`), reusing the Phase-3 provider. The out-of-the-box resolvability
/// matrix behind `cargo xtask gem-compat`.
pub fn gem_compat(
    store: &std::path::Path,
    lockfile: &std::path::Path,
) -> Result<Vec<GemCompatEntry>, String> {
    let parsed = lockfile::parse_file(lockfile)?;
    classify(store, &parsed)
}

/// Like [`gem_compat`], but over EVERY gem installed in the store rather than a
/// lockfile's subset -- the broad out-of-the-box sample `cargo xtask
/// gem-compat` runs when given no lockfile. Builds a synthetic gem set from the
/// store's own `specifications/`.
pub fn gem_compat_installed(
    store: &std::path::Path,
) -> Result<Vec<GemCompatEntry>, String> {
    let parsed = gem_store::installed_as_lockfile(store)?;
    classify(store, &parsed)
}

fn classify(
    store: &std::path::Path,
    parsed: &lockfile::Lockfile,
) -> Result<Vec<GemCompatEntry>, String> {
    use super::lockfile::GemSource;
    let resolution = gem_store::resolve(store, parsed)?;

    let compiled: std::collections::HashSet<&str> =
        resolution.roots.iter().map(|(n, _)| n.as_str()).collect();
    let disclosed: std::collections::HashMap<&str, &crate::gem_report::SatisfiedBy> = resolution
        .disclosures
        .iter()
        .map(|r| (r.name.as_str(), &r.by))
        .collect();

    let mut out = Vec::with_capacity(parsed.gems.len());
    for gem in &parsed.gems {
        let outcome = if gem.source != GemSource::Rubygems {
            GemCompatOutcome::ExternalSource
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
