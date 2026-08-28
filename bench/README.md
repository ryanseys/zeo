# The benchmark suite

61 golden-output Ruby programs under `bench/`, each with a committed
`.expected`. The harness is a [criterion](https://lib.rs/crates/criterion)
bench target (`crates/zeo/benches/programs.rs`): each program is compiled
with the release `zeo -o` and the resulting native binary is timed
end-to-end as a subprocess.

Provenance is in [`UPSTREAM.md`](UPSTREAM.md): the programs come from the
`benchmark/` suite of [spinel](https://github.com/matz/spinel), Zeo's
C-emitting predecessor. Many are adaptations of the Computer Language
Benchmarks Game and yjit-bench-style micros.

## How to run it

```console
$ make bench                                     # the whole bank
$ cargo bench -p zeo --bench programs            # the same, spelled out
$ cargo bench -p zeo --bench programs -- 'zeo/bm_fib$'   # one benchmark (regex)
$ ZEO_BENCH_ORACLE=1 ZEO_BENCH_ORACLE_RUBY="$(mise which ruby)" \
    cargo bench -p zeo --bench programs -- 'cruby/'      # also time plain CRuby
$ make pgo    # the SHIPPED config (ZEO_BENCH_DIST=pgo spelled out)
```

Everything after `--` is criterion's own CLI: name filters are regexes over
the benchmark id (`zeo/<name>` or `cruby/<name>`).

The everyday bank measures the release profile. `ZEO_BENCH_DIST=pgo`
instead snapshots the shipped configuration — the full `cargo xtask dist
--pgo` pipeline (instrumented build, training on this corpus, profile-use
rebuild), staged inside the isolated bench target dir. It adds ~15
minutes of setup, so it is for release-boundary measurements, not the dev
loop. A dist-mode bank never rewrites the committed `bench/results.tsv`
(that file is the release-profile diff chain); compare dist banks with
`--save-baseline` + `critcmp`.

### The journal, and finishing an interrupted bank

A full bank with the oracle is 122 benchmarks and roughly 45 minutes, and
`results.tsv` is written ONCE, after the last one. Anything that stops the
run early — a timeout, a Ctrl-C, a closed lid — used to discard every
number already paid for. One run died at 95 of 122 and left nothing.

So every benchmark appends its median to `target/bench/journal.tsv` the
moment it finishes, flushed per row:

```
# group	benchmark	median_secs	key	measured_at_epoch
zeo	bm_fib	0.191361	5aca85d364df168c	1787932076
cruby	bm_fib	0.482375	995ae9a424d54bec	1787932083
```

`key` is a sha256 over the program, its `.expected`, and the identity of
whatever ran it — for `zeo` the snapshot compiler plus `libzeo.a`, for
`cruby` the `ruby -v` string. A row whose key still matches measured what
a re-run would measure; one whose key has changed is stale by
construction, and says so without anybody remembering to check.

`ZEO_BENCH_RESUME=1` then reuses matching rows and re-times only the rest,
which turns a killed bank into a short top-up instead of a restart:

```console
$ ZEO_BENCH_ORACLE=1 ZEO_BENCH_RESUME=1 make bench
```

**Resume is opt-in on purpose.** Quietly mixing sittings is how a bank
starts lying — see the drift numbers in the rules below — so reuse happens
only when asked for, and only for rows whose key still matches.

Only what criterion re-timed in the current run is journalled. Criterion
keeps the last result on disk for every benchmark, including ones a filter
skipped, so the estimate's mtime rather than its existence is what decides
— otherwise a filtered run would stamp the whole corpus with a fresh
timestamp and stale numbers.

### Comparing runs (baselines)

Criterion stores results under `target/criterion/` and compares against
named baselines — no committed data files:

```console
$ cargo bench -p zeo --bench programs -- --save-baseline before
  ... apply the change, rebuild ...
$ cargo bench -p zeo --bench programs -- --baseline before   # report vs "before"
$ critcmp before after                                       # side-by-side table
```

`critcmp` (`cargo install critcmp`) renders any two saved baselines side by
side. To attribute a delta across commits, bench the parent commit from a
`git worktree` and copy its `target/criterion/` baseline dirs over — same
tool on both sides, one comparison.

**A baseline is only comparable on the machine state that recorded it.** The
2026-08-22 bank stopped reproducing on 2026-08-25 — the same source at the
same commit re-timed ~65% slower (geomean) while the prebuilt CRuby binary
moved only ~6%, so the shift was the host toolchain/OS, not the tree. When
every benchmark jumps and code did not change, re-time the baseline-era
commit in a worktree before believing the number. Baselines live in
`target/`, so `cargo clean` deletes them.

## Method

- **The shipped configuration.** Each program is compiled with `zeo -o`, so
  the binary links the RELEASE runtime, statically — what a user would
  actually run, not the `-O0` dynamic build the test harness uses.
- **Correctness first.** Each program's first run must match its
  `.expected` byte for byte (a combined run checks `ruby`'s output too,
  so a stale snapshot fails loudly). Timing a wrong answer is
  meaningless.
- **Flat sampling, 10 samples.** Whole-program subprocess timings are
  criterion `iter_custom` measurements under `SamplingMode::Flat`. Long
  programs exceed the 2 s target time; criterion warns and takes its 10
  samples anyway — that warning is expected.
- **Lazy compilation.** A filtered run compiles only the programs it times.
- **An isolated snapshot.** The harness builds its own `zeo` + `libzeo.a`
  into `target/bench/` once at bench start and times only those artifacts —
  editing code, running tests, or `cargo build` in the ordinary target dir
  while a bank runs cannot touch what is being timed (though heavy parallel
  builds still add scheduler noise to the numbers).
- **Compare against the pinned ruby.** Point `ZEO_BENCH_ORACLE_RUBY` at
  the `mise.toml` ruby (4.0.6); a bare `ruby` off `PATH` answers a
  different question.

## Profiling a benchmark

[`cargo-flamegraph`](https://github.com/flamegraph-rs/flamegraph)
(`cargo install flamegraph`) profiles the compiled program directly — the
`flamegraph` binary wraps any command, not just cargo targets:

```console
$ target/release/zeo -o /tmp/bm_fib bench/bm_fib.rb
$ flamegraph -o fib.svg -- /tmp/bm_fib     # dtrace-based on macOS (may need sudo)
```

The AOT binary keeps its symbol table (the export table is load-bearing for
FFI), so runtime frames resolve by name.

## The compiler-cost instrument

`cargo xtask bench --compile` measures the other side — what a COMPILE
costs and produces (frontend wall time, emitted CLIF lines, peak RSS,
binary size) across a hello-world-to-bundler ladder — and banks into
`compile-baseline.tsv` with `--update-baseline`.

## Results

The numbers live in [`results.tsv`](results.tsv), not here: `group`
(`zeo` or `cruby`), benchmark, and the median seconds from the last full
bank. Every full release-profile bank overwrites the `zeo` rows and
commits the file, so `git log -p bench/results.tsv` is the progress
record — and each bank commit's message carries the full per-bench diff
against both the previous bank and the standing CRuby rows. The `cruby`
rows update only on a COMBINED run (`ZEO_BENCH_ORACLE=1`, which also
times plain CRuby on the same programs), so every Zeo-vs-Ruby ratio
comes from one sitting on one machine.

Read any ratio as a shape, not a portable claim: one laptop, one OS
state. Per-benchmark lever attribution — which planned optimization
serves which loser, and the dated design records for refused designs —
lives in [`docs/ROADMAP.md`](../docs/ROADMAP.md).
