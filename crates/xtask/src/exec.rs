//! Running a child process, and saying plainly what happened when it fails.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::Error;

/// What a finished child left behind. `stdout` is empty unless the caller
/// asked to capture it; stderr always reaches the terminal, because a tool
/// that swallows a compiler's own diagnostics is worse than no tool.
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Run `argv` in `dir`. `env` entries with a `None` value are REMOVED from
/// the child's environment rather than set empty -- the difference matters
/// for `RUBYOPT` and friends, which ruby reads as "present but blank".
pub fn run<S: AsRef<OsStr>>(
    argv: &[S],
    dir: &Path,
    env: &[(&str, Option<&str>)],
    capture_stdout: bool,
) -> Result<Output, Error> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| Error::new("an empty command line"))?;
    let mut cmd = Command::new(program);
    cmd.args(rest).current_dir(dir);
    for (key, value) in env {
        match value {
            Some(v) => cmd.env(key, v),
            None => cmd.env_remove(key),
        };
    }
    if capture_stdout {
        cmd.stdout(Stdio::piped());
    }
    let out = cmd.output().map_err(|e| {
        Error::new(format!(
            "spawning {}: {e}",
            program.as_ref().to_string_lossy()
        ))
    })?;
    // `output()` pipes both streams; hand stderr back to the terminal.
    let _ = std::io::Write::write_all(&mut std::io::stderr(), &out.stderr);
    Ok(Output {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    })
}
