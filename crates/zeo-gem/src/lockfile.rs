//! `Gemfile.lock`: Bundler's resolution, read whole and written back byte for
//! byte.
//!
//! zeo never resolves, fetches, or version-solves. It reads the answer Bundler
//! already wrote (`lockfile_parser.rb` is the reference) and holds every
//! section, including the ones the compiler ignores: a tool that fetches gems
//! needs the `GEM remote:` URL and the `CHECKSUMS` rows, and a tool that
//! rewrites a lock must not silently drop what it did not understand.
//!
//! [`Lockfile::to_string`] reproduces the file it parsed exactly, so a
//! round-trip is a real test of the reader.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::error::{Error, Result, read};
use crate::platform::Platform;
use crate::version::{Requirement, Version};

/// The section headers Bundler writes, at column 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Git,
    Path,
    Gem,
    Plugin,
    Platforms,
    Dependencies,
    Checksums,
    RubyVersion,
    BundledWith,
}

impl SectionKind {
    fn parse(header: &str) -> Option<SectionKind> {
        Some(match header {
            "GIT" => SectionKind::Git,
            "PATH" => SectionKind::Path,
            "GEM" => SectionKind::Gem,
            "PLUGIN SOURCE" => SectionKind::Plugin,
            "PLATFORMS" => SectionKind::Platforms,
            "DEPENDENCIES" => SectionKind::Dependencies,
            "CHECKSUMS" => SectionKind::Checksums,
            "RUBY VERSION" => SectionKind::RubyVersion,
            "BUNDLED WITH" => SectionKind::BundledWith,
            _ => return None,
        })
    }

    fn header(self) -> &'static str {
        match self {
            SectionKind::Git => "GIT",
            SectionKind::Path => "PATH",
            SectionKind::Gem => "GEM",
            SectionKind::Plugin => "PLUGIN SOURCE",
            SectionKind::Platforms => "PLATFORMS",
            SectionKind::Dependencies => "DEPENDENCIES",
            SectionKind::Checksums => "CHECKSUMS",
            SectionKind::RubyVersion => "RUBY VERSION",
            SectionKind::BundledWith => "BUNDLED WITH",
        }
    }

    fn is_source(self) -> bool {
        matches!(
            self,
            SectionKind::Git | SectionKind::Path | SectionKind::Gem | SectionKind::Plugin
        )
    }
}

/// One `GEM`/`GIT`/`PATH` block: its metadata lines, then its specs.
///
/// The metadata is kept as the ordered key/value pairs Bundler wrote, not as
/// named fields, so a key this reader has never seen survives a rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub kind: SectionKind,
    pub meta: Vec<(String, String)>,
    pub specs: Vec<Spec>,
}

impl Source {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.meta
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The first `remote:` line -- the gem server, the git URL, or the path.
    pub fn remote(&self) -> Option<&str> {
        self.get("remote")
    }

    /// Every `remote:` line. A `GEM` block may name several servers.
    pub fn remotes(&self) -> Vec<&str> {
        self.meta
            .iter()
            .filter(|(k, _)| k == "remote")
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// The commit a `GIT` block is pinned to.
    pub fn revision(&self) -> Option<&str> {
        self.get("revision")
    }
}

/// One resolved gem: a `name (version)` line and the dependency edges under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    pub version: Version,
    /// [`Platform::Ruby`] for a suffix-less row -- the source gem.
    pub platform: Platform,
    pub dependencies: Vec<Dependency>,
}

impl Spec {
    /// RubyGems' name for the gem's own files: `nokogiri-1.19.4-arm64-darwin`.
    pub fn full_name(&self) -> String {
        full_name(&self.name, &self.version, &self.platform)
    }
}

/// A requested gem: an edge under a spec, or a row in `DEPENDENCIES`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub requirement: Requirement,
    /// The `!` marker: this gem comes from a `GIT`/`PATH` source, not the
    /// gem server. Only `DEPENDENCIES` rows carry it.
    pub pinned: bool,
}

/// One `CHECKSUMS` row: what the `.gem` file must hash to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    pub name: String,
    pub version: Version,
    pub platform: Platform,
    /// Empty when Bundler recorded the gem but no digest for it.
    pub digests: Vec<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    pub algorithm: String,
    pub bytes: Vec<u8>,
}

impl Checksum {
    pub fn full_name(&self) -> String {
        full_name(&self.name, &self.version, &self.platform)
    }

    /// The sha256 digest, the only algorithm Bundler writes today.
    pub fn sha256(&self) -> Option<&[u8]> {
        self.digests
            .iter()
            .find(|d| d.algorithm == "sha256")
            .map(|d| d.bytes.as_slice())
    }
}

fn full_name(name: &str, version: &Version, platform: &Platform) -> String {
    match platform.is_ruby() {
        true => format!("{name}-{version}"),
        false => format!("{name}-{version}-{platform}"),
    }
}

/// The parsed lockfile. Every section is kept, in the order the file wrote
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lockfile {
    pub sources: Vec<Source>,
    pub platforms: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub checksums: Vec<Checksum>,
    /// `RUBY VERSION`'s value, such as `ruby 3.4.1p0`.
    pub ruby_version: Option<String>,
    /// `BUNDLED WITH`'s value: the Bundler that wrote the file.
    pub bundled_with: Option<String>,
    /// Section headers in file order, so a rewrite keeps the layout.
    order: Vec<SectionKind>,
    /// How far `RUBY VERSION` and `BUNDLED WITH` indent their one value.
    /// Bundler 4 writes two spaces, earlier versions wrote three.
    scalar_indent: usize,
}

impl Lockfile {
    pub fn parse_file(path: &Path) -> Result<Lockfile> {
        let bytes = read(path)?;
        let text = String::from_utf8_lossy(&bytes);
        Lockfile::parse(&text).map_err(|e| Error::format(format!("{}: {e}", path.display())))
    }

    pub fn parse(text: &str) -> Result<Lockfile> {
        Parser::new().run(text)
    }

    /// Every spec across every source, with the source it came from.
    pub fn specs(&self) -> impl Iterator<Item = (&Source, &Spec)> {
        self.sources
            .iter()
            .flat_map(|s| s.specs.iter().map(move |spec| (s, spec)))
    }

    /// The first `GEM` block's server -- where a `.gem` is downloaded from.
    pub fn gem_remote(&self) -> Option<&str> {
        self.sources
            .iter()
            .find(|s| s.kind == SectionKind::Gem)
            .and_then(Source::remote)
    }

    /// The `CHECKSUMS` row for a `name-version[-platform]`.
    pub fn checksum(&self, full_name: &str) -> Option<&Checksum> {
        self.checksums.iter().find(|c| c.full_name() == full_name)
    }

    /// The `DEPENDENCIES` names in file order: the Gemfile's own requests,
    /// which are the roots of the dependency graph.
    pub fn roots(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for dep in &self.dependencies {
            if !out.contains(&dep.name) {
                out.push(dep.name.clone());
            }
        }
        out
    }

    /// The spec to install for `host`: the row built for this machine when the
    /// lock offers one, else the source row.
    pub fn spec_for(&self, name: &str, host: &Platform) -> Option<&Spec> {
        let rows: Vec<&Spec> = self
            .specs()
            .map(|(_, spec)| spec)
            .filter(|s| s.name == name)
            .collect();
        rows.iter()
            .copied()
            .find(|s| !s.platform.is_ruby() && s.platform.matches(host))
            .or_else(|| rows.iter().copied().find(|s| s.platform.is_ruby()))
            .or_else(|| rows.first().copied())
    }

    /// The compiler's view: one row per gem, the source row preferred.
    ///
    /// A precompiled platform gem ships a `.bundle` zeo can never load, so the
    /// suffix-less row is the one worth resolving -- the same choice
    /// TruffleRuby's `force_ruby_platform` makes. Sorted by name.
    pub fn resolved(&self) -> Vec<LockedGem> {
        let mut gems: BTreeMap<&str, LockedGem> = BTreeMap::new();
        for (source, spec) in self.specs() {
            let gem = LockedGem {
                name: spec.name.clone(),
                version: spec.version.to_string(),
                platform: (!spec.platform.is_ruby()).then(|| spec.platform.to_string()),
                source: GemSource::of(source.kind),
                deps: spec.dependencies.iter().map(|d| d.name.clone()).collect(),
            };
            match gems.get(spec.name.as_str()) {
                Some(kept) if kept.platform.is_none() && gem.platform.is_some() => {}
                _ => {
                    gems.insert(spec.name.as_str(), gem);
                }
            }
        }
        gems.into_values().collect()
    }
}

/// One gem in [`Lockfile::resolved`]'s deduped view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedGem {
    pub name: String,
    pub version: String,
    /// `None` for the source row; the platform suffix otherwise.
    pub platform: Option<String>,
    pub source: GemSource,
    /// The names this gem depends on -- the edges of the precedence graph.
    pub deps: Vec<String>,
}

/// Which kind of source a gem came from. A `PATH`/`GIT` gem lives outside the
/// RubyGems store and is found differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GemSource {
    Rubygems,
    Git,
    Path,
}

impl GemSource {
    fn of(kind: SectionKind) -> GemSource {
        match kind {
            SectionKind::Git => GemSource::Git,
            SectionKind::Path => GemSource::Path,
            _ => GemSource::Rubygems,
        }
    }
}

/// The reader. One pass, line by line, exactly as Bundler writes them.
struct Parser {
    lock: Lockfile,
    section: Option<SectionKind>,
    /// Whether the current source block has passed its `specs:` line.
    in_specs: bool,
}

impl Parser {
    fn new() -> Parser {
        Parser {
            lock: Lockfile {
                scalar_indent: 2,
                ..Lockfile::default()
            },
            section: None,
            in_specs: false,
        }
    }

    fn run(mut self, text: &str) -> Result<Lockfile> {
        for raw in text.lines() {
            let line = raw.trim_end();
            if line.is_empty() {
                continue;
            }
            if !line.starts_with(' ') {
                self.open_section(line)?;
                continue;
            }
            self.content(line)?;
        }
        Ok(self.lock)
    }

    fn open_section(&mut self, line: &str) -> Result<()> {
        let kind = SectionKind::parse(line.trim())
            .ok_or_else(|| Error::format(format!("unknown lockfile section: {:?}", line.trim())))?;
        self.section = Some(kind);
        self.in_specs = false;
        self.lock.order.push(kind);
        if kind.is_source() {
            self.lock.sources.push(Source {
                kind,
                meta: Vec::new(),
                specs: Vec::new(),
            });
        }
        Ok(())
    }

    fn content(&mut self, line: &str) -> Result<()> {
        let body = line.trim_start();
        let indent = line.len() - body.len();
        match self.section {
            None => Err(Error::format(format!(
                "a line before any section header: {line:?}"
            ))),
            Some(kind) if kind.is_source() => self.source_line(body, indent),
            Some(SectionKind::Platforms) => {
                self.lock.platforms.push(body.to_string());
                Ok(())
            }
            Some(SectionKind::Dependencies) => {
                self.lock.dependencies.push(parse_dependency(body)?);
                Ok(())
            }
            Some(SectionKind::Checksums) => {
                self.lock.checksums.push(parse_checksum(body)?);
                Ok(())
            }
            Some(SectionKind::RubyVersion) => {
                self.lock.scalar_indent = indent;
                self.lock.ruby_version = Some(body.to_string());
                Ok(())
            }
            Some(SectionKind::BundledWith) => {
                self.lock.scalar_indent = indent;
                self.lock.bundled_with = Some(body.to_string());
                Ok(())
            }
            Some(_) => Ok(()),
        }
    }

    fn source_line(&mut self, body: &str, indent: usize) -> Result<()> {
        let source = self
            .lock
            .sources
            .last_mut()
            .expect("a source section pushed one");
        if !self.in_specs {
            if body == "specs:" {
                self.in_specs = true;
                return Ok(());
            }
            // `remote: https://rubygems.org/`, `revision: abc`, `branch: main`.
            let (key, value) = body.split_once(':').ok_or_else(|| {
                Error::format(format!("a source line that is not `key: value`: {body:?}"))
            })?;
            source
                .meta
                .push((key.trim().to_string(), value.trim().to_string()));
            return Ok(());
        }
        // A spec indents 4, its dependency edges 6.
        match indent {
            6 => {
                let dep = parse_dependency(body)?;
                let spec = source.specs.last_mut().ok_or_else(|| {
                    Error::format(format!("a dependency edge with no spec above it: {body:?}"))
                })?;
                spec.dependencies.push(dep);
                Ok(())
            }
            _ => {
                source.specs.push(parse_spec(body)?);
                Ok(())
            }
        }
    }
}

/// `addressable (>= 2.0.2, < 8.0)`, `rake`, `my_gem!`,
/// `rubocop (~> 1.71)!`.
///
/// Bundler writes the `!` LAST, after the requirement, so it comes off the
/// whole line before anything else is read.
fn parse_dependency(line: &str) -> Result<Dependency> {
    let (line, pinned) = match line.strip_suffix('!') {
        Some(rest) => (rest, true),
        None => (line, false),
    };
    let (name, requirement) = match line.split_once(" (") {
        Some((head, rest)) => {
            let inner = rest.strip_suffix(')').ok_or_else(|| {
                Error::format(format!("a dependency with no closing paren: {line:?}"))
            })?;
            (head, Requirement::parse(inner)?)
        }
        None => (line, Requirement::default()),
    };
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::format(format!(
            "a dependency with no name: {line:?}"
        )));
    }
    Ok(Dependency {
        name: name.to_string(),
        requirement,
        pinned,
    })
}

/// `nokogiri (1.19.4-arm64-darwin)`.
fn parse_spec(line: &str) -> Result<Spec> {
    let (name, inner) = line
        .split_once(" (")
        .and_then(|(n, rest)| rest.strip_suffix(')').map(|inner| (n, inner)))
        .ok_or_else(|| {
            Error::format(format!(
                "a spec line that is not `name (version)`: {line:?}"
            ))
        })?;
    let (version, platform) = split_version_platform(inner);
    Ok(Spec {
        name: name.to_string(),
        version: Version::parse(&version)?,
        platform: platform.map_or(Platform::Ruby, |p| Platform::parse(&p)),
        dependencies: Vec::new(),
    })
}

/// `abbrev (0.1.2) sha256=ad1b...`, or a row with no digest at all.
fn parse_checksum(line: &str) -> Result<Checksum> {
    let (head, inner) = line
        .split_once(" (")
        .ok_or_else(|| Error::format(format!("a checksum row with no version: {line:?}")))?;
    let (inner, rest) = inner
        .split_once(')')
        .ok_or_else(|| Error::format(format!("a checksum row with no closing paren: {line:?}")))?;
    let (version, platform) = split_version_platform(inner);
    let mut digests = Vec::new();
    for field in rest.trim().split(',').filter(|f| !f.is_empty()) {
        let (algorithm, hex) = field.split_once('=').ok_or_else(|| {
            Error::format(format!("a checksum that is not `algorithm=hex`: {field:?}"))
        })?;
        digests.push(Digest {
            algorithm: algorithm.trim().to_string(),
            bytes: from_hex(hex.trim()).ok_or_else(|| {
                Error::format(format!("a checksum that is not hexadecimal: {hex:?}"))
            })?,
        });
    }
    Ok(Checksum {
        name: head.trim().to_string(),
        version: Version::parse(&version)?,
        platform: platform.map_or(Platform::Ruby, |p| Platform::parse(&p)),
        digests,
    })
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || text.is_empty() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `1.16.0-arm64-darwin` -> (`1.16.0`, `arm64-darwin`).
///
/// Bundler joins the two with `-`, and a version's own parts are digit-led, so
/// the first part that is not is where the platform starts.
pub fn split_version_platform(inner: &str) -> (String, Option<String>) {
    let mut parts = inner.split('-');
    let mut version = parts.next().unwrap_or("").to_string();
    let mut platform: Vec<&str> = Vec::new();
    for part in parts {
        if platform.is_empty() && part.starts_with(|c: char| c.is_ascii_digit()) {
            version.push('-');
            version.push_str(part);
        } else {
            platform.push(part);
        }
    }
    (version, (!platform.is_empty()).then(|| platform.join("-")))
}

impl fmt::Display for Lockfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = " ".repeat(self.scalar_indent);
        let mut sources = self.sources.iter();
        for (i, kind) in self.order.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            writeln!(f, "{}", kind.header())?;
            match kind {
                k if k.is_source() => {
                    let source = sources.next().expect("one source per source header");
                    for (key, value) in &source.meta {
                        writeln!(f, "  {key}: {value}")?;
                    }
                    writeln!(f, "  specs:")?;
                    for spec in &source.specs {
                        writeln!(f, "    {} ({})", spec.name, version_platform(spec))?;
                        for dep in &spec.dependencies {
                            writeln!(f, "      {}", dependency_text(dep))?;
                        }
                    }
                }
                SectionKind::Platforms => {
                    for platform in &self.platforms {
                        writeln!(f, "  {platform}")?;
                    }
                }
                SectionKind::Dependencies => {
                    for dep in &self.dependencies {
                        writeln!(f, "  {}", dependency_text(dep))?;
                    }
                }
                SectionKind::Checksums => {
                    for sum in &self.checksums {
                        // `name (version[-platform]) sha256=...`
                        write!(f, "  {} ({})", sum.name, checksum_version(sum))?;
                        for (i, digest) in sum.digests.iter().enumerate() {
                            let lead = if i == 0 { " " } else { "," };
                            write!(f, "{lead}{}={}", digest.algorithm, to_hex(&digest.bytes))?;
                        }
                        writeln!(f)?;
                    }
                }
                SectionKind::RubyVersion => {
                    if let Some(v) = &self.ruby_version {
                        writeln!(f, "{indent}{v}")?;
                    }
                }
                SectionKind::BundledWith => {
                    if let Some(v) = &self.bundled_with {
                        writeln!(f, "{indent}{v}")?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn version_platform(spec: &Spec) -> String {
    join_version_platform(&spec.version, &spec.platform)
}

fn checksum_version(sum: &Checksum) -> String {
    join_version_platform(&sum.version, &sum.platform)
}

fn join_version_platform(version: &Version, platform: &Platform) -> String {
    match platform.is_ruby() {
        true => version.to_string(),
        false => format!("{version}-{platform}"),
    }
}

fn dependency_text(dep: &Dependency) -> String {
    let mut out = dep.name.clone();
    if !dep.requirement.clauses.is_empty() {
        out.push_str(&format!(" ({})", dep.requirement));
    }
    if dep.pinned {
        out.push('!');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "\
GEM
  remote: https://rubygems.org/
  specs:
    addressable (2.9.0)
      public_suffix (>= 2.0.2, < 8.0)
    ast (2.4.3)
    nokogiri (1.19.4-arm64-darwin)
      racc (~> 1.4)
    racc (1.8.1)

PLATFORMS
  arm64-darwin
  ruby

DEPENDENCIES
  addressable (~> 2.9)
  local_gem!
  rake

CHECKSUMS
  addressable (2.9.0) sha256=ad1b4eaaaed4cb722d5684d63949e4bde1d34f2a95e20db93aecfe7cbac74242
  ast (2.4.3)
  nokogiri (1.19.4-arm64-darwin) sha256=27337aeabad6ffae05c265c450490628ef3ebd4b67be58257393227588f5a97b

RUBY VERSION
   ruby 3.4.1p0

BUNDLED WITH
   2.5.6
";

    fn small() -> Lockfile {
        Lockfile::parse(SMALL).expect("the fixture parses")
    }

    #[test]
    fn every_section_is_read() {
        let lock = small();
        assert_eq!(lock.sources.len(), 1);
        assert_eq!(lock.sources[0].kind, SectionKind::Gem);
        assert_eq!(lock.gem_remote(), Some("https://rubygems.org/"));
        assert_eq!(lock.sources[0].specs.len(), 4);
        assert_eq!(lock.platforms, ["arm64-darwin", "ruby"]);
        assert_eq!(lock.dependencies.len(), 3);
        assert_eq!(lock.checksums.len(), 3);
        assert_eq!(lock.ruby_version.as_deref(), Some("ruby 3.4.1p0"));
        assert_eq!(lock.bundled_with.as_deref(), Some("2.5.6"));
    }

    /// The whole point of keeping every section: a rewrite loses nothing.
    #[test]
    fn the_file_round_trips_byte_for_byte() {
        assert_eq!(small().to_string(), SMALL);
    }

    #[test]
    fn a_three_space_scalar_indent_survives_the_round_trip() {
        // Bundler 4 writes two spaces where earlier versions wrote three.
        let two = SMALL
            .replace("\n   ruby 3.4.1p0", "\n  ruby 3.4.1p0")
            .replace("\n   2.5.6", "\n  2.5.6");
        assert_eq!(Lockfile::parse(&two).unwrap().to_string(), two);
    }

    #[test]
    fn a_spec_carries_its_dependency_edges_with_their_requirements() {
        let lock = small();
        let spec = &lock.sources[0].specs[0];
        assert_eq!(spec.name, "addressable");
        assert_eq!(spec.version.as_str(), "2.9.0");
        assert!(spec.platform.is_ruby());
        assert_eq!(spec.dependencies.len(), 1);
        assert_eq!(spec.dependencies[0].name, "public_suffix");
        assert_eq!(
            spec.dependencies[0].requirement.to_string(),
            ">= 2.0.2, < 8.0"
        );
        assert!(!spec.dependencies[0].pinned);
    }

    #[test]
    fn a_platform_suffix_splits_off_the_version() {
        let lock = small();
        let spec = lock
            .specs()
            .map(|(_, s)| s)
            .find(|s| s.name == "nokogiri")
            .unwrap();
        assert_eq!(spec.version.as_str(), "1.19.4");
        assert_eq!(spec.platform.to_string(), "arm64-darwin");
        assert_eq!(spec.full_name(), "nokogiri-1.19.4-arm64-darwin");
    }

    /// Bundler puts the `!` after the requirement, not after the name.
    #[test]
    fn a_pinned_dependency_with_a_requirement_reads_and_writes_both() {
        let text = "DEPENDENCIES\n  rubocop (~> 1.71)!\n";
        let lock = Lockfile::parse(text).unwrap();
        assert_eq!(lock.dependencies[0].name, "rubocop");
        assert!(lock.dependencies[0].pinned);
        assert_eq!(lock.dependencies[0].requirement.to_string(), "~> 1.71");
        assert_eq!(lock.to_string(), text);
    }

    #[test]
    fn a_dependency_row_keeps_its_pinned_marker() {
        let lock = small();
        let pinned = lock
            .dependencies
            .iter()
            .find(|d| d.name == "local_gem")
            .unwrap();
        assert!(pinned.pinned);
        assert!(pinned.requirement.clauses.is_empty());
        assert_eq!(lock.roots(), ["addressable", "local_gem", "rake"]);
    }

    #[test]
    fn a_checksum_row_yields_raw_bytes_and_a_full_name() {
        let lock = small();
        let sum = lock
            .checksum("addressable-2.9.0")
            .expect("a row for addressable");
        assert_eq!(sum.sha256().map(|b| b.len()), Some(32));
        assert_eq!(sum.sha256().unwrap()[0], 0xad);
        // A row Bundler wrote with no digest is legal and stays empty.
        assert!(lock.checksum("ast-2.4.3").unwrap().sha256().is_none());
        assert!(lock.checksum("nokogiri-1.19.4-arm64-darwin").is_some());
    }

    #[test]
    fn the_resolved_view_prefers_the_source_row_over_a_precompiled_one() {
        let both = "GEM\n  specs:\n    nokogiri (1.16.0)\n    nokogiri (1.16.0-arm64-darwin)\n";
        let flipped = "GEM\n  specs:\n    nokogiri (1.16.0-arm64-darwin)\n    nokogiri (1.16.0)\n";
        for text in [both, flipped] {
            let gems = Lockfile::parse(text).unwrap().resolved();
            assert_eq!(gems.len(), 1);
            assert_eq!(gems[0].version, "1.16.0");
            assert_eq!(gems[0].platform, None);
        }
    }

    #[test]
    fn the_resolved_view_is_name_sorted_and_carries_the_edges() {
        let gems = small().resolved();
        let names: Vec<&str> = gems.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["addressable", "ast", "nokogiri", "racc"]);
        assert_eq!(gems[0].deps, ["public_suffix"]);
        assert_eq!(gems[2].platform.as_deref(), Some("arm64-darwin"));
    }

    #[test]
    fn spec_for_takes_the_row_built_for_this_machine() {
        let lock = Lockfile::parse(
            "GEM\n  specs:\n    nokogiri (1.16.0)\n    nokogiri (1.16.0-arm64-darwin)\n    nokogiri (1.16.0-x86_64-linux)\n",
        )
        .unwrap();
        let arm_mac = Platform::parse("arm64-darwin-23");
        assert_eq!(
            lock.spec_for("nokogiri", &arm_mac)
                .unwrap()
                .platform
                .to_string(),
            "arm64-darwin"
        );
        // No row for this host: the source row is what remains.
        let solaris = Platform::parse("sparc-solaris-2.11");
        assert!(
            lock.spec_for("nokogiri", &solaris)
                .unwrap()
                .platform
                .is_ruby()
        );
        assert!(lock.spec_for("absent", &arm_mac).is_none());
    }

    #[test]
    fn git_and_path_sources_keep_their_metadata_and_are_tagged() {
        let text = "\
PATH
  remote: .
  specs:
    sow (0.1.0)

GIT
  remote: https://github.com/x/y.git
  revision: abc123
  branch: main
  specs:
    cuprite (0.17)

GEM
  remote: https://rubygems.org/
  specs:
    ast (2.4.3)
";
        let lock = Lockfile::parse(text).unwrap();
        assert_eq!(lock.to_string(), text);
        let by = |n: &str| {
            lock.resolved()
                .into_iter()
                .find(|g| g.name == n)
                .unwrap()
                .source
        };
        assert_eq!(by("sow"), GemSource::Path);
        assert_eq!(by("cuprite"), GemSource::Git);
        assert_eq!(by("ast"), GemSource::Rubygems);
        let git = lock
            .sources
            .iter()
            .find(|s| s.kind == SectionKind::Git)
            .unwrap();
        assert_eq!(git.revision(), Some("abc123"));
        assert_eq!(git.get("branch"), Some("main"));
        assert_eq!(git.get("tag"), None);
    }

    #[test]
    fn several_gem_remotes_are_all_kept() {
        let text = "GEM\n  remote: https://a.example/\n  remote: https://b.example/\n  specs:\n    ast (2.4.3)\n";
        let lock = Lockfile::parse(text).unwrap();
        assert_eq!(
            lock.sources[0].remotes(),
            ["https://a.example/", "https://b.example/"]
        );
        assert_eq!(lock.to_string(), text);
    }

    #[test]
    fn an_unknown_section_header_is_a_loud_error() {
        let err = Lockfile::parse("MYSTERY\n  stuff\n").unwrap_err();
        assert!(err.message().contains("unknown lockfile section"), "{err}");
    }

    #[test]
    fn malformed_rows_are_named_not_skipped() {
        for (text, want) in [
            ("GEM\n  specs:\n    broken\n", "name (version)"),
            ("GEM\n  specs:\n    ast (2.4.3\n", "name (version)"),
            ("CHECKSUMS\n  ast (2.4.3) sha256=zz\n", "hexadecimal"),
            ("CHECKSUMS\n  ast (2.4.3) nonsense\n", "algorithm=hex"),
            ("GEM\n  no-colon-here\n", "key: value"),
            ("  orphan\n", "before any section"),
        ] {
            let err = Lockfile::parse(text).unwrap_err();
            assert!(err.message().contains(want), "{text:?}: got {err}");
        }
    }

    #[test]
    fn a_version_with_a_dash_is_not_mistaken_for_a_platform() {
        let (version, platform) = split_version_platform("1.0.0-1");
        assert_eq!(version, "1.0.0-1");
        assert_eq!(platform, None);
        let (version, platform) = split_version_platform("1.16.0-arm64-darwin");
        assert_eq!(version, "1.16.0");
        assert_eq!(platform.as_deref(), Some("arm64-darwin"));
    }

    /// The repo's own lock, the largest real sample available, and the file
    /// `cargo xtask deps` reads. Round-tripping it byte-exact is the claim
    /// that nothing in a real Bundler file is dropped.
    #[test]
    fn the_repo_lockfile_round_trips_with_every_checksum() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Gemfile.lock");
        let text = std::fs::read_to_string(&path).expect("Gemfile.lock is committed");
        let lock = Lockfile::parse(&text).expect("the repo lock parses");
        assert_eq!(lock.to_string(), text, "the repo lock did not round-trip");
        assert_eq!(lock.checksums.len(), lock.sources[0].specs.len());
        assert!(
            lock.checksums
                .iter()
                .all(|c| c.sha256().is_some_and(|b| b.len() == 32))
        );
        assert_eq!(lock.gem_remote(), Some("https://rubygems.org/"));
        assert!(lock.bundled_with.is_some());
    }
}
