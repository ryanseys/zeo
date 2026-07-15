//! Child-process execution with a wall-clock timeout and full output capture,
//! dependency-free: reader threads drain stdout/stderr (so a chatty child
//! never deadlocks on a full pipe) while the parent polls `try_wait`.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Execution {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `None` when the child was killed on timeout.
    pub status: Option<std::process::ExitStatus>,
    pub timed_out: bool,
    pub duration: Duration,
}

impl Execution {
    pub fn success(&self) -> bool {
        self.status.is_some_and(|s| s.success())
    }

    /// True when the child died from a signal (a crash rather than a Ruby
    /// exception exit).
    pub fn crashed(&self) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            return self.status.is_some_and(|s| s.signal().is_some());
        }
        #[cfg(not(unix))]
        false
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
    Ok(Execution {
        stdout: stdout_reader.join().unwrap_or_default(),
        stderr: stderr_reader.join().unwrap_or_default(),
        status,
        timed_out,
        duration: start.elapsed(),
    })
}

fn drain<R: Read + Send + 'static>(mut r: R) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.read_to_end(&mut buf);
        buf
    })
}
