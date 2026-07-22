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
UTF-8-transcoded through concat/pack/Base64 (the CONCAT and OUTPUT legs
are fixed -- `StrBuf::push_buf` byte concat + raw-byte print family; the
pack/Base64/format legs still funnel through lossy text and need the same
treatment -- re-probe the digest tests when touching them);
`OpenSSL::Random` implemented in ext/openssl.rs but never registered in
zeo-abi.

Same lossy family, found while testing the single-byte encoding rows
(2026-07-21): a string LITERAL with raw high `\xNN` escapes is not
byte-faithful through lowering -- `"caf\xE9"` reaches the runtime as the
UTF-8 bytes for `é` (Latin-1 promotion in the literal path), where CRuby
keeps the single raw byte 0xE9 (UTF-8-tagged, `valid_encoding?` false).
The dumped-string `"...".force_encoding("ENC")` recognizer already
byte-round-trips its narrow shape; the general literal path needs the
same treatment (HIR literal repr carrying bytes, not String). Encoding
e2e tests construct bytes via `chr` until then.

Compiler PANIC found while testing the frozen-class guards (2026-07-21):
`Foo.define_singleton_method(:name) { ... }` with a CLASS receiver and a
LITERAL symbol, in any non-statement position (inside a block, inside a
top-level `begin`), dies with "unexpected top-level-only node in
expression position" (codegen/expr.rs) -- the literal form desugars to a
`def self.name` node that only statement position accepts. Plain
top-level statement position works, an OBJECT receiver works everywhere,
and a COMPUTED name (`define_singleton_method(sym_var)`) works everywhere
(it rides the runtime_meta path). Fix direction: desugar only in
statement position, else emit the runtime call.

Backtrace-frame divergences (2026-07-22, from the frame-tracking pass;
everything else in the 20-case oracle battery matches ruby 4.0.5
verbatim):

- **No C-method frames.** CRuby shows a frame for most (not all) C
  methods -- `Array#each` between a block and its caller, `Integer#/` at
  a division's line -- attributed to the CALLER's file:line. zeo's
  builtins are native fns that push no frame, so those rows are simply
  absent (surrounding Ruby-level frames are correct). Fixing this needs a
  frame push at the dynamic-dispatch boundary plus the codegen fast
  paths; note `Class#new` is one CRuby itself does NOT show.
- **Arity-error attribution.** CRuby raises "wrong number of arguments"
  inside the CALLEE's frame (`file:DEF_LINE:in 'Object#f'`); zeo checks
  arity at the call site, so the innermost frame is the caller's.
- **`define_method(:m) { ... }` labels.** The literal form desugars to a
  `def` at compile time, so its frames say `Foo#m` where CRuby says
  `block in <class:Foo>` (the lexical block label).
- **Class-body execution order.** Class bodies run before top-level
  statements (pre-existing), so a rescued raise in a class body prints
  before earlier top-level output, and the `<main>` frame under a
  class-body frame reads line 0.

# The lossy-UTF-8 audit (the next encoding pass)

With the 24-encoding engine landed, the remaining systematic gap is the
~211 `to_utf8_lossy` call sites across zeo-rt's builtins (CI now
ratchets the count -- see ci.yml). Each needs one judgment: a DISPLAY
path (`to_s`-ish rendering, error text) keeps the lossy call; a SEMANTIC
path (comparison, slicing, matching, formatting on the bytes) must go
byte/encoding-aware via the StrBuf char layer, which is now correct for
every encoding kind. Highest-value files first: string.rs (index/slice/
sub/gsub families), format.rs, pack.rs, regexp.rs haystacks, io.rs line
reading. The pack/Base64/format legs of the binary-transcoding family
(above) fall out of the same sweep.

# Benchmark-suite gaps (same probe discipline)

One of the 58 vendored benchmarks still fails:

| benchmark | failure | blocker |
|---|---|---|
| bm_linked_list | runtime stack overflow | deep recursion overflows the `may` coroutine's 2MB stack (`coroutine ... has overflowed its stack, size=2097152`); the OS-thread + GVL migration (plan P3, 8MiB stacks) resolves it structurally |

Closed 2026-07-21:

- bm_ao_render (ex compiler PANIC): the nested-Proc guard ran BEFORE the
  capture-cell machinery classified a deeper block's own local -- now it
  panics only for a name nobody in the subtree assigns (a genuine read of
  an enclosing block's plain per-invocation `let`). Output byte-identical
  to `.expected` after the byte-output fix below.
- bm_so_mandelbrot + bm_ao_render output mismatches (one root cause): the
  print family accumulated through the lossy DISPLAY text, promoting
  BINARY high bytes to UTF-8 (`0xB4` -> `0xC2 0xB4`) on the way to the fd.
  print/puts/putc/`IO#write`/`#<<` now accumulate and write RAW bytes
  (`io::display_bytes`/`write_bytes`/`write_value`), and `String#+`/`<<`/
  `*` concatenate raw bytes under CRuby's encoding-compatibility rule
  (`StrBuf::push_buf`; incompatible pairs raise
  `Encoding::CompatibilityError` with CRuby's message). Both benchmarks
  now byte-identical to `.expected`.
- bm_life "oracle exits 1": NOT reproducible -- `ruby bench/bm_life.rb`
  exits 0 across repeated runs and matches `.expected`, as does the zeo
  binary. Transient environment artifact, closed without action.

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
