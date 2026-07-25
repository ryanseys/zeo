# Runtime `eval` and the `binding` gap

`zeo` handles `eval("literal string")` at *compile time* (`HirNode::Eval`, wired
up in `parse/mod.rs`): the recognizer parses the literal, lowers it, and splices
the resulting HIR into the arena at the call site — the same trick `require` uses
for a file's contents. This works because the argument is a
compile-time-constant `StringLit` (no interpolation): zeo already has the whole
program in front of it, so "the instant this code would run" and "compile time"
are the same moment for a literal, and no runtime parser is ever needed.

A **dynamic** `eval` — a runtime-computed string (interpolated, read from a
variable, built from I/O) — is a different problem for a whole-program AOT
compiler: the source being "compiled" isn't known until the program is already
running. zeo handles it with a runtime tree-walking interpreter, the **eval
VM**, linked into the generated binary behind the `eval-vm` cargo feature.

## The eval VM (`crates/zeo-rt/src/eval_vm.rs`)

The eval VM walks `ruby-prism`'s own `Node` tree directly, not the compiler's
HIR. HIR bakes in decisions the AOT compiler resolves statically — `New` needs a
statically-known class, `ClassRef` has no first-class runtime `Class` value,
`super` lowers to inlining the parent body, class-var owners are pre-resolved in
`analyze` — so an interpreter over HIR would have to *undo* all of that. Prism's
`Node` tree, by contrast, is Ruby's surface semantics, and every construct maps
onto a primitive the runtime already exposes: `dispatch::send_value` for method
calls, `const_get`/`global_get`/`ivar_*`/`cvar_*` for state, the `runtime_meta`
overlay for a runtime `def`/`class`.

So the VM is a purely additive module against the *live* runtime, not a second
compiler. Eval'd code and AOT-compiled code share one value representation
(`RubyValue`), one control-flow signal type (`Signal`), and one class/method
registry — and call into each other freely: compiled code invoking `eval`,
eval'd code calling a compiled method via `send`, and an eval-defined method
being called back from compiled code.

### Selective linking — most binaries stay parser-free

`ruby-prism` is a C library and the single largest size lever in the runtime, so
the default runtime never links it. The compiler statically detects whether a
program can reach the VM (`Hir::uses_runtime_eval` — a receiverless
`Kernel#eval`, or a string-form `instance_eval`/`class_eval`/`module_eval`) and
only then links the prism-backed `eval-vm` runtime variant (`backend::Runtime`).
A program that uses `eval` still compiles and runs out of the box; it just opts
*its own* binary into carrying prism, while every other binary stays lean. This
is work matz's interpreter can't do — CRuby always ships its parser because it
can't know in advance whether a program evals. Built without the feature, the
VM's entry point is an honest stub that raises `NotImplementedError` naming
`--features eval-vm`.

### What the VM does, and the one gap

An `eval` runs with a correct `self`: its receiver's ivars, implicit-self calls,
constants, and globals all resolve. The invoking surface decides where a `def`
inside the source installs (CRuby's "default definee"): an instance method for
`class_eval`, a singleton for `instance_eval`, and `self`'s class for a plain
`eval` (a top-level eval's `self` is the main object, so `def` lands on
`Object`). Eval'd code can `def` methods, run blocks passed to calls, and
`yield`/`return` inside an eval-defined method; each such method or block
re-parses its own captured source per invocation. A local *assigned* inside an
eval is visible to later statements of the same eval (held in `Env::locals`).

What the VM does **not** yet reach is the **caller's own local variables**: those
live as Rust stack slots the interpreter can't touch without codegen
materializing a `Binding`. A first-class `binding` — the value that exposes a
compiled call site's own locals to the interpreter — is the next increment, and
the real prerequisite for bidirectional local sharing. That is why dynamic
`eval` and `binding` are naturally scoped together: a faithful caller-local
`eval` is most of the way to `binding` already.

## Runtime metaprogramming without a parser

Some runtime metaprogramming needs no parser at all: a compiled block is already
an `Arc<dyn Fn>`, so a class or method defined at runtime *from a block* needs
only a runtime-mutable method registry, not an interpreter. That foundation is
`zeo-rt/src/runtime_meta.rs`: a lock-guarded overlay beside the frozen
`OnceLock` registry, gated by a single `is_live()` atomic so parser-free
programs pay nothing. On it:

- **Runtime `define_method`** (computed name, in a class-body `each` loop) — the
  block becomes a `MethodImpl::Dynamic`. Class-body statements execute, and a
  nested block may capture the enclosing block's own local.
- **Per-object singletons** (`def obj.foo`, `class << obj`,
  `obj.define_singleton_method`) — an identity-keyed overlay table.
- **`Class.new(Super) { … }`** — a runtime class id plus a name-keyed
  `DynObject` instance type.

**Documented boundary (inherent to AOT):** a runtime `define_method` that
*overrides* a method the compiler dispatched *statically* (a direct `Klass::m`
call, not through `send`) is invisible at that call site.

## Non-goals

`TracePoint`/`ObjectSpace`, Ractors, and refinements stay out of scope. So does
a *reflective* eval the static analysis cannot see — a `send(:eval, str)` hides
the eval site from `uses_runtime_eval`, so its binary may not link the VM; the
honest failure mode there is the runtime's own `NotImplementedError`, never
silent wrong output.
