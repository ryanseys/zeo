//! Running a child process, and saying plainly what happened when it fails.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::Error;

/// What to keep from a child. Whatever is not captured goes straight to this
/// process's own streams, so a cargo build's progress and a compiler's own
/// diagnostics stay visible.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    /// Nothing: the child's output IS the tool's output.
    Nothing,
    /// stdout, for a caller that reads it. stderr still reaches the terminal.
    Stdout,
    /// Both, for a caller that reports the failure itself.
    Both,
}

impl Capture {
    fn stdout(self) -> bool {
        self != Capture::Nothing
    }

    fn stderr(self) -> bool {
        self == Capture::Both
    }
}

/// What a finished child left behind. `stdout` stays BYTES: a program's
/// output is compared against a recorded file, and a lossy decode makes a
/// binary answer unequal to its own recording however equal the bytes are.
pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// stdout as text, for the callers reading a tool's own report.
    pub fn stdout_text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }
}

/// Run `argv` in `dir`. `env` entries with a `None` value are REMOVED from
/// the child's environment rather than set empty -- the difference matters
/// for `ZEO_HOME` and friends, which are read as "present but blank".
pub fn run<S: AsRef<OsStr>>(
    argv: &[S],
    dir: &Path,
    env: &[(&str, Option<&str>)],
    capture: Capture,
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
    let pipe = |on: bool| if on { Stdio::piped() } else { Stdio::inherit() };
    cmd.stdout(pipe(capture.stdout()));
    cmd.stderr(pipe(capture.stderr()));
    let child = cmd.spawn().map_err(|e| {
        Error::new(format!(
            "spawning {}: {e}",
            program.as_ref().to_string_lossy()
        ))
    })?;
    let out = child.wait_with_output().map_err(|e| {
        Error::new(format!(
            "waiting for {}: {e}",
            program.as_ref().to_string_lossy()
        ))
    })?;
    Ok(Output {
        code: out.status.code(),
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}
