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

- **The conformance suite** (`tests/spinel/`) compiles ~2,509 golden-output
  programs and diffs stdout *and* stderr against real `ruby` as `cargo nextest`
  cases. Known-not-yet-matching programs are tracked as XFAIL gaps in
  `tests/gaps/` — a gap that starts matching `ruby` *fails* the suite, forcing
  its promotion into the corpus.
- **Substitutions are disclosed, never silent.** Where zeo backs a gem or C
  extension with its own implementation (`json`→serde_json, `psych`/`yaml`→
  yaml-rust2, `zlib`→flate2, `digest`→RustCrypto, `openssl`→a vendored
  OpenSSL 3, …), the compile
  warns once and writes a machine-readable `zeo-gems.json` ledger next to the
  artifact recording exactly which libraries diverged and why.
- **Native-only gems fail loudly.** A gem whose real implementation is a C
  extension zeo has no built-in for (`sqlite3`, `nokogiri`, `pg`, …) fails with
  an error that *names the gem* and points at the FFI escape hatch, rather than
  looking like an unsupported language feature.

The full catalogue of substitutions and known divergences is
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md); the extension model is
[`docs/EXTENSIONS.md`](docs/EXTENSIONS.md); what is still owed is
[`docs/TODO.md`](docs/TODO.md).

## Bundled gems

51 gems ship under `gems/` and resolve without a `Gemfile` — `require "csv"`
in a program zeo compiles finds the copy in this repo. **Origin** says where
the Ruby source came from:

- **git-pinned** — fetched by `cargo xtask gem` at a `rev` recorded in
  `gems.toml`, the reproducible pin.
- **upstream** — file-copied verbatim from ruby 4.0.6's default and bundled
  gems.
- **upstream +zeo** — the same, with deviations marked `zeo:` at each site.
- **zeo Ruby half** — the `.rb` is zeo's; the native half is a Rust extension
  under `crates/zeo-rt/src/ext/` (the split CRuby makes between `rubylibdir`
  and `archdir`).

**Verified by** names a golden test that compiles the gem and diffs its output
against real ruby 4.0.6, byte for byte. A `—` means no golden asserts this
gem's behaviour on its own — several are exercised only as another gem's
dependency. See `gems/UPSTREAM.md` for full provenance and licensing.

| gem | version | origin | verified by | divergences |
|---|---|---|---|---|
| abbrev | 0.1.2 | git-pinned | `issue_abbrev_missing.rb` | — |
| benchmark | 0.4.1 | git-pinned | `issue_benchmark_missing.rb` | — |
| bigdecimal | 4.1.2 | upstream +zeo | `bigdecimal.rb` | native slice reimplemented ([compat](docs/COMPATIBILITY.md)) |
| bundler | 4.0.16 | git-pinned | `gem_bundler.rb` | golden enters at `bundler/version` ([todo](docs/TODO.md)) |
| csv | 3.3.6 | git-pinned | `gem_csv.rb` | — |
| delegate | 0.6.1 | upstream | `issue_require_delegate_crashes.rb` | — |
| drb | 2.2.3 | git-pinned | `gem_drb.rb` | — |
| English | 0.8.1 | upstream | `english_special_globals.rb` | — |
| erb | 6.0.6 | git-pinned | `erb_module_function.rb` | — |
| ffi | 1.17.4 | zeo Ruby half | `ffi_struct.rb` | native half over `libffi` |
| fiddle | 1.1.8 | upstream +zeo | `fiddle.rb` | `Importer` DSL not vendored ([compat](docs/COMPATIBILITY.md)) |
| fileutils | 1.8.0 | git-pinned | `fileutils.rb` | — |
| find | 0.2.0 | git-pinned | `issue_find_missing.rb` | — |
| forwardable | 1.4.0 | upstream | `issue_3300_forwardable.rb` | — |
| ipaddr | 1.2.9 | git-pinned | — | — |
| irb | 1.18.0 | upstream | — | compiles; stops at an anonymous runtime refinement ([gap](tests/gaps/issue_runtime_refinement_module.rb)) |
| json | 2.18.0 | zeo Ruby half | `json_to_json.rb` | `serde_json`, not the json gem ([compat](docs/COMPATIBILITY.md)) |
| logger | 1.7.0 | git-pinned | `issue_logger_missing.rb` | — |
| minitest | 6.0.6 | upstream | `gem_minitest.rb` | `autorun` compiles in the `MT_HELL` branch |
| monitor | 0.1.0 | zeo Ruby half | `gem_two_halves.rb` | `Monitor` + `MonitorMixin` only |
| net-ftp | 0.3.9 | git-pinned | `issue_net_ftp_missing.rb` | — |
| net-http | 0.9.1 | git-pinned | `gem_net_http.rb` | — |
| net-protocol | 0.2.2 | git-pinned | — | — |
| net-smtp | 0.5.1 | git-pinned | `gem_net_smtp.rb` | — |
| nkf | 0.3.0 | zeo Ruby half | `nkf.rb` | option subset; `guess` reimplemented ([compat](docs/COMPATIBILITY.md)) |
| observer | 0.1.2 | git-pinned | `gem_observer.rb` | — |
| open3 | 0.2.1 | git-pinned | `open3_capture.rb` | — |
| openssl | 4.0.2 | zeo Ruby half | `openssl_cipher.rb` + 6 more | PKey generation, X509 issuance, `SSLServer` declined ([compat](docs/COMPATIBILITY.md)) |
| optparse | 0.8.1 | zeo Ruby half | `optparse_subset.rb` | the common `OptionParser` surface |
| ostruct | 0.6.3 | upstream | `issue_3331_poly_to_sym_arm.rb` | — |
| pp | 0.6.4 | upstream | `pp_pretty_print.rb` | — |
| prettyprint | 0.2.0 | upstream | — | — |
| prism | 1.9.0 | upstream +zeo | `gem_prism.rb` | `translation/` and `ffi.rb` not vendored |
| psych | 5.4.0 | zeo Ruby half | `psych_load_file_and_stream.rb` | `yaml-rust2`, not libyaml ([compat](docs/COMPATIBILITY.md)) |
| pty | 0.5.9 | zeo Ruby half | `pty_spawn.rb` | — |
| racc | 1.8.1 | git-pinned | `issue_racc_parser_missing.rb` | — |
| reline | 0.6.3 | upstream +zeo | `reline_line_editor.rb` | one `zeo:` deviation in `io.rb` |
| resolv | 0.7.1 | git-pinned | `issue_resolv_missing.rb` | — |
| rubygems | 4.0.16 | git-pinned | `gem_rubygems.rb` | golden enters below the umbrella require ([todo](docs/TODO.md)) |
| shellwords | 0.2.2 | upstream | `shellwords.rb` | — |
| singleton | 0.3.0 | upstream | — | `singleton_class.include?` ([gap](tests/gaps/issue_singleton_class_include_after_extend.rb)) |
| strscan | 3.1.6 | zeo Ruby half | `strscan_capture_surface.rb` | a reimplementation ([compat](docs/COMPATIBILITY.md)) |
| syslog | 0.4.0 | zeo Ruby half | `syslog.rb` | — |
| tempfile | 0.3.1 | git-pinned | `issue_require_tempfile_codegen_path_attr.rb` | — |
| time | 0.4.1 | git-pinned | `time_parse.rb` | — |
| timeout | 0.6.1 | upstream | — | — |
| tmpdir | 0.3.1 | git-pinned | `io_encoding.rb` | — |
| tsort | 0.2.0 | upstream | — | — |
| un | 0.3.0 | git-pinned | `issue_un_missing.rb` | — |
| uri | 1.1.1 | git-pinned | `uri_parse_and_build.rb` | — |
| zlib | 3.2.3 | zeo Ruby half | `zlib_classes.rb` | `flate2`; four entry points declined ([compat](docs/COMPATIBILITY.md)) |

A gem outside this set resolves from an external store with `--gem-path` +
`--lockfile`. A gem whose real implementation is a C extension zeo has no
built-in for fails with a message that names it: `is_known_native_gem`
(`crates/zeo/src/parse/loader.rs`) lists 14 such names — `sqlite3`,
`nokogiri`, `pg`, `mysql2`, `bcrypt`, `nio4r`, `puma`, `grpc`, `protobuf`,
`oj`, `msgpack`, `eventmachine`, `sass`, `rmagick`. It is deliberately not
exhaustive; anything else gets CRuby's plain `cannot load such file`. `ffi` is
excluded on purpose — zeo provides it.

## Standard-library extensions

A gem's *native* half — CRuby's `ext/` model — is a Rust module under
`crates/zeo-rt/src/ext/`, written in the same `ruby_class!` DSL as the core
classes. Each sits behind **two** independent gates: a Ruby `require` gate (its
constant stays invisible until the `require` fires) and a cargo `ext-<name>`
feature (`default = ext-all`, so the common build has them all).

Every method here is real and oracle-matched — there are no `todo!()`
scaffolds. Where the backing library differs from CRuby's, the compile warns
once and records it in `zeo-gems.json`; `docs/COMPATIBILITY.md` carries the
per-library reason.

| extension | `require` | cargo feature | backed by |
|---|---|---|---|
| base64 | `base64` | `ext-base64` | in-tree |
| bigdecimal | `bigdecimal` | `ext-bigdecimal` | `num-bigint` |
| cgi | `cgi/escape`, `cgi`, `cgi/util` | `ext-cgi` | in-tree (escape/unescape only) |
| coverage | `coverage` | `ext-coverage` | in-tree (line coverage only) |
| date | `date` | `ext-date` | in-tree |
| digest | `digest`, `digest/*` | `ext-digest` | RustCrypto (`md-5`, `sha1`, `sha2`) |
| etc | `etc` | `ext-etc` | `libc` |
| fcntl | `fcntl` | `ext-fcntl` | `libc` |
| ffi | `ffi` | `ext-ffi` | `libffi` (vendored) |
| json | `json` | `ext-json` | `serde_json` |
| monitor | `monitor` | `ext-monitor` | `parking_lot` |
| nkf | `nkf`, `kconv` | `ext-nkf` | zeo's encoding engine |
| openssl | `openssl` | `ext-openssl` | vendored OpenSSL 3 via rust-openssl |
| pathname | `pathname` | `ext-pathname` | in-tree |
| prism | `prism` | `ext-prism` (+ `eval-vm`) | `ruby-prism` |
| psych | `psych`, `yaml` | `ext-psych` | `yaml-rust2` |
| pty | `pty` | `ext-pty` | `libc` (`openpty(3)`) |
| readline | `readline` | `ext-readline` | `rustyline` |
| socket | `socket` | `ext-socket` | `libc` |
| stringio | `stringio` | `ext-stringio` | in-tree |
| strscan | `strscan` | `ext-strscan` | zeo's Regexp engine |
| syslog | `syslog`, `syslog/logger` | `ext-syslog` | `libc` (`syslog(3)`) |
| tracepoint | *(core — no require)* | `ext-tracepoint` | in-tree |
| zlib | `zlib` | `ext-zlib` | `flate2` (pure-Rust miniz_oxide) |

`io/wait`, `io/console`, `objspace` and `ARGF` are always-on `IO`/`ObjectSpace`
rows rather than gated modules, so their `require` is ceremony; `rbconfig`
resolves through a synthetic shim.

## Benchmarks

58 golden-output programs under `bench/`, compiled the way a user would compile
them (`zeo -o`: release runtime, static link), output-checked byte-for-byte
before any timing, then timed best-of-3 against CRuby 4.0.6 in the same run.

**Two aggregates, because either alone would mislead:**

| | geomean |
|---|---|
| all 58 benchmarks | **1.29× faster than CRuby** |
| the 37 where CRuby takes ≥ 0.10 s | **0.86×** — about 16% *slower* |

The difference between them is process startup. 13 benchmarks finish inside
50 ms of CRuby time, where a native binary starts instantly and the interpreter
pays ~35 ms of boot. Shipping a binary is a genuine advantage, but it is not a
claim about generated code — and on compute-bound work zeo currently trails
CRuby by a little. 32 of 58 are faster, 26 slower.

Best: `pidigits` 9.5×, `micro_lisp` and `bigint_fib` 9.0×, `sinatra_mini`
9.3×, `jekyll_lite` 7.6× (all startup-dominated); `range_each` 3.0×,
`so_mandelbrot` 3.1× and `nested_loop` 2.2× on real work. Worst:
`io_wordcount` 0.20×, `structaset` 0.30×, `structaref` 0.34×, `template`
0.43×. The Struct and string paths are named levers in
[`docs/TODO.md`](docs/TODO.md); `io_wordcount` is not yet root-caused.

Full per-benchmark table, method and caveats: [`bench/README.md`](bench/README.md).

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

- **`spinel`** — the conformance corpus (~2,509 programs), each diffed
  byte-for-byte against real `ruby`.
- **`examples`** — zeo-authored example programs with committed golden output.
- **`gaps`** — the XFAIL tracker: still-diverging programs, each with a header
  naming its cause. A gap that starts matching `ruby` fails the suite (an
  XPASS), forcing its promotion via `scripts/promote-gap.sh`.

`ZEO_BLESS=1` is the single golden writer — it records expected output from the
real `ruby` oracle (run with `--disable-error_highlight --disable-did_you_mean`)
instead of asserting. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the house
rule (oracle-verified, divergence-documented) and the full workflow.

## Project layout

```
crates/    the six workspace crates (above)
docs/      COMPATIBILITY, EXTENSIONS, EVAL_VM, TODO
tests/     golden-file suites — examples, the spinel corpus, the gaps tracker
gems/      51 vendored gems (git-pinned ones managed via gems.toml)
bench/     the benchmark suite (`cargo xtask bench`) — see bench/README.md
vendor/    vendored rubygems + shims
tools/     Ruby helper scripts (arity annotation, method coverage)
scripts/   corpus import, gap promotion, ruby-vs-zeo diffing
```

## Status & limitations

Experimental and moving fast. Known limitations, stated plainly:

- **No tracing GC** — `Arc` reference counting means reference cycles leak
  (documented, accepted; `GC.start` opportunistically runs finalizers).
- **Four extensions ship a deliberate subset** — `coverage` (line coverage
  only), `nkf`, `openssl` (no PKey generation, X509 issuance or `SSLServer`)
  and `TracePoint`. They raise `NoMethodError` at the edges rather than
  pretending; `docs/COMPATIBILITY.md` says what each leaves out. Nothing in
  `ext/` is a `todo!()` scaffold.
- **CRuby is still ahead on compute-bound code** — about 16% by geomean over
  the benchmarks that run longer than 100 ms. See [Benchmarks](#benchmarks).
- **Dynamic `eval` and `Ruby::Box` isolation are in flight** — see `docs/`.
- **Native C-extension gems are unsupported** — the intended escape hatch is
  the real `ffi` gem API, compiled ahead of time (see `docs/EXTENSIONS.md`).

Everything still owed is in [`docs/TODO.md`](docs/TODO.md); every known
divergence from `ruby` is an executable XFAIL in
[`tests/gaps/`](tests/gaps).

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
