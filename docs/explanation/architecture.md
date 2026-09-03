# Architecture

One pass over the whole program, and no interpreter anywhere in it.

```
foo.rb ─prism─▶ HIR arena ─analyze─▶ typed classes, MRO,   ─clif─▶ Cranelift IR ─┬─▶ JIT, run in place
                (requires             method tables,                             │   (the default)
                 spliced at           inline-iterator                            └─▶ .o + cc + libzeo.a
                 compile time)        decisions                                      ─▶ a native binary
```

- **One compilation unit.** `require` and `require_relative` resolve at
  compile time, the bundled gems included. The front end never defers a file
  to run time.
- **Two dispatch paths.** A call the compiler can resolve becomes a direct
  call. Everything else goes through the runtime's method registry, keyed by
  `Symbol`. That same registry serves `send`, `define_method`,
  `method_missing`, singletons, refinements, and classes built at run time
  with `Class.new`.
- **Two output modes, one lowering.** `-o` emits an object file and links it
  against `libzeo.a` with the system `cc` (`--backend aot`). Run mode either
  finalizes the same Cranelift IR in process (`--backend jit`) or links a
  binary and `exec`s it.
- **A program compiles once.** `zeo foo.rb` links its binary into a cache and
  runs it; the next run with the same sources skips straight to the `exec`,
  so `zeo gem --version` costs 0.4s rather than 4.9s. A cache entry is keyed
  by the compile's inputs and checked against them, and anything the cache
  cannot answer -- an edited file, a program the object backend declines --
  falls back to compiling. `ZEO_CACHE=0` turns it off.
- **A complete runtime** (`zeo-rt`): a CRuby-compatible numeric tower
  (`Integer`/`Bignum`/`Rational`/`Complex`), strings as bytes plus an
  encoding, real coroutine `Fiber`s, real OS-thread `Thread`s, and Ruby 4.0's
  `Ractor` port model. `#![forbid(unsafe_code)]` outside the FFI and syscall
  layers.

The compiler crate and the runtime crate agree only through **`zeo-abi`**,
which assigns a numeric `ClassId` to every built-in class. The compiler bakes
the number into the emitted code; the runtime's dispatch reads it.

## Where each stage lives

| Stage | Directory | Question |
|---|---|---|
| parse | `crates/zeo/src/parse/` | what does this source say, and what does it require |
| lower | `crates/zeo/src/lower/` | what is it, as HIR |
| analyze | `crates/zeo/src/analyze/` | what is true about the whole program |
| clif | `crates/zeo/src/clif/` | what Cranelift IR does it become |
| backend | `crates/zeo/src/backend/` | in memory, or through a real link |

The runtime that every compiled program links is `crates/zeo-rt/`, and
[the backend](backend.md) is where the two meet.

## What Ruby zeo speaks

Zeo targets the full language. `zeo-abi` is the single source of the target
version, so the compiler's version checks and the runtime's `RUBY_VERSION`
cannot disagree.

- **Numerics** — unlimited-precision `Integer`, `Float`, `Rational`,
  `Complex`; coercion and error text follow CRuby.
- **Strings and encodings** — bytes plus an encoding; `Encoding.list`
  reports the same 103 encodings as Ruby 4.0.6.
- **Collections** — `Array`, `Hash`, `Range`, `Symbol`, `Struct`, `Data`,
  `Set`. `Enumerable` and `Comparable` are real ancestors that call your
  `each` and `<=>`.
- **Blocks and procs** — a block whose shape is known is inlined (`3.times`
  becomes a native loop); one that escapes becomes a real closure.
- **Classes and metaprogramming** — inheritance, modules,
  `include`/`extend`/`prepend`, `super`, visibility, `alias`, `undef`,
  refinements, singleton classes, `define_method`, `method_missing`,
  `Class.new`, `Module.new`.
- **Exceptions** — `raise`/`rescue`/`else`/`ensure`/`retry`,
  `throw`/`catch`, `$!`, and CRuby-identical messages and backtraces.
- **Pattern matching** — `case/in`, the `in` predicate and the `=>` binding,
  with CRuby's `NoMatchingPatternError` texts.
- **Concurrency** — parallel OS `Thread`s (8 MiB stacks, killable,
  interruptible), coroutine `Fiber`s, `Ractor` with Ruby 4.0's port model,
  `Mutex`, `Queue`.
- **Reflection** — `Method#parameters`/`#arity`/`#source_location`,
  `TracePoint`, line coverage, `ObjectSpace`,
  `RubyVM::AbstractSyntaxTree`, `RubyVM::InstructionSequence`, `Ruby::Box`.
- **`eval`** — every `eval` is COMPILED at run time by the same compiler, a
  literal string included ([`eval`](eval.md)). The compiler is
  linked only into programs that can reach it.

For what does not match yet, read
[Compatibility](../reference/compatibility.md); for the extension model,
[Add an extension](../how-to/add-an-extension.md).
