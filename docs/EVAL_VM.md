# Dynamic `eval` / `binding`: a future embedded-interpreter phase

`spinelc` supports `eval("literal string")` today (`HirNode::Eval`, wired up
in `parse/mod.rs`) by parsing the literal at *compile time* and splicing its
lowered HIR into the arena at the call site — the same trick `require` will
eventually use for a file's contents. This works because the argument is a
compile-time-constant `StringLit` (no interpolation): spinelc already has the
whole program in front of it, so "the instant this code would run" and
"compile time" are the same moment for a literal, and no runtime parser is
ever needed.

A **dynamic** `eval` — a runtime-computed string (interpolated, read from a
variable, built from I/O) — is a fundamentally different problem for a
whole-program AOT compiler: the code being "compiled" isn't known until the
program is already running. This document sketches how that could work as a
real, buildable future phase — not a project abandoned for lack of a plan,
but one genuinely bigger than a small addition to the current spike, and
correctly deferred behind Phases 6 and 9 below.

## The core idea: the interpreter is just the dynamic twin of this same pipeline

`spinel-rt` already gives every AOT-compiled method and every `send()` call
the identical ABI: `Result<RubyValue, Signal>`. A tree-walking interpreter —

```rust
fn eval_node(hir: &Hir, id: NodeId, env: &Env) -> Result<RubyValue, Signal>
```

— slots into that ABI with **zero changes to `Signal` itself** (`Retry`/
`Raise` are already dormant, waiting for exactly this kind of consumer). It
can reuse the *exact* runtime primitives compiled code already calls —
`spinel_rt::dispatch::send`, the `arith.rs` helpers, `collections.rs` — just
resolving each operation dynamically per-node instead of by `analyze`'s
static `TyKind`. In effect: `analyze`/codegen's "Path 1 static dispatch" has
no equivalent inside the interpreter — it is *always* Path 2, dispatching
every call through `send()` by inspecting each `RubyValue`'s own runtime
tag. That's a genuine simplification, not a gap: no `TyKind` fixpoint, no
codegen, needed on this path at all.

This means eval'd code and AOT-compiled code would share one value
representation, one class/method registry, and one control-flow signal
type — and could call into each other bidirectionally: compiled code
invoking `eval`, eval'd code calling a compiled method via `send`, and (once
the pieces below exist) an eval-defined method being called back from
compiled code, recursively.

## What's genuinely reusable vs. genuinely new

**Reusable, close to free:**
- `RubyValue`/`Signal` as the interpreter's own value/error types (see above).
- The arithmetic/collection runtime helpers (`arith.rs`, `collections.rs`).
- Whatever capture-cell type Phase 6's `Proc` env ends up using
  (`Rc<RefCell<RubyValue>>` per captured local, per the roadmap) is exactly
  the right cell type for the interpreter's own variable bindings too — real,
  load-bearing reuse, not just an analogy.

**Genuinely new work, not free:**
- **The environment *container* differs, not just its cell type.** Phase 6's
  Proc env is a codegen-generated concrete Rust struct with statically-known
  fields (this project's "concrete-type-by-default" style) — an interpreter
  has no compile time, so its environment must be a real dynamic
  `HashMap<String, Rc<RefCell<RubyValue>>>` (or a chain of scopes).
- **Bidirectional interop needs `binding` to exist first.** For eval'd code
  to read/write a *caller's* locals, the compiled call site must expose its
  own locals' cells to the interpreter — a `Binding` value is precisely that
  exposure. This is why the user's framing scopes dynamic `eval` and
  `binding` together: you can't build a faithful dynamic `eval` without
  something that's already most of the way to `binding`.
- **`spinel_rt::dispatch::MethodFn` would need to widen** from a bare
  `fn(&RObj, &[RubyValue]) -> Result<RubyValue, Signal>` function pointer to
  something like `Rc<dyn Fn(&RObj, &[RubyValue]) -> Result<RubyValue, Signal>>`,
  so compiled code can `send` into a method an eval'd `def` defined at
  runtime (an interpreted closure can't be represented as a bare `fn`
  pointer). This is the same "erase to `Rc<dyn Trait>` only once a concrete
  function pointer genuinely can't work" idiom Phase 6's `Rc<dyn ProcBody>`
  already commits to — not new invention, but a real ABI change to a
  currently-shipping runtime type.
- **`ruby-prism` needs to be linked into the *generated binary*, not just
  `spinelc`.** Literal eval never needs this (it parses entirely inside the
  compiler's own process, at compile time; the parser never ships). A truly
  dynamic eval needs a real parser present in every compiled program that
  might call it — a genuine distribution-size/FFI-surface cost (though not
  unprecedented: CRuby itself always ships its own parser for the same
  reason).
- **A shared HIR representation, reachable from both crates.** `Hir`/
  `HirNode`/the lowering logic currently live only in `spinelc`, the
  compiler, which never ships in the generated binary. Recommend extracting
  `hir.rs` (and the lowering logic a runtime `eval` needs) into a new shared
  crate (e.g. `spinel-hir`) depended on by both `spinelc` and whatever
  runtime crate hosts the interpreter — not a second, drift-prone copy.
- **Runtime parse/lowering errors need to become real Ruby exceptions** —
  `Signal::Raise(RubyValue)` is the intended vehicle (see the breadcrumb in
  `spinel-rt/src/signal.rs`), but that variant isn't load-bearing until
  Phase 9 (`begin`/`rescue`) actually exists.

## Sequencing

This is its own future phase — naturally *after* Phase 6 (needs the Proc
capture-cell convention to exist for real) and coordinated with Phase 9
(needs `Signal::Raise` to be load-bearing for runtime syntax/eval errors),
not a small addition to the current eval scaffold. Recommended shape when
picked up: `binding` first (it's the real prerequisite, not eval itself),
then the interpreter loop over `Hir`, then wiring a non-literal `eval` call
to it, then the `MethodFn` widening for full bidirectional interop last
(compiled-calls-eval'd-def is the hardest, least essential slice).

## Runtime metaprogramming without a parser (#97, stage 1 — shipped)

Some of what this doc once listed as "permanently out of scope" turned out NOT
to need the parser at all — a compiled block is already an `Arc<dyn Fn>`, so a
class/method defined at runtime *from a block* needs only a runtime-mutable
method registry, not an interpreter. That foundation shipped as #97 stage 1
(see `spinel-rt/src/runtime_meta.rs`): a lock-guarded overlay beside the frozen
`OnceLock` registry, gated by a single `is_live()` atomic so parser-free
programs pay nothing. On it:

- **Runtime `define_method`** (computed name, in a class-body `each` loop) —
  the block becomes a `MethodImpl::Dynamic`. Class-body statements now execute
  (they were silently dropped), and a nested block may capture the enclosing
  block's own local (both were pre-existing codegen gaps, fixed here).
- **Per-object singletons** (`def obj.foo`, `class << obj`,
  `obj.define_singleton_method`) — an identity-keyed overlay table.
- **`Class.new(Super) { … }`** — a runtime class id + a name-keyed `DynObject`
  instance type.

**Documented boundary (inherent to AOT):** a runtime `define_method` that
*overrides* a method the compiler dispatched *statically* (a direct
`Klass::m` call, not through `send`) is invisible at that call site.

## Non-goals, still

`TracePoint`/`ObjectSpace`, Ractors, refinements, and reflection/`eval` with a
*non-literal, runtime-computed* method name or source string stay out of scope
until the interpreter (stage 2) exists — the VM is specifically about executing
*runtime-known Ruby source*, which the overlay above deliberately does not do.
