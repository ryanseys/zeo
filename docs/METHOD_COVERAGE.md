# Method coverage: the census against CRuby 4.0.6

zeo measures its method surface with a **transitive census**: every `Module`
reachable from `Object`'s constant tree (depth 4), dumped from both engines
and diffed. For each module the census records `instance_methods(false)`,
`singleton_methods(false)`, `private_instance_methods(false)`,
`constants(false)`, and the inherit-true sets. That last pair is what
separates "raises `NoMethodError`" from "works, but reflection disagrees".

**The gap ledger is empty.** `conformance/method-census-gaps.tsv` holds zero
rows: every module, method, constant, owner, and visibility that ruby 4.0.6's
census reaches has a zeo answer. The suite keeps it that way — a new gap
fails the test, and the ledger may only shrink.

## How it works

- The oracle side is recorded in `conformance/method-census.tsv`, dumped from
  `mise exec ruby@4.0.6 -- ruby`.
- The gate is `crates/zeo/tests/method_census.rs`. It classifies each
  divergence as `absent-module`, `constant`, `owner` (the name answers, but
  the wrong class owns it), or `unreachable` (absent from that kind's set —
  a wrong visibility lands here too).
- `ZEO_BLESS=1 cargo test -p zeo --test method_census` rewrites the ledger.
  The deleted rows are the review artifact.
- A new class must land with its full census-visible surface in the same
  commit: closing an `absent-module` row exposes that module's whole oracle
  surface to the diff.
- Arity is a separate gate with the same shape:
  `cargo run -p xtask -- arity-oracle` records what ruby reports for every
  builtin, and `crates/zeo/tests/builtin_arity.rs` diffs zeo's declared
  parameter lists against it. A mismatch is not blessable.

## What "zero rows" does and does not claim

The census measures the **surface**: that the method exists, on the right
class, with the right visibility and arity. Whether each method's behaviour
matches ruby is the job of the conformance corpus (`tests/spinel/`, byte-for-
byte against the oracle) and the per-feature fixtures in `tests/`. Behavioural
divergences are tracked as executable gaps in `tests/gaps/` and documented in
[`COMPATIBILITY.md`](COMPATIBILITY.md).

Two asymmetries are deliberate:

- The ledger records oracle-has-zeo-lacks only. A name zeo answers that ruby
  does not have (for example, a legacy internal class) is not a census
  failure.
- Compiler intrinsics (`block_given?`, `binding`, `__method__`, …) answer at
  call sites without living in the reflection tables; the census accounts for
  them through the inherit-true sets.

## History

The census replaced an earlier hand-counted figure ("~1,976 missing
methods") that double-counted inherited methods. Four passes brought the
ledger from its first honest measurement to zero, in roughly this order:
`module_function` pairs and the full 158-constant `Errno` table; the missing
methods on existing classes and owner/visibility fidelity; the `Ractor` port
model with real `move:` semantics and `Process::Waiter`; and the final four
subsystems — `IO::Buffer` (complete, with real `mmap`),
`RubyVM::AbstractSyntaxTree` over Prism, `RubyVM::InstructionSequence`,
`RubyVM::YJIT` (present, permanently disabled), and `Ruby`/`Ruby::Box` over
zeo's compile-time box model. The git history carries the details; the
fixtures under `tests/` pin every surface the waves added.
