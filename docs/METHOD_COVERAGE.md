# Method coverage: the census against CRuby 4.0.6

**The census is retired.** It reached zero gaps, and a ledger that only ever
reads zero gates nothing. This page records what it measured, what it never
reached, and why the remaining work is a long tail of individual fixes rather
than another sweep.

## What it measured, and what it found

A **transitive census**: every `Module` reachable from `Object`'s constant
tree (depth 4), dumped from both engines and diffed. Per module it recorded
`instance_methods(false)`, `singleton_methods(false)`,
`private_instance_methods(false)`, `constants(false)`, and the inherit-true
sets — that last pair is what separates "raises `NoMethodError`" from "works,
but reflection disagrees". Arity was a second gate of the same shape, diffing
Zeo's declared parameter lists against what ruby reports.

Both ended at zero. Every module, method, constant, owner, visibility and
arity that ruby 4.0.6's census reaches has a Zeo answer.

Four passes got there: `module_function` pairs and the full 158-constant
`Errno` table; the missing methods on existing classes and owner/visibility
fidelity; the `Ractor` port model with real `move:` semantics and
`Process::Waiter`; and the final four subsystems — `IO::Buffer` (complete,
with real `mmap`), `RubyVM::AbstractSyntaxTree` over Prism,
`RubyVM::InstructionSequence`, `RubyVM::YJIT` (present, permanently disabled),
and `Ruby`/`Ruby::Box` over Zeo's compile-time box model.

It replaced an earlier hand-counted figure ("~1,976 missing methods") that
double-counted inherited methods.

## What zero rows never claimed

The census measured the **surface**: that a method exists, on the right class,
with the right visibility and arity. Whether it BEHAVES like ruby's was always
the job of the conformance corpus (`tests/spinel/`, byte-for-byte against the
oracle) and the fixtures in `tests/`. Behavioural divergences are tracked as
executable gaps in `tests/gaps/` and documented in
[`COMPATIBILITY.md`](COMPATIBILITY.md).

Two limits are worth keeping in mind, because they are where the remaining
bugs live:

- **It never saw a require-gated class.** The walker required nothing, so it
  measured the surface a program has before its first `require`. `StringIO`,
  `Zlib`, `OpenSSL`, `Socket` and every other extension are absent from
  `Object`'s constant tree when the census runs. A missing
  `StringIO#read_nonblock` reached `main` this way, and a golden found it
  rather than the census.
- **It was one-directional.** The ledger recorded oracle-has-zeo-lacks only.
  A name Zeo answers that ruby does not was never a census failure.

Compiler intrinsics (`block_given?`, `binding`, `__method__`, …) answer at
call sites without living in the reflection tables; the census accounted for
them through the inherit-true sets.

## What replaced it

Nothing, deliberately. Surface parity is done; the open work is behavioural
and per-case, so it is tracked where behaviour is tracked — as executable
gaps under `tests/gaps/`, each carrying its own diagnosis, and as the
divergences catalogued in [`COMPATIBILITY.md`](COMPATIBILITY.md).
