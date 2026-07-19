# Runtime exception model

Deferred. This is the eventual destination for the exception hierarchy; anything
we change in this area should keep it reachable rather than entrench the current
shape further.

> **Status update (2026-07): the BUILD-TIME premise below is obsolete — that win
> was already banked by the interim step.** The per-program "exception prelude"
> (the `ruby_class!` blocks and the exception/StopIteration factories) has moved
> OUT of codegen into the prebuilt runtime: generated `main()` calls
> `spinel_rt::ClassRegistry::with_core()` (`codegen/mod.rs`), bootstrap classes
> are filtered out of emission, and the runtime constructs exceptions by name
> (`ClassRegistry::construct_exception`). `puts 1` is now ~74 lines, not 2,296, so
> the "81% of every program is exception machinery / recompiled per program"
> numbers in "The problem" below no longer hold, and the remaining per-program
> compile cost is link + codesign of the runtime artifact, not codegen (see
> `build.rs`). **Re-scope this doc as a FEATURE effort, not a build-time one:** its
> blockers (name-keyed ivars instead of typed struct fields, runtime `super`,
> MRO-walking dispatch) are the shared prerequisites for `Exception#cause`
> chaining, Ractor deep-copy, and `instance_variable_set` on arbitrary names —
> that is what now justifies the work, not compile time. The historical
> build-time analysis is retained below for context.

## The problem

`EXCEPTION_PRELUDE` (`crates/spinelc/src/parse/mod.rs:44`) is 27 Ruby exception
classes parsed into every program's HIR (`parse/mod.rs:141-144`). In Ruby source
24 of them are empty (`class TypeError < StandardError; end`), but they are not
empty once generated: `analyze::mro::materialize` copies every inherited method
onto each subclass and `collect_ivars` flattens inherited ivars onto each struct,
so each one emits full `initialize`/`message`/`to_s` bodies, three dispatch
trampolines, and an inline `FrozenError` literal in its ivar-write guard.

For a one-line program (`puts 1`, 2,296 lines of generated Rust):

| chunk | lines | share |
|---|---|---|
| 28 `ruby_class!` invocations (27 prelude + `Math::DomainError`) | 1,106 | 48% |
| exception factory (`codegen/mod.rs:505-579`) | 713 | 31% |
| StopIteration factory (`codegen/mod.rs:583-593`) | 36 | 2% |
| the program itself | 1 | — |

**81% of every generated program is exception machinery**, and it is recompiled
once per program — 453 times per `cargo test`, ~1,819 per cold conformance run.

## The target

Move the hierarchy into `spinel-rt` as runtime data: a `(id, name, parent)` table
plus native impls for the only three methods that carry behaviour
(`Exception#initialize`/`#message`/`#to_s`, and `StopIteration#__set_result`/`#result`).
`install_exception_factory` (`dispatch.rs:642`) and
`install_stop_iteration_factory` (`dispatch.rs:666`) both disappear — they exist
only because `spinel-rt` cannot construct or touch the fields of a struct that
lives in the generated crate (`dispatch.rs:634`).

Beyond build time, this unblocks things that are structurally impossible today:

- `Exception#cause` chaining — impossible now because "every ivar is a concrete,
  typed struct field, not a runtime name-keyed map" (`builtins/handling.rs:10-19`)
- `Ractor` deep-copy of an unfrozen object (`dispatch.rs:58-64`, same root cause)
- `instance_variable_set` / `instance_variable_get` on arbitrary names

## Why it is not a small change

The blockers are in the object model, not the prelude.

1. **`super` is compile-time HIR body inlining, not a call**
   (`codegen/call.rs:920-1080`). `emit_super_arg_bindings` binds the *parent's*
   parameter names as Rust `let`s and splices the parent's HIR body inline. So
   `class MyError < StandardError; def initialize(x); super("got #{x}"); end`
   requires `Exception#initialize`'s body to exist as spinelc HIR. Move the
   prelude out and there is nothing to splice. Needs either a real runtime
   `super` channel, or prelude method HIR retained in spinelc even after the
   classes move.

2. **Ivars are typed struct fields, not a map** (`spinel-rt/src/lib.rs:176`:
   `pub $ivar: parking_lot::Mutex<RubyValue>`). `@message` exists on `MyError`
   only because `collect_ivars` walked the *inlined* parent body. A runtime
   exception object needs a name-keyed ivar map first. This is the same
   prerequisite as `.cause` and `Ractor` deep-copy, so it is worth doing on its
   own merits.

3. **`ClassRegistry::lookup` is a flat probe on the receiver's own class id**
   (`dispatch.rs:381`), with no MRO walk. That is only correct because
   materialization copies every reachable method onto every class. Removing
   materialization means dispatch must walk `ancestors` — and every statically
   resolved call (`(self.clone()).to_s()?`, emitted because `ScriptError::to_s`
   exists as an inherent fn) becomes dynamic. Rust has no inheritance, so the
   static-call shape is *why* materialization exists.

4. **19 `emit_boxed_new` call sites** (`codegen/expr.rs:854`) emit
   `#class_ident::new_handle(<struct literal>)` directly. Each needs a runtime
   `construct_by_class_id(id, args)` — `ConstructorFn` (`dispatch.rs:266`)
   already has the right shape.

5. **Reopening a prelude class propagates by materialization.** Verified:
   `class RuntimeError; def foo; 42; end; end` stamps `foo` onto `FrozenError`
   too. With a runtime hierarchy this must become `registry.define_method(id, ..)`
   plus a real MRO walk. No test covers this today — worth adding one before any
   surgery here.

6. **`resolve_class` must still answer for prelude names at compile time**
   regardless of where the implementation lives — `codegen/mod.rs:503`
   (`.expect("the exception prelude always defines Exception")`),
   `exceptions.rs:227` (panics on an unknown rescue class), `expr.rs:855`. A
   `ClassInfo`-shaped stub table, like `spinel_abi::BUILTINS`, is needed either way.

## Known warts to fix while in here

- **`Math::DomainError`'s id floats.** It is registered after the user-class loop
  (`analyze/mod.rs:132-149`), so it lands at 62 with no user classes and 64 with
  two. Every other bootstrap class has a stable id (prelude is 35..61, builtins
  1..34, `Object` 0). It belongs in `spinel-abi` with the rest, under the same
  `debug_assert_eq!(id, b.id)` contiguity contract as `compiler.rs:260-264`.
- **The exception factory emits an arm per bootstrap Exception descendant (28),
  but `spinel-rt`'s 151 `raise_error` sites name only 13 distinct classes** —
  `TypeError`(74), `ArgumentError`(37), `ZeroDivisionError`(7), `IndexError`(6),
  `RangeError`(5), `NoMethodError`(4), `FloatDomainError`(3), `FiberError`(3),
  `RegexpError`(2), `Math::DomainError`(2), `LocalJumpError`(2), `NameError`(1),
  `KeyError`(1). 15 arms are dead code.
- **`run_initialize` panics** on arity mismatch (`dispatch.rs:589-606`) rather
  than raising `ArgumentError` — there is no `ArgumentError` channel from
  `spinel-rt` that doesn't route through the generated factory.

## Interim step taken instead

See `docs/todo/` history / git log: the factory's dead arms were pruned and the
prelude was moved behind a precompiled boundary rather than restructured. Both
are throwaway once this lands, and neither adds a new dependency on
classes-as-generated-structs.
