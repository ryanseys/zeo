# What a compiled program weighs, and why

A `puts 1` program emits **4,208 bytes** of object code. Every other byte of
its binary is runtime, so that is where the size work is.

## Where it went

Measured with `ld -map` on a `puts 1` binary, attributed by archive member:

| bytes | component |
|---|---|
| 6,140,527 | `zeo_rt` itself |
| ~1,010,000 | OpenSSL (`ecp_sm2p256_table` 524 KB, `nistz256` 158 KB, `obj_dat` 132 KB, …) |
| ~1,250,000 | the Rust `regex` crate (`regex_syntax` + `regex_automata`) |
| ~470,000 | Oniguruma |
| ~204,000 | prism |
| ~217,000 | encoding_rs |
| ~390,000 | std + core |
| ~180,000 | rustyline |

Inside `zeo_rt`: `builtins` 2.98 MB, `ext` 1.14 MB. A hello reaches almost
none of it.

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

An always-on class stays. A value of that kind can arrive without the program
ever naming it — `1.to_s` needs String — so nothing syntactic rules one out.
A program that can `eval` gets everything, because the embedded compiler can
name any class at all.

**Result: 15,553,816 → 13,061,176 bytes, −2.49 MB.**

## What is left

| | |
|---|---|
| always-on `builtins` | 2.34 MB of Marshal, RubyVM::AST, Complex, Dir, Process, Signal … that a hello cannot reach but nothing yet proves unreachable. Needs real reachability analysis: which classes a VALUE can flow into, not which the source names. A module is the tractable half — it can only be reached by naming it or by a needed class including it. |
| three regex engines | `Engine` is `Fast(regex)` / `Fancy(fancy_regex)` / `Onig`, and Cargo.toml already says Oniguruma "will replace fancy-regex". Retiring the two Rust engines drops 1.24 MB plus encoding_rs and unicode-normalization. It is a performance trade and wants its own bench pass. |
| OpenSSL's provider graph | Self-rooting once anything calls EVP: `evp_generic_fetch` reaches every predefined provider. Gating it at the zeo-rt boundary is the only lever; the class-table work already does this for programs that never require it. |

## A trap this cost a pass on

The tables carry `install_constants` as well as methods, and
`bootstrap::install_core_constants` iterated the slice separately. Pointing
only `registered_table` at the program's array left the constant installers
reading an empty slice, and **180 goldens failed with `uninitialized constant
File::RDWR`** — which reads like a narrowing bug and is not one. Both readers
go through `builtins::all_tables()` now.
