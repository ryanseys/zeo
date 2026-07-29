# Performance roadmap — next levers (post-overhaul)

**Status (2026-07-28):** the four-session performance overhaul is COMPLETE —
its deferred list is empty, and the cumulative result (bench geomean ≈ −60%,
most benchmarks now beating CRuby) is banked in `bench/baseline.tsv` /
`bench/compile-baseline.tsv` as of commit `f8a7f24f`. This doc is the agreed
NEXT set: seven levers, picked 2026-07-28, ordered by expected value within
each group. Evidence lines cite the banked baselines; anything not yet
root-caused says so.

What's still behind CRuby (banked medians): structaref/structaset 2.1×,
template 2.1×, splay 1.8×, so_lists 1.2×, sieve/sudoku 1.1× — plus
ao_render's 2.9s residual (2.8s ahead of nothing: no ruby ref recorded; the
gap is vs what the float arms left on the table).

## Runtime speed

### 1. Object & ivar fast paths (biggest lever)

**Evidence:** ao_render still 2.9s after the float arms — its ops are
`Vec#+`/`Vec#*` user methods, each allocating a fresh `Vec` object and doing
3 ivar writes through the per-object ivar hash map. structaref/structaset
0.51s vs CRuby 0.24s (Struct accessors dispatch dynamically). splay 1.8×,
gcbench/rbtree allocation-heavy.

**Design sketch:**

- **Ivar slot tables**: the compiler already knows every `@name` a class
  uses (it materializes them); give each class a compile-time slot index and
  store ivars in a fixed slot array instead of a hash map. Dynamically-named
  ivars (`instance_variable_set` of a computed name) fall back to an
  overflow map.
- **Registry-free construction**: for a compile-time-known class with a
  generated struct, `new` should allocate + init slots + call `initialize`
  with no registry probe — audit what the current Path-1 construction still
  pays per allocation (Arc, table init, header).
- **attr_accessor devirtualization**: slot read/write when the receiver's
  class is exact and un-reopened (same `builtin_override`-style guard family
  the collection fast paths use).
- **Struct classes**: compile `Struct.new(:a, :b)` literals to real
  generated slot classes so accessors take the static path.

**Verify:** bench ao_render / splay / structaref / structaset / gcbench /
binary_trees / object_new* / setivar*; ivar-reflection goldens
(`instance_variables` must keep first-write ORDER — slots need an order
record), `instance_variable_get/set` of unknown names, singleton-class
ivars, frozen-object writes, Marshal/inspect corpus.

### 2. String & template pipeline

**Evidence:** template 1.329s vs ruby 0.639s — the largest absolute gap
after ao_render. csv_process 0.583s, io_wordcount 0.394s. NOT yet
root-caused — the first step is a profile of the `-O2` static bm_template
binary.

**Candidates (post-profile):** interpolation segments dispatch `to_s`
dynamically → direct display for un-reopened builtins (the display-reopen
bitset from the send-path work already exists — reuse it); capacity-hinted
buffers for multi-segment interpolation; literal-pattern `gsub`/`split`/`tr`
fast paths via memmem that skip the regex engine entirely; check whether
`String#<<` still round-trips through dynamic dispatch (the collection
fast-path family only covered Array push).

**Verify:** encoding-edge oracle scratches; bench template / csv_process /
io_wordcount / wordfreq; string golden family.

### 3. Iterator inlining wave 3

**Evidence:** waves 1–2 (typed `times`/`each`/`upto`/`step`/`map`/`select`/
`reject`/`sum`/Hash `each`) are proven machinery — analyze-mark
(`inline_iter_sites`), guarded splice, `zeo_rt::iter_inline_ok` runtime
gate. The remaining common kinds still allocate an RProc per call-site
execution and dispatch dynamically.

**Kinds to add:** `inject`/`reduce` (accumulator threading — value-mode like
`sum` but the block computes the next accumulator), `each_with_object`,
`find`/`detect` (early-exit with value), `min_by`/`max_by`,
`each_slice`/`each_cons` (chunked yields), `count`-with-block, and in-place
`map!`/`select!`/`reject!` — the in-place family still snapshots, so this is
also a correctness alignment with the live-view decision.

**Verify:** per-kind oracle scratches (especially mutation-during-iteration
for the in-place forms); enumerable golden family; bench sudoku / nqueens /
life / sort_by.

## Efficiency / infrastructure

### 4. Outline the dynamic numeric match arms

**Evidence:** the float arms cost +5% emitted lines at gem scale (uri +5.1%,
rubygems +5.5%; bm_fib +36% at small-program scale) and the recursion micros
gave back +2–7% (fib 0.324→0.347) — I-cache pressure at -O2 is the suspect.

**Design:** move the Int-Int + Float/mixed arm ladder into per-op
`#[inline]` zeo-rt helpers so each emitted site shrinks to one call + the
`send_value_in` fallback; -O2 static builds inline the ladder back (expect
recursion recovery AND the line shrink). Semantics must stay EXACTLY the
current arms (tower Flo-lane, `*i as f64` promotion) — one shared source
also kills the arm-vs-row drift risk.

**Gate:** the -O0 test corpus builds turn the helpers into real calls —
verify suite wall-clock and a hot golden subset don't regress before
banking; compile-bench lines/bytes are the success metric.

### 5. uri-scale rustc time

**Evidence:** uri frontend is 253ms but rustc -O2 is 54.9s
(compile-baseline.tsv) — compile latency is now entirely rustc's.

**Candidates:** codegen-units bump for the CLI -O2 path (measure bench
geomean first — CU splitting can cost runtime; possibly only for non-`-o`
runs); `#[cold]`/`#[inline(never)]` on registration and `main`; split the
giant top-level fns so LLVM parallelizes. First step: get an honest
phase breakdown on stable (cargo build timings; `-Zself-profile` needs
nightly — method TBD).

**Success metric:** uri rustc under ~30s with no bench geomean regression.

### 6. Test-gate latency

**Evidence:** full gate 252s; the tail is one e2e test
(`metaprog::eval_dynamic_arithmetic_honours_precedence`, 41s) that lazily
builds the eval-vm runtime dylib inside the test; 6 tests flagged slow.

**Fix:** prebuild the eval-vm variant alongside the other runtime combos
(`xtask prebuild-runtimes` already exists — add the combo, or
`ensure_runtime_built` at harness init); re-check the slow set afterward.

**Success:** gate tail test < 10s, total gate ≤ ~220s.

### 7. Emission diet round 2

**Candidates:** hash literals still emit per-entry inserts — batch into one
`zeo_rt::hash_from_pairs(&[...])` (constraints: key/value evaluation order
stays left-to-right, duplicate-key last-wins semantics and the existing
parse warning are unchanged); coalesce consecutive `set_line` calls that
share a line (only true duplicates — backtrace semantics are load-bearing);
remaining match-scaffolding dedupe is mostly covered by lever 4.

**Metric:** compile-bench lines/bytes down with no `-S` semantic diffs
beyond the intended shapes.

## Considered, not scheduled

- **Block & proc call overhead** — send_cfunc_block 1.1× / send_rubyfunc_block
  1.4× faster than ruby are the worst remaining call-path ratios; a leaner
  block-invoke path (skip per-call boxing, direct call for statically-known
  blocks) could close them. Deliberately deferred: it touches the call ABI
  everywhere. Revisit after levers 1–3 land.
- **Mixed Integer↔Float comparison exactness** — pre-existing divergence:
  `num_cmp`'s Flo lane converts via `as f64`, lossy past 2^53
  (`9007199254740993 == 9007199254740992.0` answers true; CRuby compares
  exactly and answers false). A fix must update `num_cmp` AND the one
  inline-arm codegen site together. Conformance work, not perf.
