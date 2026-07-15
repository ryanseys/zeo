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

pub struct OracleOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub ok: bool,
}

pub struct Oracle {
    cache_dir: PathBuf,
    pub ruby_version: String,
    timeout: Duration,
}

impl Oracle {
    pub fn new(cache_dir: PathBuf, timeout: Duration) -> Result<Oracle, String> {
        let out = Command::new("ruby")
            .arg("-v")
            .output()
            .map_err(|e| format!("running `ruby -v`: {e}"))?;
        let ruby_version = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("creating {}: {e}", cache_dir.display()))?;
        Ok(Oracle {
            cache_dir,
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

        let mut cmd = Command::new("ruby");
        cmd.arg(&case.source)
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
