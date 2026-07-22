# Runtime exception model — LANDED

This doc used to plan the move away from per-program generated exception
machinery and the object-model blockers behind it. Everything it asked
for has since shipped; it is retained as a record of where each piece
landed. Nothing here is pending.

## What shipped, and where

- **The hierarchy is runtime data.** The exception prelude lives behind
  the prebuilt runtime: generated `main()` seeds
  `ClassRegistry::with_core()` from `zeo_abi::EXCEPTION_CLASSES` (an
  append-only `(id, name, superclass)` table), and the runtime constructs
  exceptions by name (`ClassRegistry::construct_exception`). A one-line
  program is ~74 generated lines, not 2,296.
- **`super` is a real runtime call.** The compile-time HIR body splice is
  deleted; dispatch carries per-MRO-position `own_impls` tables, a
  class-method channel (`send_super_class_from`), and a singleton channel
  (`call_singleton_super_target`).
- **Dispatch walks the MRO.** `ClassRegistry::lookup_mro` probes flat
  then walks ancestors honoring `undefined_methods`; prelude-class
  reopens propagate to subclasses through it, not through
  materialization.
- **Ivars have a name-keyed seam.** Typed struct fields remain the fast
  path; the `__overflow` map plus named accessors serve reflection
  (`instance_variable_set`/`_get` on arbitrary names), with frozen checks.
- **`Exception#cause` works.** Native cause slot on the exception
  object, `attach_cause` at every raise channel, `cause:` keyword with
  `cause: nil` suppression, cycle guard, survival through thread-join
  re-raise.
- **Ractor copies unfrozen values.** Send is by reference when
  shareable, deep copy otherwise (`ractor.rs`); the old "reject unfrozen"
  posture survives only in narrow documented corners (`dispatch.rs`).
- **The warts are gone.** `Math::DomainError` has a stable pinned id in
  `zeo-abi` (no longer floating after user classes); the exception
  factory's dead arms were pruned; `.new`/`initialize` arity mismatches
  raise a rescuable `ArgumentError` attributed to the callee's frame
  instead of panicking.

## Residual boundary (documented, deliberate)

Prelude class *bodies* still resolve at compile time for HIR-level needs
(`resolve_class` answers for every bootstrap name via the abi tables) —
that is the intended stub-table design, not a leftover.
