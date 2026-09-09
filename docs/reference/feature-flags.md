# Feature flags

What a build of zeo can leave out, and what changes when it does.

## `ext-*` — one per runtime extension

A stdlib library zeo implements in Rust is an *extension*: a Ruby half under
`crates/zeo-rt/ext/<name>/` beside the Rust that implements it. Each has a
feature.

The default set is the explicit per-extension list, in **both** crates. That
is deliberate: the compiler reads its own `cfg!(feature = "ext-*")` to decide
what a build carries, so a runtime feature the compiler could not see would
make it lie.

Turning one off is the dual-build switch. The loader then serves that gem's
pure-Ruby tree instead of the absent Rust half, or answers `LoadError` if
there is none — never a silent stub.

```console
$ cargo build -p zeo --no-default-features --features pure-stdlib
```

`pure-stdlib` is the named subtraction set: the gems that ship a pure-Ruby
half. `cargo xtask check` builds it, so the C-extension-free build cannot rot.

`ext-all` is the umbrella that turns every `ext-*` on, and it is what
`default` enables. Nothing reads it directly.

## `ambient-*` — what a program starts with

Three libraries can be loaded into every compiled program before its own
first line. Each feature is only the DEFAULT of the matching `--enable=` /
`--disable=` dial (`crates/zeo/src/cli/features.rs`), so a command line
overrides it either way.

| Feature | Dial | What it loads |
|---|---|---|
| `ambient-gems` | `--enable=gems` | `rubygems` |
| `ambient-did-you-mean` | `--enable=did_you_mean` | `did_you_mean` |
| `ambient-error-highlight` | `--enable=error_highlight` | `error_highlight` |

All three are off in the stock build for one reason: an ambient
`require "rubygems"` compiles 241 files into every program, whether or not
it names `Gem`. The default flips once those files are compiled once
instead of once per program.

## `capi`

MRI's C API surface, which is what lets zeo build a gem's own
`ext/**/*.c` from source. Without it a gem that ships C is a `LoadError`
rather than a broken load.

## `runtime-logging`

Installs a `tracing` subscriber **inside a compiled program**, so
`ZEO_LOG=zeo_rt=debug ./prog` prints the runtime's own events.

Off by default, and not for taste: the subscriber is much larger than the
binary-size gate allows. A `zeo prog.rb` run already gets a subscriber from
the compiler's own `main`, so this is only for a linked binary.

## `unit-tables`

Links every builtin method table into the binary. A shipped program installs
its tables from its own descriptor and needs none of this; a unit-test binary
of a crate that depends on `zeo-rt` has no program to read a descriptor from,
and needs it.

Cargo unifies features per invocation, which is why `zeo-capi`'s tests run in
an invocation of their own: sharing one with the `zeo` tests would give the
binary the corpus runs every builtin table, and it would stop noticing a
dropped one.

## Checking a set still builds

```console
$ cargo xtask check --only features-bare
$ cargo xtask check --only pure-stdlib
```

`cargo xtask check` runs the whole matrix: every optional feature off, the C API
alone, the compiler with nothing on, the docs.rs set, and `pure-stdlib`.
