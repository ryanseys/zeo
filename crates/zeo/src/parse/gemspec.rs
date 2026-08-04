//! Gemspec parsing -- the manifest format, replacing the invented `spin.toml`.
//!
//! A gemspec is Ruby source, but the one zeo needs to read is not
//! hand-written: RubyGems SERIALIZES installed specs through
//! `Gem::Specification#to_ruby` (`specification.rb:2370`), whose every value
//! goes through a private `ruby_code` helper (`:2239`) ending in
//! `else raise Gem::Exception`. Arbitrary Ruby therefore **cannot** enter a
//! serialized gemspec by construction, and the grammar is closed and tiny.
//!
//! Measured against the 271 gemspecs installed on the development machine:
//! zero lines fall outside `{comment, blank, Gem::Specification.new do |s|,
//! end, s.<attr> = <expr>, s.add_*_dependency(...)}`, and all 741 dependency
//! lines match one exact form. So a static parse is safe, and no eval VM is
//! needed. `every_installed_gemspec_parses` below re-checks that claim against
//! the real store on every test run rather than trusting this comment.
//!
//! # Two tiers
//!
//! 1. **The `# stub:` header.** `to_ruby` emits `# stub: <name> <version>
//!    <platform> <require_paths NUL-joined>`, plus a second `# stub:` line
//!    listing extensions when the gem has any. This is exactly why
//!    `Gem::StubSpecification` exists: everything zeo consumes is available
//!    without parsing the body at all.
//! 2. **A static prism parse of the body**, for gemspecs with no stub header --
//!    including the ones zeo itself ships under `gems/`, which are
//!    hand-written and short.
//!
//! # What is consumed, and what is ignored
//!
//! Only `name`, `version`, `require_paths`, `extensions`, and `platform`.
//! Every other attribute is **skipped without inspection** -- deliberately, and
//! not the same as tolerating malformed input. `required_ruby_version` is a
//! `Gem::Requirement.new(...)` call and `date` is a bare String; erroring on
//! RHS shapes zeo never reads would reject perfectly valid gemspecs for no
//! benefit. A consumed attribute whose RHS is *not* a literal is a loud error
//! naming the attribute, because that one zeo would otherwise get wrong.

use crate::lower::PResult;
use crate::lower_error::LowerError;
use std::path::Path;

/// The gemspec fields that determine load-path resolution. Deliberately the
/// same shape `Gem::StubSpecification` exposes.
#[derive(Debug, Default, PartialEq)]
pub(super) struct GemSpec {
    pub name: String,
    /// `None` for a hand-written manifest that omits it. zeo does not
    /// resolve versions; this is carried for the disclosure report.
    pub version: Option<String>,
    /// Never empty -- defaults to `["lib"]`, RubyGems' own default.
    pub require_paths: Vec<String>,
    /// `extconf.rb`-style paths. Non-empty means the gem declares a native
    /// half to be built at install time.
    pub extensions: Vec<String>,
    /// `"ruby"` for a source gem, `"arm64-darwin"` etc. for a precompiled one.
    ///
    /// Both fields matter for detecting native code, and neither alone is
    /// sufficient: a PRECOMPILED platform gem ships its prebuilt binary inside
    /// `lib/` and declares **no** `s.extensions` at all, so `extensions` being
    /// empty does not mean the gem is pure Ruby.
    pub platform: Option<String>,
}

/// Parse a gemspec file: stub header when present, else a static body parse.
pub(super) fn parse_file(path: &Path) -> PResult<GemSpec> {
    // Read as BYTES. Four installed specs on the development machine carry raw
    // non-UTF-8 in author names despite their own `# -*- encoding: utf-8 -*-`
    // header, so a strict decode would reject valid input.
    let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    if let Some(spec) = parse_stub_header(&text) {
        return Ok(spec);
    }
    parse_body(&text).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// `# stub: <name> <version> <platform> <require_paths NUL-joined>`, with an
/// optional second `# stub:` line carrying the extension paths.
///
/// Returns `None` rather than erroring when absent -- a hand-written gemspec
/// legitimately has no stub, and falls through to the body parse.
fn parse_stub_header(text: &str) -> Option<GemSpec> {
    let mut stubs = text
        .lines()
        // The header sits at the very top, before `Gem::Specification.new`.
        .take_while(|l| !l.starts_with("Gem::Specification"))
        .filter_map(|l| l.strip_prefix("# stub: "));
    let mut fields = stubs.next()?.split(' ');
    let name = fields.next()?.to_string();
    let version = fields.next()?.to_string();
    let platform = fields.next()?.to_string();
    // Multiple require_paths are joined with NUL inside the single field.
    let require_paths: Vec<String> = fields
        .next()?
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect();
    if name.is_empty() || require_paths.is_empty() {
        return None;
    }
    Some(GemSpec {
        name,
        version: Some(version),
        require_paths,
        extensions: stubs
            .next()
            .map(|l| {
                l.split(' ')
                    .filter(|e| !e.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
        platform: Some(platform),
    })
}

/// Static parse of the `Gem::Specification.new do |s| ... end` body.
fn parse_body(text: &str) -> PResult<GemSpec> {
    let result = ruby_prism::parse(text.as_bytes());
    if let Some(err) = result.errors().next() {
        return Err(LowerError::syntax(format!(
            "parse error: {}",
            err.message()
        )));
    }
    let node = result.node();
    let program = node
        .as_program_node()
        .ok_or("not a Ruby program".to_string())?;

    let mut spec = GemSpec::default();
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
        return Err("no `Gem::Specification.new do |s| ... end` block"
            .to_string()
            .into());
    }
    if spec.name.is_empty() {
        return Err("gemspec sets no `name`".to_string().into());
    }
    if spec.require_paths.is_empty() {
        spec.require_paths = vec!["lib".to_string()];
    }
    Ok(spec)
}

/// Walk one statement list, recording the attributes zeo consumes.
///
/// Descends through `if`/`unless` so the two trailing guards `to_ruby` emits
/// (`... if s.respond_to? :required_rubygems_version=`, `... :metadata=`) are
/// transparent -- their condition is always true against a modern RubyGems,
/// and treating the assignment as unconditional is what `Gem::StubSpecification`
/// effectively does too.
fn visit_statements(node: &ruby_prism::Node<'_>, spec: &mut GemSpec) -> PResult<()> {
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
        let Some(attr) = method.strip_suffix('=') else {
            // `s.add_runtime_dependency(...)` and friends -- zeo resolves no
            // versions, so dependency edges are not consumed here.
            continue;
        };
        let args: Vec<_> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
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
            // Every other attribute is skipped WITHOUT inspecting its RHS --
            // see the module docs on why erroring here would be wrong.
            _ => {}
        }
    }
    Ok(())
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

fn want_string(node: &ruby_prism::Node<'_>, attr: &str) -> PResult<String> {
    string_literal(node).ok_or_else(|| {
        format!("`s.{attr}` must be a string literal (zeo parses gemspecs statically, without evaluating them)").into()
    })
}

/// A string literal, or an array of them. `s.require_path = "lib"` (singular,
/// no array) is legal in hand-written gemspecs and appears in CRuby's own tree.
fn want_string_array(node: &ruby_prism::Node<'_>, attr: &str) -> PResult<Vec<String>> {
    if let Some(one) = string_literal(node) {
        return Ok(vec![one]);
    }
    let array = node.as_array_node().ok_or_else(|| {
        format!("`s.{attr}` must be a string or an array of string literals (zeo parses gemspecs statically, without evaluating them)")
    })?;
    array
        .elements()
        .iter()
        .map(|e| {
            string_literal(&e).ok_or_else(|| {
                crate::lower_error::LowerError::unsupported(format!("`s.{attr}` must contain only string literals (zeo parses gemspecs statically, without evaluating them)"))
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
    }

    #[test]
    fn a_second_stub_line_carries_extensions() {
        let spec = parse_stub_header(
            "# stub: psych 5.4.0 ruby lib\n# stub: ext/psych/extconf.rb\nGem::Specification.new do |s|\n",
        )
        .unwrap();
        assert_eq!(spec.extensions, ["ext/psych/extconf.rb"]);
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
    /// binary inside `lib/` -- so `platform` is the only signal that it carries
    /// native code. Both fields must survive the parse for that to be
    /// detectable later.
    #[test]
    fn a_precompiled_platform_gem_records_its_platform_and_no_extensions() {
        let spec = parse_stub_header(
            "# stub: nokogiri 1.19.4 arm64-darwin lib\nGem::Specification.new do |s|\n",
        )
        .unwrap();
        assert!(spec.extensions.is_empty());
        assert_eq!(spec.platform.as_deref(), Some("arm64-darwin"));
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
    }

    /// The `if s.respond_to?` guards must not hide the assignment they trail.
    #[test]
    fn respond_to_guarded_assignments_are_transparent() {
        let spec = parse_body(
            "Gem::Specification.new do |s|\n  s.name = \"x\".freeze\n  s.require_paths = [\"other\".freeze] if s.respond_to? :require_paths=\nend\n",
        )
        .unwrap();
        assert_eq!(spec.require_paths, ["other"]);
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

    /// A non-literal RHS on a CONSUMED attribute is loud and names the
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
        .expect("an ignored attribute's RHS is never inspected");
    }

    /// The claim this whole design rests on: every gemspec RubyGems actually
    /// writes is statically parseable. Enforced against the real store rather
    /// than trusted -- if a future RubyGems emits a shape the parser can't
    /// read, this fails instead of the failure surfacing as a mystery
    /// `cannot load such file` months later.
    ///
    /// Skips cleanly when there is no Ruby installation, so CI without one
    /// still passes. Parses the BODY of every spec too, not just the stub
    /// header, since the body path is the one hand-written gemspecs take and
    /// the installed corpus is the only large sample of it available.
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
                let spec = parse_file(&path).expect("parse_file");
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
    }
}
