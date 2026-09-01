//! First-use auto-packaging: the compile-side half of "stdlib as
//! packages".
//!
//! A linking compile (`opts.auto_package`) consults the machine package
//! cache for every BUNDLED gem its parse activated. A hit joins the merge
//! set and the gem's source stops splicing; a miss splices as today, and
//! after the program builds, [`build_and_cache`] packages the gem so the
//! NEXT compile hits. A package build that refuses is recorded
//! ([`crate::progcache::pkg_record_refusal`]) so no compile retries it
//! until zeo itself changes.
//!
//! The keys are the ones `zeo install` writes -- [`pkg_opts`] is the ONE
//! producer of a gem-package `CompileOptions`, shared with the CLI verbs
//! -- so the store road and the first-use road fill and read one cache.
//!
//! An artifact is used only when it can serve THIS compile whole:
//! its features cover what the compile required from the gem, and every
//! foreign feature its units require at run time is answerable -- by
//! another merged artifact, or by not being a gem feature at all (a `-I`
//! root the runtime searches, or a genuinely missing optional require,
//! which behaves the same packaged or spliced). A gem whose dependency
//! is not answerable splices from source instead; the dependency's own
//! build then fills the cache and the next compile links both.

use crate::package::UsePackage;
use std::path::{Path, PathBuf};

/// One bundled gem a compile could package: derived from the loader's
/// activation summary by the same entry probe `zeo install` runs.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub name: String,
    /// The require spelling (`net-http` -> `net/http`; `open-uri` ships
    /// `open-uri.rb` verbatim).
    pub feature: String,
    /// The file that spelling resolves to -- the package build's entry.
    pub entry: PathBuf,
}

/// What the discovery loop settled on -- rides out of the front end so
/// the object assembly links the packages and the CLI builds the misses.
#[derive(Default)]
pub(crate) struct AutoPackages {
    /// The EFFECTIVE merge set: the caller's `use_packages` plus every
    /// discovered artifact. Empty when discovery never ran.
    pub use_packages: Vec<UsePackage>,
    /// The used artifacts' own source files, re-read -- appended to the
    /// program-cache manifest so a gem edit invalidates the program too.
    pub extra_inputs: Vec<crate::progcache::Input>,
    /// Candidates with no artifact: build these after the program
    /// compiles, so the next compile hits.
    pub misses: Vec<Candidate>,
}

/// Whether a linking compile should auto-package at all: the cache is on
/// and `ZEO_DEBUG=no-auto-package` was not asked for.
pub fn enabled() -> bool {
    crate::progcache::enabled()
        && !crate::debug_flags::debug(crate::debug_flags::DebugFlag::NoAutoPackage)
}

std::thread_local! {
    /// Gems the discovery loop REJECTED for this compile (an unusable
    /// artifact: a required spelling it lacks, an uncovered dependency).
    /// The loader's resolution-time consult reads it so the next re-parse
    /// splices the gem from source. Thread-local because a compile runs
    /// whole on its one `zeo-compile` thread.
    static REJECTED: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

pub(crate) fn clear_rejected() {
    REJECTED.with(|r| r.borrow_mut().clear());
}

pub(crate) fn reject(name: &str) {
    REJECTED.with(|r| r.borrow_mut().insert(name.to_string()));
}

pub(crate) fn is_rejected(name: &str) -> bool {
    REJECTED.with(|r| r.borrow().contains(name))
}

/// The gem's package entry under `roots`: the features this compile
/// asked of it first (`require "English"` names English.rb, which no
/// name convention spells), then the conventional spellings `zeo
/// install`'s survey probes.
pub(crate) fn entry_for(
    name: &str,
    roots: &[PathBuf],
    required: &[String],
) -> Option<(String, PathBuf)> {
    if ext_backed(name) {
        return None;
    }
    required
        .iter()
        .cloned()
        .chain([name.replace('-', "/"), name.to_string()])
        .find_map(|feature| {
            roots
                .iter()
                .map(|r| r.join(format!("{feature}.rb")))
                .find(|p| p.is_file())
                .map(|p| (feature, p))
        })
}

/// A gem whose feature this build serves NATIVELY never auto-packages:
/// its Ruby half rides the builtin's class tables, and a merged package
/// would shadow the native rows (json's `module_function` singletons
/// vanished behind exactly this). The same rule `zeo install`'s survey
/// applies as its native-ext skip.
fn ext_backed(name: &str) -> bool {
    crate::lower::features::zeo_provides(&name.replace('-', "/"))
}

/// Whether the machine cache holds a servable artifact for the gem --
/// the loader's RESOLUTION-TIME probe, so a hit gem is never spliced (or
/// lowered) at all. Existence and manifest-verify only; the manifest
/// itself is read once by [`consult_new`]. A recorded refusal answers
/// false, and so does a rejection from an earlier discovery round.
/// `feature` is the require that asked, which the entry probe prefers.
pub(crate) fn cache_has(name: &str, roots: &[PathBuf], feature: &str) -> bool {
    if is_rejected(name) {
        return false;
    }
    let Some((feature, entry)) = entry_for(name, roots, &[feature.to_string()]) else {
        return false;
    };
    let Ok(opts) = pkg_opts(&feature, &entry, PathBuf::new()) else {
        return false;
    };
    let key = crate::progcache::pkg_key("", &opts);
    crate::progcache::pkg_refusal(&key).is_none() && crate::progcache::pkg_lookup(&key).is_some()
}

/// The bundled gem that provides `feature`, as a consultable candidate --
/// how a DEFERRED gem's foreign dependency joins the merge when nothing
/// in the current parse requires it (the warm road: packaged rss still
/// requires "English" at run time).
fn bundled_provider(feature: &str) -> Option<Candidate> {
    crate::parse::bundled_gem_roots()
        .into_iter()
        .find_map(|(name, roots)| {
            if ext_backed(&name) {
                return None;
            }
            let entry = roots
                .iter()
                .map(|r| r.join(format!("{feature}.rb")))
                .find(|p| p.is_file())?;
            Some(Candidate {
                name,
                feature: feature.to_string(),
                entry,
            })
        })
}

/// The ONE producer of a gem-package compile's options -- `zeo install`,
/// `zeo gem precompile`, `--package` and the first-use tier all key the
/// machine cache through this, so they fill and read one entry per gem.
/// `manifest_out` names where the build writes its manifest; the cache
/// key neutralizes it (`progcache::pkg_key`).
pub fn pkg_opts(
    feature: &str,
    entry: &Path,
    manifest_out: PathBuf,
) -> Result<crate::CompileOptions, String> {
    let root = entry
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|e| format!("resolving {}: {e}", entry.display()))?;
    Ok(crate::CompileOptions {
        input_path: Some(entry.to_path_buf()),
        file_name: Some(PathBuf::from(format!("<package {feature}>"))),
        mode: crate::CompileMode::Program,
        package_build: Some(crate::package::PackageBuild {
            entry: entry.to_path_buf(),
            feature: feature.to_string(),
            manifest_out,
            root,
        }),
        ..Default::default()
    })
}

/// The candidates a parse's activation summary implies: each with the
/// features the compile required from it, and whether the parse DEFERRED
/// it to a cached artifact. A gem without a conventional entry file
/// contributes nothing (rubygems-update's hidden lib is the shape).
pub(crate) fn candidates(hir: &crate::hir::Hir) -> Vec<(Candidate, Vec<String>, bool)> {
    hir.loader
        .activated_bundled
        .iter()
        .filter_map(|gem| {
            let (feature, entry) = entry_for(&gem.name, &gem.roots, &gem.features)?;
            Some((
                Candidate {
                    name: gem.name.clone(),
                    feature,
                    entry,
                },
                gem.features.clone(),
                gem.deferred,
            ))
        })
        .collect()
}

/// One consult of the machine cache for `cand`.
enum Consult {
    /// A valid artifact, its parsed manifest, and the source files its
    /// cache entry vouches for (re-read, for the program manifest).
    Hit {
        up: UsePackage,
        manifest: crate::package::Manifest,
        inputs: Vec<crate::progcache::Input>,
    },
    /// A recorded refusal, or no entry: splice from source. `build` says
    /// whether a post-compile build is worth attempting.
    Miss { build: bool },
}

fn consult(cand: &Candidate) -> Consult {
    let no_build = Consult::Miss { build: false };
    let Ok(opts) = pkg_opts(&cand.feature, &cand.entry, PathBuf::new()) else {
        return no_build;
    };
    let key = crate::progcache::pkg_key("", &opts);
    if let Some(reason) = crate::progcache::pkg_refusal(&key) {
        tracing::debug!(
            "autopkg: '{}' has a recorded refusal, splicing from source: {}",
            cand.feature,
            reason.lines().next().unwrap_or_default()
        );
        return no_build;
    }
    let Some(hit) = crate::progcache::pkg_lookup(&key) else {
        return Consult::Miss { build: true };
    };
    let Ok((manifest_text, bytes)) = crate::package::read_zeopkg(&hit) else {
        return Consult::Miss { build: true };
    };
    // A stale entry (another manifest version, another ABI) reads as a
    // miss: the rebuild overwrites it.
    let Ok(manifest) = crate::package::Manifest::parse(&manifest_text) else {
        return Consult::Miss { build: true };
    };
    let object_digest = crate::package::fnv64(&bytes);
    let Ok(object) = crate::progcache::pkg_object_file(object_digest, &bytes) else {
        return no_build;
    };
    // The entry's own manifest rows name every source the artifact was
    // built from; re-read them so the PROGRAM cache's manifest covers the
    // gem too, and a gem edit invalidates the cached binary.
    let inputs = crate::progcache::pkg_manifest_rows(&key)
        .map(|rows| {
            crate::progcache::manifest_files(&rows)
                .into_iter()
                .filter_map(|p| {
                    let text = std::fs::read_to_string(&p).ok()?;
                    Some(crate::progcache::Input {
                        name: p.display().to_string(),
                        source: std::sync::Arc::from(text.as_str()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Consult::Hit {
        up: UsePackage {
            manifest_path: hit,
            manifest_text,
            object,
            object_digest,
        },
        manifest,
        inputs,
    }
}

/// Every feature spelling `m` can answer a require for.
fn manifest_features(m: &crate::package::Manifest) -> Vec<String> {
    let mut v: Vec<String> = m.units.iter().map(|(f, _)| f.clone()).collect();
    v.push(m.feature.clone());
    v
}

/// One round of discovery against `hir`: consult the cache for every
/// candidate not already merged, keep the hits that can serve this
/// compile whole, and record the rest as misses. Returns the newly
/// usable artifacts plus whether any DEFERRED gem was rejected -- either
/// way the caller re-parses (merging the hits, or splicing the rejects)
/// and asks again until neither changes.
pub(crate) fn consult_new(
    hir: &crate::hir::Hir,
    merged: &[UsePackage],
    auto: &mut AutoPackages,
) -> (Vec<UsePackage>, bool) {
    // What the merge set can already answer.
    let mut covered: std::collections::HashSet<String> = merged
        .iter()
        .filter_map(|p| crate::package::Manifest::parse(&p.manifest_text).ok())
        .flat_map(|m| manifest_features(&m))
        .collect();
    struct HitRow {
        cand: Candidate,
        required: Vec<String>,
        deferred: bool,
        up: UsePackage,
        features: Vec<String>,
        host_features: Vec<String>,
    }
    let mut hits: Vec<HitRow> = Vec::new();
    let mut rejected_any = false;
    for (cand, required, deferred) in candidates(hir) {
        if covered.contains(&cand.feature)
            || auto.misses.iter().any(|m| m.name == cand.name)
        {
            continue;
        }
        match consult(&cand) {
            Consult::Hit {
                up,
                manifest,
                inputs,
            } => {
                auto.extra_inputs.extend(inputs);
                hits.push(HitRow {
                    cand,
                    required,
                    deferred,
                    features: manifest_features(&manifest),
                    host_features: manifest.host_features,
                    up,
                });
            }
            Consult::Miss { build } => {
                tracing::debug!("autopkg: no artifact for '{}', splicing", cand.feature);
                // A DEFERRED gem left its requires unanswered on the
                // artifact's promise; with no artifact after all, the
                // gem is rejected and the re-parse splices it.
                if deferred {
                    reject(&cand.name);
                    rejected_any = true;
                }
                if build {
                    auto.misses.push(cand);
                }
            }
        }
    }
    for h in &hits {
        covered.extend(h.features.iter().cloned());
    }
    // Keep only hits every requirement of which is answerable: the
    // features this compile required from the gem, and the foreign
    // features its units require at run time -- each must come from a
    // merged or kept artifact, because a packaged gem's interior require
    // is invisible to the re-parse and no source splice will stand in.
    // An uncovered feature is first CHASED as a dependency: its own
    // bundled provider's artifact joins the hits (the warm road, where a
    // deferred gem's dependencies appear in no parse at all). Only a
    // dependency the cache cannot answer drops the hit; the drop shrinks
    // the covered set, so this runs to a fixpoint.
    let mut chased: std::collections::HashSet<String> = std::collections::HashSet::new();
    loop {
        let Some((i, f)) = hits.iter().enumerate().find_map(|(i, h)| {
            h.required
                .iter()
                .chain(&h.host_features)
                // A feature the build serves natively is covered by the
                // host binary itself; its gem never packages (`ext_backed`).
                .find(|f| !covered.contains(*f) && !crate::lower::features::zeo_provides(f))
                .map(|f| (i, f.clone()))
        }) else {
            break;
        };
        if chased.insert(f.clone())
            && let Some(cand) = bundled_provider(&f)
            && let Consult::Hit {
                up,
                manifest,
                inputs,
            } = consult(&cand)
        {
            auto.extra_inputs.extend(inputs);
            covered.extend(manifest_features(&manifest));
            hits.push(HitRow {
                cand,
                required: vec![f],
                deferred: false,
                features: manifest_features(&manifest),
                host_features: manifest.host_features,
                up,
            });
            continue;
        }
        // No artifact can answer `f`: queue its provider for the
        // post-compile build, and drop the hit that needed it.
        if let Some(cand) = bundled_provider(&f)
            && !auto.misses.iter().any(|m| m.name == cand.name)
            && crate::progcache::pkg_refusal(&crate::progcache::pkg_key(
                "",
                &pkg_opts(&cand.feature, &cand.entry, PathBuf::new()).unwrap_or_default(),
            ))
            .is_none()
        {
            auto.misses.push(cand);
        }
        let dropped = hits.swap_remove(i);
        tracing::debug!(
            "autopkg: artifact for '{}' cannot answer '{f}' here, splicing",
            dropped.cand.feature
        );
        if dropped.deferred {
            reject(&dropped.cand.name);
            rejected_any = true;
        }
        covered = merged
            .iter()
            .filter_map(|p| crate::package::Manifest::parse(&p.manifest_text).ok())
            .flat_map(|m| manifest_features(&m))
            .collect();
        for h in &hits {
            covered.extend(h.features.iter().cloned());
        }
    }
    hits.iter().for_each(|h| {
        tracing::debug!("autopkg: linking '{}' from the package cache", h.cand.feature);
    });
    (hits.into_iter().map(|h| h.up).collect(), rejected_any)
}

/// Build `cand`'s package into the machine cache, for the NEXT compile.
/// A build refusal is recorded and answered `Ok`: the gem keeps splicing,
/// which is the fallback contract, not an error.
pub fn build_and_cache(cand: &Candidate) -> Result<(), String> {
    let manifest_out = std::env::temp_dir().join(format!(
        "zeo-autopkg-{}-{:016x}.zman",
        std::process::id(),
        crate::package::fnv64(cand.entry.as_os_str().as_encoded_bytes())
    ));
    let opts = pkg_opts(&cand.feature, &cand.entry, manifest_out.clone())?;
    let key = crate::progcache::pkg_key("", &opts);
    if crate::progcache::pkg_refusal(&key).is_some()
        || crate::progcache::pkg_lookup(&key).is_some()
    {
        return Ok(());
    }
    match crate::compile_to_object_with("", &opts, false) {
        Err(e) => {
            let reason = e.to_string();
            tracing::debug!(
                "autopkg: the package build for '{}' refused: {}",
                cand.feature,
                reason.lines().next().unwrap_or_default()
            );
            crate::progcache::pkg_record_refusal(&key, &reason);
            Ok(())
        }
        Ok(compiled) => {
            let manifest_json = std::fs::read_to_string(&manifest_out)
                .map_err(|e| format!("reading {}: {e}", manifest_out.display()))?;
            let _ = std::fs::remove_file(&manifest_out);
            let slot = crate::progcache::pkg_reserve(&key)
                .map_err(|e| format!("reserving the cache entry: {e}"))?;
            crate::package::write_zeopkg(&slot, &manifest_json, &compiled.object)
                .map_err(|e| format!("writing {}: {e}", slot.display()))?;
            crate::progcache::pkg_commit(&key, &compiled.inputs)
                .map_err(|e| format!("recording the cache manifest: {e}"))?;
            tracing::debug!("autopkg: packaged '{}' for the next compile", cand.feature);
            Ok(())
        }
    }
}
