# Run the tests

```console
$ cargo nextest run
```

That is the dev loop and the gate for the corpus both: every program under
`test/` compiles and runs once, and is held to the answer recorded in its own
file.

## The two profiles

| Profile | What it adds |
|---|---|
| `cargo nextest run` | the whole corpus, plus the api and checks suites |
| `cargo nextest run -P full` | the tests that are slow one at a time: the whole require graphs under `milestones/`, the benchmark bank, the binary-size check, the bundled-stdlib compile, and the two that need a target directory of their own |

`-P full` is not a second pass over the corpus. One run of the corpus is the
corpus gate.

## One case, or a few

Filters take the case name nextest reports, `<suite>::<path>`:

```console
$ cargo nextest run -E 'test(core::string/upcase.rb)'
$ cargo nextest run -E 'test(/^stdlib::json/)'
$ cargo nextest run -p zeo --test corpus
$ cargo nextest run -p zeo --test suite
```

Two test binaries carry everything: `corpus` runs the `.rb` programs, and
`suite` holds `checks::` (in-process invariants: snapshots, hygiene, the
class surface) and `api::` (the tests that drive the CLI or the library in
more than one step).

## The tests that build a real gem

Three tiers of test build a real third-party gem from its own source: the
C-gem sweep, the bundler-parity pair, and the packaged-gem shipping test.
They are `#[ignore]`d out of both profiles, and one verb runs them:

```console
$ cargo nextest run --run-ignored all -E 'test(api::capi_sweep::)'
$ cargo nextest run --run-ignored all -E 'test(=api::capi_sweep::prism)'
```

**Nothing runs it for you.** They cost more than half the suite's wall clock
and hold its critical path, and what they prove -- that a gem still builds --
does not change between edits to the compiler the way a corpus program's
answer does. The trade is the one `test/gaps/README.md` also records: a gem
that stops building is found when someone next runs this, not the day it
breaks.

The cheap half stays in the ordinary suite: that `XFAIL.json` names only
swept gems, and that every locked C gem is swept or excluded with a reason.
Those are file reads, and a record nothing checks is worse than none.
A different question -- can zeo compile some gem that is not in the lock at
all? -- has its own verb, which writes nothing and takes one row of TSV per
gem:

```console
$ cargo xtask test-gem rake rack thor
$ cargo xtask test-gem nokogiri --aot --no-clean
```

## What a run costs

Nothing on disk. Every corpus child compiles in memory (`ZEO_CACHE=0`) and
writes nothing, so adding tests costs time and not space. The handful of
programs that link a real binary do it under `target/`, and on Linux under
`/dev/shm`.

## Everything else CI runs

```console
$ cargo xtask check
```

Clippy, `cargo deny`, `cargo machete`, the four C-header checks, the
doctests, and the feature-set builds — in order, each reported, none stopping
the rest. It builds into `target/ci-features`, so a suite run afterwards
still finds the ordinary `zeo`.
