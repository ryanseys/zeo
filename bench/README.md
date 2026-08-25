# The benchmark suite

61 golden-output Ruby programs under `bench/`, each with a committed
`.expected`. `tools/zeo-dev bench` compiles every one with `zeo -o`, checks
its output, times it, and compares against the banked baseline.

Provenance is in [`UPSTREAM.md`](UPSTREAM.md): the programs come from the
`benchmark/` suite of [spinel](https://github.com/matz/spinel), Zeo's
C-emitting predecessor. Many are adaptations of the Computer Language
Benchmarks Game and yjit-bench-style micros.

## How to run it

```console
$ tools/zeo-dev bench                       # all 61
$ tools/zeo-dev bench --filter fib          # substring match on the name
$ tools/zeo-dev bench --runs 5              # best-of-5 instead of best-of-3
$ tools/zeo-dev bench --ruby                # re-time the CRuby oracle too
$ tools/zeo-dev bench --update-baseline     # bank the result
$ tools/zeo-dev bench --update-baseline --resume   # continue an interrupted bank
```

Never combine `--filter` with `--update-baseline`: the bank rewrites
`baseline.tsv` to only the filtered rows.

## Method

- **The shipped configuration.** Each program is compiled with `zeo -o`, so
  the binary links the RELEASE runtime, statically — what a user would
  actually run, not the `-O0` dynamic build the test harness uses.
- **Correctness first.** Before any timing, the binary's stdout must match
  its `.expected` byte for byte. A mismatch fails the run outright: timing a
  wrong answer is meaningless.
- **Best-of-N wall clock.** `--runs N` (default 3) executions, keeping the
  MINIMUM. Noise on a quiet machine is strictly additive, so the minimum is
  the cleanest estimator.
- **A repeat budget.** Every benchmark gets its first, correctness-gated
  run; repeats happen only while total time on THAT benchmark is under 30 s
  (`REPEAT_BUDGET_SECS`). A minutes-long benchmark is therefore timed once —
  its own length already averages out scheduler noise.
- **The oracle is the pinned ruby.** `--ruby` times the `mise.toml` ruby
  (4.0.6), resolved exactly as the golden harness resolves it — not a bare
  `ruby` off `PATH`. The oracle's own output is checked against `.expected`
  too, so a stale snapshot surfaces as a failure instead of a bogus ratio.
- **Geometric mean of per-benchmark ratios** for the aggregate — the one
  average that treats a 2× win on a fast benchmark and a 2× loss on a slow
  one symmetrically.

## The data files

| file | what it records |
|---|---|
| `baseline.tsv` | the committed regression reference (`name`, `secs`); written only by `--update-baseline`, so its diff is the reviewable record of every accepted shift |
| `ruby.tsv` | the CRuby 4.0.6 oracle's time per benchmark; re-timed only under `--ruby` and reused by every report's `vs ruby` column |
| `history.tsv` | the last 5 timings per benchmark, appended by every run; the report's `med5` column is their median, showing drift without touching the baseline |

`--update-baseline` rewrites `baseline.tsv` after every completed benchmark
rather than once at the end, so an interrupted run keeps what it finished;
`--resume` then skips those and fills in the rest.

**A baseline is only comparable on the machine state that banked it.** The
2026-08-22 bank stopped reproducing on 2026-08-25 — the same source at the
same commit re-timed ~65% slower (geomean) while the prebuilt CRuby binary
moved only ~6%, so the shift was the host toolchain/OS, not the tree. When
the geomean-vs-baseline column jumps and code did not change, re-time the
BASELINE-ERA commit in a worktree before believing the number.

## Results

Measured 2026-08-25 on one Apple-silicon laptop, best-of-5, `--ruby`, so Zeo
and CRuby 4.0.6 are timed in the **same run** under the same conditions.
Read them as a shape, not a portable claim.

**Two aggregates, because one would mislead:**

| | geomean |
|---|---|
| all 61 benchmarks | **1.01× — parity with CRuby** |
| the 40 where CRuby takes ≥ 0.10 s | **0.62× — slower** |

The gap between the two rows is process startup. 21 benchmarks finish in
under 100 ms of CRuby time, where Zeo's native binary starts instantly and
the interpreter pays ~30 ms of boot — a real advantage of shipping a binary,
but not a claim about generated code. The second row describes generated
code.

Split by outcome: **22 of 61 faster, 38 slower**; on the compute-bound 40,
**7 faster, 33 slower**.

The spread, compute-bound only:

| winners | | losers | |
|---|---|---|---|
| `range_each` | 3.28× | `tree_walker` | 0.13× |
| `so_mandelbrot` | 2.16× | `tree_walker_frames` | 0.15× |
| `nested_loop` | 1.35× | `rbtree` | 0.24× |
| `partial_sums` | 1.27× | `send_rubyfunc_block` | 0.30× |
| `attr_accessor` | 1.26× | `life` | 0.31× |
| `send_cfunc_block` | 1.24× | `linked_list` | 0.34× |
| `matmul` | 1.18× | `getivar_module` | 0.35× |
| | | `splay` | 0.38× |

Every loser maps to a named, unbuilt lever: the typed `InlineIterKind`
splices (`so_lists`, `rbtree`, `splay`, `life`, the two `tree_walker`s), a
`CivarSite` the emitter never emits (`getivar_module`), a direct
compiled-to-compiled call for a statically-known receiver class
(`send_rubyfunc_block`), and the per-call runtime block (`tak`, `tarai`).
The ranked lever list lives in the perf backlog; per decision, perf work is
its own pass, not part of correctness or refactor sweeps.

Two dated design records still govern this profile:

- **Object-graph-bound losers are refcount bound, not width bound.**
  Narrowing `RubyValue` from 24 bytes to 16 was measured on 2026-08-14 and
  refused: widening the enum by 8 bytes cost only ~1.2% on exactly these
  programs, so removing 8 would buy about as little.
- **Builtin-call-bound losers pay a fidelity cost on purpose.** Every
  builtin dispatch pushes a synthetic C frame so a backtrace raised inside a
  builtin shows the same rows CRuby shows. Neutering those frames entirely
  measured −9.2% on `ruby_xor`, −7.7% on `template`, −5.8% on `matmul` —
  most of what separates that group from parity.

### The full table

Ratio is `ruby ÷ zeo`: above 1.00× Zeo is faster, below it CRuby is.

| benchmark | Zeo (s) | ruby 4.0.6 (s) | ratio |
|---|---|---|---|
| `ackermann` | 0.491 | 0.333 | 0.68× |
| `ao_render` | 2.886 | 1.691 | 0.59× |
| `attr_accessor` | 0.675 | 0.851 | 1.26× |
| `bigint_fib` | 0.004 | 0.033 | 8.25× |
| `binary_trees` | 0.051 | 0.051 | 1.00× |
| `csv_process` | 0.635 | 0.506 | 0.80× |
| `fannkuch` | 0.010 | 0.036 | 3.60× |
| `fasta` | 0.009 | 0.035 | 3.89× |
| `fib` | 0.614 | 0.424 | 0.69× |
| `gcbench` | 4.225 | 2.128 | 0.50× |
| `getivar` | 0.071 | 0.095 | 1.34× |
| `getivar_module` | 1.905 | 0.673 | 0.35× |
| `huffman` | 0.076 | 0.066 | 0.87× |
| `inline` | 2.319 | 0.957 | 0.41× |
| `io_wordcount` | 0.089 | 0.076 | 0.85× |
| `jekyll_lite` | 0.005 | 0.032 | 6.40× |
| `json_parse` | 0.407 | 0.250 | 0.61× |
| `keyword_args` | 0.186 | 0.156 | 0.84× |
| `life` | 1.705 | 0.536 | 0.31× |
| `linked_list` | 0.659 | 0.222 | 0.34× |
| `loops_times` | 0.944 | 0.596 | 0.63× |
| `mandel_term` | 0.019 | 0.043 | 2.26× |
| `matmul` | 0.267 | 0.316 | 1.18× |
| `micro_lisp` | 0.005 | 0.032 | 6.40× |
| `nbody` | 0.011 | 0.037 | 3.36× |
| `nested_loop` | 0.307 | 0.414 | 1.35× |
| `nqueens` | 0.195 | 0.191 | 0.98× |
| `object_new` | 0.132 | 0.102 | 0.77× |
| `object_new_init` | 0.238 | 0.143 | 0.60× |
| `object_new_no_escape` | 0.338 | 0.201 | 0.59× |
| `partial_sums` | 0.581 | 0.736 | 1.27× |
| `pidigits` | 0.005 | 0.034 | 6.80× |
| `poly_cells` | 0.005 | 0.034 | 6.80× |
| `range_each` | 4.362 | 14.294 | 3.28× |
| `rbtree` | 1.447 | 0.346 | 0.24× |
| `ruby_xor` | 1.264 | 0.948 | 0.75× |
| `send_bmethod` | 0.277 | 0.171 | 0.62× |
| `send_cfunc_block` | 0.615 | 0.762 | 1.24× |
| `send_rubyfunc_block` | 1.697 | 0.510 | 0.30× |
| `setivar` | 0.100 | 0.063 | 0.63× |
| `setivar_object` | 0.100 | 0.063 | 0.63× |
| `setivar_young` | 0.098 | 0.063 | 0.64× |
| `sieve` | 0.618 | 0.436 | 0.71× |
| `sinatra_mini` | 0.004 | 0.033 | 8.25× |
| `so_lists` | 0.635 | 0.263 | 0.41× |
| `so_mandelbrot` | 0.452 | 0.975 | 2.16× |
| `sort_by` | 0.018 | 0.039 | 2.17× |
| `spectral_norm` | 0.032 | 0.058 | 1.81× |
| `splay` | 0.365 | 0.139 | 0.38× |
| `stark_field` | 1.961 | 0.771 | 0.39× |
| `str_concat` | 0.005 | 0.033 | 6.60× |
| `structaref` | 0.173 | 0.170 | 0.98× |
| `structaset` | 0.280 | 0.151 | 0.54× |
| `sudoku` | 0.117 | 0.109 | 0.93× |
| `tak` | 0.711 | 0.390 | 0.55× |
| `tarai` | 0.564 | 0.287 | 0.51× |
| `template` | 0.744 | 0.566 | 0.76× |
| `throw` | 0.398 | 0.179 | 0.45× |
| `tree_walker` | 10.364 | 1.351 | 0.13× |
| `tree_walker_frames` | 1.712 | 0.261 | 0.15× |
| `wordfreq` | 0.007 | 0.034 | 4.86× |
