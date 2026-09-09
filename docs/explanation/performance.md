# Performance

What zeo's generated code costs, measured rather than claimed. The method is
in [Measure performance](../how-to/measure-performance.md); this page is the
numbers and what they mean.

A compiled program starts in **under a millisecond**. `test/bench/` holds 61
programs, each with its
correct output, and the criterion bench harness (`cargo bench`) verifies the
output before it times anything.

Measured 2026-08-25 on one machine, against CRuby 4.0.6 (the Zeo rows from
that day's full run; the CRuby rows from a combined Zeo + Ruby run earlier
the same day — the committed [`test/bench/results.tsv`](../../test/bench/results.tsv) is
the record):

| | geomean |
|---|---|
| all 61 benchmarks | **1.77× — faster than CRuby** |
| the 40 where CRuby takes ≥ 0.10 s | **1.19× — faster** |

The first row is inflated by process startup: about a third of the programs
finish before CRuby's interpreter is done booting, where a native binary
starts instantly. That is a real advantage of shipping a binary, but the second
row is the claim about generated code: on the compute-bound set Zeo wins
22 of 40 (`range_each` 8.0×, `so_mandelbrot` 4.9×, `nested_loop` 4.0×,
`matmul` 3.0×, `sieve` 2.4×). Release builds made with `cargo xtask dist
--pgo` (profile-guided optimization, what the shipped artifacts use) run
another ~10% faster than the release profile these numbers were taken on.

The losses that remain cluster in two shapes, and both are named, ranked
levers rather than mysteries. Programs whose hot receiver the compiler
cannot statically type (`tree_walker` 0.07×, `linked_list` 0.38×,
`rbtree` 0.62× — pointer-chasing object graphs) are waiting on wider type
inference: the typed direct call that took `send_rubyfunc_block` from
0.30× to 1.2× only fires where the receiver's class is proven at
compile time. And `getivar_module` (0.39×) waits on a class-level ivar
cache the emitter does not yet emit. [Measure performance](../how-to/measure-performance.md)
has the method.

## The full table

Sorted fastest-relative-to-Ruby first. Rows marked \* finish within
CRuby's startup time, so they mostly measure a native binary starting
instantly, not generated code.

| benchmark | Zeo (s) | Ruby (s) | vs Ruby |
|---|---|---|---|
| `bigint_fib` \* | 0.003 | 0.031 | **11.2× faster** |
| `pidigits` \* | 0.003 | 0.032 | **11.1× faster** |
| `poly_cells` \* | 0.003 | 0.032 | **11.0× faster** |
| `sinatra_mini` \* | 0.003 | 0.032 | **10.6× faster** |
| `jekyll_lite` \* | 0.003 | 0.032 | **10.3× faster** |
| `str_concat` \* | 0.003 | 0.032 | **9.9× faster** |
| `micro_lisp` \* | 0.003 | 0.032 | **9.4× faster** |
| `range_each` | 1.828 | 14.715 | **8.0× faster** |
| `fannkuch` \* | 0.005 | 0.035 | **7.5× faster** |
| `wordfreq` \* | 0.005 | 0.034 | **6.8× faster** |
| `fasta` \* | 0.005 | 0.035 | **6.4× faster** |
| `nbody` \* | 0.007 | 0.036 | **5.1× faster** |
| `so_mandelbrot` | 0.200 | 0.982 | **4.9× faster** |
| `spectral_norm` \* | 0.014 | 0.059 | **4.2× faster** |
| `nested_loop` | 0.104 | 0.417 | **4.0× faster** |
| `send_cfunc_block` | 0.245 | 0.771 | **3.1× faster** |
| `matmul` | 0.106 | 0.319 | **3.0× faster** |
| `mandel_term` \* | 0.015 | 0.043 | **3.0× faster** |
| `sort_by` \* | 0.013 | 0.038 | **2.9× faster** |
| `keyword_args` | 0.060 | 0.157 | **2.6× faster** |
| `sudoku` | 0.047 | 0.111 | **2.4× faster** |
| `sieve` | 0.186 | 0.439 | **2.4× faster** |
| `send_bmethod` | 0.075 | 0.174 | **2.3× faster** |
| `fib` | 0.192 | 0.425 | **2.2× faster** |
| `nqueens` | 0.093 | 0.192 | **2.1× faster** |
| `tak` | 0.190 | 0.394 | **2.1× faster** |
| `loops_times` | 0.295 | 0.602 | **2.0× faster** |
| `tarai` | 0.156 | 0.290 | **1.9× faster** |
| `ackermann` | 0.195 | 0.334 | **1.7× faster** |
| `getivar` \* | 0.055 | 0.095 | **1.7× faster** |
| `attr_accessor` | 0.529 | 0.863 | **1.6× faster** |
| `partial_sums` | 0.461 | 0.745 | **1.6× faster** |
| `object_new` | 0.066 | 0.104 | **1.6× faster** |
| `binary_trees` \* | 0.034 | 0.050 | **1.5× faster** |
| `huffman` \* | 0.045 | 0.066 | **1.5× faster** |
| `object_new_no_escape` | 0.141 | 0.206 | **1.5× faster** |
| `structaref` | 0.119 | 0.170 | **1.4× faster** |
| `object_new_init` | 0.121 | 0.145 | **1.2× faster** |
| `send_rubyfunc_block` | 0.435 | 0.520 | **1.2× faster** |
| `structaset` | 0.158 | 0.153 | ≈ parity |
| `io_wordcount` \* | 0.081 | 0.077 | 1.1× slower |
| `template` | 0.642 | 0.574 | 1.1× slower |
| `csv_process` | 0.581 | 0.512 | 1.1× slower |
| `inline` | 1.106 | 0.971 | 1.1× slower |
| `throw` | 0.210 | 0.181 | 1.2× slower |
| `life` | 0.627 | 0.537 | 1.2× slower |
| `splay` | 0.167 | 0.141 | 1.2× slower |
| `gcbench` | 2.629 | 2.159 | 1.2× slower |
| `ruby_xor` | 1.177 | 0.953 | 1.2× slower |
| `ao_render` | 2.181 | 1.710 | 1.3× slower |
| `setivar_object` \* | 0.083 | 0.064 | 1.3× slower |
| `json_parse` | 0.332 | 0.252 | 1.3× slower |
| `setivar_young` \* | 0.083 | 0.063 | 1.3× slower |
| `setivar` \* | 0.083 | 0.063 | 1.3× slower |
| `stark_field` | 1.118 | 0.781 | 1.4× slower |
| `rbtree` | 0.564 | 0.349 | 1.6× slower |
| `so_lists` | 0.630 | 0.270 | 2.3× slower |
| `getivar_module` | 1.738 | 0.679 | 2.6× slower |
| `linked_list` | 0.592 | 0.226 | 2.6× slower |
| `tree_walker_frames` | 3.392 | 0.266 | 12.8× slower |
| `tree_walker` | 5.037 | 0.374 | 13.5× slower |
