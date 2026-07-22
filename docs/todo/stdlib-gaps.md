# Vendored-stdlib compile gaps (probe log)

Ten pure-Ruby default gems from ruby 4.0.5 are vendored under `gems/`
(see `gems/UPSTREAM.md`). Requiring each in a probe program (2026-07-21)
gives the real remaining blocker per gem -- these are compiler work items,
now surfaced honestly instead of `cannot load such file`:

| gem | status | blocker |
|---|---|---|
| tsort | **works** | -- |
| ostruct | compiles | ex-alias-of-inherited-builtin, fixed (`alias_method :raise!, :raise` + the 3-arg `raise!` shape now work). Startup blockers remain: the bulk `give_access.each { alias_method "#{m}!", m }` loop runs as class-body code and needs a RUNTIME `alias_method` row on class receivers (deferred with the metaprogramming family), and `Warning[:performance]` needs a `Warning` module; attribute access then needs `method_missing` dispatch (plan M8) |
| delegate | blocked | the `alias __raise__ raise` label was the SHALLOW symptom: it sits inside `kernel = ::Kernel.dup` + `kernel.class_eval do ... end` + `include kernel` -- Module#dup, block-form class_eval, and include-of-a-value are runtime metaprogramming (same territory as singleton's remaining blockers); lowering rejects the alias in BLOCK position |
| shellwords | blocked | `class << self` body with `extend`/ivars (lowering handles only defs/constants/include/attr_*/private/alias) |
| English | blocked | `alias $FULL_MATCH $&` -- alias of punctuation globals (lowering accepts only plain `$name` pairs) |
| forwardable | blocked | a multi-assignment splat-target shape lowering rejects (`expected *name as a multi-assignment's splat target`) |
| pp | blocked | `require` in non-top-level position (inside method/conditional) |
| timeout | compiles; partial | ex-invalid-Rust, fixed (duplicate captured-param cell; exception-subclass typing) plus new runtime surface (ThreadGroup/Thread#group/handle_interrupt/private_constant/Process.method). `Timeout.timeout(n) { }` works when the block finishes; actually INTERRUPTING a running block needs `Thread#raise` delivery into a sleeping/blocked coroutine -- lands with the OS-thread + GVL migration (interruptible `kernel_sleep`) |
| prettyprint | **works** | ex-invalid-Rust, fixed: a later param default reading an earlier CAPTURED param (`width = sep.length`) now sees its capture cell (wraps interleave with bindings in parameter order); breakable/group line-breaking oracle-verified |
| singleton | blocked (compiles) | ex-PANIC, now fixed: top-level `if defined?(Ractor)` guards fold at analyze time, extended-module `super` resolves the singleton chain, and Ruby methods named `clone` no longer hijack internal Arc clones. Remaining: `include Singleton` must fire the `Module.included` HOOK at runtime, `extend` on a CLASS receiver must install class methods (runtime_meta::runtime_extend only handles per-object singletons), and class OBJECTS need ivar storage (`@singleton__instance__` lives on the class) -- runtime-redesign territory (hybrid model / MRO fallback) |

Grind order suggestion: the lowering shapes next (class << self extend,
global aliases, splat target, nested require). Re-probe each gem after its
blocker lands; move rows out of this table as they turn green, and delete
the file when empty.

Alias-of-inherited-builtin family DONE (2026-07-21): analyze records
unresolved alias sources as name indirections, codegen substitutes the
`raise`/`fail` family statically (3-arg raise accepted, backtrace arg
evaluated-then-dropped until frame tracking), the registry carries
validated alias rows for dynamic dispatch (typo sources -> NameError at
program start, CRuby's timing), and `Kernel#raise` is a real dispatch row
(closes the `send(:raise, ...)` NoMethodError divergence).

FFI corpus port findings (2026-07-21, from the spinel-intrinsic -> real
ffi gem test port; 25/29 pass): Proc -> C-function-pointer marshaling
unimplemented (parse/ffi.rs resolves `callback` tags to plain `:pointer`);
`:varargs` missing from `ffi_type_of`; binary `Digest#digest` bytes get
UTF-8-transcoded through concat/pack/Base64 (zeo-rt string-encoding bug,
likely affects other binary-data tests); `OpenSSL::Random` implemented in
ext/openssl.rs but never registered in zeo-abi.

# Benchmark-suite gaps (same probe discipline)

Three of the 58 vendored benchmarks fail; `bench/baseline.tsv` records the
other 55. Each is a real bug, not a harness artifact:

| benchmark | failure | blocker |
|---|---|---|
| bm_ao_render | compiler PANIC | nested escaping block capturing its enclosing BLOCK's local (documented spike-scope limit in codegen/call/procs.rs:104 -- but it must become a diagnostic, and the capture shape must land for real programs) |
| bm_linked_list | runtime stack overflow | deep recursion overflows the `may` coroutine's 2MB stack (`coroutine ... has overflowed its stack, size=2097152`); the OS-thread + GVL migration (plan P3, 8MiB stacks) resolves it structurally |
| bm_so_mandelbrot | output mismatch | genuine divergence in the generated program's output (binary PBM differs from the oracle at line 3) -- miscompilation or runtime arithmetic bug; triage by diffing intermediate rows |

Also: `bm_life`'s RUBY-oracle timing leg fails (`ruby bench/bm_life.rb`
exits 1 while the compiled zeo binary matches `.expected`) -- triage
whether the benchmark depends on something environment-specific or the
`.expected` was recorded from a different oracle state.

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
