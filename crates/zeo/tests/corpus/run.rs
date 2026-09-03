//! Running one child under hard bounds and keeping what it said.
//!
//! Without bounds a single miscompiled program takes the machine down rather
//! than failing: `Command::output()` reads both pipes to EOF into an
//! unbounded `Vec<u8>`, so `loop { puts "x" }` writes into the test process
//! at full speed forever, and `(1..).to_a` allocates without writing. A run
//! that trips a bound is an ordinary test FAILURE naming the bound: a hang
//! is a divergence from ruby like any other.

use std::process::Command;
use std::time::{Duration, Instant};

use crate::case::{Answer, Exit};

/// 64 MiB per stream.
const MAX_CAPTURE: usize = 64 << 20;

/// What a child may take. The ordinary figures fit every program in the
/// corpus with room; a whole-gem compile (`test/milestones/`) gets the
/// raised ones.
#[derive(Clone, Copy)]
pub struct Bounds {
    pub deadline: Duration,
    pub max_rss: u64,
}

impl Bounds {
    /// 120s, not 60: a whole-gem program (`require "rss"`) compiles in ~12s
    /// alone and ran past 60s under a loaded suite. 768 MiB is 2x that
    /// gem-graph peak and still a fraction of what a runaway wants; 741
    /// ordinary children peak at 13 MiB.
    pub const ORDINARY: Bounds = Bounds {
        deadline: Duration::from_secs(120),
        max_rss: 768 << 20,
    };
    /// A milestone splices a library's whole require graph: minutes and
    /// gigabytes.
    pub const WHOLE_GRAPH: Bounds = Bounds {
        deadline: Duration::from_secs(300),
        max_rss: 4096 << 20,
    };
    /// A gap is green when it does NOT match, so a hang answers the question
    /// as well as a crash does -- and the whole suite waits for it. Half a
    /// minute is long enough for the slowest gap to reach its wrong answer.
    pub const GAP: Bounds = Bounds {
        deadline: Duration::from_secs(30),
        ..Bounds::ORDINARY
    };
}

/// The address-space ceiling, a DIFFERENT quantity from the RSS cap: Darwin
/// accepts `RLIMIT_AS` and ignores it, Linux enforces it, and an ordinary
/// zeo program RESERVES far more than it touches (a 64 MiB main stack, 8
/// MiB per Ruby thread, mimalloc's arenas). 4 GiB is above anything
/// legitimate and still under what a runaway wants.
const MAX_CHILD_ADDRESS_SPACE: u64 = 4 << 30;

#[cfg(unix)]
fn bound_address_space(cmd: &mut Command) {
    use std::os::unix::process::CommandExt as _;
    // `pre_exec` runs in the forked child, where only async-signal-safe
    // calls are legal; `setrlimit` is one, and the closure does nothing else.
    unsafe {
        cmd.pre_exec(|| {
            let lim = libc::rlimit {
                rlim_cur: MAX_CHILD_ADDRESS_SPACE,
                rlim_max: MAX_CHILD_ADDRESS_SPACE,
            };
            // A platform that refuses RLIMIT_AS must not fail the spawn: the
            // watchdog is the bound that has to work everywhere.
            libc::setrlimit(libc::RLIMIT_AS, &lim);
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn bound_address_space(_cmd: &mut Command) {}

/// Resident set size of `pid`, or `None` when it cannot be read (the
/// process just exited, or the platform is not covered: the watchdog does
/// not fire and the other bounds still apply).
#[cfg(target_os = "macos")]
fn child_rss(pid: u32) -> Option<u64> {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let ptr = std::ptr::addr_of_mut!(info).cast::<libc::c_void>();
    let n = unsafe { libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTASKINFO, 0, ptr, size) };
    (n == size).then_some(info.pti_resident_size)
}

#[cfg(target_os = "linux")]
fn child_rss(pid: u32) -> Option<u64> {
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn child_rss(_pid: u32) -> Option<u64> {
    None
}

/// Run `cmd` to completion under `bounds`. Both pipes drain on their own
/// threads, and stdin is written on one too: writing it inline deadlocks
/// whenever the child fills its stdout pipe before consuming all of stdin.
pub fn run_bounded(
    cmd: &mut Command,
    stdin: Option<&[u8]>,
    what: &str,
    bounds: Bounds,
) -> Result<Answer, String> {
    use std::io::Read as _;
    use std::process::Stdio;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    match stdin {
        Some(_) => cmd.stdin(Stdio::piped()),
        None => cmd.stdin(Stdio::null()),
    };
    bound_address_space(cmd);
    let mut child = cmd.spawn().map_err(|e| format!("spawn {what}: {e}"))?;

    let overflowed = Arc::new(AtomicBool::new(false));
    let spawn_reader = |mut pipe: Box<dyn std::io::Read + Send>, flag: Arc<AtomicBool>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 64 * 1024];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.len() > MAX_CAPTURE {
                            // Stop draining: the child blocks on its next
                            // write and the deadline loop kills it.
                            flag.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
            }
            buf
        })
    };
    let out_h = spawn_reader(
        Box::new(child.stdout.take().expect("piped stdout")),
        Arc::clone(&overflowed),
    );
    let err_h = spawn_reader(
        Box::new(child.stderr.take().expect("piped stderr")),
        Arc::clone(&overflowed),
    );
    let in_h = stdin.map(|input| {
        let mut sink = child.stdin.take().expect("piped stdin");
        let input = input.to_vec();
        std::thread::spawn(move || {
            use std::io::Write as _;
            // A child that exits without reading stdin gives EPIPE; that is
            // its business, not an error here.
            let _ = sink.write_all(&input);
        })
    });

    let started = Instant::now();
    let pid = child.id();
    let mut limit = None;
    let mut ticks: u32 = 0;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Err(e) => return Err(format!("waiting on {what}: {e}")),
            Ok(None) => {}
        }
        if overflowed.load(Ordering::SeqCst) {
            limit = Some(format!("wrote more than {} MiB", MAX_CAPTURE >> 20));
        } else if started.elapsed() > bounds.deadline {
            limit = Some(format!("ran longer than {}s", bounds.deadline.as_secs()));
        } else if ticks.is_multiple_of(10) {
            // Every ~100ms: reading RSS is a syscall per child, and a runaway
            // needs seconds to matter.
            if let Some(rss) = child_rss(pid)
                && rss > bounds.max_rss
            {
                limit = Some(format!("allocated more than {} MiB", bounds.max_rss >> 20));
            }
        }
        if limit.is_some() {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        ticks = ticks.wrapping_add(1);
        std::thread::sleep(Duration::from_millis(10));
    };

    let stdout = out_h.join().unwrap_or_default();
    let stderr = err_h.join().unwrap_or_default();
    if let Some(h) = in_h {
        let _ = h.join();
    }
    match (limit, status) {
        (Some(why), _) => Err(format!("{what} killed: {why}")),
        (None, Some(status)) => Ok(Answer {
            stdout,
            stderr,
            exit: exit_of(status),
        }),
        (None, None) => Err(format!("{what}: no exit status")),
    }
}

pub fn exit_of(status: std::process::ExitStatus) -> Exit {
    if let Some(code) = status.code() {
        return Exit::Code(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(sig) = status.signal() {
            return Exit::Signal(sig);
        }
    }
    Exit::Code(-1)
}
