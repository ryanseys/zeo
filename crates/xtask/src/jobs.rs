//! How many whole `zeo` compiles a sweep may run at once, and what each one is
//! allowed to hold.
//!
//! The limit is MEMORY, not cores. One gem-scale compile has been measured at
//! 9.8 GB resident; twelve of them at once -- one per core, which is what both
//! sweeps used to do -- exhausted a 16 GB machine's memory and swap and
//! panicked the kernel with a watchdog timeout. `gem-probe` answered that with
//! a hand-tuned `DEFAULT_JOBS = 4`, which is the right number for THIS machine
//! and says nothing on any other.
//!
//! So the shared rule is a budget rather than a count. A fixed fraction of RAM
//! is the whole sweep's to spend, each job gets an equal share, and the share
//! is handed to the child as `ZEO_MEMORY_LIMIT` so the child enforces it on
//! itself (`zeo::memguard`). `--jobs` then trades job count against per-job
//! headroom instead of multiplying an unbounded number: asking for more jobs
//! makes each one smaller, and the total stays where it was.
//!
//! On a 16 GB / 12-core machine this derives 4 jobs -- the same number the
//! hand-tuned constant landed on, which is the calibration this model is
//! checked against.

use std::process::Command;

/// The share of RAM a sweep may spend across all of its children at once. The
/// rest is the OS, the editor, the sweep's own parent process, and the slack
/// that keeps a burst off the compressor.
const BUDGET_NUMERATOR: u64 = 3;
const BUDGET_DENOMINATOR: u64 = 5;

/// What one front-end compile is expected to want. Not a hard figure -- it is
/// the divisor that turns the budget into a job count, and it is set where a
/// gem-scale compile fits without swapping.
const PER_JOB_TARGET: u64 = 2 * 1024 * 1024 * 1024;

/// The smallest per-job ceiling worth handing out. Below this a compile fails
/// on its own startup, so an explicit `--jobs` past this point is reported
/// rather than silently honoured.
const PER_JOB_FLOOR: u64 = 512 * 1024 * 1024;

/// The fallback when the machine's RAM cannot be read -- the number
/// `gem-probe` carried as its hand-tuned constant.
const FALLBACK_JOBS: usize = 4;

/// How wide a sweep may run, and what each child may hold.
#[derive(Clone, Copy)]
pub struct Budget {
    pub jobs: usize,
    /// Bytes of resident memory each child is allowed, or `None` when the
    /// machine's RAM could not be read and the children run unbounded.
    pub per_job_bytes: Option<u64>,
}

impl Budget {
    /// Derives the budget, honouring an explicit `--jobs` by shrinking the
    /// per-job share rather than growing the total.
    pub fn derive(requested: Option<usize>) -> Budget {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let Some(total) = zeo::memguard::physical_memory() else {
            return Budget {
                jobs: requested.unwrap_or(FALLBACK_JOBS).max(1),
                per_job_bytes: None,
            };
        };
        let budget = total / BUDGET_DENOMINATOR * BUDGET_NUMERATOR;
        let derived = (budget / PER_JOB_TARGET).clamp(1, cores as u64) as usize;
        let jobs = requested.unwrap_or(derived).max(1);
        let per_job = budget / jobs as u64;
        if per_job < PER_JOB_FLOOR {
            eprintln!(
                "xtask: {jobs} jobs leaves {} per compile, under the {} floor -- \
                 the machine's {} may not hold them all",
                gib(per_job),
                gib(PER_JOB_FLOOR),
                gib(total),
            );
        }
        Budget {
            jobs,
            per_job_bytes: Some(per_job.max(PER_JOB_FLOOR)),
        }
    }

    /// Hands a child its share. A compile that outruns it exits
    /// [`zeo::memguard::EXIT_MEMORY_LIMIT`] instead of taking the machine down.
    pub fn apply(&self, cmd: &mut Command) {
        if let Some(bytes) = self.per_job_bytes {
            cmd.env("ZEO_MEMORY_LIMIT", bytes.to_string());
        }
    }

    /// The one-line summary a sweep prints before it starts.
    pub fn describe(&self) -> String {
        match self.per_job_bytes {
            Some(b) => format!("{} job(s), {} each", self.jobs, gib(b)),
            None => format!("{} job(s), unbounded", self.jobs),
        }
    }
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_derived_width_never_exceeds_the_core_count() {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let b = Budget::derive(None);
        assert!(b.jobs >= 1 && b.jobs <= cores, "{} jobs", b.jobs);
    }

    #[test]
    fn asking_for_more_jobs_shrinks_the_share_instead_of_the_budget() {
        // The invariant the whole module exists for: jobs x per-job is the
        // same budget however the caller splits it.
        let (one, four) = (Budget::derive(Some(1)), Budget::derive(Some(4)));
        let (Some(a), Some(b)) = (one.per_job_bytes, four.per_job_bytes) else {
            return; // no RAM reader on this platform
        };
        assert!(a >= b * 4, "one job should get at least four jobs' share");
    }

    #[test]
    fn a_child_is_handed_its_share() {
        let budget = Budget::derive(Some(2));
        let mut cmd = Command::new("true");
        budget.apply(&mut cmd);
        let set = cmd
            .get_envs()
            .any(|(k, v)| k == "ZEO_MEMORY_LIMIT" && v.is_some());
        assert_eq!(set, budget.per_job_bytes.is_some());
    }
}
