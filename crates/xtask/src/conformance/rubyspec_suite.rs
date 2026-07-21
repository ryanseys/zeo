//! Adapter for the ruby/spec `language/` (and simple `core/`) corpus, run
//! through the AOT-compilable `mspec_lite` shim (`conformance/shims/mspec_lite.rb`).
//!
//! Each `*_spec.rb` becomes a synthesized driver written under the suite's work
//! dir:
//!
//! ```ruby
//! require_relative "<repo>/conformance/shims/mspec_lite"
//! require_relative "<abs spec path>"
//! puts MSpecLite.summary_line
//! ```
//!
//! zeo is invoked with `ZEO_MSPEC_STUBS=1` (set by the runner for this
//! suite) so the spec's own `require_relative '../spec_helper'` -- which pulls
//! in the real `mspec` framework -- collapses to a no-op. The driver runs every
//! `describe`/`it` as the spec is required and prints the summary the runner
//! parses (`Expectation::SelfReport`). A clean run (0 failures, 0 errors) is a
//! PASS; ruby/spec's assertions already encode CRuby's behavior, so no separate
//! oracle diff is needed. Specs zeo can't compile surface as `FAIL_COMPILE`
//! -- exactly the language-feature gaps this suite exists to track.

use std::path::{Path, PathBuf};

use super::suite::{home, Expectation, Suite, TestCase};

pub struct RubySpecSuite {
    /// The zeo-rs repo root, so the driver can `require_relative` the shim.
    pub repo_root: PathBuf,
}

impl Suite for RubySpecSuite {
    fn name(&self) -> &'static str {
        "rubyspec"
    }

    fn root_env_var(&self) -> &'static str {
        "RUBYSPEC_DIR"
    }

    fn default_root(&self) -> Option<PathBuf> {
        home().map(|h| h.join("dev/spec/language"))
    }

    fn discover(&self, root: &Path, work_dir: &Path) -> Result<Vec<TestCase>, String> {
        let shim = self.repo_root.join("conformance/shims/mspec_lite.rb");
        if !shim.exists() {
            return Err(format!("mspec_lite shim missing: {}", shim.display()));
        }
        // require_relative appends `.rb`, so strip it from both paths.
        let shim_base = strip_rb(&shim);
        let drivers = work_dir.join("drivers");
        std::fs::create_dir_all(&drivers)
            .map_err(|e| format!("creating {}: {e}", drivers.display()))?;

        let mut cases = Vec::new();
        for spec in list_specs(root)? {
            let id = stem(&spec);
            let spec_base = strip_rb(&spec);
            let driver = drivers.join(format!("{id}.rb"));
            let body = format!(
                "require_relative {shim:?}\nrequire_relative {spec:?}\nputs MSpecLite.summary_line\n",
                shim = shim_base,
                spec = spec_base,
            );
            std::fs::write(&driver, body)
                .map_err(|e| format!("writing {}: {e}", driver.display()))?;
            cases.push(TestCase {
                id,
                source: driver,
                args: Vec::new(),
                stdin: None,
                // The spec's own relative file ops (rare) resolve against its
                // real dir.
                run_cwd: root.to_path_buf(),
                expectation: Expectation::SelfReport,
            });
        }
        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(cases)
    }
}

/// Every `*_spec.rb` directly under `root` (top level only -- `fixtures/` and
/// `shared/` subdirs hold helpers reached via the specs' own `require_relative`,
/// not tests in their own right).
fn list_specs(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Err(format!(
            "{} is not a directory; point --dir or $RUBYSPEC_DIR at a ruby/spec subdir \
             (e.g. ~/dev/spec/language)",
            dir.display()
        ));
    }
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("reading {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("_spec.rb"))
        })
        .collect();
    files.sort();
    Ok(files)
}

fn strip_rb(path: &Path) -> PathBuf {
    path.with_extension("")
}

fn stem(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
}
