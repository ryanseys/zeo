# Vendored-stdlib compile gaps (probe log)

Ten pure-Ruby default gems from ruby 4.0.5 are vendored under `gems/`
(see `gems/UPSTREAM.md`). Requiring each in a probe program (2026-07-21)
gives the real remaining blocker per gem -- these are compiler work items,
now surfaced honestly instead of `cannot load such file`:

| gem | status | blocker |
|---|---|---|
| tsort | **works** | -- |
| ostruct | blocked | `alias_method :raise!, :raise` -- alias of an INHERITED builtin (Kernel) on a user class; plus the bulk `instance_methods.each { alias_method ... }` loop; attribute misses then need `method_missing` dispatch (plan M8) |
| delegate | blocked | `alias __raise__ raise` -- same alias-of-inherited-builtin family |
| shellwords | blocked | `class << self` body with `extend`/ivars (lowering handles only defs/constants/include/attr_*/private/alias) |
| English | blocked | `alias $FULL_MATCH $&` -- alias of punctuation globals (lowering accepts only plain `$name` pairs) |
| forwardable | blocked | a multi-assignment splat-target shape lowering rejects (`expected *name as a multi-assignment's splat target`) |
| pp | blocked | `require` in non-top-level position (inside method/conditional) |
| timeout | blocked | generated Rust fails rustc -- codegen bug, triage via the kept temp source |
| prettyprint | blocked | generated Rust fails rustc -- codegen bug, triage via the kept temp source |
| singleton | blocked | compiler PANIC during compile (run with RUST_BACKTRACE=1) -- highest-priority of these: panics are never acceptable diagnostics |

Grind order suggestion: the singleton panic first (compiler bug class),
then the two rustc-failure codegen bugs, then the alias-of-inherited
family (unblocks ostruct + delegate together), then the lowering shapes
(class << self extend, global aliases, splat target, nested require).
Re-probe each gem after its blocker lands; move rows out of this table as
they turn green, and delete the file when empty.

# Benchmark-suite gaps (same probe discipline)

Three of the 58 vendored benchmarks fail; `bench/baseline.tsv` records the
other 55. Each is a real bug, not a harness artifact:

| benchmark | failure | blocker |
|---|---|---|
| bm_ao_render | compiler PANIC | nested escaping block capturing its enclosing BLOCK's local (documented spike-scope limit in codegen/call/procs.rs:104 -- but it must become a diagnostic, and the capture shape must land for real programs) |
| bm_linked_list | runtime stack overflow | deep recursion overflows the `may` coroutine's 2MB stack (`coroutine ... has overflowed its stack, size=2097152`); the OS-thread + GVL migration (plan P3, 8MiB stacks) resolves it structurally |
| bm_so_mandelbrot | output mismatch | genuine divergence in the generated program's output (binary PBM differs from the oracle at line 3) -- miscompilation or runtime arithmetic bug; triage by diffing intermediate rows |

Perf root causes found (2026-07-21):

- bm_loops_times (7min -> 1.04s, FIXED): Poly-typed `Array.new` result +
  untyped `.times` counter forced 32M dynamic sends; both inference gaps
  closed in the compiler.
- bm_huffman (255s, diagnosed): `Array#[]`'s dynamic table row does
  `recv_array!(recv).lock().clone()` -- a WHOLE-Vec snapshot per access
  (array.rs:16), so loop-indexing a growing array is quadratic; profiler
  shows Vec<RubyValue>::clone + array::index dominating. array.rs has ~57
  snapshot-clone sites of this family. Fix: single-Int index locks and
  clones ONE element; `[start, len]`/Range paths clone only the requested
  span; snapshot the whole Vec ONLY where iteration re-enters user code
  (rb_eq / blocks) and the lock cannot be held. Queued behind the
  in-flight convert.rs migration (same file).
