# The benchmark suite

58 golden-output Ruby programs under `bench/`, each with a committed
`.expected`. `cargo xtask bench` compiles every one with `zeo -o`, checks its
output, times it, and compares against the banked baseline.

Provenance is in [`UPSTREAM.md`](UPSTREAM.md): the programs come from the
`benchmark/` suite of [spinel](https://github.com/matz/spinel), zeo's
C-emitting predecessor. Many are adaptations of the Computer Language
Benchmarks Game and yjit-bench-style micros.

## Running it

```console
$ cargo run -p xtask -- bench                       # all 58
$ cargo run -p xtask -- bench --filter fib          # substring match on the name
$ cargo run -p xtask -- bench --runs 5              # best-of-5 instead of best-of-3
$ cargo run -p xtask -- bench --ruby                # re-time the CRuby oracle too
$ cargo run -p xtask -- bench --update-baseline     # bank the result
$ cargo run -p xtask -- bench --update-baseline --resume   # continue an interrupted bank
```

## Method

- **The shipped configuration.** Each program is compiled with `zeo -o`, so the
  binary links the RELEASE runtime, statically — what a user would actually
  run, not the `-O0` dynamic build the test harness uses.
- **Correctness first.** Before any timing, the binary's stdout must match its
  `.expected` byte for byte. A mismatch fails the run outright: timing a wrong
  answer is meaningless.
- **Best-of-N wall clock.** `--runs N` (default 3) executions, keeping the
  MINIMUM. Noise on a quiet machine is strictly additive, so the minimum is the
  cleanest estimator.
- **A repeat budget.** Every benchmark gets its first, correctness-gated run;
  repeats happen only while total time on THAT benchmark is under 30 s
  (`REPEAT_BUDGET_SECS`). A minutes-long benchmark is therefore timed once —
  its own length already averages out scheduler noise.
- **The oracle is the pinned ruby.** `--ruby` times the `mise.toml` ruby
  (4.0.6), resolved exactly as the golden harness resolves it — not a bare
  `ruby` off `PATH`, which is whatever version the shell happens to offer.
  The oracle's own output is checked against `.expected` too, so a stale
  snapshot surfaces as a failure instead of a bogus ratio.
- **Geometric mean of per-benchmark ratios** for the aggregate — the one
  average that treats a 2× win on a fast benchmark and a 2× loss on a slow one
  symmetrically.

## The data files

| file | what it records |
|---|---|
| `baseline.tsv` | the committed regression reference (`name`, `secs`); written only by `--update-baseline`, so its diff is the reviewable record of every accepted shift |
| `ruby.tsv` | the CRuby 4.0.6 oracle's time per benchmark; re-timed only under `--ruby` and reused by every report's `vs ruby` column |
| `history.tsv` | the last 5 timings per benchmark, appended by every run; the report's `med5` column is their median, showing drift without touching the baseline |
| `compile-baseline.tsv` | the compile-side twin, written by `cargo xtask compile-bench`: front-end ms, emitted Rust lines and bytes, rustc ms, binary bytes, and rustc warning count (target 0) over a fixed 10-program set |

`--update-baseline` rewrites `baseline.tsv` after every completed benchmark
rather than once at the end, so an interrupted run keeps what it finished;
`--resume` then skips those and fills in the rest.

## Results

Measured 2026-07-31 on one Apple-silicon laptop, zeo and CRuby 4.0.6 timed in
the **same run** under the same conditions. Read them as a shape, not a
portable claim.

**Two aggregates, because one would mislead:**

| | geomean |
|---|---|
| all 58 benchmarks | **1.78× faster than CRuby** |
| the 37 where CRuby takes ≥ 0.10 s | **1.23× faster** |

The gap between those two numbers is process startup. 13 benchmarks finish in
under 50 ms of CRuby time, where zeo's native binary starts instantly and the
interpreter pays ~35 ms of boot — that is a real advantage of shipping a
binary, but it is not a claim about generated code. The second row is the one
that describes generated code.

Split by outcome: **47 of 58 faster, 11 slower.**

The 11 that still lose: `life` 0.52×, `rbtree` 0.66×, `linked_list` 0.70×,
`splay` 0.74×, `so_lists` 0.74×, `structaset` 0.75×, `inline` 0.83×,
`getivar_module` 0.91×, `ao_render` 0.93×, `ruby_xor` 0.95×, `attr_accessor`
0.99×. These build and tear down object graphs, so their time goes into
reference-count traffic and allocation rather than into dispatch or ivar
access, which the completed work already addressed.
[`docs/TODO.md`](../docs/TODO.md) records what is measured about them and what
the next lever would be.

The separate figure sometimes quoted — **geomean ≈ −60%** — is zeo against
zeo's OWN pre-overhaul baseline across four optimization sessions. It is a real
number and a different claim; it says nothing about CRuby.

| benchmark | zeo (s) | ruby 4.0.6 (s) | ratio |
|---|---|---|---|
| `ackermann` | 0.211 | 0.329 | 1.56× |
| `ao_render` | 1.817 | 1.683 | 0.93× |
| `attr_accessor` | 0.856 | 0.846 | 0.99× |
| `bigint_fib` | 0.004 | 0.035 | 8.75× |
| `binary_trees` | 0.026 | 0.052 | 2.00× |
| `csv_process` | 0.499 | 0.503 | 1.01× |
| `fannkuch` | 0.009 | 0.038 | 4.22× |
| `fasta` | 0.007 | 0.037 | 5.29× |
| `fib` | 0.262 | 0.419 | 1.60× |
| `gcbench` | 1.917 | 2.178 | 1.14× |
| `getivar` | 0.058 | 0.096 | 1.66× |
| `getivar_module` | 0.746 | 0.677 | 0.91× |
| `huffman` | 0.057 | 0.070 | 1.23× |
| `inline` | 1.144 | 0.954 | 0.83× |
| `io_wordcount` | 0.072 | 0.086 | 1.19× |
| `jekyll_lite` | 0.004 | 0.035 | 8.75× |
| `json_parse` | 0.234 | 0.258 | 1.10× |
| `keyword_args` | 0.084 | 0.161 | 1.92× |
| `life` | 1.043 | 0.546 | 0.52× |
| `linked_list` | 0.339 | 0.237 | 0.70× |
| `loops_times` | 0.547 | 0.597 | 1.09× |
| `mandel_term` | 0.018 | 0.045 | 2.50× |
| `matmul` | 0.272 | 0.314 | 1.15× |
| `micro_lisp` | 0.004 | 0.033 | 8.25× |
| `nbody` | 0.010 | 0.038 | 3.80× |
| `nested_loop` | 0.163 | 0.412 | 2.53× |
| `nqueens` | 0.160 | 0.192 | 1.20× |
| `object_new` | 0.047 | 0.103 | 2.19× |
| `object_new_init` | 0.084 | 0.145 | 1.73× |
| `object_new_no_escape` | 0.101 | 0.204 | 2.02× |
| `partial_sums` | 0.442 | 0.728 | 1.65× |
| `pidigits` | 0.004 | 0.034 | 8.50× |
| `poly_cells` | 0.004 | 0.034 | 8.50× |
| `range_each` | 4.573 | 14.230 | 3.11× |
| `rbtree` | 0.520 | 0.343 | 0.66× |
| `ruby_xor` | 0.979 | 0.934 | 0.95× |
| `send_bmethod` | 0.090 | 0.175 | 1.94× |
| `send_cfunc_block` | 0.622 | 0.752 | 1.21× |
| `send_rubyfunc_block` | 0.303 | 0.508 | 1.68× |
| `setivar` | 0.060 | 0.064 | 1.07× |
| `setivar_object` | 0.060 | 0.065 | 1.08× |
| `setivar_young` | 0.059 | 0.064 | 1.08× |
| `sieve` | 0.403 | 0.436 | 1.08× |
| `sinatra_mini` | 0.004 | 0.033 | 8.25× |
| `so_lists` | 0.355 | 0.263 | 0.74× |
| `so_mandelbrot` | 0.285 | 0.965 | 3.39× |
| `sort_by` | 0.016 | 0.041 | 2.56× |
| `spectral_norm` | 0.028 | 0.061 | 2.18× |
| `splay` | 0.192 | 0.142 | 0.74× |
| `str_concat` | 0.004 | 0.035 | 8.75× |
| `structaref` | 0.165 | 0.170 | 1.03× |
| `structaset` | 0.204 | 0.153 | 0.75× |
| `sudoku` | 0.109 | 0.110 | 1.01× |
| `tak` | 0.268 | 0.387 | 1.44× |
| `tarai` | 0.238 | 0.286 | 1.20× |
| `template` | 0.542 | 0.562 | 1.04× |
| `throw` | 0.149 | 0.179 | 1.20× |
| `wordfreq` | 0.006 | 0.037 | 6.17× |

Ratio is `ruby ÷ zeo`: above 1.00× zeo is faster, below it CRuby is.
