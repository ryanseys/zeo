# Zeo: an ahead-of-time Ruby compiler

Zeo compiles a full Ruby program into **one native executable**. It reads the
source with [Prism], analyzes the whole program, writes Rust, and links a
runtime that is already compiled.

The binary starts immediately. It boots no interpreter and warms up no JIT,
and the machine that runs it needs no Ruby installation.

```console
$ cat hello.rb
puts [1, 2, 3].map { |x| x * 2 }.sum
$ zeo hello.rb -o hello
$ ./hello
12
```

Zeo's test suite runs each program against real Ruby and compares the output
byte for byte. [`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md) records every
difference that remains.

Zeo reads **RubyGems and Bundler**. It ships both, and it compiles an
application together with the gems that its `Gemfile.lock` selects. Read
[RubyGems and Bundler](#rubygems-and-bundler).

[Prism]: https://github.com/ruby/prism

## How Zeo works

```
foo.rb ──prism──▶ HIR arena ──analyze──▶ typed classes/MRO ──codegen──▶ Rust ──rustc──▶ foo
                  (full program:            (compile-time             (quote! +        (links the
                   requires spliced          method tables,            prettyplease)    precompiled
                   at compile time)          type inference)                            zeo-rt runtime)
```

- **Zeo compiles the full program at one time.** It resolves `require` and
  `require_relative` at compile time. The full program is one compilation unit.
  This includes the gems in `gems/`. The front end never defers a file to run
  time.
- **There are two dispatch paths.** If the compiler can resolve a call, that
  call becomes a direct Rust call. All other calls go through the runtime
  method registry, which uses `Symbol` keys. The same registry serves `send`,
  `define_method`, `method_missing`, singletons, and classes that the program
  makes at run time with `Class.new`.
- **The runtime is complete** (`zeo-rt`). It includes a numeric tower that
  agrees with CRuby (`Integer`, `Bignum`, `Rational`, `Complex`). A string is a
  sequence of bytes plus an encoding. A `Fiber` uses a real coroutine that
  changes the stack. A `Thread` is a real OS thread, and threads run in
  parallel. An exception propagates as a Rust `Result`. The crate has
  `#![forbid(unsafe_code)]` outside of the FFI and syscall layers.

The compiler and the runtime **never link to each other**. They agree only
through `zeo-abi`. That crate gives a numeric `ClassId` to each built-in class.
The compiler writes the number into the generated code, and the runtime
dispatch reads it.

## Quick start

Today you build Zeo from source. Build the compiler one time, then compile your
Ruby programs with it. The `cargo install zeo` and `gem install zeo` channels
arrive with the 0.1.0 release.

```console
# Build the compiler. This writes target/release/zeo.
$ cargo build --release -p zeo

# Compile a file and run it immediately, as `ruby hello.rb` does.
$ target/release/zeo hello.rb

# Compile a file to a native binary instead of running it.
$ target/release/zeo hello.rb -o build/hello
$ target/release/zeo hello.rb --compile   # writes ./hello (the input path, without the extension)

# Compile and run a program from the command line, as `ruby -e` does.
$ target/release/zeo -e 'puts "hello, world"'

# Run a test file, as `ruby -Itest test/foo_test.rb` does.
$ target/release/zeo -Itest test/foo_test.rb

# Show the generated Rust. This does not build a binary.
$ target/release/zeo hello.rb --emit-rust --pretty
```

**Note:** `zeo foo.rb` compiles the program and runs it, exactly as
`ruby foo.rb` interprets and runs it: stdout, stderr and the exit status go to
the caller, and the trailing arguments become `ARGV`. A binary artifact is the
opt-in: give `-o <path>`, or `--compile` for the default path. There are no
subcommands. The command line agrees with Ruby's: `zeo <file>` or
`zeo -e <code>`.

Repeated runs are fast. The compiled binary is kept in a content-addressed
cache under `target/`, keyed by the generated Rust and the exact runtime. When
you run an unchanged program again, only the Ruby front end runs; the `rustc`
step is skipped.

## Command-line options

```
usage: zeo [options] [--] (<input.rb> | -e <code>) [args...]
```

The flags follow CRuby's conventions. Every long option also accepts the
attached form `--flag=<value>`. An option that Zeo does not know is an error;
Zeo names the replacement for each removed old spelling.

| Option | Function |
|---|---|
| `<input.rb>` | Compiles the file and runs it immediately, as `ruby` does. Sends stdout, stderr and the exit status to the caller. The trailing arguments become the program's `ARGV`. Zeo parses options after the file name too, so put `ARGV` entries that look like options after a `--`. |
| `-e <code>` | Compiles the given code and runs it immediately. You can give this option more than one time; Zeo joins the parts with newlines. The arguments that follow the code (or follow `--`) become the program's `ARGV`, as in `ruby -e`. With `-o`, Zeo writes a binary and does not run it. |
| `-o <output>` | Compiles to a native binary at this path, and does not run it. |
| `--compile` | Compiles to a native binary at the default path — the input path without its extension — and does not run it. |
| `-I <dir>` | Adds a directory to the `require` search path, as Ruby's `-I` does. You can give this option more than one time. The forms `-I<dir>` and `-I=<dir>` are also correct. |
| `--gems <dir>` | Adds a directory of vendored gems. Each subdirectory that contains a `.gemspec` file is one gem. You can give this option more than one time. See the search order below. |
| `--gem-path <dir>` | Adds an installed RubyGems store (`gem env gemdir`). You can give this option more than one time; without it, Zeo reads `GEM_PATH`. A store is only used together with a Gemfile from `--bundle-gemfile` or `BUNDLE_GEMFILE`. |
| `--bundle-gemfile <path>` | Gives the Gemfile. Zeo reads its lockfile — `Gemfile` → `Gemfile.lock`, `gems.rb` → `gems.locked` — and that lockfile selects the versions in the store. A `<path>` that already ends in `.lock` is read directly. Without this option, Zeo reads `BUNDLE_GEMFILE`. |
| `--report[=<path>]` | Writes the `zeo-gems.json` record. Without a path, the record goes next to the output artifact. The record is off by default. |
| `-W0` | Stops all Zeo warnings. |
| `-W:no-<category>` | Stops one category of disclosure warning; `-W:<category>` starts it again. The one category today is `zeo-builtin-substitute`. An unknown category is an error. `-w`, `-W`, `-W1` and `-W2` are accepted and change nothing: the warnings are on by default. |
| `--emit-rust[=<path>]` | Writes the generated Rust source to `<path>`, or to stdout without a path, then stops. Does not build. The path form streams and holds no copy of the program in memory. |
| `--pretty` | With `--emit-rust`: formats the Rust source for a person to read. This costs a re-parse and a second copy of the program. |
| `-v`, `--version` | Prints the version, then stops. |
| `-h`, `--help` | Shows the help text, then stops. |
| `--` | Ends the options. The next argument is the input file (or, with `-e`, the start of `ARGV`). |

**The `require` search order.** For a plain `require "feature"`, Zeo searches,
in this order, and the first gem with a given name wins:

1. The `-I` roots, in the order given, then the `RUBYLIB` entries.
2. The `--gems` directories, in the order given.
3. The input file's sibling `gems/` directory.
4. Zeo's own bundled gems, then the external gem store.

Environment variables:

| Variable | Function |
|---|---|
| `RUBYOPT` | Gives extra options, applied before the command line (the command line wins). Only `-I`, `-w` and `-W` are permitted, as in CRuby. |
| `RUBYLIB` | Adds `require` search roots after every `-I` root. |
| `GEM_PATH` | Gives the gem store directories for `--gem-path`, separated by `:`. An ambient store alone never changes a compile: Zeo uses it only when a Gemfile is also known. |
| `BUNDLE_GEMFILE` | Gives the Gemfile for `--bundle-gemfile`. |
| `ZEO_LOG`, `RUST_LOG` | Give a `tracing` `EnvFilter` directive. Example: `ZEO_LOG=zeo::analyze=debug,zeo::lower=trace`. If you set none of these variables, Zeo installs no subscriber and writes no diagnostics. |
| `ZEO_RUNTIME_PROFILE` | Selects the profile of the linked runtime: `debug` or `release`. The default is `debug` for an immediate run, and `release` for an `-o` or `--compile` artifact. |
| `ZEO_GVL` | If you set `ZEO_GVL=1`, the threads in that run use CRuby's schedule. This is a FIFO global lock with a 100 ms timer. The default is parallel OS threads. |
| `ZEO_BLESS` | If you set `ZEO_BLESS=1`, the test suites record their expected output again from real Ruby. Use this only during development. |

## Ruby features

Zeo aims at the full language. These features work today:

- **The numeric tower.** `Integer` and `Bignum` have unlimited precision, and
  `Integer` becomes `Bignum` when necessary. `Float`, `Rational` and `Complex`
  are also available. Coercion and error messages agree with CRuby.
- **Strings and encodings.** A string is a sequence of bytes plus an encoding.
  `Encoding.list` reports 103 encodings, the same set and count as Ruby
  4.0.6. 52 single-byte tables cover the Windows-125x and ISO-8859 families,
  and Shift_JIS, EUC-JP, GBK and Big5 come from the WHATWG tables.
  `docs/COMPATIBILITY.md` lists the differences.
- **Core collections.** `Array`, `Hash`, `Range`, `Symbol` and `Struct` are
  available. `Enumerable` and `Comparable` are real ancestors in the method
  resolution order, and they use your `each` and `<=>`.
- **Blocks, procs and yields.** If the compiler knows the shape of a block, it
  puts the block inline. For example, `3.times` becomes a native loop. A block
  that leaves its scope becomes a real closure object.
- **Classes and metaprogramming.** Inheritance, modules, `include`, `extend`
  and `super` work. Dynamic dispatch is complete: `send`, `define_method`,
  `method_missing`, singletons for one object (`def obj.foo` and
  `class << obj`), and `Class.new(Super) { … }` all use the runtime method
  registry.
- **Exceptions.** `raise`, `rescue`, `ensure` and `retry` work. Internally, an
  exception propagates as a Rust `Result`. The message text is part of the
  behaviour, and it agrees with CRuby word for word.
- **Concurrency.** A `Thread` is a real OS thread with an 8 MiB stack, and
  threads run in parallel by default. There is no GVL. You can kill a busy
  loop, raise in it, and interrupt a `sleep`. A `Fiber` uses a real coroutine
  that changes the stack (`corosensei`). A `Ractor` implements Ruby 4.0's full
  port model: `Ractor::Port`, `select`, `monitor`, `join`/`value` with
  `Ractor::RemoteError`, per-ractor locals, and real `move: true` semantics
  (the source graph is poisoned; later calls raise `Ractor::MovedError`).
  `Mutex` and `Queue` are available.
- **Regexp.** Real engines do the work: `regex` and `fancy-regex`. Zeo also
  includes Oniguruma for the paths that need Onigmo behaviour.
- **VM introspection.** `IO::Buffer` is complete, with typed value access,
  slices over shared backing, and a real `mmap` for `.map`.
  `RubyVM::AbstractSyntaxTree` parses through Prism and answers parse.y node
  types; `RubyVM::InstructionSequence` compiles and evaluates;
  `RubyVM.stat` reports Zeo's real counters. `Ruby::Box` carries its full
  surface over Zeo's compile-time box model.
- **`eval`.** Zeo parses a literal `eval("…")` and puts it into the program at
  compile time. A dynamic `eval`, whose text the program computes at run time,
  needs the interpreter in [`docs/EVAL_VM.md`](docs/EVAL_VM.md). The `eval-vm`
  feature controls that interpreter, and Zeo links it only into a program that
  can get to it.

Zeo is experimental, and some parts are not complete. This list is a summary,
and the conformance corpus below is the record of what agrees with Ruby.

## Compatibility

Zeo targets **CRuby 4.0.6**. The version has one source, `zeo-abi`. Therefore
the compiler's version tests and the runtime's `RUBY_VERSION` always agree.

The **method census** compares every module, method, constant and visibility
Ruby 4.0.6 reaches from `Object`'s constant tree with Zeo's surface
(`crates/zeo-tests/tests/method_census.rs`). Its gap ledger,
`conformance/method-census-gaps.tsv`, holds **zero rows**.

That surface is the one a program sees before it calls `require`. A class a
`require` brings in — `StringIO`, `Zlib`, `OpenSSL` and the rest of the
extensions below — is not in the constant tree at census time, so the census
does not measure it. The per-extension goldens cover those. Read
[`docs/METHOD_COVERAGE.md`](docs/METHOD_COVERAGE.md) for how the census works.

- **The conformance suite** in `tests/spinel/` compiles 2,568 programs. It compares stdout and stderr with real Ruby, byte for byte, as
  `cargo nextest` cases. `tests/gaps/` holds the programs that do not agree
  yet. Each of these must fail. If one starts to agree with Ruby, the suite
  fails, and that program then moves into the corpus.
- **Zeo declares each substitution.** For some libraries, Zeo supplies its own
  code: `json` uses serde_json, `psych` and `yaml` use yaml-rust2, `zlib` uses
  flate2, `digest` uses RustCrypto, and `openssl` uses a vendored OpenSSL 3.
  The compile writes one warning. With `--report`, it also writes a
  `zeo-gems.json` record with the artifact. That record tells you which
  libraries are different, and why.
- **A gem with a C extension fails clearly.** If Zeo has no built-in for the
  extension, the error gives the name of the gem, such as `sqlite3`, `nokogiri`
  or `pg`. The error also points to the FFI path. It does not look like an
  unknown language feature.

For the full list of substitutions and differences, read
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md). For the extension model, read
[`docs/EXTENSIONS.md`](docs/EXTENSIONS.md). For the remaining work, read
[`docs/ROADMAP.md`](docs/ROADMAP.md).

## Bundled gems

Zeo includes 51 gems in `gems/`. They resolve without a `Gemfile`. If a program
that Zeo compiles has `require "csv"`, Zeo uses the copy in this repository.

The **Origin** column tells you where the Ruby source comes from:

- **git-pinned** — `cargo xtask gem` gets the gem from its own repository. The
  `rev` in `gems.toml` gives the exact commit.
- **upstream** — a copy of a default or bundled gem from a Ruby installation,
  with no changes. Most come from Ruby 4.0.5. irb, minitest and reline come
  from Ruby 4.0.6.
- **upstream +Zeo** — the same, but with changes. Each change has a `zeo:` mark
  at its location.
- **Zeo Ruby half** — Zeo wrote the `.rb` files. A Rust extension in
  `crates/zeo-rt/src/ext/` supplies the native part. CRuby makes the same
  division between `rubylibdir` and `archdir`.

The **Test** column gives a test, with a path relative to `tests/`. The
programs that exercise one gem alone are in `tests/gems/`; the others are
general tests that also use the gem. That test compiles the gem and
compares its output with real Ruby 4.0.6, byte for byte. A dash means that no
test measures this gem alone. Some gems run only as a dependency of another
gem. For the full source information and the licences, read
`gems/UPSTREAM.md`.

| Gem | Version | Origin | Test | Differences |
|---|---|---|---|---|
| abbrev | 0.1.2 | git-pinned | `gems/abbrev.rb` | — |
| benchmark | 0.5.0 | git-pinned | `gems/benchmark.rb` | — |
| bigdecimal | 4.1.2 | upstream +Zeo | `bigdecimal.rb` | Zeo supplies the native part ([compat](docs/COMPATIBILITY.md)) |
| bundler | 4.0.18 | git-pinned | `gems/bundler.rb` | the test starts at `bundler/version` ([roadmap](docs/ROADMAP.md)) |
| csv | 3.3.6 | git-pinned | `gems/csv.rb` | — |
| delegate | 0.6.1 | upstream | `gems/delegate.rb` | — |
| drb | 2.2.3 | git-pinned | `gems/drb.rb` | — |
| English | 0.8.1 | upstream | `english_special_globals.rb` | — |
| erb | 6.0.7 | git-pinned | `erb_module_function.rb` | — |
| ffi | 1.17.4 | Zeo Ruby half | `ffi_struct.rb` | the native part uses `libffi` |
| fiddle | 1.1.8 | upstream +Zeo | `fiddle.rb` | no `Importer` DSL ([compat](docs/COMPATIBILITY.md)) |
| fileutils | 1.8.0 | git-pinned | `fileutils.rb` | — |
| find | 0.2.0 | git-pinned | `gems/find.rb` | — |
| forwardable | 1.4.0 | upstream | `gems/forwardable.rb` | — |
| ipaddr | 1.2.9 | git-pinned | `gems/ipaddr.rb` | — |
| irb | 1.18.0 | upstream | — | compiles, but stops at a refinement module that the program makes at run time ([gap](tests/gaps/issue_runtime_refinement_module.rb)) |
| json | 2.18.0 | Zeo Ruby half | `json_to_json.rb` | uses serde_json, not the json gem ([compat](docs/COMPATIBILITY.md)) |
| logger | 1.7.0 | git-pinned | `gems/logger.rb` | — |
| minitest | 6.0.6 | upstream | `gems/minitest.rb` | a full-program compile includes the `MT_HELL` branch of `autorun`, which writes a note to stderr |
| monitor | 0.1.0 | Zeo Ruby half | `gems/two_halves.rb` | `Monitor` and `MonitorMixin` only |
| net-ftp | 0.3.9 | git-pinned | `gems/net_ftp.rb` | — |
| net-http | 0.9.1 | git-pinned | `gems/net_http.rb` | — |
| net-protocol | 0.2.2 | git-pinned | `gems/net_protocol.rb` | — |
| net-smtp | 0.5.1 | git-pinned | `gems/net_smtp.rb` | — |
| nkf | 0.3.0 | Zeo Ruby half | `nkf.rb` | some options only; `guess` uses a different method ([compat](docs/COMPATIBILITY.md)) |
| observer | 0.1.2 | git-pinned | `gems/observer.rb` | — |
| open3 | 0.2.1 | git-pinned | `open3_capture.rb` | — |
| openssl | 4.0.2 | Zeo Ruby half | `openssl_cipher.rb` and 6 more | no PKey generation, no X509 issue, no `SSLServer` ([compat](docs/COMPATIBILITY.md)) |
| optparse | 0.8.1 | Zeo Ruby half | `optparse_subset.rb` | the common `OptionParser` methods |
| ostruct | 0.6.3 | upstream | `gems/ostruct.rb` | — |
| pp | 0.6.4 | upstream | `pp_pretty_print.rb` | — |
| prettyprint | 0.2.0 | upstream | `gems/prettyprint.rb` | — |
| prism | 1.9.0 | upstream +Zeo | `gems/prism.rb` | no `translation/` and no `ffi.rb` |
| psych | 5.4.0 | Zeo Ruby half | `psych_load_file_and_stream.rb` | uses yaml-rust2, not libyaml ([compat](docs/COMPATIBILITY.md)) |
| pty | 0.5.9 | Zeo Ruby half | `pty_spawn.rb` | — |
| racc | 1.8.1 | git-pinned | `gems/racc.rb` | — |
| reline | 0.6.3 | upstream +Zeo | `reline_line_editor.rb` | one `zeo:` change in `io.rb` |
| resolv | 0.7.1 | git-pinned | `gems/resolv.rb` | — |
| rubygems | 4.0.18 | git-pinned | `gems/rubygems.rb` | the test starts below the top-level require ([roadmap](docs/ROADMAP.md)) |
| shellwords | 0.2.2 | upstream | `shellwords.rb` | — |
| singleton | 0.3.0 | upstream | — | `singleton_class.include?` ([gap](tests/gaps/issue_singleton_class_include_after_extend.rb)) |
| strscan | 3.1.6 | Zeo Ruby half | `strscan_capture_surface.rb` | Zeo supplies its own code ([compat](docs/COMPATIBILITY.md)) |
| syslog | 0.4.0 | Zeo Ruby half | `syslog.rb` | — |
| tempfile | 0.3.1 | git-pinned | `gems/tempfile.rb` | — |
| time | 0.4.2 | git-pinned | `time_parse.rb` | — |
| timeout | 0.6.1 | upstream | `gems/timeout.rb` | — |
| tmpdir | 0.3.1 | git-pinned | `io_encoding.rb` | — |
| tsort | 0.2.0 | upstream | `gems/tsort.rb` | — |
| un | 0.3.0 | git-pinned | `gems/un.rb` | — |
| uri | 1.1.1 | git-pinned | `uri_parse_and_build.rb` | — |
| zlib | 3.2.3 | Zeo Ruby half | `zlib_classes.rb` | uses flate2; 4 functions are not available ([compat](docs/COMPATIBILITY.md)) |

To use a gem that is not in this list, give `--gem-path` and
`--bundle-gemfile` (or set `GEM_PATH` and `BUNDLE_GEMFILE`).

### Which gems compile

`cargo xtask gem-probe` fetches a gem straight from rubygems.org, compiles it,
and records the verdict — one row per gem, with the reason when there is one —
in [`conformance/gem-probe.tsv`](conformance/gem-probe.tsv). The gem does not
need to be installed.

```console
$ cargo xtask gem-probe kramdown          # newest release
$ cargo xtask gem-probe rake 13.3.1       # a pinned version
$ cargo xtask gem-probe --corpus          # every gem in gem-probe-corpus.txt
$ cargo xtask gem-probe --popular 1000    # the 1000 most downloaded gems
$ cargo xtask gem-probe --all --check     # re-probe; fail if one regressed
```

This measures something the gem table above does not. A gem can be pure Ruby,
resolve correctly, and still use a construct Zeo cannot lower. `gem-probe`
runs the front end, so an `ok` row means the compiler succeeded.

The ledger covers the full rubygems.org index. The harness writes the numbers
below on every probe, and `cargo xtask gem-probe --check` fails when they
disagree with the ledger — so this block cannot go stale.

<!-- gem-probe-stats:begin -->
**182,830 of 195,778 probed gems compile to Rust (93.4%).** The probe runs zeo's full front end (parse, lower, analyze, codegen) on the newest release of every gem on rubygems.org. `ok` means zeo produced Rust; no rustc ran. One row per gem in [`conformance/gem-probe.tsv`](conformance/gem-probe.tsv).

| Verdict | Gems | Share of probed |
|---|---|---|
| Compile to Rust (`ok`) | 182,830 | 93.4% |
| Compiler gaps, zeo's to fix (`lowering-gap`, `compiler-panic`, `rustc-error`) | 1,973 | 1.0% |
| Unresolved dependency in the probe's view (`missing-dependency`) | 1,326 | 0.7% |
| Harness limits, not compiler verdicts (`no-entry-point`, `no-lib-dir`, `fetch-failed`, `view-failed`, `ambiguous-require`) | 5,075 | 2.6% |
| Nothing to compile (`invalid-ruby`, `native-extension`, `ext-only`, `meta-gem`, `platform-gem`) | 4,527 | 2.3% |
| No verdict reached (`timeout`, `out-of-memory`) | 47 | 0.0% |

Of the 186,176 gems zeo can attempt -- the probed set minus the harness limits and the gems no Ruby loads -- **182,830 compile (98.2%)**.
<!-- gem-probe-stats:end -->

Read [`docs/GEM_TESTING.md`](docs/GEM_TESTING.md) for the workflow and for
what an `ok` row does and does not claim, and
[Does it support Rails?](#does-it-support-rails) for that closure in
particular.


Each probe sees only its own gem and that gem's dependencies, so a verdict does
not depend on what else has been fetched. `--all` re-probes at the version the
ledger recorded, which makes a run reproducible. To widen coverage, add a name
to [`conformance/gem-probe-corpus.txt`](conformance/gem-probe-corpus.txt).

If a gem needs a C extension that Zeo has no built-in for, the `require` fails
and the error gives the name of the gem. The function `is_known_native_gem` in
`crates/zeo/src/parse/loader.rs` holds 14 such names: `sqlite3`, `nokogiri`,
`pg`, `mysql2`, `bcrypt`, `nio4r`, `puma`, `grpc`, `protobuf`, `oj`, `msgpack`,
`eventmachine`, `sass` and `rmagick`. The list is deliberately not complete.
For all other names, Zeo gives CRuby's usual message, `cannot load such file`.
The list does not include `ffi`, because Zeo supplies `ffi`.

## RubyGems and Bundler

Zeo supports RubyGems and Bundler in two different ways. Keep them separate,
because they answer two different questions.

### 1. Compile an application with its bundled gems

This is the part that most applications need. Give Zeo your `Gemfile` and the
gem store that `bundle install` wrote. Zeo then compiles your program **and its
dependencies** into one binary.

```console
# Install the dependencies one time, with real Bundler.
$ bundle install

# Compile the application together with those gems.
$ zeo app.rb -o app --gem-path "$(gem env gemdir)" --bundle-gemfile Gemfile

# The same thing through the environment, which Bundler already sets.
$ GEM_PATH="$(gem env gemdir)" BUNDLE_GEMFILE=Gemfile zeo app.rb -o app
```

Zeo **reads** what Bundler and RubyGems already decided. It never resolves a
dependency graph, never contacts a network, and never installs or builds a gem.
The `Gemfile.lock` selects each version, and Zeo reads the gemspec of that
version from the store. Therefore the result agrees with `bundle exec`.

Give both options together. A gem store alone does not change a compile,
because without a lockfile Zeo does not know which versions you mean.

Three rules control which gems work:

- **A pure-Ruby gem usually compiles.** Zeo puts its source into the program,
  the same as any other file. Pure Ruby is necessary but not sufficient: a gem
  can still use a construct Zeo does not lower yet, such as a class definition
  inside a run-time conditional. The compile then stops and names that
  construct.
- **A gem with a C extension stops the compile**, and the error gives the name
  of the gem. Read the list above.
- **Zeo needs the `ruby` platform gem, not a precompiled one.** A precompiled
  platform gem carries a `.bundle` or `.so` file, which Zeo cannot read. This
  is the same rule as Bundler's `force_ruby_platform`.

To see the result before you compile, ask:

```console
$ cargo xtask gem-compat Gemfile.lock --gem-path "$(gem env gemdir)"
```

That command reads your lockfile and puts each gem into a class: it compiles,
Zeo has a built-in for it, it needs a C extension, or it comes from a git or
path source. It prints a table with the number of gems that resolve, and
writes `conformance/gem-compat.tsv` and `.md`. This is a static
classification, and not a compile.

### Does it support Rails?

**The framework compiles. An application does not run yet.** The gap is
measured rather than guessed: `cargo xtask gem-probe` compiles each gem in
the Rails closure, and the ledger holds the verdicts.

Every framework gem reaches code generation today: `activesupport`,
`activemodel`, `activerecord`, `actionpack`, `actionview`, `actionmailer`,
`actioncable`, `activejob`, `activestorage` and `railties`, and the view and
mail closure with them (`loofah`, `rails-dom-testing`,
`rails-html-sanitizer`, `mail`, `globalid`, `erubi`, `builder`). The `rails`
gem itself ships no `lib/` — a meta-gem that only names dependencies.

`codegen ok` means Zeo produced Rust, and nothing more. Between that and a
running application:

- **A database adapter.** `sqlite3`, `pg` and `mysql2` are C extensions. Zeo
  cannot build them, and there is no pure-Ruby substitute.
- **Boot itself.** No Rails application has been compiled and run under Zeo.
  Everything above measures code generation, not a booting server.

So Rails is a target Zeo measures against — not a supported configuration.
Follow [`conformance/gem-probe.tsv`](conformance/gem-probe.tsv) for the
current state, and [`docs/GEM_TESTING.md`](docs/GEM_TESTING.md) for how to
reproduce it.

### 2. Compile RubyGems and Bundler themselves

Zeo ships both, from their own upstream repository (`rubygems/rubygems`,
version 4.0.18, pinned by commit in `gems.toml`). A program can
`require "rubygems"` or `require "bundler"`, and the whole require graph
reaches code generation. The test
`crates/zeo-tests/tests/e2e/gems_vendored.rs` compiles that graph and confirms
that the classes each one registers survive it.

One difference remains, and it is the reason the goldens
`tests/gems/rubygems.rb` and `tests/gems/bundler.rb` start below the top file.
`rubygems.rb` writes `require "bundler"` inside a **method body**. Zeo resolves
every require at compile time, so it lifts that require to where it is written.
Bundler's `rubygems_ext` then runs before `rubygems/specification`, which is
not the order CRuby uses. The program compiles; the load order is different.
[`docs/ROADMAP.md`](docs/ROADMAP.md) holds the remaining work.

So: use `--bundle-gemfile` to compile **your application** today. Running
`bundle` itself as a Zeo binary is not finished.

## Standard-library extensions

The native part of a gem is a Rust module in `crates/zeo-rt/src/ext/`. CRuby
uses the same `ext/` model. Each module uses the `ruby_class!` DSL, which is
also used for the core classes.

Two independent gates control each extension:

1. **A Ruby `require` gate.** The constant of the extension stays invisible
    until the program executes its `require`.
2. **A cargo feature**, `ext-<name>`. The default feature set is `ext-all`, so
    a usual build includes all of the extensions.

Each method in these modules is complete, and Zeo compares it with real Ruby.
No method is an empty `todo!()`. If the library behind a module is different
from CRuby's, the compile writes one warning and, with `--report`, records it
in `zeo-gems.json`. `docs/COMPATIBILITY.md` gives the reason for each library.

| Extension | `require` | Cargo feature | Uses |
|---|---|---|---|
| base64 | `base64` | `ext-base64` | Zeo code |
| bigdecimal | `bigdecimal` | `ext-bigdecimal` | `num-bigint` |
| cgi | `cgi/escape`, `cgi`, `cgi/util` | `ext-cgi` | Zeo code; escape and unescape only |
| coverage | `coverage` | `ext-coverage` | Zeo code; line coverage only |
| date | `date` | `ext-date` | Zeo code |
| digest | `digest`, `digest/*` | `ext-digest` | RustCrypto (`md-5`, `sha1`, `sha2`) |
| etc | `etc` | `ext-etc` | `libc` |
| fcntl | `fcntl` | `ext-fcntl` | `libc` |
| ffi | `ffi` | `ext-ffi` | `libffi`, included in this repository |
| json | `json` | `ext-json` | `serde_json` |
| monitor | `monitor` | `ext-monitor` | Zeo thread primitives |
| nkf | `nkf`, `kconv` | `ext-nkf` | the Zeo encoding engine |
| openssl | `openssl` | `ext-openssl` | OpenSSL 3 through rust-openssl, included in this repository |
| pathname | `pathname` | `ext-pathname` | Zeo code |
| prism | `prism` | `ext-prism` (with `eval-vm`) | `ruby-prism` |
| psych | `psych`, `yaml` | `ext-psych` | `yaml-rust2` |
| pty | `pty` | `ext-pty` | `libc`, function `openpty(3)` |
| readline | `readline` | `ext-readline` | `rustyline` |
| socket | `socket` | `ext-socket` | `libc` |
| stringio | `stringio` | `ext-stringio` | Zeo code |
| strscan | `strscan` | `ext-strscan` | the Zeo Regexp engine |
| syslog | `syslog`, `syslog/logger` | `ext-syslog` | `libc`, function `syslog(3)` |
| tracepoint | none; part of the core | `ext-tracepoint` | Zeo code |
| zlib | `zlib` | `ext-zlib` | `flate2` |

Four extensions give only a part of the methods that CRuby gives. At the limit,
they raise `NoMethodError`, so the limit appears immediately.

- `openssl` — no PKey generation, no X509 issue, no PKCS#7, no ASN1, no
  `SSLServer`.
- `nkf` — some conversion options only.
- `coverage` — lines only, not branches and not methods.
- `TracePoint` — no `b_call` or `c_call` events, and no `#binding`.

`docs/EXTENSIONS.md` gives the details for each extension.

Four other libraries need no module. `io/wait`, `io/console`, `objspace` and
`ARGF` are always present as methods on `IO` and `ObjectSpace`. Their `require`
does nothing. `rbconfig` resolves through a file that Zeo generates.

## Benchmarks

The `bench/` directory holds 58 Ruby programs, and each one has its correct
output. `cargo xtask bench` compiles each program with `zeo -o`, which links
the release runtime and makes a static binary. This is the same configuration
that a user gets.

Before it measures a program, the tool compares the output of that program with
the correct output, byte for byte. The tool does not measure a program that
gives a wrong answer. The tool then measures the time more than one time and
keeps the smallest value. It measures CRuby 4.0.6 in the same run.

Zeo gives two results:

| Programs | Geometric mean |
|---|---|
| all 58 programs | **1.78 times faster than CRuby** |
| the 37 programs where CRuby needs 0.10 s or more | **1.23 times faster** |

The difference between the two results is the start time. The CRuby
interpreter needs approximately 35 ms before it runs the first line of a
program. A native binary needs less than 1 ms. For the 13 programs that finish
in less than 50 ms, this start time is most of the measurement, so the first
result mostly measures start time. The second result removes this effect: it
keeps only the programs that run long enough for the generated code to control
the time. Zeo is faster for 47 programs, and slower for 11 programs.

The largest advantages are `bigint_fib`, `jekyll_lite` and `str_concat` (8.8),
`pidigits` and `poly_cells` (8.5), and `micro_lisp` and `sinatra_mini` (8.3).
The start time controls all of these. For programs that calculate, the largest
advantages are `so_mandelbrot` (3.4), `range_each` (3.1), `nested_loop` (2.5)
and `object_new` (2.2).

The largest disadvantages are `life` (0.52), `rbtree` (0.66), `linked_list`
(0.70), `splay` (0.74) and `so_lists` (0.74). These programs make and release
many objects. Their time goes into the reference counts and the memory
allocation, and not into the method calls or the instance variables.
[`docs/ROADMAP.md`](docs/ROADMAP.md) gives the measurements and the planned work.

[`bench/README.md`](bench/README.md) gives the full table, the method and the
limits of these measurements.

## The workspace

Zeo is a Cargo workspace with seven crates. It uses edition 2024, and the minimum
Rust version is 1.88.

```
crates/
  zeo         the compiler and the CLI: parse ▸ lower ▸ analyze ▸ codegen ▸ backend
  zeo-rt      the runtime that links into each compiled program (the largest crate)
  zeo-abi     a leaf crate with no dependencies: the ClassId numbers that both sides use
  zeo-dsl     the shared `syn` grammar for the ruby_class! and ruby_module! DSL
  zeo-macros  the macro that expands that DSL into runtime code
  zeo-tests   the integration and golden suites (not published)
  xtask       development tools (bench, gem, gem-compat, stdlib-status)
```

- **`zeo`** is the driver. The front end (`parse/`, `lower/`, `hir.rs`)
  resolves the requires and lowers the Prism tree into a typed HIR arena. Then
  `analyze/` calculates the ancestors of each class (`mro.rs`) and the type of
  each local variable in one pass (`locals.rs`). Then `codegen/` writes Rust as
  a `proc_macro2` TokenStream, reads it again with `syn`, and formats it with
  `prettyplease`. Last, `backend/` calls `rustc` against the runtime, and uses
  a build cache with content addresses. This crate is a library, not only a
  `main.rs`. Read the API section below.
- **`zeo-rt`** is the runtime. Each value is one `enum RubyValue`. A collection
  is an `Arc<Freezable<…>>`, which makes `freeze` and structural sharing cheap.
  A user object is an `Arc<dyn RubyObject>`. Memory management uses `Arc`
  reference counts, and there is **no tracing collector**. Therefore a cycle of
  references leaks its memory. This is a known limit. Each Ruby core class has
  one Rust module in `builtins/`. The `linkme` crate collects the method tables
  at link time into one slice, and the `ClassId` is the index.
- **`zeo-abi`** is a leaf crate with no dependencies. It sets the numeric
  `ClassId` of each built-in class. It holds the `BUILTINS` table, which gives
  the superclass and the included modules of each class. Real Ruby supplies
  these values. It also holds `RUNTIME_CLASS_ID_BASE`, for classes that a
  program makes at run time, and `RUBY_VERSION`. This is the only ABI that the
  compiler and the runtime share.
- **`zeo-dsl`** and **`zeo-macros`** hold the DSL for the core classes. Read
  the next section.
- **`zeo-tests`** holds the integration and golden suites. It is `publish =
  false`: its tests read the repository's own `tests/` and `gems/`
  directories, which no published crate carries.
- **`xtask`** holds the development commands. Use `cargo xtask <cmd>`. The
  commands are `bench` for the performance suite, `gem` to manage the gems in
  `gems.toml`, `gem-compat` to measure how much of a gem store compiles, and
  `stdlib-status`.

### The `ruby_class!` DSL

You write each core class one time, in a grammar that looks like Ruby. The
method bodies stay in Rust. This example comes from
`crates/zeo-rt/src/builtins/string.rs`:

```rust
ruby_class! {
    String = zeo_abi::STRING_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    def "length" | "size" (recv) {
        Ok(RubyValue::Int(crate::string_len(recv_str!(recv))))
    }
    def "empty?" (recv) {
        Ok(RubyValue::Bool(crate::string_len(recv_str!(recv)) == 0))
    }
}
```

A def's parameter list gives both the argument-count check the body runs behind
and the number `Method#arity` reports:

```rust
def "length" (recv)                             // 0
def "index" cfunc (recv, needle, start = nil)   // -1
def "unpack" (recv, fmt, **opts)                // -2
def "push" (recv, *items)                       // -1
def "insert" (recv, at, *rest)                  // -2, and "expected 1+"
def "each" (recv, &block)                       // 0
```

The number follows CRuby's equation: take `min` and `max` from the signature,
then `(min == max) ? min : -min-1`. CRuby applies it to C methods too, but C
declares only `argc = N` or `argc = -1` — it cannot say "one required plus one
optional". That is why `String#index` reports -1, and why no C method reports
below -1. The `cfunc` marker records that lost precision.

`cargo run -p xtask -- arity-oracle` records what ruby 4.0.6 reports, and a test
diffs the declarations against it with no ruby needed at test time.

The crate `zeo-dsl` reads this grammar. Three consumers then use it, so they
cannot disagree:

- `zeo-macros` expands it into the runtime method functions, the lookup tables
  with `ClassId` keys, the constant installers, and the `linkme` registration.
- The `build.rs` of `zeo` reads the same text again and builds `CLASS_SURFACE`.
  These are the names that the compiler uses to resolve `respond_to?`, `is_a?`
  and constant lookups. The compiler sees the headers only. The method bodies
  are not visible to it.
- The build tools read it to check arity against the oracle and to keep the
  `cfunc` markers current.

The standard-library extensions in `crates/zeo-rt/src/ext/` use the same DSL
and the two gates above. Read [`docs/EXTENSIONS.md`](docs/EXTENSIONS.md).

## Public API

The file `crates/zeo/src/lib.rs` is a library. The CLI and the test harness
both call it directly. To use the compiler from Rust:

```rust
use zeo::{compile_to_rust, compile_to_rust_with, CompileOptions};

// The simple form: Ruby source goes in, formatted Rust source comes out.
let rust: String = compile_to_rust("puts 1 + 1").unwrap();

// The full form: load roots, gem store, disclosure report, warning control.
let opts = CompileOptions::default();
let out = compile_to_rust_with("puts 1 + 1", &opts).unwrap();
// out.rust_source     the generated Rust text
// out.needs_eval_vm   true if the program can get to the runtime eval VM

// Compile the generated Rust into a native binary against the runtime.
zeo::backend::build_binary(/* … */);
```

The public API has `CompileOptions`, `CompileOutput` and `CompileError`. It
also has the pipeline modules: `parse`, `lower`, `analyze`, `codegen`,
`backend`, `compiler`, `hir`, `types`, `diagnostics` and `gem_report`.

**Note:** this API gives you the **compiler**. The crate `zeo-rt` is the link
target of a compiled program. It is not an API to call Ruby from Rust.

## Build from source

You need only three items:

- **Rust 1.88 or later**, because Zeo uses edition 2024. Read `rust-version`.
- **A C compiler**, for Prism and Oniguruma.
- **Ruby 4.0.6**, but only to record the expected test output again. The file
  `mise.toml` gives this version.

```console
$ git clone https://github.com/ryanseys/zeo && cd zeo
$ cargo build --release -p zeo
$ target/release/zeo yourprogram.rb -o yourprogram
```

Zeo builds the runtime `zeo-rt` automatically, the first time that you compile
a program. It builds each combination of profile, runtime variant and linkage
one time only. To build the runtime yourself, use
`cargo build --release -p zeo-rt`. Add `--features eval-vm` for programs that
use a dynamic `eval`.

The file `mise.toml` sets the development tools: `ruby = "4.0.6"` and
`rust = "1.97.1"`.

### A relocatable install

`cargo xtask dist` assembles a tree that runs from any directory:

```console
$ cargo xtask dist                      # writes target/dist/zeo-<version>-<triple>.tar.gz
$ tar xzf zeo-<version>-<triple>.tar.gz -C /usr/local
$ zeo hello.rb -o hello
```

```
zeo-<version>-<triple>/
  bin/zeo
  share/zeo/{gems, runtime, dist-manifest.json}
  share/doc/zeo/
```

The binary finds its payload beside itself, through `bin/../share/zeo`, so the
tree moves anywhere. `ZEO_HOME` overrides that search.

**The target machine still needs cargo and rustc**, at 1.88 or later. A Rust
`rlib` is tied to the exact compiler that built it, so Zeo builds `zeo-rt`
one time with your toolchain, on your first compile. The tree carries a
vendored dependency tree for that build, so it needs no network. The result
goes in the user cache directory, not in the install tree, so the install
stays read-only.

## Tests and conformance

The test suites are in `tests/`. They run as
[`datatest-stable`](https://crates.io/crates/datatest-stable) targets under
`cargo test` or `cargo nextest`, with one case for each `.rb` file.

```console
$ cargo nextest run --workspace                   # unit, e2e and all test suites
$ cargo nextest run -p zeo --test spinel          # the full Ruby corpus
$ cargo nextest run -p zeo --test examples --test gaps
$ ZEO_BLESS=1 cargo test -p zeo --test spinel     # record the expected output again
$ cargo run -p xtask -- bench                     # the performance suite
```

- **`spinel`** is the conformance corpus, with 2,568 programs.
  The suite compares each one with real Ruby, byte for byte.
- **`examples`** holds the programs that Zeo authors wrote, with their correct
  output.
- **`gaps`** holds the programs that do not agree with Ruby yet. The comment at
  the top of each file gives the cause. Each of these tests must fail. If one
  starts to agree with Ruby, the suite fails, and `scripts/promote-gap.sh`
  moves the file.

`ZEO_BLESS=1` is the only way to write the expected output. It runs real Ruby
with `--disable-error_highlight` and `--disable-did_you_mean`, and it records
the result instead of comparing it. [`CONTRIBUTING.md`](CONTRIBUTING.md) gives
the full procedure.

## Project layout

```
crates/      the seven crates of the workspace (above)
docs/        COMPATIBILITY, EXTENSIONS, EVAL_VM, GEM_TESTING, METHOD_COVERAGE, ROADMAP
tests/       the test suites: examples, the spinel corpus, the gaps tracker
gems/        51 gems (gems.toml controls the git-pinned ones)
bench/       the performance suite (`cargo xtask bench`); read bench/README.md
conformance/ what the ruby oracle reports, recorded for the drift tests
vendor/      rubygems and the generated files
tools/       Ruby helper scripts (the arity oracle, method coverage)
scripts/     corpus import, gap promotion, Ruby-against-zeo comparison
```

## Limits

Zeo is experimental. Here are the known limits:

- **There is no tracing garbage collector.** Memory management uses `Arc`
  reference counts, so a cycle of references leaks its memory. This is a known
  limit. `GC.start` runs the finalizers that it can.
- **Four extensions give only a part of their methods.** These are `coverage`,
  `nkf`, `openssl` and `TracePoint`. Read the extension section above.
- **CRuby is faster for 11 of the 58 benchmark programs.** These programs make
  and release many objects. Read the benchmark section above.
- **`Ruby::Box` allocation is compile-time only.** `box = Ruby::Box.new` works
  as a top-level statement, and `box.eval` isolates its constants. A box that
  the program makes at run time, and a run-time `box.require`, raise a clear
  `NotImplementedError`. Read `docs/COMPATIBILITY.md`.
- **Zeo cannot use a gem with a C extension.** Use the `ffi` gem API instead.
  Zeo compiles it ahead of time. Read `docs/EXTENSIONS.md`.

[`docs/ROADMAP.md`](docs/ROADMAP.md) gives the remaining work. Each known difference
from Ruby is a test in [`tests/gaps/`](tests/gaps) that must fail.

## How to contribute

Issues and contributions are welcome. Please help us keep the differences from
Ruby visible:

1. Compare each new behaviour with real Ruby.
2. Write a comment at the code location for each difference that you accept.
3. If a user can see the difference, add a row to `docs/COMPATIBILITY.md`.

Read [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

You can use Zeo under the [MIT](LICENSE-MIT) license or the
[Apache 2.0](LICENSE-APACHE) license. Select the license that you prefer.
