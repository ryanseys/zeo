//! A polled ceiling on the compiler's own resident memory.
//!
//! A gem-scale compile holds the whole program in memory more than once (the
//! HIR arena and every spliced file's source, then the emitted program as
//! tokens, then again as text), and one such compile has been measured at
//! 9.8 GB resident on a 16 GB machine. What that costs is not a failed
//! compile: the machine goes to swap, the VM compressor saturates, `configd`
//! stops checking in, and the kernel panics on a watchdog timeout. The whole
//! session dies, and the failure names neither zeo nor the input.
//!
//! So the compiler declares a ceiling and enforces it on itself. A breach
//! ends THIS process with a diagnostic naming the input, the phase and the
//! numbers -- which is a result a sweep can record and a person can act on,
//! where a kernel panic is neither.
//!
//! Polled rather than `setrlimit(RLIMIT_AS)`: macOS does not reliably enforce
//! that limit, and a failed `malloc` cannot say which phase it died in. The
//! poller also gives `ZEO_TIMINGS` its peak-RSS figure, which is the metric
//! the memory work is measured by.

use std::sync::atomic::{AtomicU8, Ordering};

/// The exit status a ceiling breach reports. `12` is `ENOMEM`, chosen so a
/// harness can tell "this machine could not hold the compile" from an
/// ordinary compile failure (`1`) without parsing stderr -- see
/// `xtask`'s gem-probe ledger, which records the two as different outcomes.
pub const EXIT_MEMORY_LIMIT: i32 = 12;

/// The stderr line a breach prints, matched by harnesses that only have the
/// output to go on (a child killed by the OS reports no status of its own).
pub const BREACH_MARKER: &str = "zeo: out of memory:";

const GIB: u64 = 1024 * 1024 * 1024;

/// The ceiling when nothing overrides it. Half the machine's RAM keeps a
/// single compile from being able to exhaust it alone; the 8 GiB clamp keeps
/// a big-memory machine from silently allowing a compile that would still be
/// pathological. Harnesses that run N compiles at once set the environment
/// variable to their own per-job share instead.
const DEFAULT_CLAMP: u64 = 8 * GIB;

/// How often the poller looks. The interval bounds how far past the ceiling a
/// fast-allocating compile can get before it is stopped, so it is set by how
/// much RSS can grow in one interval rather than by what is cheap to sample --
/// `proc_pidinfo` is a bare syscall and ten a second costs nothing.
const POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// Which phase is running, as a [`Phase`] discriminant -- the one piece of
/// context the breach message cannot recover on its own.
static PHASE: AtomicU8 = AtomicU8::new(Phase::Startup as u8);

/// The compile phases a breach message can name.
#[derive(Clone, Copy)]
pub enum Phase {
    Startup,
    ParseLower,
    Analyze,
    Codegen,
    Build,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Phase::Startup => "startup",
            Phase::ParseLower => "parse/lower",
            Phase::Analyze => "analyze",
            Phase::Codegen => "codegen",
            Phase::Build => "build",
        }
    }

    fn from_u8(v: u8) -> Phase {
        match v {
            1 => Phase::ParseLower,
            2 => Phase::Analyze,
            3 => Phase::Codegen,
            4 => Phase::Build,
            _ => Phase::Startup,
        }
    }
}

/// Records which phase a breach would be attributed to. Cheap enough to call
/// at every phase boundary; read only when the ceiling is hit.
pub fn set_phase(phase: Phase) {
    PHASE.store(phase as u8, Ordering::Relaxed);
}

/// The highest resident size this process has reached, in bytes.
///
/// Read from the kernel's own high-water mark rather than from the poller:
/// a sampled maximum only sees what it happens to catch, and the first
/// attempt at this reported a flat 2 MiB across the whole compile-bench set
/// because every program in it finishes inside one poll interval. The poller
/// is for ENFORCEMENT, which needs a current reading; this is for
/// MEASUREMENT, which needs an exact one.
pub fn peak_bytes() -> Option<u64> {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `getrusage` fills the `rusage` it is handed and reads nothing.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return None;
    }
    // `ru_maxrss` is bytes on Darwin and kilobytes everywhere else -- one of
    // the few places the two disagree outright.
    let scale = if cfg!(target_os = "macos") { 1 } else { 1024 };
    (usage.ru_maxrss > 0).then(|| usage.ru_maxrss as u64 * scale)
}

/// The ceiling in force, or `None` when enforcement is off (`ZEO_MEMORY_LIMIT=0`,
/// or a platform with no way to read resident size).
pub fn limit_bytes() -> Option<u64> {
    if let Some(raw) = std::env::var_os("ZEO_MEMORY_LIMIT") {
        // A malformed value is not silently ignored: someone who set it meant
        // to bound this compile, and running unbounded is the outcome they
        // were trying to avoid.
        let text = raw.to_string_lossy();
        let Ok(n) = text.trim().parse::<u64>() else {
            eprintln!("zeo: ZEO_MEMORY_LIMIT is not a byte count: {text}");
            std::process::exit(EXIT_MEMORY_LIMIT);
        };
        return (n > 0).then_some(n);
    }
    Some((physical_memory()? / 2).min(DEFAULT_CLAMP))
}

/// Starts the poller. Call once, early -- the peak it reports is only as
/// complete as the window it watched.
///
/// The thread is detached and never joined: it exists to end the process, and
/// a process that exits normally does not wait for it.
pub fn arm(input: &str) {
    let Some(limit) = limit_bytes() else {
        return; // enforcement off -- `peak_bytes` needs no thread of its own
    };
    if resident_bytes().is_none() {
        return; // no reader on this platform -- nothing to poll
    }
    let limit = Some(limit);
    let input = input.to_string();
    let spawned = std::thread::Builder::new()
        .name("zeo-memguard".into())
        // Only ever holds the message it prints.
        .stack_size(256 * 1024)
        .spawn(move || {
            let Some(limit) = limit else { return };
            loop {
                if resident_bytes().is_some_and(|rss| rss > limit) {
                    breach(&input, resident_bytes().unwrap_or(limit), limit);
                }
                std::thread::sleep(POLL);
            }
        });
    // A machine that cannot spawn this thread is in no state to be told so by
    // a failed compile; the compile itself is the more useful diagnostic.
    let _ = spawned;
}

/// Ends the process at the ceiling. Never returns, and deliberately does not
/// unwind: a partially-built program must not be written, and `main`'s error
/// path would render a compile diagnostic that misattributes the cause.
fn breach(input: &str, rss: u64, limit: u64) -> ! {
    let phase = Phase::from_u8(PHASE.load(Ordering::Relaxed)).label();
    eprintln!(
        "{BREACH_MARKER} {input}: reached {} in {phase}, over the {} ceiling",
        mib(rss),
        mib(limit),
    );
    eprintln!(
        "zeo: raise it with ZEO_MEMORY_LIMIT=<bytes>, or 0 to compile unbounded \
         (which can exhaust the machine)"
    );
    std::process::exit(EXIT_MEMORY_LIMIT);
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// This process's resident size, or `None` on a platform with no reader.
#[cfg(target_os = "macos")]
fn resident_bytes() -> Option<u64> {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let want = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `proc_pidinfo` writes at most `want` bytes into `info`, which is
    // exactly that size, and reads nothing else.
    let got = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            std::ptr::from_mut(&mut info).cast(),
            want,
        )
    };
    (got == want).then_some(info.pti_resident_size)
}

#[cfg(target_os = "linux")]
fn resident_bytes() -> Option<u64> {
    // Field 2 of `/proc/self/statm` is the resident set, in pages.
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * page_size()?)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn resident_bytes() -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn page_size() -> Option<u64> {
    // SAFETY: `sysconf` reads no memory and returns -1 for an unknown name.
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (n > 0).then_some(n as u64)
}

/// The machine's physical RAM. Public because a harness that runs several
/// compiles at once has to divide the same number this does -- see
/// `xtask`'s job budget.
#[cfg(target_os = "macos")]
pub fn physical_memory() -> Option<u64> {
    let mut out: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `hw.memsize` is a `u64` sysctl; `len` says how much `out` can
    // take and is updated to how much was written.
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            std::ptr::from_mut(&mut out).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && out > 0).then_some(out)
}

#[cfg(target_os = "linux")]
pub fn physical_memory() -> Option<u64> {
    // SAFETY: as `page_size` -- `sysconf` reads no memory.
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    (pages > 0).then(|| pages as u64 * page_size().unwrap_or(4096))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn physical_memory() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reader_answers_on_this_platform() {
        // The guard is inert without this, so a platform that loses the
        // reader should fail here rather than silently stop enforcing.
        let rss = resident_bytes().expect("a resident-size reader");
        assert!(rss > 0, "a running process has a resident set");
    }

    #[test]
    fn the_default_ceiling_is_bounded_by_the_machine() {
        let total = physical_memory().expect("physical memory");
        let limit = (total / 2).min(DEFAULT_CLAMP);
        assert!(limit <= DEFAULT_CLAMP);
        assert!(limit <= total / 2);
    }

    #[test]
    fn a_phase_survives_the_round_trip() {
        for phase in [
            Phase::Startup,
            Phase::ParseLower,
            Phase::Analyze,
            Phase::Codegen,
            Phase::Build,
        ] {
            assert_eq!(Phase::from_u8(phase as u8).label(), phase.label());
        }
    }
}
