# Crates

Eight crates, edition 2024. Seven are published together, version-locked
with `=`; `xtask` is not published at all.

| Crate | What it is |
|---|---|
| `zeo` | the compiler and the CLI: parse ▸ lower ▸ analyze ▸ clif ▸ backend |
| `zeo-rt` | the runtime linked into every compiled program — the object model, the core classes, the dispatcher, threads, fibers, the heap |
| `zeo-abi` | a dependency-free leaf: the `ClassId` numbers and the ABI both sides agree on |
| `zeo-dsl` | the shared `syn` grammar for the `ruby_class!` / `ruby_module!` DSL |
| `zeo-macros` | the proc-macro that expands that DSL into runtime code |
| `zeo-capi` | MRI's C extension API (`rb_*`), implemented in Rust over the runtime; linked when the `capi` feature is on |
| `zeo-gem` | RubyGems' formats: `Gemfile.lock`, gemspecs, `.gem` files, the store |
| `xtask` | the repo's own chores (`cargo xtask`) |

## The rules between them

**`zeo-abi` depends on nothing.** It exists so the compiler and the runtime
can agree on numbers without either depending on the other. Anything either
side must know about the other belongs here.

**One grammar, two consumers.** `zeo-macros` expands the DSL into runtime
code; `zeo`'s build script parses the *same* declarations with `zeo-dsl` to
project the compiler's view of the builtin class surface. Because both sides
parse the same text with the same code, what the runtime registers and what
the compiler folds against cannot drift.

**The family is version-locked.** The compiler emits code against an exact
runtime surface, every emitted object links against that exact `libzeo.a`,
and the class surface is projected from an exact `zeo-rt` revision. Anything
looser invites drift crates.io cannot express, so releases bump every crate
in lockstep.

**`zeo-gem` has no HTTP client and no resolver.** It reads and writes
RubyGems' formats. Which gem to fetch is the lockfile's answer, and fetching
it is the caller's business.

## Inside `crates/zeo/src`

| Directory | Question it answers |
|---|---|
| `parse/` | what does this source say, and what does it require |
| `lower/` | what is it, as HIR |
| `analyze/` | what is true about the whole program |
| `clif/` | what Cranelift IR does it become |
| `backend/` | in memory, or through a real link |
| `cli/` | what did the user ask for |
| `diagnostics/` | what went wrong, and where |
| `gems/` | where does a `require` find its library |
| `packages/` | compiled artifacts on disk |
| `compiler/`, `hir/`, `cext/` | the shared machinery each of those uses |

## Inside `crates/zeo-rt/src`

`builtins/` is one file per Ruby class, mirroring CRuby's own file-per-class
layout. A class with real machinery has a second file at the crate root: the
root file is the machinery, and `builtins/<name>.rs` is the method table that
calls into it. The eleven helpers that belong to no class live in
`builtins/support/`.
