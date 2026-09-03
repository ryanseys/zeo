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

/// What a finished child left behind. Both streams stay BYTES: a program's
/// output is recorded into a golden and compared against it later, and a
/// lossy decode makes a non-UTF-8 answer unequal to its own recording however
/// equal the bytes are. The `_text` accessors are for the callers reading a
/// tool's own report, where the output is ASCII by construction.
pub struct Output {
    pub code: Option<i32>,
    /// The signal that killed the child, when `code` is `None`.
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// How the child ended, in the corpus's own spelling.
    pub fn exit(&self) -> crate::case::Exit {
        match (self.code, self.signal) {
            (Some(code), _) => crate::case::Exit::Code(code),
            (None, Some(sig)) => crate::case::Exit::Signal(sig),
            (None, None) => crate::case::Exit::Code(-1),
        }
    }

    pub fn stdout_text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }

    pub fn stderr_text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.stderr)
    }

    /// The exit status for a person to read. `None` means a signal killed the
    /// child, which `Some(0)`-style formatting buries.
    pub fn code_text(&self) -> String {
        match self.code {
            Some(code) => code.to_string(),
            None => "killed by a signal".into(),
        }
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
    run_with_stdin(argv, dir, env, capture, None)
}

/// The same, feeding `stdin` to the child. The write runs on its own thread:
/// a child that fills its stdout pipe while the parent is still writing stdin
/// deadlocks both sides otherwise.
pub fn run_with_stdin<S: AsRef<OsStr>>(
    argv: &[S],
    dir: &Path,
    env: &[(&str, Option<&str>)],
    capture: Capture,
    stdin: Option<&[u8]>,
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
    run_command(&mut cmd, capture, stdin)
}

/// Run a `Command` the caller built, capturing per `capture` and feeding
/// `stdin` from its own thread.
pub fn run_command(
    cmd: &mut Command,
    capture: Capture,
    stdin: Option<&[u8]>,
) -> Result<Output, Error> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let pipe = |on: bool| if on { Stdio::piped() } else { Stdio::inherit() };
    cmd.stdout(pipe(capture.stdout()));
    cmd.stderr(pipe(capture.stderr()));
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::new(format!("spawning {program}: {e}")))?;
    let writer = stdin.map(|bytes| {
        let bytes = bytes.to_vec();
        let mut pipe = child.stdin.take().expect("stdin was piped");
        std::thread::spawn(move || {
            // A child may exit without reading; a broken pipe is fine.
            let _ = std::io::Write::write_all(&mut pipe, &bytes);
        })
    });
    let out = child
        .wait_with_output()
        .map_err(|e| Error::new(format!("waiting for {program}: {e}")))?;
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt as _;
        out.status.signal()
    };
    #[cfg(not(unix))]
    let signal = None;
    Ok(Output {
        code: out.status.code(),
        signal,
        stdout: out.stdout,
        stderr: out.stderr,
    })
}
