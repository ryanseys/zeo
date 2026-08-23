# Zeo — an ahead-of-time Ruby compiler

Zeo compiles a whole Ruby program to native code. It reads the source with
[Prism], analyzes the entire program at once, lowers it to [Cranelift] IR, and
links a runtime that is already compiled. There is no interpreter to boot and
no JIT to warm up, and the machine that runs the binary needs no Ruby.

```console
$ cat hello.rb
puts [1, 2, 3].map { |x| x * 2 }.sum
$ zeo hello.rb          # runs it, like `ruby hello.rb`
12
$ zeo hello.rb -o hello # or write a native binary
$ ./hello
12
```

Every test compares Zeo's output with real Ruby 4.0.6, byte for byte.

[Prism]: https://github.com/ruby/prism
[Cranelift]: https://cranelift.dev

---

## Status

Zeo is **experimental** and moving fast. What is measured today:

| Gate | Result |
|---|---|
| Conformance corpus (`tests/spinel/`) | 2,924 / 2,924 |
| Example goldens (`tests/*.rb`) | 1,124 / 1,124 |
| End-to-end suite (`crates/zeo-tests/tests/e2e/`) | 1,072 / 1,072 |
| Method census vs Ruby 4.0.6 (`conformance/method-census-gaps.tsv`) | zero gaps |

Each of those compares stdout, stderr and the exit status with real Ruby,
byte for byte. `tests/gaps/` holds the 13 programs that still diverge.

Every difference is recorded — in
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md) when a user can see it, and as
a test in [`tests/gaps/`](tests/gaps) that **must** fail until it is fixed.

See [Limits](#limits) before you depend on Zeo for anything.

---

## Getting started

You need a Rust toolchain (**1.94+**, edition 2024), a **C compiler** (Prism,
Oniguruma, and the link step), and — only to re-record test goldens — **Ruby
4.0.6**. `mise.toml` pins both.

```console
$ git clone https://github.com/ryanseys/zeo && cd zeo
$ cargo build --release -p zeo
```

One cargo build produces two artifacts side by side in `target/release/`:

- **`zeo`** — the compiler and CLI, with the runtime and the JIT linked in.
- **`libzeo.a`** — the runtime archive that `-o` binaries link against.

Then:

```console
# Run a program, like `ruby foo.rb`. Compiles in memory and runs in process.
$ zeo hello.rb

# Write a native binary instead of running.
$ zeo hello.rb -o build/hello
$ zeo hello.rb --compile          # writes ./hello

# Inline code, like `ruby -e`. Works with -o too, so no .rb file is needed.
$ zeo -e 'puts "hello, world"'
$ zeo -e 'puts :ok' -o my_bin

# A literal `eval` is parsed and spliced at COMPILE time -- it costs nothing
# at run time and still sees the surrounding scope.
$ zeo -e "puts eval('1 + 1 + (\"hello\" * 8).length')"
42

# A test file, like `ruby -Itest test/foo_test.rb`.
$ zeo -Itest test/foo_test.rb

# Program arguments go after `--`.
$ zeo test/foo_test.rb -- --seed 42
```

There are no subcommands: the command line follows `ruby`'s. `zeo foo.rb`
compiles **and runs**; a binary artifact is the opt-in (`-o` / `--compile`).

`cargo install zeo` and `gem install zeo` arrive with the 0.1.0 release.

---

## How it works

```
foo.rb ─prism─▶ HIR arena ─analyze─▶ typed classes, MRO,   ─clif─▶ Cranelift IR ─┬─▶ JIT, run in place
                (requires             method tables,                             │   (the default)
                 spliced at           inline-iterator                            └─▶ .o + cc + libzeo.a
                 compile time)        decisions                                      ─▶ a native binary
```

- **One compilation unit.** `require` and `require_relative` resolve at compile
  time — the bundled gems included. The front end never defers a file to run
  time.
- **Two dispatch paths.** A call the compiler can resolve becomes a direct
  call. Everything else goes through the runtime's method registry, keyed by
  `Symbol`. That same registry serves `send`, `define_method`,
  `method_missing`, singletons, refinements, and classes built at run time with
  `Class.new`.
- **Two output modes, one lowering.** Run mode finalizes the same Cranelift IR
  in process (`--backend jit`, the default). `-o` emits an object file and
  links it against `libzeo.a` with the system `cc` (`--backend aot`).
- **A complete runtime** (`zeo-rt`): a CRuby-compatible numeric tower
  (`Integer`/`Bignum`/`Rational`/`Complex`), strings as bytes plus an encoding,
  real coroutine `Fiber`s, real OS-thread `Thread`s, Ruby 4.0's `Ractor` port
  model. `#![forbid(unsafe_code)]` outside the FFI and syscall layers.

The compiler crate and the runtime crate agree only through **`zeo-abi`**,
which assigns a numeric `ClassId` to every built-in class. The compiler bakes
the number into the emitted code; the runtime's dispatch reads it.

---

## Command-line reference

```
usage: zeo [options] [--] (<input.rb> | -e <code>) [args...]
```

Run `zeo --help` for the authoritative list. Every long option also accepts
`--flag=<value>`. An unknown option is an error, and Zeo names the replacement
for any removed spelling.

| Option | Function |
|---|---|
| `<input.rb>` | Compile and run immediately. Trailing arguments become `ARGV`; put option-shaped ones after `--`. |
| `-e <code>` | Compile and run inline code (repeatable; joined with newlines). With `-o`, writes a binary instead. |
| `-o <output>` | Write a native binary here instead of running. |
| `--compile` | Write a native binary at the input path minus its extension. |
| `--backend <jit\|aot>` | Pick the output mode. Default: `jit` when running, `aot` with `-o`. `ZEO_BACKEND` is the env spelling. |
| `-I <dir>` | Add a `require` search root (repeatable; `-I<dir>` and `-I=<dir>` too). |
| `--gems <dir>` | Add a directory of vendored gems — each subdirectory with a `.gemspec` is one gem (repeatable). |
| `--root-gem <name>` | Treat this gem as the root package when several provide the same feature (Bundler-root semantics). |
| `--gem-path <dir>` | An installed RubyGems store (`gem env gemdir`). Needs `--bundle-gemfile`. Defaults to `GEM_PATH`. |
| `--bundle-gemfile <path>` | The Gemfile whose lockfile selects versions in the store. Defaults to `BUNDLE_GEMFILE`. |
| `--report[=<path>]` | Write the `zeo-gems.json` disclosure record (default: beside the artifact). Off by default. |
| `--emit-clif[=<path>]` | Print the Cranelift IR and stop. |
| `-w`, `-W[0-2]`, `-W:[no-]<category>` | Accepted in Ruby's shapes. Zeo emits no warnings of its own, so they change nothing. |
| `-v`, `--version`, `-h`, `--help` | Print and stop. |

**`require` search order** (first gem with a given name wins):

1. `-I` roots in order, then `RUBYLIB`.
2. `--gems` directories in order.
3. The input file's sibling `gems/` directory.
4. Zeo's own bundled gems, then the external gem store.

**Environment:**

| Variable | Function |
|---|---|
| `RUBYOPT` / `RUBYLIB` | As in CRuby. `RUBYOPT` accepts only `-I`, `-w`, `-W`. |
| `GEM_PATH` / `BUNDLE_GEMFILE` | Defaults for `--gem-path` / `--bundle-gemfile`. An ambient store alone never changes a compile. |
| `ZEO_BACKEND` | `jit` or `aot`; the `--backend` flag wins. |
| `ZEO_LOG` / `RUST_LOG` | A `tracing` `EnvFilter` directive, e.g. `zeo::analyze=debug,zeo::lower=trace`. Unset means no subscriber and no output. |
| `ZEO_MEMORY_LIMIT` | Bytes of resident memory a compile may use (default: half of RAM, capped at 8 GiB). A breach exits 12 and names the phase. |
| `ZEO_GVL` | `1` runs threads on CRuby's schedule (a FIFO global lock with a 100 ms timer). The default is parallel OS threads. |

---

## Ruby support

Zeo targets the full language. `zeo-abi` is the single source of the target
version, so the compiler's version checks and the runtime's `RUBY_VERSION`
cannot disagree.

- **Numerics** — unlimited-precision `Integer`, `Float`, `Rational`, `Complex`;
  coercion and error text follow CRuby.
- **Strings and encodings** — bytes plus an encoding; `Encoding.list` reports
  the same 103 encodings as Ruby 4.0.6.
- **Collections** — `Array`, `Hash`, `Range`, `Symbol`, `Struct`, `Data`, `Set`.
  `Enumerable` and `Comparable` are real ancestors that call your `each` and
  `<=>`.
- **Blocks and procs** — a block whose shape is known is inlined (`3.times`
  becomes a native loop); one that escapes becomes a real closure.
- **Classes and metaprogramming** — inheritance, modules, `include`/`extend`/
  `prepend`, `super`, visibility, `alias`, `undef`, refinements, singleton
  classes, `define_method`, `method_missing`, `Class.new`, `Module.new`.
- **Exceptions** — `raise`/`rescue`/`else`/`ensure`/`retry`, `throw`/`catch`,
  `$!`, and CRuby-identical messages and backtraces.
- **Pattern matching** — `case/in`, the `in` predicate and the `=>` binding,
  with CRuby's `NoMatchingPatternError` texts.
- **Concurrency** — parallel OS `Thread`s (8 MiB stacks, killable, interruptible),
  coroutine `Fiber`s, `Ractor` with Ruby 4.0's port model, `Mutex`, `Queue`.
- **Reflection** — `Method#parameters`/`#arity`/`#source_location`,
  `TracePoint`, line coverage, `ObjectSpace`, `RubyVM::AbstractSyntaxTree` and
  `RubyVM::InstructionSequence`, `Ruby::Box`.
- **`eval`** — every `eval` is COMPILED at run time by the same compiler,
  a literal string included ([`docs/EVAL.md`](docs/EVAL.md)). The compiler is
  linked only into programs that can reach it.

For what does not match yet, read
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md); for the extension model,
[`docs/EXTENSIONS.md`](docs/EXTENSIONS.md); for how the census works,
[`docs/METHOD_COVERAGE.md`](docs/METHOD_COVERAGE.md).

---

## Gems, RubyGems and Bundler

Zeo ships the gems in [`gems/`](gems) — 65 of them, including `bundler` and
`rubygems` themselves. A program that requires `csv` gets the copy in this
repository with no `Gemfile` at all. `gems/UPSTREAM.md` records each gem's
origin, version and licence; `gems.toml` pins the ones tracked from git.

To compile an application against its own locked dependencies:

```console
$ bundle install                                  # real Bundler, one time
$ zeo app.rb -o app --gem-path "$(gem env gemdir)" --bundle-gemfile Gemfile
```

Bundler already exports `BUNDLE_GEMFILE` and `GEM_PATH`, so under `bundle exec`
the flags are optional. The lockfile — `Gemfile.lock` or `gems.locked` —
selects the versions; Zeo never resolves versions itself.

For some libraries Zeo substitutes its own implementation (`json` on serde_json,
`psych` on yaml-rust2, `zlib` on flate2, `digest` on RustCrypto, `openssl` on a
vendored OpenSSL 3). `--report` writes a `zeo-gems.json` record naming every
substitution beside the artifact.

A gem with a **C extension** that Zeo has no built-in for fails with a clear
error naming the gem and pointing at the FFI path — never as an unknown
language feature.

---

## Performance

A compiled program starts in **under a millisecond**; CRuby needs roughly 30 ms
before the first line runs. `bench/` holds 58 programs, each with its correct
output, and `cargo xtask bench` verifies the output before it times anything.

Measured 2026-08-21, Zeo and CRuby 4.0.6 timed in the same run:

| | geomean |
|---|---|
| all 58 benchmarks | **1.17× faster than CRuby** |
| the 37 where CRuby takes ≥ 0.10 s | **0.71× — slower** |

Read the second row. The first is inflated by process startup: 21 of the 58
finish inside CRuby's ~30 ms of boot, where a native binary starts instantly.
That is a real advantage of shipping a binary and it is not a claim about
generated code.

**The Cranelift backend is correctness-complete and its performance pass is
not finished.** Two waves have run — they took it to **+52.3%** over the
Rust-emitting backend it replaced — and on compute-bound work it is still
behind CRuby. It wins big where the emitter has a fused
loop or an inline cache (`range_each` 3.27×, `so_mandelbrot` 2.18×,
`attr_accessor` 1.27×) and loses where it does not (`rbtree` 0.25×,
`send_rubyfunc_block` 0.31×, `life` 0.35×, `getivar_module` 0.35×). Those
losses are a ranked list of unbuilt levers, not a mystery: typed iterator
splices, a class-level ivar site, a direct call for a statically-known
receiver class. [`bench/README.md`](bench/README.md) has the method,
[`docs/ROADMAP.md`](docs/ROADMAP.md) the levers.

---

## The workspace

Seven crates, edition 2024, MSRV 1.94.

```
crates/
  zeo         the compiler + CLI: parse ▸ lower ▸ analyze ▸ clif ▸ backend
  zeo-rt      the runtime linked into every compiled program (the largest crate)
  zeo-abi     a dependency-free leaf: the ClassId numbers both sides agree on
  zeo-dsl     the shared `syn` grammar for the ruby_class! / ruby_module! DSL
  zeo-macros  the macro that expands that DSL into runtime code
  zeo-tests   the integration and golden suites (not published)
  xtask       development commands (bench, gem, gem-probe, bless, dist, …)
```

- **`zeo`** — the driver. `parse/` and `lower/` resolve requires and lower the
  Prism tree into a typed HIR arena; `analyze/` computes ancestors, method
  tables, local types and fusion decisions; `clif/` lowers HIR to Cranelift IR;
  `backend/` finalizes it in process (JIT) or emits an object and links it
  (AOT). It is a **library** as well as a binary — see
  [Public API](#public-api).
- **`zeo-rt`** — the runtime. One `enum RubyValue`; collections are
  `Arc<Freezable<…>>` so `freeze` and sharing are cheap; user objects are
  `Arc<dyn RubyObject>`. Memory is reference-counted; an opt-in cycle
  collector (`ZEO_GC=1`) reclaims what refcounting cannot. Each core class is
  one module under `builtins/`, and `linkme` collects their method tables at
  link time.
- **`zeo-abi`** — the only shared contract: `ClassId` numbering, the `BUILTINS`
  hierarchy table, `RUNTIME_CLASS_ID_BASE`, `RUBY_VERSION`, and the C-ABI row
  layouts the two sides pass over.

### The `ruby_class!` DSL

Core classes are declared once in a Ruby-shaped grammar, with Rust bodies:

```rust
ruby_class! {
    String = zeo_abi::STRING_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    def "length" | "size" (recv) {
        Ok(RubyValue::Int(crate::string_len(recv_str!(recv))))
    }
}
```

The parameter list is both the runtime arity check and what `Method#arity`
reports, following CRuby's `(min == max) ? min : -min-1`:

```rust
def "length" (recv)                             // 0
def "index" cfunc (recv, needle, start = nil)   // -1
def "unpack" (recv, fmt, **opts)                // -2
def "insert" (recv, at, *rest)                  // -2, and "expected 1+"
```

Three consumers read the same grammar, so they cannot drift: `zeo-macros`
expands it into runtime functions and lookup tables; `zeo`'s `build.rs` derives
`CLASS_SURFACE`, the names the compiler folds `respond_to?`/`is_a?`/constant
lookups against (headers only — bodies stay invisible to it); and the dev tools
check every arity against what real Ruby reports
(`cargo run -p xtask -- arity-oracle`).

### Public API

`crates/zeo/src/lib.rs` is a library the CLI and the test harness both call:

```rust
use zeo::{compile_to_object_with, compile_to_rust_with, CompileOptions};

let opts = CompileOptions::default();

// The default pipeline: Ruby in, a linkable object file out.
let obj = compile_to_object_with("puts 1 + 1", &opts).unwrap();

// The frozen differential oracle: Ruby in, Rust source out.
let out = compile_to_rust_with("puts 1 + 1", &opts).unwrap();
```

This is the **compiler's** API. `zeo-rt` is a link target for compiled
programs, not a way to call Ruby from Rust.

---

## Development

```console
$ cargo build -p zeo                                    # zeo + libzeo.a
$ cargo nextest run -p zeo-tests --test-threads 4       # goldens, e2e, ratchets
$ cargo nextest run -p zeo-tests -P full                # + whole-gem cases
$ cargo nextest run -p zeo-tests --test spinel          # the conformance corpus
$ cargo run -p xtask -- bless spinel::                  # re-record goldens from ruby
$ cargo run -p xtask -- bench                           # the performance suite
```

Suites are [`datatest-stable`](https://crates.io/crates/datatest-stable)
targets — one case per `.rb` file:

- **`tests/spinel/`** — the conformance corpus, compared with real Ruby byte
  for byte.
- **`tests/*.rb`** — Zeo's own example goldens.
- **`tests/gaps/`** — known divergences. Each **must** fail; when one starts
  agreeing with Ruby the suite goes red and `scripts/promote-gap.sh` moves it.

`ZEO_GOLDEN_BACKEND=aot` runs the goldens through a linked binary instead of
the JIT.

Goldens are only ever written by `cargo run -p xtask -- bless <filter>`, which
runs Ruby with `--disable-error_highlight --disable-did_you_mean`, records
instead of comparing, and reports everything it changed. The filter is
mandatory, so a bless is always scoped. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

### A relocatable install

```console
$ cargo xtask dist      # target/dist/zeo-<version>-<triple>.tar.gz
$ tar xzf zeo-<version>-<triple>.tar.gz -C /usr/local
```

```
zeo-<version>-<triple>/
  bin/zeo
  share/zeo/{gems, runtime, dist-manifest.json}
  share/doc/zeo/
```

The binary finds its payload through `bin/../share/zeo`, so the tree relocates
anywhere; `ZEO_HOME` overrides the search. Building a binary needs a linker
(`cc`) on the target machine, the same requirement any native toolchain has.

---

## Project layout

```
crates/      the seven workspace crates (above)
docs/        COMPATIBILITY, EXTENSIONS, EVAL, GEM_TESTING, METHOD_COVERAGE, ROADMAP
tests/       example goldens, the spinel corpus, the gaps tracker, gemtests drivers
gems/        65 bundled gems (gems.toml pins the git-tracked ones)
bench/       58 benchmark programs; read bench/README.md
conformance/ what the ruby oracle reports, recorded for the drift tests
vendor/      rubygems and fetched test trees (gitignored)
tools/       Ruby helper scripts (the arity oracle, method coverage)
scripts/     corpus import, gap promotion, ruby-against-zeo comparison
```

---

## Limits

Zeo is experimental. The known limits, all of them deliberate and recorded:

- **Cycles are collected only on request.** Memory is reference-counted, so a
  reference cycle leaks unless `ZEO_GC=1` arms the cycle collector, which then
  reclaims one at the next `GC.start`. It is off by default because recording
  every allocation into the registry it walks costs about 1.5% over the bench
  corpus. Armed, it reclaims every cycle `tests/a_cycle_is_reclaimed.rb`
  builds -- among objects, Arrays, Hashes, Structs, exceptions and their
  subclasses, and through a `Proc`'s captured local, a `Range` endpoint, a
  per-object singleton and an ivar written on a bare value. There is no
  automatic trigger and no tracing collector: the pass reconciles reference
  counts rather than tracing, because zeo's lowering declares no stack maps
  for Cranelift to build root sets from.
- **No C-extension gems.** A gem whose native half Zeo has no built-in for
  fails with a clear error. Use the `ffi` gem API, which Zeo compiles ahead of
  time. Source-compatible C extensions are on the roadmap.
- **`Ruby::Box` isolation is partial.** A box works at compile time and at
  run time — `Ruby::Box.new`, `box.eval`, `box.require` and `Box.current` all
  answer — and it isolates constants and globals. What it does not yet isolate
  is a monkeypatch of a shared builtin, which reaches main, and a box cannot
  require a feature the whole-program compile already spliced. Both are
  tracked in [`tests/gaps/`](tests/gaps).
- **Four extensions are partial** — `coverage`, `nkf`, `openssl`, `TracePoint`.
  See [`docs/EXTENSIONS.md`](docs/EXTENSIONS.md).
- **The Cranelift backend's performance pass is not finished.** Correctness
  is complete; code quality is not. Two waves have run and the levers left are
  ranked and measured. On compute-bound work it currently runs at
  **0.71× of CRuby** — see [Performance](#performance).

[`docs/ROADMAP.md`](docs/ROADMAP.md) tracks the rest.

---

## Contributing

Issues and pull requests are welcome. The one rule that matters: **keep every
difference from Ruby visible.**

1. Check each new behaviour against real Ruby.
2. Leave a comment at the site of any difference you accept.
3. If a user can observe it, add a row to `docs/COMPATIBILITY.md` and a failing
   test in `tests/gaps/`.

Read [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache 2.0
([LICENSE-APACHE](LICENSE-APACHE)), at your option.
