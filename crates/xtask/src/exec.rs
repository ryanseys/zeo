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
//! **stdout is drained and discarded.** Draining is not optional -- a child
//! that fills the pipe blocks forever against a parent that never reads -- but
//! KEEPING it was: the generated Rust for one gem has been measured near a
//! gigabyte, and with several children in flight the sweep's own memory was in
//! the same class as the compiles it was measuring. Nothing needs those bytes
//! any more. A caller that wants a child's program output asks the child to
//! write it to a file (`zeo --emit-clif`), which is both cheaper and the only
//! shape that works at gem scale.
//!
//! stderr IS kept, bounded at [`MAX_CAPTURE`] -- every caller classifies a
//! failure from it, and a diagnostic that runs past 64 MiB is already past
//! being read. The golden harness has had this bound since a miscompiled `.rb`
//! first took the machine down (`zeo-tests/tests/support/golden.rs`).

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The most of stderr `run_with_timeout` keeps. Matches the golden harness's
/// bound.
pub const MAX_CAPTURE: usize = 64 << 20;

pub struct Execution {
    /// The first [`MAX_CAPTURE`] bytes of the child's stderr. stdout is
    /// discarded -- see the module docs.
    pub stderr: Vec<u8>,
    /// `None` when the child was killed on timeout.
    pub status: Option<std::process::ExitStatus>,
    pub timed_out: bool,
}

impl Execution {
    pub fn success(&self) -> bool {
        self.status.is_some_and(|s| s.success())
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
    let stdout_reader = drain(child.stdout.take().unwrap(), 0);
    let stderr_reader = drain(child.stderr.take().unwrap(), MAX_CAPTURE);

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
    let _ = stdout_reader.join();
    Ok(Execution {
        stderr: stderr_reader.join().unwrap_or_default(),
        status,
        timed_out,
    })
}

/// Drains one pipe to EOF, keeping at most `keep` bytes of it.
///
/// Draining continues past `keep` rather than stopping: a reader that stops
/// backpressures the child, which would turn a big-output program into a bogus
/// timeout. `keep = 0` reads and discards, which is what stdout gets.
fn drain<R: Read + Send + 'static>(mut r: R, keep: usize) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) if buf.len() < keep => {
                    let room = keep - buf.len();
                    buf.extend_from_slice(&chunk[..n.min(room)]);
                }
                Ok(_) => {}
            }
        }
        buf
    })
}
