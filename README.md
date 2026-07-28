# zeo — an ahead-of-time Ruby compiler

zeo compiles a whole Ruby program to a **single native executable**. It parses
with [Prism] (CRuby's own parser), runs whole-program analysis over a typed IR,
emits Rust, and links a precompiled runtime. There is no interpreter startup, no
JIT warmup, and no Ruby installation needed on the machine that runs the binary —
you ship one file.

```console
$ cat hello.rb
puts [1, 2, 3].map { |x| x * 2 }.sum
$ zeo hello.rb -o hello
$ ./hello
12
```

zeo's compatibility contract is **oracle-verified behavior, with every
divergence documented** — approximation is fine, silent wrongness is not. The
north star: `zeo` compiles **rubygems and bundler out of the box**.

[Prism]: https://github.com/ruby/prism

## How it works

```
foo.rb ──prism──▶ HIR arena ──analyze──▶ typed classes/MRO ──codegen──▶ Rust ──rustc──▶ foo
                  (whole program:            (compile-time             (quote! +        (links the
                   requires spliced           method tables,            prettyplease)    precompiled
                   at compile time)           type inference)                            zeo-rt runtime)
```

- **Whole-program.** `require`/`require_relative` are resolved at *compile
  time* — the entire program, including bundled pure-Ruby gems under `gems/`, is
  one compilation unit. The front end never defers a file to runtime.
- **Two dispatch paths.** Calls the compiler can resolve statically compile to
  direct Rust calls. Everything else routes through the runtime's real
  `Symbol`-keyed method registry — which also serves `send`, `define_method`,
  `method_missing`, singletons, and runtime-minted classes (`Class.new`).
- **A real runtime** (`zeo-rt`). A CRuby-faithful numeric tower
  (Integer/Bignum/Rational/Complex), strings as bytes + an encoding
  interpretation, Fibers on real stack-switching coroutines, **truly parallel
  threads** (real OS threads, no GVL by default), and exceptions as `Result`
  propagation. `#![forbid(unsafe_code)]` outside the FFI/syscall shims.

The compiler and the runtime **deliberately never link each other**. They agree
only through `zeo-abi` — a numeric `ClassId` for every built-in class, baked
into the generated code as a literal and interpreted by the runtime's dispatch.

## Quick start

zeo is built from source; there is no `gem install zeo`. Build the compiler
once, then compile Ruby programs with it.

```console
# Build the compiler (produces target/release/zeo)
$ cargo build --release -p zeo

# Compile a file to a native binary, then run it.
# NOTE: `zeo foo.rb` COMPILES but does not run — you run the binary yourself.
$ target/release/zeo hello.rb        # writes ./hello (input path, extension stripped)
$ ./hello

# Choose the output path
$ target/release/zeo hello.rb -o build/hello

# Evaluate inline — like `ruby -e`. This one DOES compile *and* run,
# forwarding stdout/stderr and the program's own exit status.
$ target/release/zeo -e 'puts "hello, world"'

# Inspect the generated Rust without building anything
$ target/release/zeo hello.rb -S
```

The one newcomer gotcha: **plain `zeo foo.rb` compiles but does not execute**
the result — only `-e` auto-runs. There are no subcommands (no `zeo run`); the
CLI is shaped after `ruby`'s own (`zeo <file>` / `zeo -e <code>`).

## Command-line reference

```
usage: zeo (<input.rb> | -e <code>) [options]
```

| Argument / flag | Meaning |
|---|---|
| `<input.rb>` | Compile the file to a native binary (default output: input path with its extension stripped). |
| `-e <code>` | Compile **and run** an inline program immediately, forwarding stdout/stderr and exit status. Repeatable (snippets joined with newlines). With `-o`, writes the binary instead of running it. |
| `-o <output>` | Where to write the compiled binary. |
| `-S` | Print the generated Rust source and exit (no build). |
| `-I <dir>` | Add a `require` search root, like ruby's `-I`. Repeatable; attached form `-I<dir>` also works. |
| `--packages <dir>` | An extra gem directory, searched before the default project-local/bundled ones. Repeatable. |
| `--gem-path <dir>` | The external gem store to resolve against. **Requires `--lockfile`.** |
| `--lockfile <path>` | The `Gemfile.lock` resolving `--gem-path` versions. Must be given together with `--gem-path`. |
| `--no-report` | Suppress the `zeo-gems.json` disclosure record (written by default next to an artifact). |
| `--nowarn <slug>` | Suppress a disclosure-warning category. Repeatable; `--nowarn=<slug>` also accepted. |
| `--log-level <level>` | Log the compiler's internals to stderr: `off\|error\|warn\|info\|debug\|trace`. `--log-level=<level>` also accepted; overrides `ZEO_LOG`/`RUST_LOG`. |
| `-h`, `--help` | Show help and exit. |

Environment variables:

| Variable | Effect |
|---|---|
| `ZEO_LOG` / `RUST_LOG` | A `tracing` `EnvFilter` directive for finer control than `--log-level`, e.g. `ZEO_LOG=zeo::analyze=debug,zeo::lower=trace`. With none of these set, no subscriber is installed and compiles stay silent. |
| `ZEO_RUNTIME_PROFILE` | `debug` or `release` — override the linked runtime's profile (default: `debug` for `-e`, `release` for file/`-o` compiles). |
| `ZEO_GVL` | `ZEO_GVL=1` opts a *run* into CRuby-style serialized thread scheduling (a FIFO global lock with 100ms timer preemption) instead of the default truly-parallel OS threads. |
| `ZEO_BLESS` | `ZEO_BLESS=1` re-records golden test output from the real `ruby` oracle (development only). |

## Ruby features

zeo aims at real programs, not a toy subset. What it runs today:

- **Numeric tower** — `Integer`/`Bignum` (auto-promoting, arbitrary precision),
  `Float`, `Rational`, `Complex`, with CRuby-exact coercion and error shapes.
- **Strings & encodings** — strings are bytes plus an encoding interpretation;
  the engine carries 24 encodings (UTF-8, the Windows-125x/ISO-8859 family,
  Shift_JIS/EUC-JP/GBK/Big5 via WHATWG tables). Divergences are catalogued.
- **Core collections** — `Array`, `Hash`, `Range`, `Symbol`, `Struct`, with
  `Enumerable` and `Comparable` as real MRO ancestors driving your `each`/`<=>`.
- **Blocks, procs & yields** — known-shape blocks inline (`3.times` becomes a
  native loop); escaping blocks become a real closure runtime type.
- **Classes & metaprogramming** — inheritance, modules, `include`/`extend`,
  `super`, and genuine open-world dynamic dispatch: `send`, `define_method`,
  `method_missing`, per-object singletons (`def obj.foo`, `class << obj`), and
  `Class.new(Super) { … }` all run through a real runtime method registry.
- **Exceptions** — `raise`/`rescue`/`ensure`/`retry`, propagated as Rust
  `Result` under the hood. Error **messages** are part of the behavior and are
  matched to CRuby's exact wording.
- **Concurrency** — `Thread` on real 8 MiB OS threads, **truly parallel by
  default** (no GVL; killable/raisable busy loops, interruptible `sleep`);
  `Fiber` on real stack-switching coroutines (`corosensei`); `Ractor` with a
  frozen-or-copy boundary; `Mutex`/`Queue`.
- **Regexp** — backed by real engines (`regex`/`fancy-regex`, with Oniguruma
  vendored for the Onigmo-compatible paths).
- **`eval`** — a literal `eval("…")` is parsed and spliced at compile time
  today. A *dynamic* (runtime-computed) `eval` needs the embedded interpreter
  described in [`docs/EVAL_VM.md`](docs/EVAL_VM.md), gated behind the runtime's
  `eval-vm` feature and linked only into programs that can reach it.

This is experimental and moving fast — some corners are still in flight. The
honest source of truth for "what actually matches `ruby`" is the conformance
corpus (below), not this list.

## Compatibility, honestly

zeo targets **CRuby 4.0.6** (the version is single-sourced in `zeo-abi` so the
compiler's version-gate folding and the runtime's `RUBY_VERSION` can never
disagree). Compatibility is expressed as **prose, not a percentage** — a green
corpus run is the record, and a claim like "zeo's `json` is not the `json` gem"
is a fact that can only be stated, never inferred from a score.

- **The conformance suite** (`tests/spinel/`) compiles ~2,368 golden-output
  programs and diffs stdout *and* stderr against real `ruby` as `cargo nextest`
  cases. Known-not-yet-matching programs are tracked as XFAIL gaps in
  `tests/gaps/` — a gap that starts matching `ruby` *fails* the suite, forcing
  its promotion into the corpus.
- **Substitutions are disclosed, never silent.** Where zeo backs a gem or C
  extension with its own implementation (`json`→serde_json, `psych`/`yaml`→
  yaml-rust2, `zlib`→flate2, `digest`/`openssl`→RustCrypto, …), the compile
  warns once and writes a machine-readable `zeo-gems.json` ledger next to the
  artifact recording exactly which libraries diverged and why.
- **Native-only gems fail loudly.** A gem whose real implementation is a C
  extension zeo has no built-in for (`sqlite3`, `nokogiri`, `pg`, …) fails with
  an error that *names the gem* and points at the FFI escape hatch, rather than
  looking like an unsupported language feature.

The full catalogue of substitutions and known divergences is
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md); the extension model is
[`docs/EXTENSIONS.md`](docs/EXTENSIONS.md).

## Architecture — the workspace

zeo is a Cargo workspace (edition 2024, MSRV 1.87) of six crates:

```
crates/
  zeo         the compiler + CLI  — parse ▸ lower ▸ analyze ▸ codegen ▸ backend
  zeo-rt      the runtime linked into every compiled program (the largest crate)
  zeo-abi     zero-dependency leaf: the ClassId numbering both sides share
  zeo-dsl     the shared `syn` grammar for the ruby_class!/ruby_module! DSL
  zeo-macros  the proc-macro that expands that DSL into runtime code
  xtask       dev automation (bench, gem, gem-compat, stdlib-status)
```

- **`zeo`** — the driver. Its front end (`parse/`, `lower/`, `hir.rs`) resolves
  requires and lowers the Prism tree into a typed HIR arena; `analyze/` does
  whole-program ancestor linearization (`mro.rs`) and single-pass local type
  inference (`locals.rs`); `codegen/` emits Rust as a `proc_macro2` TokenStream
  (re-parsed with `syn`, formatted by `prettyplease`); `backend/` shells out to
  `rustc` against the prebuilt runtime, with a content-addressed build cache.
  It is a real library, not just a `main.rs` (see the API section).
- **`zeo-rt`** — the runtime. Every value is one `enum RubyValue`; collections
  are `Arc<Freezable<…>>` (so freezing and structural sharing are cheap) and
  user objects are an erased `Arc<dyn RubyObject>`. Memory is `Arc`
  reference-counting with **no tracing collector** — cycles genuinely leak, a
  documented and accepted trade. One Rust module per Ruby core class lives under
  `builtins/`; method tables are collected at link time via `linkme` into a
  distributed slice indexed by `ClassId`, replacing hand-written match arms.
- **`zeo-abi`** — the zero-dependency leaf. It fixes the numeric `ClassId` of
  every built-in, the `BUILTINS` table (each class's CRuby-exact superclass and
  includes, oracle-verified), `RUNTIME_CLASS_ID_BASE` for runtime-minted
  classes, and `RUBY_VERSION`. This is the *only* hard ABI the compiler and
  runtime share.
- **`zeo-dsl`** / **`zeo-macros`** — the class-authoring DSL (below).
- **`xtask`** — `cargo xtask <cmd>`: `bench` (the golden-output performance
  suite), `gem` (manage the vendored gems in `gems.toml`), `gem-compat` (measure
  how much of a gem store compiles), `stdlib-status`.

### The `ruby_class!` DSL

Core classes are written once, in a Ruby-like grammar whose method bodies stay
real Rust. Here is a slice of `String` (from `crates/zeo-rt/src/builtins/string.rs`):

```rust
ruby_class! {
    String = zeo_abi::STRING_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    def "length" arity 0 | "size" arity 0 (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(crate::string_len(recv_str!(recv))))
    }
    def "empty?" arity 0 (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(crate::string_len(recv_str!(recv)) == 0))
    }
}
```

That single grammar (parsed by `zeo-dsl`) feeds **two** consumers so they can
never drift: `zeo-macros` expands it into the runtime's method fns, `ClassId`-
keyed lookup tables, constant installers, and `linkme` registration; and `zeo`'s
`build.rs` re-parses the *same* invocations to project `CLASS_SURFACE` — the
names the compiler folds `respond_to?`/`is_a?`/const lookups against (headers
only; method bodies are opaque to it). Standard-library C extensions live under
`crates/zeo-rt/src/ext/` behind the same DSL and a two-gate model (a Ruby
`require` gate plus a `cargo` `ext-<name>` feature); see
[`docs/EXTENSIONS.md`](docs/EXTENSIONS.md).

## Public API (embedding the compiler)

`crates/zeo/src/lib.rs` is a real library — the CLI and the in-process test
harness both call it directly. To embed the compiler in Rust:

```rust
use zeo::{compile_to_rust, compile_to_rust_with, CompileOptions};

// Simple: Ruby source in, formatted Rust source out.
let rust: String = compile_to_rust("puts 1 + 1").unwrap();

// Full control: load roots, gem store, disclosure report, warning suppression.
let opts = CompileOptions::default();
let out = compile_to_rust_with("puts 1 + 1", &opts).unwrap();
// out.rust_source        — the generated Rust text
// out.needs_eval_vm      — whether the program can reach the runtime eval VM

// Compile the generated Rust to a native binary against the prebuilt runtime.
zeo::backend::build_binary(/* … */);
```

The public surface includes `CompileOptions`, `CompileOutput`, `CompileError`,
and the pipeline modules (`parse`, `lower`, `analyze`, `codegen`, `backend`,
`compiler`, `hir`, `types`, `diagnostics`, `gem_report`). Note this embeds the
**compiler**; `zeo-rt` is the link target of generated programs, not a "call
Ruby from Rust" host API.

## Building from source

Requirements: **Rust ≥ 1.87** (edition 2024; see `rust-version`), a **C
compiler** (for the vendored Prism and Oniguruma), and — only for re-blessing
goldens from the oracle — a real **Ruby 4.0.6** matching `mise.toml`.

```console
$ git clone https://github.com/ryanseys/zeo && cd zeo
$ cargo build --release -p zeo
$ target/release/zeo yourprogram.rb -o yourprogram
```

The runtime (`zeo-rt`) is built automatically the first time you compile a
program, at most once per (profile, runtime variant, linkage). You can prebuild
it explicitly with `cargo build --release -p zeo-rt` (add `--features eval-vm`
for programs that use dynamic `eval`).

`mise.toml` pins the development toolchain (`ruby = "4.0.6"`, `rust = "1.97.1"`).

## Testing & conformance

The golden-file suites live under `tests/` and run as
[`datatest-stable`](https://crates.io/crates/datatest-stable) `cargo test`/
nextest targets — one case per `.rb` file:

```console
$ cargo nextest run --workspace                   # unit + e2e + all golden suites
$ cargo nextest run -p zeo --test spinel          # the full ruby-oracle corpus
$ cargo nextest run -p zeo --test examples --test gaps
$ ZEO_BLESS=1 cargo test -p zeo --test spinel     # re-record goldens from ruby
$ cargo run -p xtask -- bench                      # golden-output benchmarks
```

- **`spinel`** — the conformance corpus (~2,368 programs), each diffed
  byte-for-byte against real `ruby`.
- **`examples`** — zeo-authored example programs with committed golden output.
- **`gaps`** — the XFAIL tracker: still-diverging programs. A gap that starts
  matching `ruby` fails the suite (an XPASS), forcing promotion into `spinel`.

`ZEO_BLESS=1` is the single golden writer — it records expected output from the
real `ruby` oracle (run with `--disable-error_highlight --disable-did_you_mean`)
instead of asserting. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the house
rule (oracle-verified, divergence-documented) and the full workflow.

## Project layout

```
crates/    the six workspace crates (above)
docs/      design & compatibility docs (COMPATIBILITY, EXTENSIONS, EVAL_VM, …)
tests/     golden-file suites — examples, the spinel corpus, the gaps tracker
gems/      22 vendored pure-Ruby stdlib gems (managed via gems.toml)
bench/     golden-output benchmark programs (run by `cargo xtask bench`)
vendor/    vendored rubygems + shims
tools/     Ruby helper scripts (arity annotation, method coverage)
scripts/   corpus import, gap promotion, ruby-vs-zeo diffing
```

## Status & limitations

Experimental and moving fast. Known limitations, stated plainly:

- **No tracing GC** — `Arc` reference counting means reference cycles leak
  (documented, accepted; `GC.start` opportunistically runs finalizers).
- **Some extensions are scaffolded** — a few `ext/` modules resolve their
  constant and let `require` succeed but `todo!()` on unbuilt methods
  (greppable: `rg 'todo!' crates/zeo-rt/src/ext`).
- **Dynamic `eval` and `Ruby::Box` isolation are in flight** — see `docs/`.
- **Native C-extension gems are unsupported** — the intended escape hatch is
  the real `ffi` gem API, compiled ahead of time (see `docs/EXTENSIONS.md`).

The larger structural pieces in flight are tracked under `docs/`.

## Contributing

Issues and contributions welcome. The one rule is zeo's house style —
*approximation is fine, silent wrongness is not*: new behavior is verified
against real `ruby`, and every intentional divergence gets a comment at the code
site (and, if user-visible, a row in `docs/COMPATIBILITY.md`). See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or
[Apache License 2.0](LICENSE-APACHE), at your option. Unless you explicitly
state otherwise, any contribution intentionally submitted for inclusion in
zeo by you, as defined in the Apache-2.0 license, shall be dual licensed as
above, without any additional terms or conditions.

Vendored components keep their own (compatible) licenses — see
`gems/UPSTREAM.md`, `tests/spinel/UPSTREAM.md`, and
`bench/UPSTREAM.md` for provenance.
