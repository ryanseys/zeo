//! A `.gem` file: the format `gem install` reads.
//!
//! Since 2007 a `.gem` has been an UNCOMPRESSED tar holding compressed
//! members:
//!
//! ```text
//! metadata.gz        the gemspec, as YAML with Ruby object tags
//! data.tar.gz        the gem's own files, rooted at the gem directory
//! checksums.yaml.gz  digests of the two above
//! ```
//!
//! Signature members (`*.sig`) may sit beside them; this reader ignores them,
//! because nothing here claims to check a signature. The older format -- a
//! single YAML file with the data inlined and Base64-encoded -- is refused
//! rather than half-read.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::error::{Error, Result, read};
use crate::gemspec::{Dependency, DependencyKind, Gemspec};
use crate::version::Requirement;

/// An opened `.gem`. The members are held in memory: the largest gem in a
/// normal lock is a few megabytes, and an installer reads every byte anyway.
pub struct Package {
    pub spec: Gemspec,
    /// The `data.tar.gz` member, still compressed.
    data: Vec<u8>,
    /// sha256 of the whole file -- what a lockfile's `CHECKSUMS` row states.
    sha256: [u8; 32],
}

impl Package {
    pub fn open(path: &Path) -> Result<Package> {
        let bytes = read(path)?;
        Package::from_bytes(&bytes).map_err(|e| Error::format(format!("{}: {e}", path.display())))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Package> {
        if !is_tar(bytes) {
            return Err(Error::format(
                "not a `.gem`: no tar header. A gem written before 2007 is a \
                 single YAML file, which this reader does not accept",
            ));
        }
        let mut metadata = None;
        let mut data = None;
        let mut checksums = None;
        let mut archive = tar::Archive::new(bytes);
        for entry in archive
            .entries()
            .map_err(|e| Error::format(format!("reading the gem's tar: {e}")))?
        {
            let mut entry =
                entry.map_err(|e| Error::format(format!("reading the gem's tar: {e}")))?;
            let name = entry
                .path()
                .map_err(|e| Error::format(format!("a tar entry with no name: {e}")))?
                .to_string_lossy()
                .into_owned();
            let mut member = Vec::new();
            entry
                .read_to_end(&mut member)
                .map_err(|e| Error::format(format!("reading `{name}`: {e}")))?;
            match name.as_str() {
                "metadata.gz" => metadata = Some(member),
                "data.tar.gz" => data = Some(member),
                "checksums.yaml.gz" => checksums = Some(member),
                _ => {}
            }
        }
        let metadata = metadata.ok_or_else(|| Error::format("the gem has no `metadata.gz`"))?;
        let data = data.ok_or_else(|| Error::format("the gem has no `data.tar.gz`"))?;
        if let Some(checksums) = checksums {
            verify_members(&gunzip(&checksums, "checksums.yaml.gz")?, &metadata, &data)?;
        }
        Ok(Package {
            spec: parse_metadata(&gunzip(&metadata, "metadata.gz")?)?,
            data,
            sha256: Sha256::digest(bytes).into(),
        })
    }
}

/// The members are megabytes of compressed bytes; a report says how big they
/// are, never what they hold.
impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Package")
            .field("full_name", &self.full_name())
            .field("data_bytes", &self.data.len())
            .field("sha256", &self.sha256_hex())
            .finish()
    }
}

impl Package {
    /// sha256 of the whole file, the digest a lockfile's `CHECKSUMS` row
    /// states.
    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    pub fn sha256_hex(&self) -> String {
        self.sha256.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// `nokogiri-1.19.4-arm64-darwin`, the name its files go under.
    pub fn full_name(&self) -> String {
        self.spec.full_name()
    }

    /// Unpack `data.tar.gz` into `dir`, creating it.
    ///
    /// A member whose path escapes `dir` is refused: a `.gem` is downloaded
    /// from a server, and an entry named `../../etc/x` must not land there.
    pub fn unpack_data_to(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&self.data[..]));
        archive.set_overwrite(true);
        for entry in archive
            .entries()
            .map_err(|e| Error::format(format!("reading `data.tar.gz`: {e}")))?
        {
            let mut entry =
                entry.map_err(|e| Error::format(format!("reading `data.tar.gz`: {e}")))?;
            // `unpack_in` refuses an absolute path and any `..` that would
            // leave the directory, and answers false when it skipped one.
            let unpacked = entry
                .unpack_in(dir)
                .map_err(|e| Error::format(format!("unpacking into {}: {e}", dir.display())))?;
            if !unpacked {
                let name = entry
                    .path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                return Err(Error::format(format!(
                    "the gem holds `{name}`, which points outside the gem's own directory"
                )));
            }
        }
        Ok(())
    }

    /// The paths inside `data.tar.gz`, without unpacking anything.
    pub fn data_paths(&self) -> Result<Vec<PathBuf>> {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&self.data[..]));
        archive
            .entries()
            .map_err(|e| Error::format(format!("reading `data.tar.gz`: {e}")))?
            .map(|entry| {
                let entry =
                    entry.map_err(|e| Error::format(format!("reading `data.tar.gz`: {e}")))?;
                let path = entry
                    .path()
                    .map_err(|e| Error::format(format!("a tar entry with no name: {e}")))?;
                Ok(path.into_owned())
            })
            .collect()
    }
}

/// The tar magic sits at offset 257. Its absence is what tells the pre-2007
/// format apart from this one.
fn is_tar(bytes: &[u8]) -> bool {
    bytes.len() > 262 && &bytes[257..262] == b"ustar"
}

fn gunzip(bytes: &[u8], what: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|e| Error::format(format!("decompressing `{what}`: {e}")))?;
    Ok(out)
}

/// Hold `metadata.gz` and `data.tar.gz` to the digests `checksums.yaml.gz`
/// states. A gem whose own members do not match it is corrupt, whatever the
/// outer file hashes to.
fn verify_members(checksums: &[u8], metadata: &[u8], data: &[u8]) -> Result<()> {
    let text = String::from_utf8_lossy(checksums);
    let docs = yaml_rust2::YamlLoader::load_from_str(&text)
        .map_err(|e| Error::format(format!("`checksums.yaml.gz` is not YAML: {e}")))?;
    let Some(sha256) = docs.first().map(|d| &d["SHA256"]) else {
        return Ok(());
    };
    for (member, bytes) in [("metadata.gz", metadata), ("data.tar.gz", data)] {
        let Some(want) = sha256[member].as_str() else {
            continue;
        };
        let got: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != want {
            return Err(Error::format(format!(
                "`{member}` does not match the gem's own checksum\n  expected: {want}\n  actual:   {got}"
            )));
        }
    }
    Ok(())
}

/// `metadata.gz`: the gemspec as YAML with Ruby object tags.
///
/// Only the fields a loader and an installer read are taken. The tags are
/// ignored -- what matters is the shape under each key, and every serialized
/// spec writes the same one.
fn parse_metadata(bytes: &[u8]) -> Result<Gemspec> {
    let text = String::from_utf8_lossy(bytes);
    let docs = yaml_rust2::YamlLoader::load_from_str(&text)
        .map_err(|e| Error::format(format!("`metadata.gz` is not YAML: {e}")))?;
    let doc = docs
        .first()
        .ok_or_else(|| Error::format("`metadata.gz` is empty"))?;
    let name = doc["name"]
        .as_str()
        .ok_or_else(|| Error::format("`metadata.gz` names no gem"))?
        .to_string();
    let mut spec = Gemspec {
        name,
        version: yaml_version(&doc["version"]),
        require_paths: string_list(&doc["require_paths"]),
        extensions: string_list(&doc["extensions"]),
        platform: yaml_platform(&doc["platform"]),
        dependencies: Vec::new(),
    };
    if spec.require_paths.is_empty() {
        spec.require_paths = vec!["lib".to_string()];
    }
    if let yaml_rust2::Yaml::Array(deps) = &doc["dependencies"] {
        for dep in deps {
            if let Some(dep) = yaml_dependency(dep)? {
                spec.dependencies.push(dep);
            }
        }
    }
    Ok(spec)
}

/// `version: !ruby/object:Gem::Version\n  version: 1.2.3`, or a bare scalar.
fn yaml_version(node: &yaml_rust2::Yaml) -> Option<String> {
    scalar(node).or_else(|| scalar(&node["version"]))
}

/// `platform: ruby`, or the object form with `cpu`/`os`/`version`.
fn yaml_platform(node: &yaml_rust2::Yaml) -> Option<String> {
    if let Some(text) = scalar(node) {
        return Some(text);
    }
    let os = scalar(&node["os"])?;
    let mut out = String::new();
    if let Some(cpu) = scalar(&node["cpu"]) {
        out.push_str(&cpu);
        out.push('-');
    }
    out.push_str(&os);
    if let Some(version) = scalar(&node["version"]) {
        out.push('-');
        out.push_str(&version);
    }
    Some(out)
}

fn yaml_dependency(node: &yaml_rust2::Yaml) -> Result<Option<Dependency>> {
    let Some(name) = scalar(&node["name"]) else {
        return Ok(None);
    };
    // `type: :runtime` -- a Ruby symbol, which YAML reads as the text `:runtime`.
    let kind = match scalar(&node["type"]).as_deref() {
        Some(":development") | Some("development") => DependencyKind::Development,
        _ => DependencyKind::Runtime,
    };
    let mut clauses = Vec::new();
    // `requirement: { requirements: [[">=", !ruby/object:Gem::Version {version: 0}]] }`
    if let yaml_rust2::Yaml::Array(rows) = &node["requirement"]["requirements"] {
        for row in rows {
            let (Some(op), Some(version)) = (scalar(&row[0]), yaml_version(&row[1])) else {
                continue;
            };
            clauses.extend(Requirement::parse(&format!("{op} {version}"))?.clauses);
        }
    }
    Ok(Some(Dependency {
        name,
        requirement: Requirement { clauses },
        kind,
    }))
}

/// A YAML scalar as text. A version is often written unquoted (`0`), so a
/// number has to read back as its digits.
fn scalar(node: &yaml_rust2::Yaml) -> Option<String> {
    match node {
        yaml_rust2::Yaml::String(s) => Some(s.clone()),
        yaml_rust2::Yaml::Integer(i) => Some(i.to_string()),
        yaml_rust2::Yaml::Real(r) => Some(r.clone()),
        yaml_rust2::Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

fn string_list(node: &yaml_rust2::Yaml) -> Vec<String> {
    match node {
        yaml_rust2::Yaml::Array(items) => items.iter().filter_map(scalar).collect(),
        other => scalar(other).into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{gem_bytes, metadata_yaml};

    fn rake_gem() -> Vec<u8> {
        gem_bytes(
            &metadata_yaml("rake", "13.4.2", "ruby"),
            &[
                ("lib/rake.rb", "module Rake; end\n"),
                ("lib/rake/task.rb", "class Rake::Task; end\n"),
                ("README.md", "# rake\n"),
            ],
        )
    }

    #[test]
    fn the_metadata_becomes_a_gemspec() {
        let package = Package::from_bytes(&rake_gem()).unwrap();
        assert_eq!(package.spec.name, "rake");
        assert_eq!(package.spec.version.as_deref(), Some("13.4.2"));
        assert_eq!(package.spec.platform.as_deref(), Some("ruby"));
        assert_eq!(package.spec.require_paths, ["lib"]);
        assert!(package.spec.extensions.is_empty());
        assert_eq!(package.full_name(), "rake-13.4.2");
        assert!(!package.spec.is_native());
    }

    #[test]
    fn the_metadata_carries_the_dependencies_with_their_kind() {
        let package = Package::from_bytes(&rake_gem()).unwrap();
        let deps = &package.spec.dependencies;
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "racc");
        assert_eq!(deps[0].kind, DependencyKind::Runtime);
        assert_eq!(deps[0].requirement.to_string(), "~> 1.4");
        assert_eq!(deps[1].name, "rspec");
        assert_eq!(deps[1].kind, DependencyKind::Development);
        assert!(deps[1].requirement.is_default());
    }

    #[test]
    fn the_digest_is_of_the_whole_file() {
        let bytes = rake_gem();
        let package = Package::from_bytes(&bytes).unwrap();
        let want: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(package.sha256_hex(), want);
        assert_eq!(package.sha256().len(), 32);
    }

    #[test]
    fn the_data_member_unpacks_with_its_directory_structure() {
        let dir = tempdir("unpack");
        let package = Package::from_bytes(&rake_gem()).unwrap();
        package.unpack_data_to(&dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("lib/rake.rb")).unwrap(),
            "module Rake; end\n"
        );
        assert!(dir.join("lib/rake/task.rb").is_file());
        assert!(dir.join("README.md").is_file());
        let mut paths = package.data_paths().unwrap();
        paths.sort();
        assert_eq!(
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
            ["README.md", "lib/rake/task.rb", "lib/rake.rb"]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A `.gem` comes off a server. An entry that climbs out of the gem's own
    /// directory must be refused, not written.
    #[test]
    fn an_entry_that_escapes_the_directory_is_refused() {
        let bytes = crate::testing::gem_with_raw_entry_path(
            &metadata_yaml("evil", "1.0", "ruby"),
            "../escaped.rb",
            "boom\n",
        );
        let dir = tempdir("escape");
        let err = Package::from_bytes(&bytes)
            .unwrap()
            .unpack_data_to(&dir)
            .unwrap_err();
        assert!(err.message().contains("outside"), "{err}");
        assert!(!dir.parent().unwrap().join("escaped.rb").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_platform_gem_keeps_its_platform_in_the_full_name() {
        let bytes = gem_bytes(
            &metadata_yaml("nokogiri", "1.19.4", "arm64-darwin"),
            &[("lib/nokogiri.rb", "")],
        );
        let package = Package::from_bytes(&bytes).unwrap();
        assert_eq!(package.full_name(), "nokogiri-1.19.4-arm64-darwin");
        assert!(package.spec.is_native());
    }

    /// RubyGems also writes the platform as an object rather than a string.
    #[test]
    fn a_platform_written_as_an_object_reads_the_same_way() {
        let metadata = "--- !ruby/object:Gem::Specification\n\
             name: nokogiri\n\
             version: !ruby/object:Gem::Version\n  version: 1.19.4\n\
             platform: !ruby/object:Gem::Platform\n  cpu: arm64\n  os: darwin\n  version: \n\
             require_paths:\n- lib\n";
        let package = Package::from_bytes(&gem_bytes(metadata, &[("lib/x.rb", "")])).unwrap();
        assert_eq!(package.spec.platform.as_deref(), Some("arm64-darwin"));
    }

    #[test]
    fn a_gem_whose_own_checksums_disagree_is_refused() {
        let mut bytes = rake_gem();
        // Corrupt a byte inside the `data.tar.gz` member. The outer tar still
        // reads; the gem's own checksum does not match.
        let at = bytes.len() / 2;
        bytes[at] ^= 0xff;
        let err = Package::from_bytes(&bytes).unwrap_err();
        assert!(
            err.message().contains("checksum") || err.message().contains("decompress"),
            "{err}"
        );
    }

    #[test]
    fn the_pre_2007_format_is_refused_by_name() {
        let err =
            Package::from_bytes(b"--- !ruby/object:Gem::Specification\nname: old\n").unwrap_err();
        assert!(err.message().contains("before 2007"), "{err}");
    }

    #[test]
    fn a_tar_missing_a_member_says_which_one() {
        let metadata = crate::testing::gzip(metadata_yaml("x", "1.0", "ruby").as_bytes());
        let only_metadata = crate::testing::tar_of_bytes(&[("metadata.gz", &metadata)]);
        let err = Package::from_bytes(&only_metadata).unwrap_err();
        assert!(err.message().contains("data.tar.gz"), "{err}");

        let only_data = crate::testing::tar_of_bytes(&[("data.tar.gz", &metadata)]);
        let err = Package::from_bytes(&only_data).unwrap_err();
        assert!(err.message().contains("metadata.gz"), "{err}");
    }

    /// A scratch directory under the crate's target dir -- no `$TMPDIR`, so a
    /// killed run leaves nothing on the developer's machine outside `target/`.
    fn tempdir(name: &str) -> PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/zeo-gem-tests")
            .join(name);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("creating the scratch dir");
        dir
    }
}
