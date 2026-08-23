# The benchmark suite

58 golden-output Ruby programs under `bench/`, each with a committed
`.expected`. `tools/zeo-dev bench` compiles every one with `zeo -o`, checks its
output, times it, and compares against the banked baseline.

Provenance is in [`UPSTREAM.md`](UPSTREAM.md): the programs come from the
`benchmark/` suite of [spinel](https://github.com/matz/spinel), Zeo's
C-emitting predecessor. Many are adaptations of the Computer Language
Benchmarks Game and yjit-bench-style micros.

## Running it

```console
$ tools/zeo-dev bench                       # all 58
$ tools/zeo-dev bench --filter fib          # substring match on the name
$ tools/zeo-dev bench --runs 5              # best-of-5 instead of best-of-3
$ tools/zeo-dev bench --ruby                # re-time the CRuby oracle too
$ tools/zeo-dev bench --update-baseline     # bank the result
$ tools/zeo-dev bench --update-baseline --resume   # continue an interrupted bank
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

`--update-baseline` rewrites `baseline.tsv` after every completed benchmark
rather than once at the end, so an interrupted run keeps what it finished;
`--resume` then skips those and fills in the rest.

## Results

Measured 2026-08-21 on one Apple-silicon laptop, `--ruby`, so Zeo and CRuby
4.0.6 are timed in the **same run** under the same conditions. Read them as a
shape, not a portable claim.

**Two aggregates, because one would mislead:**

| | geomean |
|---|---|
| all 58 benchmarks | **1.17× faster than CRuby** |
| the 37 where CRuby takes ≥ 0.10 s | **0.71× — slower** |

The gap between those two numbers is process startup. 21 benchmarks finish in
under 100 ms of CRuby time, where Zeo's native binary starts instantly and the
interpreter pays ~30 ms of boot — that is a real advantage of shipping a
binary, but it is not a claim about generated code. The second row is the one
that describes generated code.

Split by outcome: **25 of 58 faster, 33 slower**; on the compute-bound 37,
**8 faster, 29 slower**.

> **These numbers replaced a much better-looking set on 2026-08-21, and the
> reason is worth stating.** Until then the figures here were the **Rust-
> emitting backend's** (1.71× / 1.20×, 42 of 58) — the backend that was
> retired that day. The Cranelift backend that ships now is **+53% geomean
> against that baseline**: it has had two optimization waves and has not had
> the rest. The per-benchmark analysis in the sections below still describes
> the retired backend's profile and is kept only until it is re-measured.

The current spread, compute-bound only:

| winners | | losers | |
|---|---|---|---|
| `range_each` | 3.27× | `rbtree` | 0.25× |
| `so_mandelbrot` | 2.18× | `send_rubyfunc_block` | 0.31× |
| `nested_loop` | 1.35× | `life` | 0.35× |
| `partial_sums` | 1.33× | `getivar_module` | 0.35× |
| `matmul` | 1.27× | `linked_list` | 0.38× |
| `attr_accessor` | 1.27× | `splay` | 0.39× |
| `send_cfunc_block` | 1.24× | `inline` | 0.41× |
| `nqueens` | 1.04× | `so_lists` | 0.46× |

Every loser is a named, unbuilt lever: the typed `InlineIterKind` splices
(`so_lists`, `rbtree`, `splay`, `life`), a `CivarSite` the emitter never emits
(`getivar_module`), a direct compiled-to-compiled call for a statically-known
receiver class (`send_rubyfunc_block`), and the per-call runtime block
(`tak`, `tarai`).

### The 16 that lose, in two groups

They are not one phenomenon, and the distinction matters because the two have
different next levers.

**Object-graph bound** — `rbtree` 0.59×, `structaset` 0.61×, `linked_list`
0.63×, `life` 0.67×, `splay` 0.77×. These build and tear down object graphs, so
their time goes into reference-count traffic and allocation rather than into
dispatch or ivar access. The obvious lever — narrowing `RubyValue` from 24
bytes to 16 — was measured on 2026-08-14 and **refused**: widening the enum by
8 bytes costs only ~1.2% on exactly these programs, so removing 8 would buy
about as little. They are refcount bound, not width bound.

**Builtin-call bound** — `ruby_xor` 0.75×, `inline` 0.83×, `template` 0.84×,
`sudoku` 0.86×, `structaref` 0.87×, `csv_process` 0.88×, `ao_render` 0.91×,
`matmul` 0.96×. This group is newer, and it is a **fidelity cost paid on
purpose**: every builtin dispatch pushes a synthetic C frame so a backtrace
raised inside a builtin shows the same rows CRuby shows. Neutering those frames
entirely was measured at −9.2% on `ruby_xor`, −7.7% on `template` and −5.8% on
`matmul` — which is most of what separates this group from parity.

**Ties, inside the noise** — `json_parse` 0.97×, `io_wordcount` 0.97×,
`getivar_module` 0.98×, `gcbench` 1.00×.

[`docs/ROADMAP.md`](../docs/ROADMAP.md) records both measurements, including
the prototype that showed a pending-label frame register would recover only
0.4% of that 4.7%.

### What moved since the 2026-07-31 run

Comparing like with like is awkward here, and the reason is worth stating:
**the CRuby oracle re-timed ~5.5% faster than in July** (median across 58),
while Zeo's median time was unchanged. So several benchmarks that sat just
above 1.00× crossed below it without Zeo getting slower. Two changes are real
and large, in opposite directions:

- `so_lists` **0.74× → 1.44×** (Zeo −49.6%) and `partial_sums` → 1.68×, from
  the Enumerable driver work and an inline cache for statically-known class
  receivers.
- `template` +19.4%, `matmul` +16.5%, `structaref` +12.7%, `csv_process`
  +11.4%, `sudoku` +11.0% — the builtin-frame group above, plus the cost of
  making every native class subclassable.

The separate figure sometimes quoted — **geomean ≈ −60%** — is Zeo against
Zeo's OWN pre-overhaul baseline across four optimization sessions. It is a real
number and a different claim; it says nothing about CRuby.

| benchmark | Zeo (s) | ruby 4.0.6 (s) | ratio |
|---|---|---|---|
| `ackermann` | 0.205 | 0.317 | 1.55× |
| `ao_render` | 1.790 | 1.628 | 0.91× |
| `attr_accessor` | 0.688 | 0.819 | 1.19× |
| `bigint_fib` | 0.004 | 0.029 | 7.25× |
| `binary_trees` | 0.025 | 0.048 | 1.92× |
| `csv_process` | 0.555 | 0.487 | 0.88× |
| `fannkuch` | 0.008 | 0.033 | 4.12× |
| `fasta` | 0.008 | 0.032 | 4.00× |
| `fib` | 0.290 | 0.404 | 1.39× |
| `gcbench` | 2.052 | 2.054 | 1.00× |
| `getivar` | 0.051 | 0.089 | 1.75× |
| `getivar_module` | 0.653 | 0.643 | 0.98× |
| `huffman` | 0.061 | 0.062 | 1.02× |
| `inline` | 1.107 | 0.922 | 0.83× |
| `io_wordcount` | 0.074 | 0.072 | 0.97× |
| `jekyll_lite` | 0.004 | 0.030 | 7.50× |
| `json_parse` | 0.247 | 0.239 | 0.97× |
| `keyword_args` | 0.079 | 0.148 | 1.87× |
| `life` | 0.761 | 0.512 | 0.67× |
| `linked_list` | 0.335 | 0.212 | 0.63× |
| `loops_times` | 0.456 | 0.572 | 1.25× |
| `mandel_term` | 0.016 | 0.040 | 2.50× |
| `matmul` | 0.316 | 0.302 | 0.96× |
| `micro_lisp` | 0.004 | 0.030 | 7.50× |
| `nbody` | 0.011 | 0.034 | 3.09× |
| `nested_loop` | 0.152 | 0.395 | 2.60× |
| `nqueens` | 0.130 | 0.182 | 1.40× |
| `object_new` | 0.043 | 0.097 | 2.26× |
| `object_new_init` | 0.083 | 0.137 | 1.65× |
| `object_new_no_escape` | 0.096 | 0.193 | 2.01× |
| `partial_sums` | 0.421 | 0.707 | 1.68× |
| `pidigits` | 0.004 | 0.029 | 7.25× |
| `poly_cells` | 0.004 | 0.029 | 7.25× |
| `range_each` | 4.312 | 13.995 | 3.25× |
| `rbtree` | 0.565 | 0.331 | 0.59× |
| `ruby_xor` | 1.228 | 0.922 | 0.75× |
| `send_bmethod` | 0.089 | 0.165 | 1.85× |
| `send_cfunc_block` | 0.414 | 0.747 | 1.80× |
| `send_rubyfunc_block` | 0.278 | 0.492 | 1.77× |
| `setivar` | 0.051 | 0.060 | 1.18× |
| `setivar_object` | 0.051 | 0.060 | 1.18× |
| `setivar_young` | 0.051 | 0.060 | 1.18× |
| `sieve` | 0.413 | 0.416 | 1.01× |
| `sinatra_mini` | 0.004 | 0.029 | 7.25× |
| `so_lists` | 0.179 | 0.257 | 1.44× |
| `so_mandelbrot` | 0.258 | 0.936 | 3.63× |
| `sort_by` | 0.014 | 0.036 | 2.57× |
| `spectral_norm` | 0.029 | 0.055 | 1.90× |
| `splay` | 0.172 | 0.132 | 0.77× |
| `str_concat` | 0.004 | 0.030 | 7.50× |
| `structaref` | 0.185 | 0.161 | 0.87× |
| `structaset` | 0.236 | 0.144 | 0.61× |
| `sudoku` | 0.121 | 0.104 | 0.86× |
| `tak` | 0.269 | 0.372 | 1.38× |
| `tarai` | 0.225 | 0.274 | 1.22× |
| `template` | 0.647 | 0.542 | 0.84× |
| `throw` | 0.146 | 0.172 | 1.18× |
| `wordfreq` | 0.006 | 0.031 | 5.17× |

Ratio is `ruby ÷ zeo`: above 1.00× Zeo is faster, below it CRuby is.
