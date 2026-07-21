//! The live `ruby` oracle for tests without a committed `.expected` snapshot,
//! with a content-addressed cache so each oracle runs **once ever** per
//! (source, args, stdin, ruby-version) -- that determinism is what keeps the
//! scoreboard reproducible for snapshot-less tests. The external corpus is
//! never written to.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::exec::run_with_timeout;
use super::suite::TestCase;
use super::util::fnv1a64;

/// Flags passed to EVERY oracle run. `error_highlight` (the source snippet +
/// caret) and `did_you_mean` (the "Did you mean?" suggestion) are opt-out
/// default GEMS, not core language semantics -- any user can disable them, and
/// their output is a per-AST-node presentation layer zeo does not aim to
/// reproduce byte-for-byte. Disabling them makes the reference the plain core
/// backtrace (`file:line:in 'ctx': msg (Class)` + `from` frames), which is the
/// error format zeo actually targets. Recorded in `ruby_version` below so
/// changing this set invalidates the oracle cache and every result stamp.
const ORACLE_FLAGS: &[&str] = &["--disable-error_highlight", "--disable-did_you_mean"];

pub struct OracleOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub ok: bool,
}

pub struct Oracle {
    cache_dir: PathBuf,
    /// The resolved interpreter, used for EVERY oracle run -- not just the
    /// version probe. Resolving once and reusing the path is what keeps the
    /// recorded `ruby_version` and the actual reference outputs in agreement.
    ruby: PathBuf,
    pub ruby_version: String,
    timeout: Duration,
}

impl Oracle {
    pub fn new(cache_dir: PathBuf, timeout: Duration, root: &Path) -> Result<Oracle, String> {
        let required = required_ruby(root)?;
        let ruby = resolve_ruby(root);
        let out = Command::new(&ruby)
            .arg("-v")
            .output()
            .map_err(|e| format!("running `{} -v`: {e}", ruby.display()))?;
        let reported = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if !version_matches(&reported, &required) {
            return Err(format!(
                "conformance oracle is ruby {required}, but `{}` is:\n  {reported}\n\n\
                 The suite diffs against real CRuby, so the wrong interpreter yields a\n\
                 scoreboard that looks authoritative and is not. Fix with one of:\n  \
                 mise install ruby@{required}   (then re-run)\n  \
                 eval \"$(mise activate zsh)\"   (if mise is installed but not active)",
                ruby.display()
            ));
        }
        // The oracle IDENTITY is the interpreter PLUS the flags it runs under:
        // both feed the cache key and the stamp fingerprint (each keys on
        // `ruby_version`), so adding/removing an `ORACLE_FLAGS` entry forces an
        // honest re-run instead of replaying references built under the old set.
        let ruby_version = format!("{reported} [{}]", ORACLE_FLAGS.join(" "));
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("creating {}: {e}", cache_dir.display()))?;
        Ok(Oracle {
            cache_dir,
            ruby,
            ruby_version,
            timeout,
        })
    }

    fn cache_key(&self, case: &TestCase, source: &[u8], stdin: Option<&[u8]>) -> u64 {
        let mut key = Vec::new();
        key.extend_from_slice(source);
        key.push(0);
        for a in &case.args {
            key.extend_from_slice(a.as_bytes());
            key.push(0);
        }
        if let Some(s) = stdin {
            key.extend_from_slice(s);
        }
        key.push(0);
        key.extend_from_slice(self.ruby_version.as_bytes());
        fnv1a64(&key)
    }

    /// The oracle's (stdout, stderr) for this case, from cache or a live run.
    /// `bypass_cache` forces a fresh run (used by `oracle-verify`).
    pub fn run(&self, case: &TestCase, bypass_cache: bool) -> Result<OracleOutput, String> {
        let source = std::fs::read(&case.source)
            .map_err(|e| format!("reading {}: {e}", case.source.display()))?;
        let stdin = match &case.stdin {
            Some(p) => Some(std::fs::read(p).map_err(|e| format!("reading {}: {e}", p.display()))?),
            None => None,
        };
        let key = self.cache_key(case, &source, stdin.as_deref());
        let base = self.cache_dir.join(format!("{key:016x}"));
        let (out_path, err_path, ok_path) = (
            base.with_extension("stdout"),
            base.with_extension("stderr"),
            base.with_extension("ok"),
        );

        if !bypass_cache {
            if let (Ok(stdout), Ok(stderr), Ok(ok)) = (
                std::fs::read(&out_path),
                std::fs::read(&err_path),
                std::fs::read_to_string(&ok_path),
            ) {
                return Ok(OracleOutput {
                    stdout,
                    stderr,
                    ok: ok.trim() == "1",
                });
            }
        }

        let mut cmd = Command::new(&self.ruby);
        cmd.args(ORACLE_FLAGS)
            .arg(&case.source)
            .args(&case.args)
            .current_dir(&case.run_cwd);
        // A nonzero exit is a valid reference (the program may legitimately
        // raise); only a timeout disqualifies the oracle.
        let exec = run_with_timeout(cmd, stdin, self.timeout)?;
        let ok = !exec.timed_out;

        write(&out_path, &exec.stdout)?;
        write(&err_path, &exec.stderr)?;
        write(&ok_path, if ok { b"1" } else { b"0" })?;
        Ok(OracleOutput {
            stdout: exec.stdout,
            stderr: exec.stderr,
            ok,
        })
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Where the oracle interpreter comes from, in preference order:
///
/// 1. `mise which ruby` evaluated AT THE REPO ROOT, so the repo's `mise.toml`
///    pin decides. This works from a non-interactive shell that never ran
///    `mise activate` -- the case that silently mis-resolved before.
/// 2. Bare `ruby` from `PATH`, for a machine without mise.
///
/// Never a hardcoded install path: the pin lives in `mise.toml`, and this only
/// asks mise to resolve it.
fn resolve_ruby(root: &Path) -> PathBuf {
    let out = Command::new("mise")
        .arg("which")
        .arg("ruby")
        .current_dir(root)
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }
    }
    PathBuf::from("ruby")
}

/// The `ruby = "X"` pin from the repo's `mise.toml`. Read rather than compiled
/// in so the version is declared in exactly one place, next to the toolchain
/// it configures.
fn required_ruby(root: &Path) -> Result<String, String> {
    let path = root.join("mise.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("reading {} (the oracle pin): {e}", path.display()))?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "ruby" {
            continue;
        }
        let value = value.trim().trim_matches('"');
        if !value.is_empty() {
            return Ok(value.to_owned());
        }
    }
    Err(format!("no `ruby = \"...\"` pin in {}", path.display()))
}

/// Does `ruby -v` output report exactly the pinned version?
///
/// Compares the version TOKEN, not a prefix: a `starts_with` test would accept
/// 4.0.51 for a 4.0.5 pin.
fn version_matches(reported: &str, required: &str) -> bool {
    reported
        .strip_prefix("ruby ")
        .and_then(|rest| rest.split_whitespace().next())
        .is_some_and(|found| found == required)
}

#[cfg(test)]
mod tests {
    use super::{required_ruby, version_matches};

    #[test]
    fn the_reported_version_must_match_the_pin_exactly() {
        let v = "ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]";
        assert!(version_matches(v, "4.0.5"));
        // The failure that actually happened: macOS system ruby.
        assert!(!version_matches(
            "ruby 2.6.10p210 (2022-04-12 revision 67958) [universal.arm64e-darwin25]",
            "4.0.5"
        ));
        // A prefix test would wrongly accept these.
        assert!(!version_matches("ruby 4.0.51 (2026-05-20)", "4.0.5"));
        assert!(!version_matches("ruby 4.0.5p1 (2026-05-20)", "4.0.5"));
    }

    /// The pin is the repo's own `mise.toml`, so this also fails if that file
    /// loses its `[tools] ruby` entry.
    #[test]
    fn the_repo_pins_its_oracle() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        let pinned = required_ruby(root).expect("mise.toml must pin ruby");
        assert!(
            pinned.starts_with('4'),
            "oracle pin should be a ruby 4.x, got {pinned}"
        );
    }
}
