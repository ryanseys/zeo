//! Every bundled gem reaches codegen.
//!
//! `gems/` is zeo's shipped stdlib -- ~50 trees, most of them vendored from
//! upstream by `zeo-dev gem`. Until this file existed nothing swept them: a
//! version bump could drag in a construct zeo cannot lower, and the only
//! signal would be whichever golden happened to `require` that gem, or
//! nothing at all for the seven gems no golden covers.
//!
//! Front end only (`check_program_with` -- parse, splice, analyze; no CLIF
//! emission), so the whole sweep is seconds rather than the minutes a
//! build-and-run pass costs. That is the right depth for this check: "does
//! zeo still accept this library" is a front-end question, and behaviour is
//! already the goldens' job.

use std::path::PathBuf;

fn repo(rel: &str) -> PathBuf {
    crate::paths::workspace_root().join(rel)
}

/// The feature name to `require`, when it is not the directory name.
///
/// A gem directory is named for the GEM (`net-http`); the feature its users
/// require is a path (`net/http`). The rest are gems whose entry point simply
/// is not the gem name.
fn entry_point(dir: &str) -> &str {
    match dir {
        "net-http" => "net/http",
        "net-ftp" => "net/ftp",
        "net-smtp" => "net/smtp",
        "net-protocol" => "net/protocol",
        "English" => "English",
        // The gem is `nkf`; its Ruby half is the Kconv wrapper.
        "nkf" => "kconv",
        other => other,
    }
}

/// Gems deliberately not swept, each for a reason this test failing would not
/// fix. `the_skip_list_names_real_gems` reads the same list, so a renamed gem
/// cannot drop silently out of the sweep.
const SKIPPED: &[(&str, &str)] = &[
    // Requiring irb reaches a runtime-refinement construct zeo does not lower
    // yet. It is tracked as a gap, and irb is not usable AOT regardless.
    ("irb", "runtime refinements (known gap)"),
];

fn skip_reason(dir: &str) -> Option<&'static str> {
    SKIPPED.iter().find(|(n, _)| *n == dir).map(|(_, why)| *why)
}

/// Every subdirectory of `gems/` that carries a gemspec -- the same rule
/// `parse/loader.rs` uses to decide what is a package.
fn bundled_gems() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(repo("gems"))
        .expect("gems/ is readable")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            std::fs::read_dir(e.path()).is_ok_and(|mut d| {
                d.any(|f| f.is_ok_and(|f| f.path().extension().is_some_and(|x| x == "gemspec")))
            })
        })
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn every_bundled_gem_compiles() {
    let gems = bundled_gems();
    assert!(
        gems.len() > 40,
        "expected the full bundled stdlib, found {} gems -- has gems/ moved?",
        gems.len()
    );

    let opts = zeo::CompileOptions {
        package_dirs: vec![repo("gems")],
        ..Default::default()
    };

    let mut failures = Vec::new();
    let mut compiled = 0;
    for gem in &gems {
        if skip_reason(gem).is_some() {
            continue;
        }
        let source = format!("require {:?}\n", entry_point(gem));
        match zeo::analyze_program(&source, &opts) {
            Ok(a) => {
                compiled += 1;
                failures.extend(colliding_unit_features(&a).into_iter().map(|c| {
                    format!("  {gem}: two compiled-in files claim `require \"{}\"` -- {} and {}\n    (the demand recording is the bug: one of them registered a spelling that is not its own)", c.0, c.1, c.2)
                }));
                failures.extend(zeo::dump::unrevealable_classes(&a).into_iter().map(|name| {
                    format!("  {gem}: `{name}` can never be revealed -- no class body site that any statement stream runs\n    (`zeo --dump=classes={name}` shows the sites and the stream each belongs to)")
                }));
            }
            Err(e) => failures.push(format!("  {gem}: {e}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} bundled gems no longer reach codegen:\n{}",
        failures.len(),
        compiled + failures.len(),
        failures.join("\n")
    );
}

/// Every feature spelling two of this program's units both claim.
///
/// `zeo_rt::features::UNITS` is a map, so a duplicate key means one file
/// silently answers a `require` written for another -- and the require
/// reports SUCCESS and records the feature loaded, so no later require can
/// recover. `pub_grub/static_package_source.rb`'s `require_relative
/// 'rubygems'` claimed the global spelling `rubygems` this way and left
/// `Gem` undefined behind a `require "rubygems"` that answered true.
///
/// Checked over the bundled gems because that is where the shape lives:
/// eight files are named `version.rb` across the vendored rubygems and
/// bundler trees alone.
fn colliding_unit_features(a: &zeo::analyze::Analyzed) -> Vec<(String, String, String)> {
    let mut claimed: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    let mut hits = Vec::new();
    for (names, absolute, _) in &a.feature_units {
        for name in names {
            match claimed.get(name.as_str()) {
                Some(&winner) if winner != absolute.as_str() => {
                    hits.push((name.clone(), winner.to_string(), absolute.clone()));
                }
                Some(_) => {}
                None => {
                    claimed.insert(name, absolute);
                }
            }
        }
    }
    hits
}

/// A skip entry naming a gem that no longer exists would drop that gem out of
/// the sweep instead of failing it.
#[test]
fn the_skip_list_names_real_gems() {
    let gems = bundled_gems();
    let stale: Vec<&str> = SKIPPED
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !gems.iter().any(|g| g == n))
        .collect();
    assert!(
        stale.is_empty(),
        "skipped by name but no longer a bundled gem: {stale:?}"
    );
}
