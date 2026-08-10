//! Child-process execution with a wall-clock timeout and output capture,
//! dependency-free: a reader thread drains each pipe (so a chatty child never
//! deadlocks on a full one) while the parent polls `try_wait`.
//!
//! Both pipes are drained by their own thread. Capturing stdout as well as
//! stderr is what lets `gem-probe` measure and keep `--dump=rust` output, and
//! draining it is not optional once it is a pipe: the Rust for a large gem is
//! megabytes, far past the pipe buffer, so a child writing it would block
//! forever against a parent that never read.
//!
//! What is kept is bounded, though. The generated Rust for one gem has been
//! measured near a gigabyte, and a sweep runs several children at once, so an
//! unbounded `Vec<u8>` per stream put the SWEEP's own memory in the same class
//! as the compiles it was measuring. Past [`MAX_CAPTURE`] the reader keeps
//! draining -- backpressuring the child would turn a big-output gem into a
//! bogus timeout -- but discards, and records the true byte count so a caller
//! can tell a truncated capture from a short one. The golden harness has had
//! this bound since a miscompiled `.rb` first took the machine down
//! (`zeo-tests/tests/support/golden.rs`); this is the same rule for xtask.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The most of each stream `run_with_timeout` KEEPS. Matches the golden
/// harness's bound.
pub const MAX_CAPTURE: usize = 64 << 20; // 64 MiB per stream

pub struct Execution {
    /// The first [`MAX_CAPTURE`] bytes of the child's stdout.
    pub stdout: Vec<u8>,
    /// The first [`MAX_CAPTURE`] bytes of the child's stderr.
    pub stderr: Vec<u8>,
    /// How many bytes the child actually wrote to stdout -- more than
    /// `stdout.len()` when the capture was truncated. stderr is bounded the
    /// same way but has no counterpart: nothing measures it, and a diagnostic
    /// that runs past 64 MiB is already past being read.
    pub stdout_bytes: u64,
    /// `None` when the child was killed on timeout.
    pub status: Option<std::process::ExitStatus>,
    pub timed_out: bool,
}

impl Execution {
    pub fn success(&self) -> bool {
        self.status.is_some_and(|s| s.success())
    }

    /// Whether `stdout` holds less than the child wrote. A caller that treats
    /// the capture as the whole output must check this rather than assume.
    pub fn stdout_truncated(&self) -> bool {
        self.stdout_bytes > self.stdout.len() as u64
    }
}

pub fn run_with_timeout(
    mut cmd: Command,
    stdin_bytes: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Execution, String> {
    cmd.stdin(if stdin_bytes.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let start = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawning {:?}: {e}", cmd.get_program()))?;

    let stdin_writer = stdin_bytes.map(|bytes| {
        let mut stdin = child.stdin.take().unwrap();
        std::thread::spawn(move || {
            // The child may exit without reading; a broken pipe is fine.
            let _ = stdin.write_all(&bytes);
        })
    });
    let stdout_reader = drain(child.stdout.take().unwrap());
    let stderr_reader = drain(child.stderr.take().unwrap());

    let mut timed_out = false;
    let status = loop {
        match child.try_wait().map_err(|e| format!("waiting: {e}"))? {
            Some(status) => break Some(status),
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    };

    if let Some(w) = stdin_writer {
        let _ = w.join();
    }
    let (stdout, stdout_bytes) = stdout_reader.join().unwrap_or_default();
    let (stderr, _) = stderr_reader.join().unwrap_or_default();
    Ok(Execution {
        stdout,
        stderr,
        stdout_bytes,
        status,
        timed_out,
    })
}

/// Drains one pipe to EOF, keeping at most [`MAX_CAPTURE`] bytes. Returns what
/// was kept and how much was written -- see the module docs for why draining
/// continues past the cap instead of backpressuring the child.
fn drain<R: Read + Send + 'static>(mut r: R) -> std::thread::JoinHandle<(Vec<u8>, u64)> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut total = 0u64;
        let mut chunk = [0u8; 64 * 1024];
        loop {
            match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    total += n as u64;
                    if buf.len() < MAX_CAPTURE {
                        let room = MAX_CAPTURE - buf.len();
                        buf.extend_from_slice(&chunk[..n.min(room)]);
                    }
                }
            }
        }
        (buf, total)
    })
}
