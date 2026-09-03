//! Every bundled gem reaches codegen.
//!
//! The sweep covers the two upstream tiers of zeo's shipped stdlib: the
//! committed rubygems/bundler bootstrap under `lib/ruby/`, and the ~52
//! libraries resolved out of `vendor/bundle` from `Gemfile.lock`. Until this
//! file existed nothing swept them: a version bump could drag in a construct
//! zeo cannot lower, and the only signal would be whichever golden happened
//! to `require` that gem, or nothing at all for the seven gems no golden
//! covers. A lock bump is now the way a version moves, so the sweep is what
//! reads the new code first.
//!
//! zeo's OWN halves (`crates/zeo-rt/ext/`) are not here. They are zeo's
//! source, covered by the suites that exercise the Rust beside them.
//!
//! Front end only (`check_program_with` -- parse, splice, analyze; no CLIF
//! emission), so the whole sweep is seconds rather than the minutes a
//! build-and-run pass costs. That is the right depth for this check: "does
//! zeo still accept this library" is a front-end question, and behaviour is
//! already the goldens' job.

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
        // ruby's own `lib/English.gemspec` names the gem `english`; the file
        // it ships is `English.rb`.
        "english" => "English",
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

/// The upstream libraries zeo ships: the committed bootstrap pair and every
/// gem the lock resolves. Asked of the compiler's own resolver, so the sweep
/// covers exactly what a compile would load.
fn bundled_gems() -> Vec<String> {
    let root = crate::paths::workspace_root();
    let mut names: Vec<String> =
        zeo::bundled::libraries_in(&root.join(zeo::bundled::BOOTSTRAP_TIER))
            .into_iter()
            .chain(zeo::bundled::resolved_libraries(&root))
            .map(|lib| lib.name)
            .collect();
    names.sort();
    names
}

#[test]
fn every_bundled_gem_compiles() {
    let gems = bundled_gems();
    assert!(
        gems.len() > 40,
        "expected the full bundled stdlib, found {} gems -- has `bundle \
         install` run? (`make deps`)",
        gems.len()
    );

    // No package dirs: the compiler appends its own libraries unconditionally,
    // so naming them here would only be a second, staler spelling of the set.
    let opts = zeo::CompileOptions::default();

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
