# gem-compat `--verify`: actually compile the gems, not just classify them

Deferred (2026-07). `cargo xtask gem-compat` today reports a **resolvability**
number, not a **compiles-successfully** number. Its `pure-ruby` bucket means
"zeo resolved the gem's `lib/` and *would attempt* to compile it" — a static
classification, never a real compile. So the headline (e.g. *163/195 = 84%*)
overstates: many pure-Ruby gems still fail on stdlib/dep gaps (`rake` needs
`rbconfig`, and so on).

`--verify` closes that gap the way `stdlib-status` measures stdlib coverage:
actually run the compiler over each candidate gem and record the real outcome.

## What it should do

1. Classify as today (reuse `zeo::gem_compat` / `gem_compat_installed`).
2. For every `pure-ruby` gem, actually compile a probe:
   - synthesize a temp `Gemfile.lock` from the installed set
     (`gem_store::installed_as_lockfile` already produces exactly this), OR reuse
     the lockfile the user passed;
   - run the `zeo` binary on a `require "<name>"` probe with `--gem-path
     <store> --lockfile <temp> -S` (`-S` = codegen only, no link — the same
     fast path `stdlib-status` uses);
   - map the outcome: exit 0 → `compiles`; a clean rejection or a compiler panic
     → `pure-ruby (fails)`, bucketed by its stderr message
     (`stdlib_status::reason_bucket` is the existing distiller — factor it out
     and share it).
3. Parallelize like `stdlib-status` (worker pool over a queue; `zeo` built
   once up front so the workers don't race cargo).
4. Report the REAL number — *"N/M pure-Ruby gems actually compile"* — and write
   the per-gem failure reason into `conformance/gem-compat.tsv`, most-common
   bucket first. That TSV becomes the work-list for what to fix next.

## Notes / gotchas

- Only under `--verify` (opt-in): the default stays the fast static
  classification, since the verify pass is ~150+ subprocess compiles.
- The number WILL drop from the resolvability figure — that is the point; it is
  the honest out-of-the-box compile rate.
- A gem's probe pulls its whole `require` graph, so a failure may be a
  transitive dep (a native gem, or a stdlib file zeo lacks), not the gem
  itself. Keep the bucketed reason so those aggregate (e.g. "cannot load such
  file -- rbconfig" will dominate).
- `-I $(ruby -e 'print RbConfig::CONFIG["rubylibdir"]')` should probably be
  added to the probe so a gem that only needs a plain-Ruby stdlib file
  (`shellwords`, `forwardable`, …) resolves it, matching how `stdlib-status`
  already reaches the installed stdlib. Without it the number understates for a
  different reason than it overstates today.
- Shares infrastructure with `stdlib_status.rs` (`ruby_query`, the worker pool,
  `run_with_timeout`, `reason_bucket`, the TSV/MD writer) — factor the common
  bits into a small shared module rather than copying.

## Why deferred

The static classification already answers "which gems is zeo even in a
position to provide," which is what drives the FFI work-list (native gems by
layout). The actual-compile number is more valuable but costs a real
compile-orchestration harness; it is its own focused effort, not a tail-end
addition to the classifier.
