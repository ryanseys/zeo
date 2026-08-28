# The golden corpus

Every `.rb` here is compiled by zeo, run, and diffed against a committed
`.rb.expected` recorded from **ruby 4.0.6**. One nextest case per file,
through `crates/zeo/tests/goldens.rs`.

**A golden's contract is its DIRECTORY**, never a per-file marker. The
directories are a 2×2: what a case costs decides which pair it joins, and
whether it works yet decides which half.

| | ordinary bounds | whole require graph |
|---|---|---|
| **must match ruby** | `tests/` | `tests/milestones/`, `tests/gemtests/` |
| **XFAIL, tracked** | `tests/gaps/` | `tests/milestones/pending/` |

So `tests/gaps/` is the XFAIL tier of `tests/`, and `milestones/pending/` is
the XFAIL tier of `milestones/`. Both run in `Mode::Xfail`: a case that still
diverges PASSES, and one that starts matching ruby FAILS with a promote
message rather than going quietly green.

The right-hand column exists because those cases splice a whole library's
require graph — minutes of compile and gigabytes of arena, past the ordinary
60s / 512 MiB bounds. The harness raises both, and the default nextest
profile opts them out so the dev loop stays fast (`-P full` runs them).

A case cannot simply move between columns: the bounds AND the load roots come
from the directory. `pending/rspec_reports_a_failure.rb` in `tests/gaps/`
would fail on `require "rspec/autorun"` instead of on what it is tracking,
and would keep passing after that was fixed.

Three more directories carry their own rule, all in the left column:

- `tests/divergences/` — zeo answers differently on purpose, so the golden
  records ZEO's output. See that directory's README.
- `tests/macos/` — macOS-only; the output is platform-specific.
- `tests/jit/` — JIT-only; the program needs the compiler and itself in one
  process.

A SUBDIRECTORY anywhere else holds fixtures — files another program requires
— not tests. Every pattern matches one level.

`tests/bench/` is compile-side input for the perf bank and has no goldens.

Re-record with `tools/zeo-dev bless <filter>`, which is the only golden
writer. The filter is required and is a substring of the case name,
`<corpus>::<path>` — `bless gap::`, `bless spinel::yield_`.
