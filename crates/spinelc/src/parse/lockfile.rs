//! `Gemfile.lock` reader -- Bundler's NORMALIZED resolution output, consumed
//! verbatim. spinel never resolves, fetches, or version-solves; it reads the
//! answer Bundler already wrote (`lockfile_parser.rb` is the reference).
//!
//! Only what an AOT compiler needs is kept: the locked `(name, version)` set
//! across every source section (`GEM`/`GIT`/`PATH`), plus the declared
//! `PLATFORMS`. Dependency EDGES (the 6-space-indented lines under a spec) are
//! skipped -- an AOT compiler pulls in only what a `require` actually reaches,
//! so the edge graph adds nothing over "these gems are available".
//!
//! Platform handling follows TruffleRuby's `force_ruby_platform`: when a gem is
//! locked for several platforms (`nokogiri (1.16.0)` AND
//! `nokogiri (1.16.0-arm64-darwin)`), the `ruby`-platform (suffix-less) row is
//! preferred, because a precompiled platform gem ships a `.bundle` spinel can
//! never load -- forcing the source platform is what makes it resolvable.

use std::collections::BTreeMap;
use std::path::Path;

use crate::parse::PResult;

/// The parsed lockfile: every locked gem, and the declared platforms.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Lockfile {
    /// Locked gems, deduped by name with the `ruby`-platform row preferred
    /// (see the module docs). Sorted by name for determinism.
    pub gems: Vec<LockedGem>,
    /// The `PLATFORMS` section, verbatim (`"ruby"`, `"arm64-darwin"`, ...).
    pub platforms: Vec<String>,
    /// `BUNDLED WITH`'s version, when present.
    pub bundler_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct LockedGem {
    pub name: String,
    pub version: String,
    /// The platform suffix on the spec line (`arm64-darwin`), or `None` for a
    /// `ruby`-platform (suffix-less) row. Retained so the store provider can
    /// see whether a lockfile even offered a source-platform variant.
    pub platform: Option<String>,
    pub source: GemSource,
}

/// Which lockfile section a gem came from -- a `PATH`/`GIT` gem lives outside
/// the RubyGems store and is handled differently by the provider (Phase 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GemSource {
    /// A RubyGems `GEM` section gem -- found in the store's `specifications/`.
    Rubygems,
    /// A `GIT` section gem -- a checkout, not in the store.
    Git,
    /// A `PATH` section gem -- a local path, not in the store.
    Path,
}

/// The top-level (column-0) section headers Bundler writes. A line that is one
/// of these starts a new section; anything else is section content.
const KNOWN_SECTIONS: &[&str] = &[
    "GIT",
    "PATH",
    "GEM",
    "PLATFORMS",
    "DEPENDENCIES",
    "CHECKSUMS",
    "RUBY VERSION",
    "BUNDLED WITH",
];

/// Which source section we're currently inside, once past its `specs:` header.
#[derive(Clone, Copy)]
enum Section {
    None,
    Specs(GemSourceKind),
    Platforms,
    BundledWith,
    /// A section we read past without collecting (DEPENDENCIES, CHECKSUMS, ...).
    Ignored,
}

#[derive(Clone, Copy)]
enum GemSourceKind {
    Gem,
    Git,
    Path,
}

impl GemSourceKind {
    fn source(self) -> GemSource {
        match self {
            GemSourceKind::Gem => GemSource::Rubygems,
            GemSourceKind::Git => GemSource::Git,
            GemSourceKind::Path => GemSource::Path,
        }
    }
}

pub(super) fn parse_file(path: &Path) -> PResult<Lockfile> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Parse lockfile text. `name (version)` at 4-space indent is a spec; a
/// `ruby`-platform row wins over a platform-suffixed one of the same name.
pub(super) fn parse(text: &str) -> PResult<Lockfile> {
    // Keyed by name so a later row (e.g. the platform variant) can be compared
    // against an earlier one and the ruby-platform row kept. BTree for a
    // deterministic final order.
    let mut gems: BTreeMap<String, LockedGem> = BTreeMap::new();
    let mut platforms: Vec<String> = Vec::new();
    let mut bundler_version: Option<String> = None;
    let mut section = Section::None;
    // Whether a source block has passed its `specs:` line yet (before it, the
    // block still holds `remote:`/`revision:` metadata, not gems).
    let mut in_specs = false;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        // A column-0 line is a section header; it must be one we know, else the
        // lockfile has a shape this reader doesn't model -- a loud error beats
        // silently skipping locked gems.
        if !line.starts_with(' ') {
            let header = line.trim();
            let kind = KNOWN_SECTIONS.iter().find(|h| **h == header);
            let Some(&kind) = kind else {
                return Err(format!("unknown lockfile section header: {header:?}"));
            };
            section = match kind {
                "GEM" => Section::Specs(GemSourceKind::Gem),
                "GIT" => Section::Specs(GemSourceKind::Git),
                "PATH" => Section::Specs(GemSourceKind::Path),
                "PLATFORMS" => Section::Platforms,
                "BUNDLED WITH" => Section::BundledWith,
                _ => Section::Ignored,
            };
            in_specs = false;
            continue;
        }

        match section {
            Section::Specs(kind) => {
                let content = line.trim_start();
                if content == "specs:" {
                    in_specs = true;
                    continue;
                }
                if !in_specs {
                    // Still in the block's metadata (`remote:`, `revision:`,
                    // `branch:`, `glob:`) -- not a gem.
                    continue;
                }
                // A spec line is 4-space-indented; a dependency edge is
                // 6-space-indented. Only specs carry a locked version.
                let indent = line.len() - content.len();
                if indent != 4 {
                    continue;
                }
                if let Some((name, version, platform)) = parse_name_version(content) {
                    let gem = LockedGem {
                        name: name.clone(),
                        version,
                        platform,
                        source: kind.source(),
                    };
                    // ruby-platform row (no suffix) wins over a platform variant.
                    match gems.get(&name) {
                        Some(existing)
                            if existing.platform.is_none() && gem.platform.is_some() => {}
                        _ => {
                            gems.insert(name, gem);
                        }
                    }
                }
            }
            Section::Platforms => {
                platforms.push(line.trim().to_string());
            }
            Section::BundledWith => {
                bundler_version = Some(line.trim().to_string());
            }
            Section::None | Section::Ignored => {}
        }
    }

    Ok(Lockfile {
        gems: gems.into_values().collect(),
        platforms,
        bundler_version,
    })
}

/// `name (version)` or `name (version-platform)` -> `(name, version, platform)`.
/// The version's own dashes never split (a version is `\d`-led segments); only
/// a trailing non-numeric-led segment is a platform (`1.16.0-arm64-darwin`).
fn parse_name_version(s: &str) -> Option<(String, String, Option<String>)> {
    let open = s.find(" (")?;
    let name = s[..open].to_string();
    let inner = s[open + 2..].strip_suffix(')')?;
    if name.is_empty() || inner.is_empty() {
        return None;
    }
    // Split a trailing platform off the version. Bundler joins them with `-`;
    // the version is the leading `-`-separated run whose segments are all
    // numeric-led, and the platform is whatever remains.
    let (version, platform) = split_version_platform(inner);
    Some((name, version, platform))
}

/// `1.16.0-arm64-darwin` -> (`1.16.0`, Some(`arm64-darwin`)); `1.16.0` ->
/// (`1.16.0`, None). A platform segment is one that does not start with a
/// digit -- a gem version's segments always do (`1`, `0`, `beta1` is digit-led
/// too, but a pre-release rides the version, and no platform token is
/// digit-led).
fn split_version_platform(inner: &str) -> (String, Option<String>) {
    let mut parts = inner.split('-');
    let mut version = parts.next().unwrap_or("").to_string();
    let mut platform_parts: Vec<&str> = Vec::new();
    for part in parts {
        if platform_parts.is_empty() && part.starts_with(|c: char| c.is_ascii_digit()) {
            // Still part of the version (a `-`-joined pre-release like
            // `1.0-rc` never happens in a lockfile version, which uses `.`,
            // but a numeric-led trailing segment is treated as version).
            version.push('-');
            version.push_str(part);
        } else {
            platform_parts.push(part);
        }
    }
    let platform = (!platform_parts.is_empty()).then(|| platform_parts.join("-"));
    (version, platform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gem_specs_and_skips_dependency_edges() {
        let lock = parse(
            "GEM\n  remote: https://rubygems.org/\n  specs:\n    addressable (2.9.0)\n      public_suffix (>= 2.0.2, < 8.0)\n    ast (2.4.3)\n\nPLATFORMS\n  arm64-darwin-24\n  ruby\n\nBUNDLED WITH\n   2.5.6\n",
        )
        .unwrap();
        assert_eq!(lock.gems.len(), 2);
        assert_eq!(lock.gems[0], LockedGem {
            name: "addressable".into(),
            version: "2.9.0".into(),
            platform: None,
            source: GemSource::Rubygems,
        });
        assert_eq!(lock.gems[1].name, "ast");
        assert_eq!(lock.platforms, vec!["arm64-darwin-24", "ruby"]);
        assert_eq!(lock.bundler_version.as_deref(), Some("2.5.6"));
    }

    #[test]
    fn ruby_platform_row_wins_over_a_precompiled_variant() {
        // Whichever order they appear, the suffix-less row is kept.
        let a = parse("GEM\n  specs:\n    nokogiri (1.16.0)\n    nokogiri (1.16.0-arm64-darwin)\n")
            .unwrap();
        let b = parse("GEM\n  specs:\n    nokogiri (1.16.0-arm64-darwin)\n    nokogiri (1.16.0)\n")
            .unwrap();
        for lock in [a, b] {
            assert_eq!(lock.gems.len(), 1);
            assert_eq!(lock.gems[0].version, "1.16.0");
            assert_eq!(lock.gems[0].platform, None);
        }
    }

    #[test]
    fn git_and_path_sources_are_tagged() {
        let lock = parse(
            "GIT\n  remote: https://github.com/x/y.git\n  revision: abc\n  specs:\n    cuprite (0.17)\n\nPATH\n  remote: .\n  specs:\n    sow (0.1.0)\n\nGEM\n  specs:\n    ast (2.4.3)\n",
        )
        .unwrap();
        let by = |n: &str| lock.gems.iter().find(|g| g.name == n).unwrap().source.clone();
        assert_eq!(by("cuprite"), GemSource::Git);
        assert_eq!(by("sow"), GemSource::Path);
        assert_eq!(by("ast"), GemSource::Rubygems);
    }

    #[test]
    fn an_unknown_section_header_is_a_loud_error() {
        let err = parse("MYSTERY\n  stuff\n").unwrap_err();
        assert!(err.contains("unknown lockfile section"), "{err}");
    }

    /// The checked-in fixture -- a real `Gemfile.lock` generated offline with
    /// `bundle lock --local` (see `tests/fixtures/gem_store/`). Guards the
    /// parser against real Bundler output, including a native precompiled gem
    /// with a platform suffix and the `CHECKSUMS`/`BUNDLED WITH` sections.
    #[test]
    fn parses_the_checked_in_fixture_lockfile() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/gem_store/Gemfile.lock");
        let lock = parse_file(&path).unwrap();
        let gem = |n: &str| lock.gems.iter().find(|g| g.name == n).unwrap();

        // Pure-Ruby gems: no platform suffix.
        assert_eq!(gem("rake").version, "13.4.2");
        assert_eq!(gem("rake").platform, None);
        assert_eq!(gem("racc").platform, None);

        // A native gem shipped precompiled: the version and the platform suffix
        // split cleanly (the store provider forces the ruby platform later).
        assert_eq!(gem("nokogiri").version, "1.19.4");
        assert_eq!(gem("nokogiri").platform.as_deref(), Some("arm64-darwin"));

        assert!(lock.platforms.contains(&"arm64-darwin".to_string()));
        assert_eq!(lock.bundler_version.as_deref(), Some("4.0.15"));
    }
}
