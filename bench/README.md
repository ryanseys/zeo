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

Measured 2026-07-30 on one Apple-silicon laptop, zeo and CRuby 4.0.6 timed in
the **same run** under the same conditions. Read them as a shape, not a
portable claim.

**Two aggregates, because one would mislead:**

| | geomean |
|---|---|
| all 58 benchmarks | **1.29× faster than CRuby** |
| the 37 where CRuby takes ≥ 0.10 s | **0.86×** — i.e. ~16% *slower* |

The gap between those two numbers is process startup. 13 benchmarks finish in
under 50 ms of CRuby time, where zeo's native binary starts instantly and the
interpreter pays ~35 ms of boot — that is a real advantage of shipping a
binary, but it is not a claim about generated code. On compute-bound work zeo
currently trails CRuby by a little, and the object/ivar and string levers in
[`docs/TODO.md`](../docs/TODO.md) are aimed squarely at that.

Split by outcome: **32 of 58 faster, 26 slower.**

Worst ratios, all of them known and tracked: `io_wordcount` 0.20×,
`structaset` 0.30×, `structaref` 0.34×, `life` 0.40×, `rbtree` 0.41×,
`template` 0.43×. The Struct pair and `template` are named levers; the IO one
is not yet root-caused.

The separate figure sometimes quoted — **geomean ≈ −60%** — is zeo against
zeo's OWN pre-overhaul baseline across four optimization sessions. It is a real
number and a different claim; it says nothing about CRuby.

| benchmark | zeo (s) | ruby 4.0.6 (s) | ratio |
|---|---|---|---|
| `ackermann` | 0.355 | 0.342 | 0.96× |
| `ao_render` | 2.978 | 1.731 | 0.58× |
| `attr_accessor` | 1.687 | 0.854 | 0.51× |
| `bigint_fib` | 0.004 | 0.036 | 9.00× |
| `binary_trees` | 0.032 | 0.054 | 1.69× |
| `csv_process` | 0.597 | 0.514 | 0.86× |
| `fannkuch` | 0.009 | 0.045 | 5.00× |
| `fasta` | 0.010 | 0.039 | 3.90× |
| `fib` | 0.359 | 0.424 | 1.18× |
| `gcbench` | 2.524 | 2.208 | 0.87× |
| `getivar` | 0.113 | 0.097 | 0.86× |
| `getivar_module` | 1.376 | 0.675 | 0.49× |
| `huffman` | 0.090 | 0.072 | 0.80× |
| `inline` | 1.731 | 0.966 | 0.56× |
| `io_wordcount` | 0.400 | 0.082 | 0.20× |
| `jekyll_lite` | 0.005 | 0.038 | 7.60× |
| `json_parse` | 0.576 | 0.256 | 0.44× |
| `keyword_args` | 0.106 | 0.163 | 1.54× |
| `life` | 1.394 | 0.554 | 0.40× |
| `linked_list` | 0.425 | 0.229 | 0.54× |
| `loops_times` | 0.568 | 0.600 | 1.06× |
| `mandel_term` | 0.019 | 0.049 | 2.58× |
| `matmul` | 0.361 | 0.325 | 0.90× |
| `micro_lisp` | 0.004 | 0.036 | 9.00× |
| `nbody` | 0.018 | 0.041 | 2.28× |
| `nested_loop` | 0.189 | 0.417 | 2.21× |
| `nqueens` | 0.166 | 0.196 | 1.18× |
| `object_new` | 0.074 | 0.110 | 1.49× |
| `object_new_init` | 0.103 | 0.153 | 1.49× |
| `object_new_no_escape` | 0.167 | 0.212 | 1.27× |
| `partial_sums` | 0.501 | 0.733 | 1.46× |
| `pidigits` | 0.004 | 0.038 | 9.50× |
| `poly_cells` | 0.004 | 0.035 | 8.75× |
| `range_each` | 4.933 | 14.666 | 2.97× |
| `rbtree` | 0.856 | 0.354 | 0.41× |
| `ruby_xor` | 1.631 | 0.953 | 0.58× |
| `send_bmethod` | 0.129 | 0.175 | 1.36× |
| `send_cfunc_block` | 0.645 | 0.759 | 1.18× |
| `send_rubyfunc_block` | 0.503 | 0.519 | 1.03× |
| `setivar` | 0.109 | 0.068 | 0.62× |
| `setivar_object` | 0.107 | 0.068 | 0.64× |
| `setivar_young` | 0.108 | 0.067 | 0.62× |
| `sieve` | 0.522 | 0.438 | 0.84× |
| `sinatra_mini` | 0.004 | 0.037 | 9.25× |
| `so_lists` | 0.366 | 0.271 | 0.74× |
| `so_mandelbrot` | 0.318 | 0.983 | 3.09× |
| `sort_by` | 0.017 | 0.044 | 2.59× |
| `spectral_norm` | 0.033 | 0.062 | 1.88× |
| `splay` | 0.331 | 0.145 | 0.44× |
| `str_concat` | 0.005 | 0.037 | 7.40× |
| `structaref` | 0.518 | 0.175 | 0.34× |
| `structaset` | 0.530 | 0.159 | 0.30× |
| `sudoku` | 0.148 | 0.113 | 0.76× |
| `tak` | 0.345 | 0.394 | 1.14× |
| `tarai` | 0.288 | 0.294 | 1.02× |
| `template` | 1.327 | 0.568 | 0.43× |
| `throw` | 0.181 | 0.184 | 1.02× |
| `wordfreq` | 0.006 | 0.035 | 5.83× |

Ratio is `ruby ÷ zeo`: above 1.00× zeo is faster, below it CRuby is.
