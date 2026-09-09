# What a compiled program weighs, and why

A `puts 1` program emits a few kilobytes of object code. Every other byte of
its binary is runtime, so that is where the size work is: which parts of
`libzeo.a` a program links, and what keeps each part alive.

## A program names its tables

`ruby_class!` exports each class's method table under
`zeo_ctable_<ID_CONST>` — an ordinary, strippable static. The emitted program
carries an array of pointers to the tables it can reach
(`ProgramDesc::class_tables`), and `builtins::registered_table` reads that.
Referencing a table is what keeps its class's code; not referencing one is
what lets `-dead_strip` / `--gc-sections` drop it, together with every
method body it points at.

The export name comes from the class's **id const**, not its name: the ids
are unique by construction and the names are not — `Random`, `Location` and
`Digest` each name two classes in different modules, and `#[export_name]` is
global.

`BUILTIN_TABLES` (a `linkme` slice) exists for zeo-rt's own unit tests, which
never run `register_program`. In a release build the slice has no entries,
because a slice entry is a `no_dead_strip` root in its own right.

`class_tables_are_complete` gates the symbol list against `libzeo.a` in both
directions: a missing name is a class that loses every method **and every
constant**, silently and only at run time; an extra name is a link error in
every program.

## Which tables a program names

A require-gated extension registers per program, so a class the emitter's
`feature_active` gate excludes has no constant and no dispatch path. That
takes OpenSSL, socket, zlib, StringIO, FFI and friends out of a program that
never asks for them.

For the core classes, `analyze::class_reach` answers which ones a program can
reach, over three channels:

1. **The program names it** — a constant read, a superclass, an `include`.
   Closed over the declared ancestry, because a class cannot answer a call
   without the chain behind it.
2. **A value arrives as one.** A hard seed set covers what every literal and
   operator produces; a table keyed by METHOD NAME covers the rest, because a
   builtin row can return an instance of a class nothing named —
   `caller_locations` hands back a `Thread::Backtrace::Location`.
3. **Reflection hands out every class there is.** `Marshal.load` rebuilds an
   arbitrary graph, `ObjectSpace.each_object` walks the heap,
   `Module#constants` enumerates every name, `RubyVM::AbstractSyntaxTree
   .parse` answers with a literal of whatever the text says, and a `const_get`
   or `send` with a COMPUTED argument names something no scan can read. Each
   is a hatch: the answer becomes "everything".

Narrowing is safe to be wrong in one direction only. A class kept for
nothing costs bytes; a class dropped that the program does reach aborts at
the first dispatch, naming itself. So every rule over-approximates on
purpose, and the abort is what makes a missing rule findable.

A dropped table takes its class's ROWS and never its name. Registration
comes from the runtime's own builtin list, so `Object.const_defined?(:Marshal)`
still answers `true` in a program with no Marshal table, exactly as CRuby
does.

`RubyVM::AbstractSyntaxTree` and `RubyVM::InstructionSequence` PARSE at run
time, and no program can reach either without naming `RubyVM`. A program
that never does carries neither, and the prism library they root goes with
them. The predicate is `Compiler::needs_prism_runtime`; its eval half reads
the NARROWED `runtime_eval` flag, not the flat `Hir` scan, so a program that
dead-strips the compiler drops these with it.

Every `eval` a program can reach links the compiler into the binary, about
14 MB. [`eval`](eval.md) says when a program can reach one.

## How to measure it

`checks::binary_size` links `puts 1` with the release compiler and compares
it against a committed baseline, so a regression is loud rather than prose.
It is in the `full` nextest profile.

Per-table attribution comes from linking `puts 1` once per class table with
that table dropped (`ZEO_DEBUG_DROP_TABLE`) and diffing. The columns OVERLAP:
two tables can root the same code, so they do not sum. The largest, measured
2026-08-24:

| table | bytes |
|---|---:|
| `STRING_CLASS` | 318,144 |
| `IO_CLASS` | 202,176 |
| `ARRAY_CLASS` | 200,448 |
| `RUBYVM_AST_MODULE` | 152,784 |
| `PATHNAME_CLASS` | 116,976 |
| `HASH_CLASS` | 100,592 |
| `INTEGER_CLASS` | 85,552 |
| `TIME_CLASS` | 83,936 |
| `IO_BUFFER_CLASS` | 83,504 |
| `MARSHAL_MODULE` | 66,816 |
| `SET_CLASS` | 66,752 |

Below those the tail is flat at roughly 34 KB a table, which is what makes
the always-on set worth more than any single row in it.

Dropping a table is safe to measure because a missing one is LOUD:
`builtins::registered_table` aborts naming the class rather than answering
`NoMethodError` for every row it has and losing its constants silently.

## What is left

| | |
|---|---|
| always-on class tables | The core of the set — String, Array, Hash, Integer, Float, Range — is unavoidable: a value of that kind arrives without the program naming it. The rest needs real reachability analysis (which classes a VALUE can flow into, not which the source names) plus give-everything hatches for `Marshal`, `ObjectSpace`, a computed `const_get` and a computed `send`. A module is the tractable half — it can only be reached by naming it or by a needed class including it. |
| OpenSSL's provider graph | Self-rooting once anything calls EVP: `evp_generic_fetch` reaches every predefined provider. Gating it at the zeo-rt boundary is the only lever; the class-table work already does this for programs that never require it. |
| a library's own definitions | A whole require graph (`rubygems`, `bundler`) emits every method body the graph defines, reached or not. Body-level reachability is the next lever, and a program that reaches a computed `send` keeps everything. |
