# Zeo

Zeo compiles a whole Ruby program to native code. It reads the source with
[Prism], analyzes the entire program at once, lowers it to [Cranelift] IR,
and links `libzeo.a` — Zeo's own Ruby runtime, written in Rust and compiled
ahead of time: the object model, the core classes, the dispatcher, threads,
fibers, and the reference-counted heap. There is no interpreter to boot and
no JIT to warm up. The machine that runs the binary needs no Ruby.

```console
$ cat hello.rb
puts [1, 2, 3].map { |x| x * 2 }.sum
$ zeo hello.rb          # runs it, like `ruby hello.rb`
12
$ zeo -o hello hello.rb # or write a native binary
$ ./hello
12
```

Every test compares Zeo's output with real Ruby 4.0.6, byte for byte.

[Prism]: https://github.com/ruby/prism
[Cranelift]: https://cranelift.dev

---

## Status

Zeo is **experimental** and moving fast. Three golden suites — the
conformance corpus ([`tests/spinel/`](tests/spinel)), the example goldens
([`tests/*.rb`](tests)), and the end-to-end suite
(`crates/zeo/tests/e2e/`) — run green in CI, each case comparing stdout,
stderr, and the exit status with real Ruby, byte for byte.
[`tests/gaps/`](tests/gaps) holds the programs that still
diverge. Each one **must** fail until it is fixed.

Every module, method, constant, and visibility that Ruby 4.0.6 reaches has a
Zeo answer. The census that measured this reached zero rows and was retired.
What remains is behavioral, not missing surface.

Every known difference is recorded: in
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md) when a user can see it, and
as a failing test in `tests/gaps/`.

Read [Limits](#limits) before you depend on Zeo for anything.

---

## Getting started

You need:

- A Rust toolchain, **1.94+** (`rust-toolchain.toml` pins the exact version).
- A **C compiler** (for Prism, Oniguruma, and the link step).
- **Ruby 4.0.6** — only if you re-record test goldens. `mise.toml` pins it.

Build and run:

```console
$ git clone https://github.com/ryanseys/zeo && cd zeo
$ make                       # cargo build --workspace
$ target/debug/zeo -e 'puts "hello"'
hello
```

One cargo build produces two artifacts side by side:

- **`zeo`** — the compiler and CLI, with the runtime and the JIT linked in.
- **`libzeo.a`** — the runtime archive that `-o` binaries link against.

`cargo install zeo` and `gem install zeo` arrive with the 0.1.0 release. A
`cargo install`ed zeo builds its runtime archive once, on the first `zeo -o`:
`cargo install` copies binaries and nothing else, and the archive is 78 MB
against crates.io's 10 MB crate limit. The release tarball and the platform
gems carry the archive and never do this.

---

## Using Zeo

There are no subcommands. The command line follows `ruby`'s. `zeo foo.rb`
compiles **and runs**; a binary artifact is the opt-in (`-o` / `--compile`).

```console
# Run a program, like `ruby foo.rb`. Compiles in memory, runs in process.
$ zeo hello.rb

# Write a native binary instead of running.
$ zeo -o build/hello hello.rb
$ zeo --compile hello.rb          # writes ./hello

# Inline code, like `ruby -e`. Works with -o too.
$ zeo -e 'puts "hello, world"'
$ zeo -e 'puts :ok' -o my_bin

# A test file, like `ruby -Itest test/foo_test.rb`.
$ zeo -Itest test/foo_test.rb

# Everything after the file name is the program's ARGV, as in ruby.
$ zeo test/foo_test.rb --seed 42 --verbose
```

Every `eval` is a real run-time compile: the embedded compiler ships inside
the binary. That is why a program that can reach `eval` costs about 14 MB of
artifact.

```console
$ zeo -e "puts eval('1 + 1 + (\"hello\" * 8).length')"
42
```

### Compile an app against its locked gems

Zeo ships its bundled gems in [`gems/`](gems) — `bundler` and `rubygems` included. A
program that requires `csv` gets the copy in this repository, with no
`Gemfile` at all. `gems/UPSTREAM.md` records each gem's origin, version, and
license; `upstream.lock` pins the ones tracked from git.

To compile against your own locked dependencies instead:

```console
$ bundle install                                  # real Bundler, one time
$ zeo -o app --gem-path "$(gem env gemdir)" --bundle-gemfile Gemfile app.rb
```

Bundler already exports `BUNDLE_GEMFILE` and `GEM_PATH`, so under
`bundle exec` the flags are optional. The lockfile (`Gemfile.lock` or
`gems.locked`) selects the versions. Zeo never resolves versions itself.

For some libraries Zeo substitutes its own implementation (`json` on
serde_json, `psych` on yaml-rust2, `zlib` on flate2, `digest` on RustCrypto,
`openssl` on a vendored OpenSSL 3). `--report` writes a `zeo-gems.json`
record beside the artifact that names every substitution.

A gem that ships its **C extension as source** is compiled from that source:
Zeo runs the gemspec's `extconf.rb`, reads the Makefile mkmf writes, and
compiles and links without `make`. A gem that ships a *precompiled* `.so`
can never load — that object is built against CRuby's ABI — and Zeo says so,
naming the gem and the fix (install the ruby-platform variant). See
[`docs/EXTENSIONS.md`](docs/EXTENSIONS.md).

---

## Command-line reference

```
usage: zeo [options] (<input.rb> [args...] | -e <code> [--] [args...])
```

Run `zeo --help` for the authoritative list. Every long option also accepts
`--flag=<value>`. An unknown option is an error, and Zeo names the
replacement for any removed spelling.

| Option | Function |
|---|---|
| `<input.rb>` | Compile and run immediately. Option parsing **stops here**, as in ruby: everything after the file name becomes `ARGV`, so `zeo test.rb --seed 42` works. Zeo's own options go **before** the file. |
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
| `--dump=<kind>` | Inspect instead of building, then stop. `clif` is `--emit-clif` to stdout; `syntax` prints `Syntax OK` (`-c` is the short spelling); `units` prints the compiled-in load path; `classes` prints every class with the body sites that reveal it. `insns` and `parsetree` are refused. |
| `-w`, `-W[0-2]`, `-W:[no-]<category>` | Accepted in Ruby's shapes. Zeo emits no warnings of its own, so they change nothing. |
| `-v`, `--version`, `-h`, `--help` | Print and stop. |

**`require` search order** (the first gem with a given name wins):

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
| `ZEO_CACHE` | `0` turns the compiled-program cache off, so the run compiles from scratch. |
| `ZEO_PROGRAM_CACHE` | Where cached programs live (default: `<build root>/programs`). |
| `ZEO_LOG` / `RUST_LOG` | A `tracing` `EnvFilter` directive, e.g. `zeo::analyze=debug`. Unset means no subscriber and no output. |
| `ZEO_MEMORY_LIMIT` | Bytes of resident memory a compile may use (default: half of RAM, capped at 8 GiB). A breach exits 12 and names the phase. |
| `ZEO_GVL` | `1` runs threads on CRuby's schedule (a FIFO global lock with a 100 ms timer). The default is parallel OS threads. |
| `ZEO_GC` | `1` arms the cycle collector — see [Limits](#limits). |

---

## How it works

```
foo.rb ─prism─▶ HIR arena ─analyze─▶ typed classes, MRO,   ─clif─▶ Cranelift IR ─┬─▶ JIT, run in place
                (requires             method tables,                             │   (the default)
                 spliced at           inline-iterator                            └─▶ .o + cc + libzeo.a
                 compile time)        decisions                                      ─▶ a native binary
```

- **One compilation unit.** `require` and `require_relative` resolve at
  compile time, the bundled gems included. The front end never defers a file
  to run time.
- **Two dispatch paths.** A call the compiler can resolve becomes a direct
  call. Everything else goes through the runtime's method registry, keyed by
  `Symbol`. That same registry serves `send`, `define_method`,
  `method_missing`, singletons, refinements, and classes built at run time
  with `Class.new`.
- **Two output modes, one lowering.** `-o` emits an object file and links it
  against `libzeo.a` with the system `cc` (`--backend aot`). Run mode either
  finalizes the same Cranelift IR in process (`--backend jit`) or links a
  binary and `exec`s it.
- **A program compiles once.** `zeo foo.rb` links its binary into a cache and
  runs it; the next run with the same sources skips straight to the `exec`,
  so `zeo gem --version` costs 0.4s rather than 4.9s. A cache entry is keyed
  by the compile's inputs and checked against them, and anything the cache
  cannot answer -- an edited file, a program the object backend declines --
  falls back to compiling. `ZEO_CACHE=0` turns it off.
- **A complete runtime** (`zeo-rt`): a CRuby-compatible numeric tower
  (`Integer`/`Bignum`/`Rational`/`Complex`), strings as bytes plus an
  encoding, real coroutine `Fiber`s, real OS-thread `Thread`s, and Ruby 4.0's
  `Ractor` port model. `#![forbid(unsafe_code)]` outside the FFI and syscall
  layers.

The compiler crate and the runtime crate agree only through **`zeo-abi`**,
which assigns a numeric `ClassId` to every built-in class. The compiler bakes
the number into the emitted code; the runtime's dispatch reads it.

### Ruby support

Zeo targets the full language. `zeo-abi` is the single source of the target
version, so the compiler's version checks and the runtime's `RUBY_VERSION`
cannot disagree.

- **Numerics** — unlimited-precision `Integer`, `Float`, `Rational`,
  `Complex`; coercion and error text follow CRuby.
- **Strings and encodings** — bytes plus an encoding; `Encoding.list`
  reports the same 103 encodings as Ruby 4.0.6.
- **Collections** — `Array`, `Hash`, `Range`, `Symbol`, `Struct`, `Data`,
  `Set`. `Enumerable` and `Comparable` are real ancestors that call your
  `each` and `<=>`.
- **Blocks and procs** — a block whose shape is known is inlined (`3.times`
  becomes a native loop); one that escapes becomes a real closure.
- **Classes and metaprogramming** — inheritance, modules,
  `include`/`extend`/`prepend`, `super`, visibility, `alias`, `undef`,
  refinements, singleton classes, `define_method`, `method_missing`,
  `Class.new`, `Module.new`.
- **Exceptions** — `raise`/`rescue`/`else`/`ensure`/`retry`,
  `throw`/`catch`, `$!`, and CRuby-identical messages and backtraces.
- **Pattern matching** — `case/in`, the `in` predicate and the `=>` binding,
  with CRuby's `NoMatchingPatternError` texts.
- **Concurrency** — parallel OS `Thread`s (8 MiB stacks, killable,
  interruptible), coroutine `Fiber`s, `Ractor` with Ruby 4.0's port model,
  `Mutex`, `Queue`.
- **Reflection** — `Method#parameters`/`#arity`/`#source_location`,
  `TracePoint`, line coverage, `ObjectSpace`,
  `RubyVM::AbstractSyntaxTree`, `RubyVM::InstructionSequence`, `Ruby::Box`.
- **`eval`** — every `eval` is COMPILED at run time by the same compiler, a
  literal string included ([`docs/EVAL.md`](docs/EVAL.md)). The compiler is
  linked only into programs that can reach it.

For what does not match yet, read
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md); for the extension model,
[`docs/EXTENSIONS.md`](docs/EXTENSIONS.md).

---

## Performance

A compiled program starts in **under a millisecond**. `bench/` holds 61
programs, each with its
correct output, and the criterion bench harness (`make bench`) verifies the
output before it times anything.

Measured 2026-08-25 on one machine, against CRuby 4.0.6 (the Zeo rows from
that day's full run; the CRuby rows from a combined Zeo + Ruby run earlier
the same day — the committed [`bench/results.tsv`](bench/results.tsv) is
the record):

| | geomean |
|---|---|
| all 61 benchmarks | **1.77× — faster than CRuby** |
| the 40 where CRuby takes ≥ 0.10 s | **1.19× — faster** |

The first row is inflated by process startup: about a third of the programs
finish before CRuby's interpreter is done booting, where a native binary
starts instantly. That is a real advantage of shipping a binary, but the second
row is the claim about generated code: on the compute-bound set Zeo wins
22 of 40 (`range_each` 8.0×, `so_mandelbrot` 4.9×, `nested_loop` 4.0×,
`matmul` 3.0×, `sieve` 2.4×). Release builds made with `cargo xtask dist
--pgo` (profile-guided optimization, what the shipped artifacts use) run
another ~10% faster than the release profile these numbers were taken on.

The losses that remain cluster in two shapes, and both are named, ranked
levers rather than mysteries. Programs whose hot receiver the compiler
cannot statically type (`tree_walker` 0.07×, `linked_list` 0.38×,
`rbtree` 0.62× — pointer-chasing object graphs) are waiting on wider type
inference: the typed direct call that took `send_rubyfunc_block` from
0.30× to 1.2× only fires where the receiver's class is proven at
compile time. And `getivar_module` (0.39×) waits on a class-level ivar
cache the emitter does not yet emit. [`bench/README.md`](bench/README.md)
has the method; [`docs/ROADMAP.md`](docs/ROADMAP.md) has the levers.

### The full table

Sorted fastest-relative-to-Ruby first. Rows marked \* finish within
CRuby's startup time, so they mostly measure a native binary starting
instantly, not generated code.

| benchmark | Zeo (s) | Ruby (s) | vs Ruby |
|---|---|---|---|
| `bigint_fib` \* | 0.003 | 0.031 | **11.2× faster** |
| `pidigits` \* | 0.003 | 0.032 | **11.1× faster** |
| `poly_cells` \* | 0.003 | 0.032 | **11.0× faster** |
| `sinatra_mini` \* | 0.003 | 0.032 | **10.6× faster** |
| `jekyll_lite` \* | 0.003 | 0.032 | **10.3× faster** |
| `str_concat` \* | 0.003 | 0.032 | **9.9× faster** |
| `micro_lisp` \* | 0.003 | 0.032 | **9.4× faster** |
| `range_each` | 1.828 | 14.715 | **8.0× faster** |
| `fannkuch` \* | 0.005 | 0.035 | **7.5× faster** |
| `wordfreq` \* | 0.005 | 0.034 | **6.8× faster** |
| `fasta` \* | 0.005 | 0.035 | **6.4× faster** |
| `nbody` \* | 0.007 | 0.036 | **5.1× faster** |
| `so_mandelbrot` | 0.200 | 0.982 | **4.9× faster** |
| `spectral_norm` \* | 0.014 | 0.059 | **4.2× faster** |
| `nested_loop` | 0.104 | 0.417 | **4.0× faster** |
| `send_cfunc_block` | 0.245 | 0.771 | **3.1× faster** |
| `matmul` | 0.106 | 0.319 | **3.0× faster** |
| `mandel_term` \* | 0.015 | 0.043 | **3.0× faster** |
| `sort_by` \* | 0.013 | 0.038 | **2.9× faster** |
| `keyword_args` | 0.060 | 0.157 | **2.6× faster** |
| `sudoku` | 0.047 | 0.111 | **2.4× faster** |
| `sieve` | 0.186 | 0.439 | **2.4× faster** |
| `send_bmethod` | 0.075 | 0.174 | **2.3× faster** |
| `fib` | 0.192 | 0.425 | **2.2× faster** |
| `nqueens` | 0.093 | 0.192 | **2.1× faster** |
| `tak` | 0.190 | 0.394 | **2.1× faster** |
| `loops_times` | 0.295 | 0.602 | **2.0× faster** |
| `tarai` | 0.156 | 0.290 | **1.9× faster** |
| `ackermann` | 0.195 | 0.334 | **1.7× faster** |
| `getivar` \* | 0.055 | 0.095 | **1.7× faster** |
| `attr_accessor` | 0.529 | 0.863 | **1.6× faster** |
| `partial_sums` | 0.461 | 0.745 | **1.6× faster** |
| `object_new` | 0.066 | 0.104 | **1.6× faster** |
| `binary_trees` \* | 0.034 | 0.050 | **1.5× faster** |
| `huffman` \* | 0.045 | 0.066 | **1.5× faster** |
| `object_new_no_escape` | 0.141 | 0.206 | **1.5× faster** |
| `structaref` | 0.119 | 0.170 | **1.4× faster** |
| `object_new_init` | 0.121 | 0.145 | **1.2× faster** |
| `send_rubyfunc_block` | 0.435 | 0.520 | **1.2× faster** |
| `structaset` | 0.158 | 0.153 | ≈ parity |
| `io_wordcount` \* | 0.081 | 0.077 | 1.1× slower |
| `template` | 0.642 | 0.574 | 1.1× slower |
| `csv_process` | 0.581 | 0.512 | 1.1× slower |
| `inline` | 1.106 | 0.971 | 1.1× slower |
| `throw` | 0.210 | 0.181 | 1.2× slower |
| `life` | 0.627 | 0.537 | 1.2× slower |
| `splay` | 0.167 | 0.141 | 1.2× slower |
| `gcbench` | 2.629 | 2.159 | 1.2× slower |
| `ruby_xor` | 1.177 | 0.953 | 1.2× slower |
| `ao_render` | 2.181 | 1.710 | 1.3× slower |
| `setivar_object` \* | 0.083 | 0.064 | 1.3× slower |
| `json_parse` | 0.332 | 0.252 | 1.3× slower |
| `setivar_young` \* | 0.083 | 0.063 | 1.3× slower |
| `setivar` \* | 0.083 | 0.063 | 1.3× slower |
| `stark_field` | 1.118 | 0.781 | 1.4× slower |
| `rbtree` | 0.564 | 0.349 | 1.6× slower |
| `so_lists` | 0.630 | 0.270 | 2.3× slower |
| `getivar_module` | 1.738 | 0.679 | 2.6× slower |
| `linked_list` | 0.592 | 0.226 | 2.6× slower |
| `tree_walker_frames` | 3.392 | 0.266 | 12.8× slower |
| `tree_walker` | 5.037 | 0.374 | 13.5× slower |

---

## The workspace

Six crates, edition 2024, MSRV 1.94.

```
crates/
  zeo         the compiler + CLI: parse ▸ lower ▸ analyze ▸ clif ▸ backend
  zeo-rt      the runtime linked into every compiled program (the largest crate)
  zeo-abi     a dependency-free leaf: the ClassId numbers both sides agree on
  zeo-dsl     the shared `syn` grammar for the ruby_class! / ruby_module! DSL
  zeo-macros  the macro that expands that DSL into runtime code
  xtask       the repo's own chores (`cargo xtask`); not published
```

- **`zeo`** — the driver. `parse/` and `lower/` resolve requires and lower
  the Prism tree into a typed HIR arena; `analyze/` computes ancestors,
  method tables, local types, and fusion decisions; `clif/` lowers HIR to
  Cranelift IR; `backend/` finalizes it in process (JIT) or emits an object
  and links it (AOT). It is a **library** as well as a binary — see
  [Public API](#public-api).
- **`zeo-rt`** — the runtime. One `enum RubyValue`; collections are
  `Arc<Freezable<…>>` so `freeze` and sharing are cheap; user objects are
  `Arc<dyn RubyObject>`. Memory is reference-counted; an opt-in cycle
  collector (`ZEO_GC=1`) reclaims what refcounting cannot. Each core class
  is one module under `builtins/` and exports its method table under a
  `zeo_ctable_<ID>` symbol. A compiled program names that symbol only if it
  can reach the class, so a binary links the classes it can use and no more.
- **`zeo-abi`** — the only shared contract: `ClassId` numbering, the
  `BUILTINS` hierarchy table, `RUNTIME_CLASS_ID_BASE`, `RUBY_VERSION`, and
  the C-ABI row layouts the two sides pass over.

### The `ruby_class!` DSL

Core classes are declared once, in a Ruby-shaped grammar with Rust bodies:

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
expands it into runtime functions and lookup tables, and `zeo`'s `build.rs`
derives `CLASS_SURFACE` — the names the compiler folds
`respond_to?`/`is_a?`/constant lookups against (headers only; bodies stay
invisible to it). A def's parameter list is the only place its shape is
written.

### Public API

`crates/zeo/src/lib.rs` is a library that the CLI and the test harness both
call:

```rust
use zeo::{compile_to_clif_text, compile_to_object_with, CompileOptions};

let opts = CompileOptions::default();

// The default pipeline: Ruby in, a linkable object file out.
let obj = compile_to_object_with("puts 1 + 1", &opts).unwrap();

// The same lowering, stopped one stage earlier: Ruby in, Cranelift IR out.
let clif = compile_to_clif_text("puts 1 + 1", &opts).unwrap();
```

This is the **compiler's** API. `zeo-rt` is a link target for compiled
programs, not a way to call Ruby from Rust.

---

## Development

The Makefile is the front door. Every recipe is one blessed invocation, and
CI calls the same targets, so the two cannot drift.

```console
$ make            # build the workspace (zeo + libzeo.a)
$ make test       # the dev loop: unit + e2e + golden suites
$ make check      # clippy at CI's severity
$ make gate       # everything: the CI legs, whole-gem cases, doctests, bench
$ make linux      # the Linux container verification loop (needs podman)
```

Narrower runs go through cargo and the dev CLI directly:

```console
$ cargo nextest run -p zeo --test goldens       # every golden corpus
$ cargo xtask bless spinel::                  # re-record goldens from ruby
$ make bench                                    # the performance suite (criterion)
$ make ci-size                                   # the linked-binary size gate
```

Suites are [`datatest-stable`](https://crates.io/crates/datatest-stable)
targets — one case per `.rb` file:

- **`tests/spinel/`** — the conformance corpus, compared with real Ruby byte
  for byte.
- **`tests/*.rb`** — Zeo's own example goldens.
- **`tests/gaps/`** — known divergences. Each **must** fail; when one starts
  agreeing with Ruby the suite goes red and `cargo xtask promote-gap`
  moves it.

`ZEO_GOLDEN_BACKEND=aot` runs the goldens through a linked binary instead of
the JIT.

Goldens are only ever written by `cargo xtask bless <filter>`, which runs
Ruby with `--disable-error_highlight --disable-did_you_mean`, records instead
of comparing, and reports everything it changed. The filter is mandatory, so
a bless is always scoped. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

### A relocatable install

```console
$ cargo xtask dist        # target/dist/zeo-<version>-<triple>.tar.gz
$ tar xzf zeo-<version>-<triple>.tar.gz -C /usr/local
```

```
zeo-<version>-<triple>/
  bin/zeo
  share/zeo/{gems, lib/<triple>/libzeo.a, dist-manifest.json}
  share/doc/zeo/
```

The binary finds its payload through `bin/../share/zeo`, so the tree
relocates anywhere; `ZEO_HOME` overrides the search. Building a binary needs
a linker (`cc`) on the target machine — the same requirement any native
toolchain has.

`libzeo.a` **is** the payload. `zeo -o` links a compiled program against it,
so a tree without it can run programs but compile none. The two paths fail
differently, which is worth knowing when an install misbehaves: `zeo file.rb`
and `zeo -e` go through the JIT and never touch the archive; `zeo -o` cannot
proceed without it. That is why the archive, not the gems, is what
`share/zeo` is checked for.

Releases are built **natively on each platform** — `aarch64-apple-darwin`,
`x86_64-apple-darwin`, and both Linux triples. Zeo does not cross-compile to
Linux: `libzeo.a` bundles Prism, Oniguruma, OpenSSL, and libffi; glibc
cannot be redistributed; and a cross build cannot smoke-test itself — it
could not run the program it just compiled. From a Mac, build a Linux
tarball in the container instead:

```console
$ podman build --platform linux/arm64 -t zeo-linux .
$ cargo xtask linux dist      # target/dist/zeo-<version>-<triple>.tar.gz
```

The artifact is genuinely natively built, so a cross-compile divergence is
structurally impossible.

### Project layout

```
crates/      the six workspace crates (above)
docs/        BINARY_SIZE, CLIF, COMPATIBILITY, EVAL, EXTENSIONS,
             ROADMAP
tests/       example goldens, the spinel corpus, the gaps tracker, gemtests
gems/        67 bundled gems (upstream.lock pins the git-tracked ones)
bench/       61 benchmark programs; read bench/README.md
vendor/      rubygems and fetched test trees (gitignored)
Makefile     the front door: make / test / check / gate / linux
Dockerfile   the linux verification image (`cargo xtask linux`)
```

---

## Limits

Zeo is experimental. The known limits, all deliberate and recorded:

- **Cycles are collected only on request.** Memory is reference-counted, so
  a reference cycle leaks unless `ZEO_GC=1` arms the cycle collector, which
  then reclaims cycles at the next `GC.start`. It is off by default because
  recording every allocation costs about 1.5% over the bench corpus. There
  is no automatic trigger and no tracing collector: the pass reconciles
  reference counts, because Zeo's lowering declares no stack maps for
  Cranelift to build root sets from.
- **C extensions are source-only.** A gem that ships its C source compiles
  and loads — `fast_blank`, `bcrypt`, and `msgpack` all build and answer,
  and bcrypt reproduces a published OpenBSD test vector byte for byte. All
  but 33 of the `rb_*` entry points a gem can link against are answered, and
  each of those 33 raises with its reason (they boot an interpreter, or
  expose a representation Zeo does not have). A gem shipping a *precompiled*
  `.so` never loads: that object is CRuby's ABI. Autotools and
  `mini_portile` builds of a vendored C library are out of scope;
  `have_library` works. Loading an extension arms the GVL process-wide.
- **`Ruby::Box` isolation is partial.** A box works at compile time and at
  run time — `Ruby::Box.new`, `box.eval`, `box.require`, and `Box.current`
  all answer — and it isolates constants and globals. It does not yet
  isolate a monkeypatch of a shared builtin (which reaches main), and a box
  cannot require a feature the whole-program compile already spliced. Both
  are tracked in [`tests/gaps/`](tests/gaps).
- **Four extensions are partial** — `coverage`, `nkf`, `openssl`,
  `TracePoint`. See [`docs/EXTENSIONS.md`](docs/EXTENSIONS.md).
- **The Cranelift backend's performance pass is not finished.** Correctness
  is complete; code quality is not. On compute-bound work it currently runs
  at **0.71× of CRuby** — see [Performance](#performance).

[`docs/ROADMAP.md`](docs/ROADMAP.md) tracks the rest.

---

## Contributing

Issues and pull requests are welcome. The one rule that matters: **keep
every difference from Ruby visible.**

1. Check each new behavior against real Ruby.
2. Leave a comment at the site of any difference you accept.
3. If a user can observe it, add a row to `docs/COMPATIBILITY.md` and a
   failing test in `tests/gaps/`.

Read [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache 2.0
([LICENSE-APACHE](LICENSE-APACHE)), at your option.
