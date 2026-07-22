# Changelog

All notable changes are documented here, following
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/) once releases begin.

## [Unreleased]

Pre-release development: the open-source-readiness and CRuby-compatibility
push.

**Threads are real OS threads, truly parallel by default.** The former
cooperative coroutine scheduler (`may`) is gone: every `Thread.new` is an
8MiB OS thread, busy loops are killable/raisable (interruption checkpoints
at loop back-edges, method prologues, and block exits), `sleep` — including
`sleep` with no duration — is genuinely interruptible, and the main thread
is a first-class `Thread#raise` target. `ZEO_GVL=1` opts into CRuby-style
serialized scheduling (a FIFO global lock with 100ms timer preemption).
The retired `--no-gvl` and `ZEO_THREADS` scheduler knobs are now silently
ignored.

Other highlights so far: dual MIT/Apache-2.0 licensing, edition 2024
workspace with enforced lints, OS-CSPRNG SecureRandom, vendored
conformance corpus + benchmark suite with self-contained CI, typed
error-constructor macros, the rb_convert_type implicit-conversion
protocol (duck-typed arguments, oracle-exact messages) now consumed by
~120 formerly open-coded builtin argument sites (to_int/to_str/to_ary/
to_hash ducks, NUM2LONG Float truncation, and oracle-corrected error
shapes across Array/String/Hash/IO/Time/File and the ext modules),
Enumerable/Comparable on real method tables, and ten vendored pure-Ruby
stdlib gems.
