# What a compiled program weighs, and why

A `puts 1` program emits **4,208 bytes** of object code. Every other byte of
its binary is runtime, so that is where the size work is.

## Where it went

Measured on a `puts 1` binary of **9,764,552 bytes**, by summing `__text`
per symbol and attributing each to the crate that mangled it:

| bytes | component |
|---|---|
| 3,651,860 | `zeo_rt` itself |
| 729,016 | the Rust `regex` crate (`regex_syntax` + `regex_automata`) |
| 572,732 | unattributed (C objects with no mangling to read) |
| 455,728 | std + core |
| 227,808 | prism |
| 186,232 | mimalloc |
| 149,532 | Oniguruma |
| 36,028 | encoding_rs + unicode-normalization |
| 22,908 | rustyline |
| 11,780 | OpenSSL |
| **6,027,296** | **`__text` total** |

The rest of the file is not code: `__const` 1,238,088, `__eh_frame` 647,464,
`__gcc_except_tab` 324,916, `__cstring` 98,481, `__unwind_info` 116,176, and
`__LINKEDIT`.

OpenSSL is 11 KB here and was ~1 MB before the alias tables became named
symbols; that whole component now arrives only through a class table a program
names. The `regex` crate is still 729 KB in a program with no regex in it,
because `Marshal.load` can rebuild a `Regexp` and `Marshal`'s table is
always-on.

## The cause: a link-time table is unstrippable

Every `ruby_class!` used to emit its method table as a
`#[linkme::distributed_slice(BUILTIN_TABLES)]` static. **A slice entry is a
`no_dead_strip` root in its own right** — nothing has to read it — and each one
points at its class's method table, which points at every method body.

`ld -why_live` on Date's parser in a `puts 1` binary:

```
zeo_rt::ext::date::parse::date_parse
  ← rc__parse_72 ← lookup_class ← ___RUBY_CLASS_TABLE_DATE
```

and on OpenSSL's KDF tables:

```
_ossl_kdf_pvk_functions ← … ← openssl_sys::init
  ← MessageDigest::from_name ← zeo_rt::ext::openssl::md_by_name
  ← algo_class::class_raw ← rc_digest_1 ← lookup_class ← SHA1_TABLE
```

So `puts 1` linked Date's parser, Marshal, RubyVM::AST, readline and all of
OpenSSL. Two things this is **not**, both checked: `-Wl,-force_load` accounts
for 40 KB (linking without it saves only that), and removing the *readers* of
`BUILTIN_TABLES` changes nothing at all.

## The fix: a program names its tables

`ruby_class!` now exports each table under `zeo_ctable_<ID_CONST>` — an
ordinary, strippable static. The emitted program carries an array of pointers
to the tables it can reach (`ProgramDesc::class_tables`), and
`registered_table` reads that. Referencing a table is what keeps its class's
code; not referencing one is what lets it strip.

The export name comes from the class's **id const**, not its name: the ids are
unique by construction and the names are not — `Random`, `Location` and
`Digest` each name two classes in different modules, and `#[export_name]` is
global.

`BUILTIN_TABLES` survives for zeo-rt's own unit tests, which never run
`register_program`. In a release build the slice has no entries.

`class_tables_are_complete` gates the symbol list against `libzeo.a` in both
directions: a missing name is a class that loses every method **and every
constant**, silently and only at run time; an extra name is a link error in
every program.

## What is narrowed today

A require-gated extension registers per program, so a class the emitter's own
`feature_active` gate excludes has no constant and no dispatch path. That
takes OpenSSL, socket, zlib, StringIO, FFI and friends out of a program that
never asks for them.

**Result: 15,553,816 → 13,061,176 bytes, −2.49 MB.** Three later passes
(alias tables as named symbols, the residual hand-rolled `class_table` arms,
and moving every bootstrap seeder into its class's own `install_constants`)
took it to **9,764,552**, and narrowing the prism-backed `RubyVM` tables
took it to **9,252,936**.

### The always-on set

An always-on class used to stay unconditionally, on the grounds that a value
of that kind can arrive without the program naming it — `1.to_s` needs
String. That is true of a couple of dozen classes and false of the rest:
`puts 1` cannot reach `Ractor`, `Marshal`, `TracePoint` or `Pathname` by any
route at all.

`analyze::class_reach` answers which it can, over three channels:

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

Narrowing is safe to be wrong in one direction only. A class kept for nothing
costs bytes; a class dropped that the program does reach aborts at the first
dispatch, naming itself. So every rule over-approximates on purpose — and
that abort is what made the rules findable: the corpus reported each missing
class by name, thirty-odd of them, until it did not.

A dropped table takes its class's ROWS and never its name. Registration comes
from the runtime's own builtin list, so `Object.const_defined?(:Marshal)`
still answers `true` in a program with no Marshal table, exactly as CRuby
does.

**Result: 9,252,936 → 7,493,552 bytes, −1.76 MB.** `puts 1` names 33 class
tables where it named 88.

### The prism-backed `RubyVM` surfaces

`RubyVM::AbstractSyntaxTree` and `RubyVM::InstructionSequence` PARSE at run
time, and no program can reach either without naming `RubyVM` — they are
namespaced under it. So a program that never does carries neither, and the
prism library they root goes with them: **−528,800 bytes**, taking `puts 1`
to **9,252,936**.

`RubyVM` itself stays, because ruby defines it in every program. Only
`RubyVM.constants` can see the five go, and that already names `RubyVM`.

The predicate is `Compiler::needs_prism_runtime`. Its eval half reads the
NARROWED `runtime_eval` flag, not the flat `Hir` scan — a program whose
`load` is its own method, or whose deferred `require` names a feature this
compile emitted as a unit, dead-strips the compiler, and it must drop these
five with it.

## How to measure it

`tools/zeo-dev size` links `puts 1`, then links it again once per class table
with that table dropped (`ZEO_DEBUG_DROP_TABLE`), and diffs. That is exact
per-table attribution, and there is no other way to get it; every figure in
this document before it was prose. `zeo-dev size --check` is a CI gate on the
baseline, so a regression is loud.

The columns OVERLAP: two tables can root the same code, so they do not sum.
The largest, measured 2026-08-24:

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

Below those the tail is flat at roughly 34 KB a table for 150 of them, which
is what makes the always-on set worth more than any single row in it.

Dropping a table is safe to measure because a missing one is now LOUD:
`builtins::registered_table` aborts naming the class rather than answering
`NoMethodError` for every row it has and losing its constants silently.

## What is left

| | |
|---|---|
| always-on class tables | A `puts 1` still names most of the 170. The whole set attributes 2,482,240 bytes with the columns overlapping, and the core of it — String, Array, Hash, Integer, Float, Range — is unavoidable: a value of that kind arrives without the program naming it. What is left needs real reachability analysis (which classes a VALUE can flow into, not which the source names) plus give-everything hatches for `Marshal`, `ObjectSpace`, a computed `const_get` and a computed `send`. A module is the tractable half — it can only be reached by naming it or by a needed class including it. |
| three regex engines | `Engine` is `Fast(regex)` / `Fancy(fancy_regex)` / `Onig`, and Cargo.toml already says Oniguruma "will replace fancy-regex". Retiring the two Rust engines drops 729 KB plus encoding_rs and unicode-normalization. It is a performance trade and wants its own bench pass. |
| OpenSSL's provider graph | Self-rooting once anything calls EVP: `evp_generic_fetch` reaches every predefined provider. Gating it at the zeo-rt boundary is the only lever; the class-table work already does this for programs that never require it. |

## A trap this cost a pass on

The tables carry `install_constants` as well as methods, and
`bootstrap::install_core_constants` iterated the slice separately. Pointing
only `registered_table` at the program's array left the constant installers
reading an empty slice, and **180 goldens failed with `uninitialized constant
File::RDWR`** — which reads like a narrowing bug and is not one. Both readers
go through `builtins::all_tables()` now.
