//! An advisory, PID-based run lock so two `conformance run`s don't silently
//! contend on the shared `target/` and binary cache (the `--update-scoreboard`
//! authority in particular must not run twice at once). Best effort: it detects
//! a still-live peer and refuses with a clear message, and reclaims a lock whose
//! owner has died. Not a hard mutex -- a determined caller can delete the file.

use std::path::{Path, PathBuf};

/// Held for the duration of a run; removes the lockfile on drop.
pub struct RunLock {
    path: PathBuf,
}

impl RunLock {
    /// Acquire the lock under `target/`, or return a message describing the live
    /// run that already holds it. Uses an atomic `create_new` so two runs racing
    /// to start can't both win, and reclaims the lock if its recorded owner is
    /// gone.
    pub fn acquire(target_dir: &Path) -> Result<RunLock, String> {
        let path = target_dir.join("zeo-conformance.lock");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        for attempt in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    let _ = write!(file, "{}", std::process::id());
                    return Ok(RunLock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let owner = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|s| s.trim().parse::<u32>().ok());
                    match owner {
                        Some(pid) if pid_alive(pid) => {
                            return Err(format!(
                                "another conformance run is in progress (pid {pid}, lock at {}). \
                                 Wait for it to finish, or remove that file if it is stale.",
                                path.display()
                            ));
                        }
                        // Stale lock (dead owner or unparseable) -- reclaim it
                        // and retry the atomic create once.
                        _ if attempt == 0 => {
                            let _ = std::fs::remove_file(&path);
                        }
                        _ => break,
                    }
                }
                Err(e) => return Err(format!("creating run lock {}: {e}", path.display())),
            }
        }
        Err(format!("could not acquire run lock at {}", path.display()))
    }
}

impl Drop for RunLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Whether a process with this pid is alive, via POSIX `kill -0` (best-effort;
/// no `libc` dependency). Exit 0 means the process exists and is signalable.
fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
