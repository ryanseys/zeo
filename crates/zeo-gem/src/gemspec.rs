//! Gemspec reading, without evaluating Ruby.
//!
//! A gemspec is Ruby source, but the one a store holds is not hand-written:
//! RubyGems SERIALIZES installed specs through `Gem::Specification#to_ruby`
//! (`specification.rb:2370`), whose every value goes through a private
//! `ruby_code` helper (`:2239`) ending in `else raise Gem::Exception`.
//! Arbitrary Ruby therefore **cannot** enter a serialized gemspec by
//! construction, and the grammar is closed and tiny.
//!
//! Measured against the 271 gemspecs installed on the development machine:
//! zero lines fall outside `{comment, blank, Gem::Specification.new do |s|,
//! end, s.<attr> = <expr>, s.add_*_dependency(...)}`, and all 741 dependency
//! lines match one exact form. So a static parse is safe, and no evaluator is
//! needed. `every_installed_gemspec_parses` below re-checks that claim against
//! the real store on every test run rather than trusting this comment.
//!
//! # Two tiers
//!
//! 1. **The `# stub:` header.** `to_ruby` emits `# stub: <name> <version>
//!    <platform> <require_paths NUL-joined>`, plus a second `# stub:` line
//!    listing extensions when the gem has any. This is exactly why
//!    `Gem::StubSpecification` exists: most of what a loader needs is
//!    available without parsing the body at all.
//! 2. **A static prism parse of the body**, for gemspecs with no stub header
//!    -- including the short hand-written stubs a store may hold.
//!
//! # What is consumed, and what is ignored
//!
//! Only `name`, `version`, `require_paths`, `extensions`, `platform` and the
//! dependency calls. Every other attribute is **skipped without inspection**
//! -- deliberately, and not the same as tolerating malformed input.
//! `required_ruby_version` is a `Gem::Requirement.new(...)` call and `date` is
//! a bare String; erroring on shapes nothing reads would reject perfectly
//! valid gemspecs for no benefit. A consumed attribute whose value is *not* a
//! literal is a loud error naming the attribute, because that one would
//! otherwise be read wrong.

use std::fmt::Write as _;
use std::path::Path;

use crate::error::{Error, Result, read};
use crate::platform::Platform;
use crate::version::Requirement;

/// Whether a dependency is needed to USE the gem or only to develop it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Runtime,
    Development,
}

impl DependencyKind {
    fn method(self) -> &'static str {
        match self {
            DependencyKind::Runtime => "add_runtime_dependency",
            DependencyKind::Development => "add_development_dependency",
        }
    }
}

/// One `s.add_*_dependency` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub requirement: Requirement,
    pub kind: DependencyKind,
}

/// The gemspec fields a loader and an installer need. Deliberately close to
/// the shape `Gem::StubSpecification` exposes.
#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct Gemspec {
    pub name: String,
    /// `None` for a hand-written manifest that omits it.
    pub version: Option<String>,
    /// Never empty after a parse -- defaults to `["lib"]`, RubyGems' own
    /// default.
    pub require_paths: Vec<String>,
    /// `extconf.rb`-style paths. Non-empty means the gem declares a native
    /// half to be built at install time.
    pub extensions: Vec<String>,
    /// `"ruby"` for a source gem, `"arm64-darwin"` for a precompiled one.
    ///
    /// Both this and `extensions` matter for detecting native code, and
    /// neither alone is enough: a PRECOMPILED platform gem ships its prebuilt
    /// binary inside `lib/` and declares **no** `s.extensions` at all.
    pub platform: Option<String>,
    /// The `add_*_dependency` calls, in file order. A stub header carries
    /// none, so this is empty for a spec read from one.
    pub dependencies: Vec<Dependency>,
}

impl Gemspec {
    /// Parse a gemspec file: stub header when present, else a static body
    /// parse.
    pub fn parse_file(path: &Path) -> Result<Gemspec> {
        // Read as BYTES. Four installed specs on the development machine carry
        // raw non-UTF-8 in author names despite their own
        // `# -*- encoding: utf-8 -*-` header, so a strict decode would reject
        // valid input.
        let bytes = read(path)?;
        let text = String::from_utf8_lossy(&bytes);
        if let Some(spec) = parse_stub_header(&text) {
            return Ok(spec);
        }
        Gemspec::parse(&text).map_err(|e| Error::format(format!("{}: {e}", path.display())))
    }

    /// Parse gemspec source. The stub header wins when there is one.
    pub fn parse(text: &str) -> Result<Gemspec> {
        if let Some(spec) = parse_stub_header(text) {
            return Ok(spec);
        }
        parse_body(text)
    }

    /// The `add_*_dependency` calls, which a `# stub:` line cannot carry.
    ///
    /// [`Gemspec::parse_file`] stops at the stub header when there is one --
    /// that is the whole point of a stub, and why RubyGems does not `eval`
    /// every installed spec at boot. A caller that needs the dependency
    /// EDGES has to read the body, so it asks for them separately rather
    /// than making every reader pay for a parse it does not use.
    pub fn dependencies_of_file(path: &Path) -> Result<Vec<Dependency>> {
        let bytes = read(path)?;
        let text = String::from_utf8_lossy(&bytes);
        parse_body(&text)
            .map(|spec| spec.dependencies)
            .map_err(|e| Error::format(format!("{}: {e}", path.display())))
    }

    /// `nokogiri-1.19.4-arm64-darwin`: the name RubyGems gives the gem's own
    /// directory and its `.gemspec` file.
    pub fn full_name(&self) -> String {
        let version = self.version.as_deref().unwrap_or("0");
        match self.platform() {
            Platform::Ruby => format!("{}-{version}", self.name),
            platform => format!("{}-{version}-{platform}", self.name),
        }
    }

    pub fn platform(&self) -> Platform {
        Platform::parse(self.platform.as_deref().unwrap_or("ruby"))
    }

    /// Whether the gem carries native code either way it can: an extension
    /// built at install time, or a prebuilt binary in a platform gem.
    pub fn is_native(&self) -> bool {
        !self.extensions.is_empty() || !self.platform().is_ruby()
    }

    /// The gemspec a store writes for this gem: the `# stub:` header RubyGems
    /// reads without parsing, then a body that says the same thing.
    ///
    /// Only what a loader consumes is written. A gem's description, authors
    /// and file list are not part of resolving a require, and leaving them out
    /// keeps the store's specs small and this writer honest.
    pub fn to_stub_source(&self) -> String {
        let version = self.version.as_deref().unwrap_or("0");
        let platform = self.platform.as_deref().unwrap_or("ruby");
        let mut out = String::new();
        out.push_str("# -*- encoding: utf-8 -*-\n");
        // RubyGems joins both lists with NUL inside one field.
        let _ = writeln!(
            out,
            "# stub: {} {version} {platform} {}",
            self.name,
            self.require_paths.join("\u{0}")
        );
        if !self.extensions.is_empty() {
            let _ = writeln!(out, "# stub: {}", self.extensions.join("\u{0}"));
        }
        out.push_str("\nGem::Specification.new do |s|\n");
        let _ = writeln!(out, "  s.name = {}", ruby_string(&self.name));
        let _ = writeln!(out, "  s.version = {}", ruby_string(version));
        if platform != "ruby" {
            let _ = writeln!(out, "  s.platform = {}", ruby_string(platform));
        }
        let _ = writeln!(
            out,
            "  s.require_paths = {}",
            ruby_array(&self.require_paths)
        );
        if !self.extensions.is_empty() {
            let _ = writeln!(out, "  s.extensions = {}", ruby_array(&self.extensions));
        }
        out.push_str("  s.specification_version = 4\n");
        for dep in &self.dependencies {
            let reqs: Vec<String> = dep
                .requirement
                .clauses
                .iter()
                .map(|(op, v)| format!("{} {v}", op.as_str()))
                .collect();
            let reqs = if reqs.is_empty() {
                vec![">= 0".to_string()]
            } else {
                reqs
            };
            let _ = writeln!(
                out,
                "  s.{}({}, {})",
                dep.kind.method(),
                ruby_string(&dep.name),
                ruby_array(&reqs)
            );
        }
        out.push_str("end\n");
        out
    }
}

/// A double-quoted Ruby string with the `.freeze` `to_ruby` appends. Only the
/// two characters that can appear in a gem name, version or path are escaped;
/// anything else in those fields would not survive RubyGems either.
fn ruby_string(text: &str) -> String {
    format!("{:?}.freeze", text)
}

fn ruby_array(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|i| ruby_string(i)).collect();
    format!("[{}]", items.join(", "))
}

/// `# stub: <name> <version> <platform> <require_paths NUL-joined>`, with an
/// optional second `# stub:` line carrying the extension paths.
///
/// Returns `None` rather than erroring when absent -- a hand-written gemspec
/// legitimately has no stub, and falls through to the body parse.
fn parse_stub_header(text: &str) -> Option<Gemspec> {
    let mut stubs = text
        .lines()
        // The header sits at the very top, before `Gem::Specification.new`.
        .take_while(|l| !l.starts_with("Gem::Specification"))
        .filter_map(|l| l.strip_prefix("# stub: "));
    let mut fields = stubs.next()?.split(' ');
    let name = fields.next()?.to_string();
    let version = fields.next()?.to_string();
    let platform = fields.next()?.to_string();
    let require_paths: Vec<String> = nul_list(fields.next()?);
    if name.is_empty() || require_paths.is_empty() {
        return None;
    }
    Some(Gemspec {
        name,
        version: Some(version),
        require_paths,
        extensions: stubs.next().map(nul_list).unwrap_or_default(),
        platform: Some(platform),
        dependencies: Vec::new(),
    })
}

fn nul_list(field: &str) -> Vec<String> {
    field
        .split('\u{0}')
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

/// Static parse of the `Gem::Specification.new do |s| ... end` body.
fn parse_body(text: &str) -> Result<Gemspec> {
    let result = ruby_prism::parse(text.as_bytes());
    if let Some(err) = result.errors().next() {
        return Err(Error::format(format!("parse error: {}", err.message())));
    }
    let node = result.node();
    let program = node
        .as_program_node()
        .ok_or_else(|| Error::format("not a Ruby program"))?;

    let mut spec = Gemspec::default();
    let mut saw_block = false;
    for stmt in program.statements().body().iter() {
        let Some(call) = stmt.as_call_node() else {
            continue;
        };
        // `Gem::Specification.new do |s| ... end` -- the only shape that
        // carries assignments. A file with none is a hard error below.
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else {
            continue;
        };
        saw_block = true;
        if let Some(body) = block.body() {
            visit_statements(&body, &mut spec)?;
        }
    }
    if !saw_block {
        return Err(Error::format(
            "no `Gem::Specification.new do |s| ... end` block",
        ));
    }
    if spec.name.is_empty() {
        return Err(Error::format("gemspec sets no `name`"));
    }
    if spec.require_paths.is_empty() {
        spec.require_paths = vec!["lib".to_string()];
    }
    Ok(spec)
}

/// Walk one statement list, recording what is consumed.
///
/// Descends through `if`/`unless` so the two trailing guards `to_ruby` emits
/// (`... if s.respond_to? :required_rubygems_version=`, `... :metadata=`) are
/// transparent -- their condition is always true against a modern RubyGems,
/// and treating the assignment as unconditional is what
/// `Gem::StubSpecification` effectively does too.
fn visit_statements(node: &ruby_prism::Node<'_>, spec: &mut Gemspec) -> Result<()> {
    let Some(stmts) = node.as_statements_node() else {
        return Ok(());
    };
    for stmt in stmts.body().iter() {
        if let Some(if_node) = stmt.as_if_node() {
            if let Some(body) = if_node.statements() {
                visit_statements(&body.as_node(), spec)?;
            }
            continue;
        }
        if let Some(unless_node) = stmt.as_unless_node() {
            if let Some(body) = unless_node.statements() {
                visit_statements(&body.as_node(), spec)?;
            }
            continue;
        }
        let Some(call) = stmt.as_call_node() else {
            continue;
        };
        let method = String::from_utf8_lossy(call.name().as_slice()).into_owned();
        let args: Vec<_> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        let Some(attr) = method.strip_suffix('=') else {
            if let Some(dep) = dependency_call(&method, &args)? {
                spec.dependencies.push(dep);
            }
            continue;
        };
        let [value] = args.as_slice() else {
            continue;
        };
        match attr {
            "name" => spec.name = want_string(value, attr)?,
            "version" => spec.version = Some(want_string(value, attr)?),
            "platform" => spec.platform = Some(want_string(value, attr)?),
            "require_paths" | "require_path" => {
                spec.require_paths = want_string_array(value, attr)?
            }
            "extensions" => spec.extensions = want_string_array(value, attr)?,
            // Every other attribute is skipped WITHOUT inspecting its value --
            // see the module docs on why erroring here would be wrong.
            _ => {}
        }
    }
    Ok(())
}

/// `s.add_runtime_dependency(%q<rake>.freeze, [">= 0".freeze])` and its two
/// siblings. A call whose arguments are not literals is skipped rather than
/// refused: a hand-written gemspec may compute a constraint, and a dependency
/// list is not what a loader resolves a require with.
fn dependency_call(method: &str, args: &[ruby_prism::Node<'_>]) -> Result<Option<Dependency>> {
    let kind = match method {
        "add_dependency" | "add_runtime_dependency" => DependencyKind::Runtime,
        "add_development_dependency" => DependencyKind::Development,
        _ => return Ok(None),
    };
    let Some(name) = args.first().and_then(string_literal) else {
        return Ok(None);
    };
    let mut clauses = Vec::new();
    for arg in &args[1..] {
        let parts = match string_literal(arg) {
            Some(one) => vec![one],
            None => match arg.as_array_node() {
                Some(array) => array
                    .elements()
                    .iter()
                    .filter_map(|e| string_literal(&e))
                    .collect(),
                None => continue,
            },
        };
        for part in parts {
            clauses.extend(Requirement::parse(&part)?.clauses);
        }
    }
    Ok(Some(Dependency {
        name,
        requirement: Requirement { clauses },
        kind,
    }))
}

/// A string literal, seeing through the `.freeze` `to_ruby` appends to every
/// one of them.
fn string_literal(node: &ruby_prism::Node<'_>) -> Option<String> {
    if let Some(s) = node.as_string_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    let call = node.as_call_node()?;
    if call.name().as_slice() != b"freeze" {
        return None;
    }
    string_literal(&call.receiver()?)
}

fn want_string(node: &ruby_prism::Node<'_>, attr: &str) -> Result<String> {
    string_literal(node).ok_or_else(|| {
        Error::format(format!(
            "`s.{attr}` must be a string literal (gemspecs are parsed statically, without evaluating them)"
        ))
    })
}

/// A string literal, or an array of them. `s.require_path = "lib"` (singular,
/// no array) is legal in hand-written gemspecs and appears in CRuby's own tree.
fn want_string_array(node: &ruby_prism::Node<'_>, attr: &str) -> Result<Vec<String>> {
    if let Some(one) = string_literal(node) {
        return Ok(vec![one]);
    }
    let array = node.as_array_node().ok_or_else(|| {
        Error::format(format!(
            "`s.{attr}` must be a string or an array of string literals (gemspecs are parsed statically, without evaluating them)"
        ))
    })?;
    array
        .elements()
        .iter()
        .map(|e| {
            string_literal(&e).ok_or_else(|| {
                Error::format(format!(
                    "`s.{attr}` must contain only string literals (gemspecs are parsed statically, without evaluating them)"
                ))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_header_supplies_everything_without_parsing_the_body() {
        let spec = parse_stub_header(
            "# -*- encoding: utf-8 -*-\n# stub: optparse 0.8.1 ruby lib\n\nGem::Specification.new do |s|\n",
        )
        .expect("stub present");
        assert_eq!(spec.name, "optparse");
        assert_eq!(spec.version.as_deref(), Some("0.8.1"));
        assert_eq!(spec.require_paths, ["lib"]);
        assert_eq!(spec.platform.as_deref(), Some("ruby"));
        assert!(spec.extensions.is_empty());
        assert_eq!(spec.full_name(), "optparse-0.8.1");
        assert!(!spec.is_native());
    }

    #[test]
    fn a_second_stub_line_carries_extensions_and_they_are_nul_joined() {
        let spec = parse_stub_header(
            "# stub: json 2.21.2 ruby lib\n# stub: ext/json/ext/generator/extconf.rb\u{0}ext/json/ext/parser/extconf.rb\nGem::Specification.new do |s|\n",
        )
        .unwrap();
        assert_eq!(
            spec.extensions,
            [
                "ext/json/ext/generator/extconf.rb",
                "ext/json/ext/parser/extconf.rb"
            ]
        );
        assert!(spec.is_native());
    }

    #[test]
    fn require_paths_are_nul_joined_in_the_stub() {
        let spec = parse_stub_header(
            "# stub: x 1.0 ruby lib\u{0}ext/java/lib\nGem::Specification.new do |s|\n",
        )
        .unwrap();
        assert_eq!(spec.require_paths, ["lib", "ext/java/lib"]);
    }

    /// A precompiled platform gem declares NO extensions -- it ships the built
    /// binary inside `lib/` -- so `platform` is the only signal that it
    /// carries native code.
    #[test]
    fn a_precompiled_platform_gem_records_its_platform_and_no_extensions() {
        let spec = parse_stub_header(
            "# stub: nokogiri 1.19.4 arm64-darwin lib\nGem::Specification.new do |s|\n",
        )
        .unwrap();
        assert!(spec.extensions.is_empty());
        assert_eq!(spec.platform.as_deref(), Some("arm64-darwin"));
        assert!(spec.is_native());
        assert_eq!(spec.full_name(), "nokogiri-1.19.4-arm64-darwin");
    }

    /// A stub line with fewer than four fields is not a stub; the body wins.
    #[test]
    fn a_truncated_stub_line_falls_through_to_the_body() {
        assert!(
            parse_stub_header("# stub: optparse 0.8.1\nGem::Specification.new do |s|\n").is_none()
        );
        assert!(parse_stub_header("Gem::Specification.new do |s|\n").is_none());
        // A `# stub:` line BELOW the block is not a header.
        assert!(
            parse_stub_header("Gem::Specification.new do |s|\n# stub: x 1 ruby lib\n").is_none()
        );
    }

    #[test]
    fn body_parse_handles_the_serialized_form() {
        let spec = parse_body(
            r#"
Gem::Specification.new do |s|
  s.name = "optparse".freeze
  s.version = "0.8.1".freeze
  s.required_rubygems_version = Gem::Requirement.new(">= 0".freeze) if s.respond_to? :required_rubygems_version=
  s.metadata = { "homepage_uri" => "https://example.com" } if s.respond_to? :metadata=
  s.require_paths = ["lib".freeze]
  s.date = "2026-05-20"
  s.specification_version = 4
  s.add_runtime_dependency(%q<stringio>.freeze, [">= 0".freeze])
end
"#,
        )
        .unwrap();
        assert_eq!(spec.name, "optparse");
        assert_eq!(spec.version.as_deref(), Some("0.8.1"));
        assert_eq!(spec.require_paths, ["lib"]);
        assert_eq!(spec.dependencies.len(), 1);
        assert_eq!(spec.dependencies[0].name, "stringio");
        assert_eq!(spec.dependencies[0].kind, DependencyKind::Runtime);
        assert!(spec.dependencies[0].requirement.is_default());
    }

    #[test]
    fn every_dependency_spelling_is_collected_with_its_requirement() {
        let spec = parse_body(
            r#"
Gem::Specification.new do |s|
  s.name = "x"
  s.add_dependency("rake", ">= 13.0")
  s.add_runtime_dependency(%q<json>.freeze, ["~> 2.0".freeze, "< 3".freeze])
  s.add_development_dependency("rspec".freeze, [">= 0".freeze])
  s.add_runtime_dependency(some_name, [">= 0"])
end
"#,
        )
        .unwrap();
        let names: Vec<&str> = spec.dependencies.iter().map(|d| d.name.as_str()).collect();
        // The computed name is skipped, not guessed at.
        assert_eq!(names, ["rake", "json", "rspec"]);
        assert_eq!(spec.dependencies[0].requirement.to_string(), ">= 13.0");
        assert_eq!(spec.dependencies[1].requirement.to_string(), "~> 2.0, < 3");
        assert_eq!(spec.dependencies[2].kind, DependencyKind::Development);
    }

    /// The `if s.respond_to?` guards must not hide the assignment they trail.
    #[test]
    fn respond_to_guarded_assignments_are_transparent() {
        let spec = parse_body(
            "Gem::Specification.new do |s|\n  s.name = \"x\".freeze\n  s.require_paths = [\"other\".freeze] if s.respond_to? :require_paths=\nend\n",
        )
        .unwrap();
        assert_eq!(spec.require_paths, ["other"]);

        let spec = parse_body(
            "Gem::Specification.new do |s|\n  s.name = \"x\"\n  s.extensions = [\"e/extconf.rb\"] unless s.respond_to? :nothing\nend\n",
        )
        .unwrap();
        assert_eq!(spec.extensions, ["e/extconf.rb"]);
    }

    #[test]
    fn require_paths_defaults_to_lib_and_singular_form_is_accepted() {
        let spec = parse_body("Gem::Specification.new do |s|\n  s.name = \"x\"\nend\n").unwrap();
        assert_eq!(spec.require_paths, ["lib"]);

        let spec = parse_body(
            "Gem::Specification.new do |s|\n  s.name = \"x\"\n  s.require_path = \"flat\"\nend\n",
        )
        .unwrap();
        assert_eq!(spec.require_paths, ["flat"]);
    }

    /// A non-literal value on a CONSUMED attribute is loud and names the
    /// attribute; the same shape on an ignored attribute is silently skipped.
    #[test]
    fn a_non_literal_consumed_attribute_is_a_named_error() {
        let err = parse_body(
            "Gem::Specification.new do |s|\n  s.name = \"x\"\n  s.require_paths = Dir[\"lib\"]\nend\n",
        )
        .unwrap_err();
        assert!(
            err.message().contains("s.require_paths"),
            "unexpected: {err}"
        );

        parse_body(
            "Gem::Specification.new do |s|\n  s.name = \"x\"\n  s.files = Dir[\"**/*\"]\nend\n",
        )
        .expect("an ignored attribute's value is never inspected");
    }

    #[test]
    fn a_gemspec_without_the_block_or_a_name_is_rejected() {
        assert!(
            parse_body("1 + 1\n")
                .unwrap_err()
                .message()
                .contains("Gem::Specification.new")
        );
        assert!(
            parse_body("Gem::Specification.new do |s|\n  s.version = \"1\"\nend\n")
                .unwrap_err()
                .message()
                .contains("name")
        );
        assert!(
            Gemspec::parse("Gem::Specification.new do |s|\n  s.name = \n")
                .unwrap_err()
                .message()
                .contains("parse error")
        );
    }

    /// The store writes a stub, and this crate must be able to read back what
    /// it wrote -- through the stub header AND through the body.
    #[test]
    fn a_written_stub_reads_back_the_same_way_twice() {
        let spec = Gemspec {
            name: "nokogiri".into(),
            version: Some("1.19.4".into()),
            require_paths: vec!["lib".into(), "ext/java/lib".into()],
            extensions: vec!["ext/nokogiri/extconf.rb".into()],
            platform: Some("arm64-darwin".into()),
            dependencies: vec![
                Dependency {
                    name: "racc".into(),
                    requirement: Requirement::parse("~> 1.4").unwrap(),
                    kind: DependencyKind::Runtime,
                },
                Dependency {
                    // An empty requirement is written as `>= 0`, RubyGems'
                    // own default, and reads back as that.
                    name: "rspec".into(),
                    requirement: Requirement::parse(">= 0").unwrap(),
                    kind: DependencyKind::Development,
                },
            ],
        };
        let source = spec.to_stub_source();

        let from_stub = parse_stub_header(&source).expect("the written header is a stub");
        assert_eq!(from_stub.name, "nokogiri");
        assert_eq!(from_stub.version.as_deref(), Some("1.19.4"));
        assert_eq!(from_stub.platform.as_deref(), Some("arm64-darwin"));
        assert_eq!(from_stub.require_paths, ["lib", "ext/java/lib"]);
        assert_eq!(from_stub.extensions, ["ext/nokogiri/extconf.rb"]);

        let from_body = parse_body(&source).expect("the written body parses");
        assert_eq!(
            Gemspec {
                dependencies: spec.dependencies.clone(),
                ..from_body.clone()
            },
            spec
        );
        assert_eq!(from_body.dependencies, spec.dependencies);
    }

    #[test]
    fn a_source_gem_writes_no_platform_line() {
        let spec = Gemspec {
            name: "rake".into(),
            version: Some("13.4.2".into()),
            require_paths: vec!["lib".into()],
            ..Gemspec::default()
        };
        let source = spec.to_stub_source();
        assert!(source.contains("# stub: rake 13.4.2 ruby lib\n"));
        assert!(!source.contains("s.platform"));
        assert!(!source.contains("s.extensions"));
        assert_eq!(parse_body(&source).unwrap().platform, None);
    }

    /// The claim this whole design rests on: every gemspec RubyGems actually
    /// writes is statically parseable. Enforced against the real store rather
    /// than trusted -- if a future RubyGems emits a shape the parser cannot
    /// read, this fails instead of the failure surfacing as a mystery
    /// `cannot load such file` months later.
    ///
    /// Skips cleanly when there is no Ruby installation, so a machine without
    /// one still passes.
    #[test]
    fn every_installed_gemspec_parses() {
        let Ok(out) = std::process::Command::new("gem")
            .args(["env", "gemdir"])
            .output()
        else {
            return; // no Ruby on this machine
        };
        if !out.status.success() {
            return;
        }
        let gemdir = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        let dirs = [
            gemdir.join("specifications"),
            gemdir.join("specifications/default"),
        ];
        let mut checked = 0usize;
        let mut failures = Vec::new();
        for dir in &dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
                if path.extension().is_none_or(|x| x != "gemspec") {
                    continue;
                }
                // Skip unreadable entries (CI runners ship broken symlinks in
                // the system Ruby's specifications dir).
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                checked += 1;
                // The stub header would satisfy `parse_file`, so exercise the
                // body parser explicitly -- it is the weaker of the two.
                let text = String::from_utf8_lossy(&bytes);
                if let Err(e) = parse_body(&text) {
                    failures.push(format!("{}: {e}", path.display()));
                }
                let spec = Gemspec::parse_file(&path).expect("parse_file");
                assert!(!spec.name.is_empty(), "{} has no name", path.display());
                assert!(!spec.require_paths.is_empty(), "{}", path.display());
            }
        }
        if checked == 0 {
            return; // Ruby present but no specs installed
        }
        eprintln!("gemspec corpus: parsed {checked} installed specs");
        assert!(
            failures.is_empty(),
            "{}/{checked} installed gemspecs failed to parse:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
