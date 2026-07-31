# zeo: an ahead-of-time Ruby compiler

zeo compiles a full Ruby program into **one native executable**. It reads the
source with [Prism], which is CRuby's own parser. It then analyzes the full
program, writes Rust, and links a runtime that is already compiled.

The result starts immediately. It does not start an interpreter, and it does
not warm up a JIT. The machine that runs the binary does not need Ruby.

```console
$ cat hello.rb
puts [1, 2, 3].map { |x| x * 2 }.sum
$ zeo hello.rb -o hello
$ ./hello
12
```

zeo compares each behaviour with real Ruby, and writes down each difference. The
goal is to compile **rubygems and bundler** without changes.

[Prism]: https://github.com/ruby/prism

## How zeo works

```
foo.rb ──prism──▶ HIR arena ──analyze──▶ typed classes/MRO ──codegen──▶ Rust ──rustc──▶ foo
                  (full program:            (compile-time             (quote! +        (links the
                   requires spliced          method tables,            prettyplease)    precompiled
                   at compile time)          type inference)                            zeo-rt runtime)
```

- **zeo compiles the full program at one time.** It resolves `require` and
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

For now, you build zeo from source. There is no `gem install zeo` yet. Build the
compiler one time, then compile your Ruby programs with it.

```console
# Build the compiler. This writes target/release/zeo.
$ cargo build --release -p zeo

# Compile a file to a native binary, then run the binary.
$ target/release/zeo hello.rb        # writes ./hello (the input path, without the extension)
$ ./hello

# Select a different output path.
$ target/release/zeo hello.rb -o build/hello

# Compile and run a program from the command line, as `ruby -e` does.
$ target/release/zeo -e 'puts "hello, world"'

# Show the generated Rust. This does not build a binary.
$ target/release/zeo hello.rb -S
```

**Note:** `zeo foo.rb` compiles the program, but it does not run the program.
Only `-e` compiles and runs. There are no subcommands, and there is no
`zeo run`. The command line agrees with Ruby's: `zeo <file>` or `zeo -e <code>`.

## Command-line options

```
usage: zeo (<input.rb> | -e <code>) [options]
```

| Option | Function |
|---|---|
| `<input.rb>` | Compiles the file to a native binary. The default output path is the input path without its extension. |
| `-e <code>` | Compiles the given code and runs it immediately. Sends stdout, stderr and the exit status to the caller. You can give this option more than one time; zeo joins the parts with newlines. With `-o`, zeo writes a binary and does not run it. |
| `-o <output>` | Sets the path of the compiled binary. |
| `-S` | Prints the generated Rust source, then stops. Does not build. |
| `-I <dir>` | Adds a directory to the `require` search path, as Ruby's `-I` does. You can give this option more than one time. The form `-I<dir>` is also correct. |
| `--packages <dir>` | Adds a gem directory. zeo searches it before the project and bundled directories. You can give this option more than one time. |
| `--gem-path <dir>` | Sets the external gem store. **You must also give `--lockfile`.** |
| `--lockfile <path>` | Gives the `Gemfile.lock` that selects the versions in `--gem-path`. You must give it together with `--gem-path`. |
| `--no-report` | Stops zeo from writing the `zeo-gems.json` record. zeo writes this record with each artifact by default. |
| `--nowarn <slug>` | Stops one category of disclosure warning. You can give this option more than one time. The form `--nowarn=<slug>` is also correct. |
| `--log-level <level>` | Writes compiler diagnostics to stderr. The levels are `off`, `error`, `warn`, `info`, `debug` and `trace`. The form `--log-level=<level>` is also correct. This option has priority over `ZEO_LOG` and `RUST_LOG`. |
| `-h`, `--help` | Shows the help text, then stops. |

Environment variables:

| Variable | Function |
|---|---|
| `ZEO_LOG`, `RUST_LOG` | Give a `tracing` `EnvFilter` directive. This gives more control than `--log-level`. Example: `ZEO_LOG=zeo::analyze=debug,zeo::lower=trace`. If you set none of these variables, zeo installs no subscriber and writes no diagnostics. |
| `ZEO_RUNTIME_PROFILE` | Selects the profile of the linked runtime: `debug` or `release`. The default is `debug` for `-e`, and `release` for a file or `-o` compile. |
| `ZEO_GVL` | If you set `ZEO_GVL=1`, the threads in that run use CRuby's schedule. This is a FIFO global lock with a 100 ms timer. The default is parallel OS threads. |
| `ZEO_BLESS` | If you set `ZEO_BLESS=1`, the test suites record their expected output again from real Ruby. Use this only during development. |

## Ruby features

zeo aims at the full language. These features work today:

- **The numeric tower.** `Integer` and `Bignum` have unlimited precision, and
  `Integer` becomes `Bignum` when necessary. `Float`, `Rational` and `Complex`
  are also available. Coercion and error messages agree with CRuby.
- **Strings and encodings.** A string is a sequence of bytes plus an encoding.
  The engine has 24 encodings. These include UTF-8, the Windows-125x and
  ISO-8859 families, and Shift_JIS, EUC-JP, GBK and Big5 through the WHATWG
  tables. `docs/COMPATIBILITY.md` lists the differences.
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
  that changes the stack (`corosensei`). A `Ractor` has a boundary that freezes
  or copies. `Mutex` and `Queue` are available.
- **Regexp.** Real engines do the work: `regex` and `fancy-regex`. zeo also
  includes Oniguruma for the paths that need Onigmo behaviour.
- **`eval`.** zeo parses a literal `eval("…")` and puts it into the program at
  compile time. A dynamic `eval`, whose text the program computes at run time,
  needs the interpreter in [`docs/EVAL_VM.md`](docs/EVAL_VM.md). The `eval-vm`
  feature controls that interpreter, and zeo links it only into a program that
  can get to it.

zeo is experimental, and some parts are not complete. This list is a summary,
and the conformance corpus below is the record of what agrees with Ruby.

## Compatibility

zeo targets **CRuby 4.0.6**. The version has one source, `zeo-abi`. Therefore
the compiler's version tests and the runtime's `RUBY_VERSION` always agree.

zeo reports compatibility as text, and not as a percentage. A green corpus run
is the record. A note such as "zeo's `json` is not the `json` gem" carries more
information than a score.

- **The conformance suite** in `tests/spinel/` compiles approximately 2,509
  programs. It compares stdout and stderr with real Ruby, byte for byte, as
  `cargo nextest` cases. `tests/gaps/` holds the programs that do not agree
  yet. Each of these must fail. If one starts to agree with Ruby, the suite
  fails, and that program then moves into the corpus.
- **zeo declares each substitution.** For some libraries, zeo supplies its own
  code: `json` uses serde_json, `psych` and `yaml` use yaml-rust2, `zlib` uses
  flate2, `digest` uses RustCrypto, and `openssl` uses a vendored OpenSSL 3.
  The compile writes one warning. It also writes a `zeo-gems.json` record with
  the artifact. That record tells you which libraries are different, and why.
- **A gem with a C extension fails clearly.** If zeo has no built-in for the
  extension, the error gives the name of the gem, such as `sqlite3`, `nokogiri`
  or `pg`. The error also points to the FFI path. It does not look like an
  unknown language feature.

For the full list of substitutions and differences, read
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md). For the extension model, read
[`docs/EXTENSIONS.md`](docs/EXTENSIONS.md). For the remaining work, read
[`docs/ROADMAP.md`](docs/ROADMAP.md).

## Bundled gems

zeo includes 51 gems in `gems/`. They resolve without a `Gemfile`. If a program
that zeo compiles has `require "csv"`, zeo uses the copy in this repository.

The **Origin** column tells you where the Ruby source comes from:

- **git-pinned** — `cargo xtask gem` gets the gem from its own repository. The
  `rev` in `gems.toml` gives the exact commit.
- **upstream** — a copy of a default or bundled gem from a Ruby installation,
  with no changes. Most come from Ruby 4.0.5. irb, minitest and reline come
  from Ruby 4.0.6.
- **upstream +zeo** — the same, but with changes. Each change has a `zeo:` mark
  at its location.
- **zeo Ruby half** — zeo wrote the `.rb` files. A Rust extension in
  `crates/zeo-rt/src/ext/` supplies the native part. CRuby makes the same
  division between `rubylibdir` and `archdir`.

The **Test** column gives a test in `tests/`. That test compiles the gem and
compares its output with real Ruby 4.0.6, byte for byte. A dash means that no
test measures this gem alone. Some gems run only as a dependency of another
gem. For the full source information and the licences, read
`gems/UPSTREAM.md`.

| Gem | Version | Origin | Test | Differences |
|---|---|---|---|---|
| abbrev | 0.1.2 | git-pinned | `issue_abbrev_missing.rb` | — |
| benchmark | 0.4.1 | git-pinned | `issue_benchmark_missing.rb` | — |
| bigdecimal | 4.1.2 | upstream +zeo | `bigdecimal.rb` | zeo supplies the native part ([compat](docs/COMPATIBILITY.md)) |
| bundler | 4.0.16 | git-pinned | `gem_bundler.rb` | the test starts at `bundler/version` ([roadmap](docs/ROADMAP.md)) |
| csv | 3.3.6 | git-pinned | `gem_csv.rb` | — |
| delegate | 0.6.1 | upstream | `issue_require_delegate_crashes.rb` | — |
| drb | 2.2.3 | git-pinned | `gem_drb.rb` | — |
| English | 0.8.1 | upstream | `english_special_globals.rb` | — |
| erb | 6.0.6 | git-pinned | `erb_module_function.rb` | — |
| ffi | 1.17.4 | zeo Ruby half | `ffi_struct.rb` | the native part uses `libffi` |
| fiddle | 1.1.8 | upstream +zeo | `fiddle.rb` | no `Importer` DSL ([compat](docs/COMPATIBILITY.md)) |
| fileutils | 1.8.0 | git-pinned | `fileutils.rb` | — |
| find | 0.2.0 | git-pinned | `issue_find_missing.rb` | — |
| forwardable | 1.4.0 | upstream | `issue_3300_forwardable.rb` | — |
| ipaddr | 1.2.9 | git-pinned | — | — |
| irb | 1.18.0 | upstream | — | compiles, but stops at a refinement module that the program makes at run time ([gap](tests/gaps/issue_runtime_refinement_module.rb)) |
| json | 2.18.0 | zeo Ruby half | `json_to_json.rb` | uses serde_json, not the json gem ([compat](docs/COMPATIBILITY.md)) |
| logger | 1.7.0 | git-pinned | `issue_logger_missing.rb` | — |
| minitest | 6.0.6 | upstream | `gem_minitest.rb` | a full-program compile includes the `MT_HELL` branch of `autorun`, which writes a note to stderr |
| monitor | 0.1.0 | zeo Ruby half | `gem_two_halves.rb` | `Monitor` and `MonitorMixin` only |
| net-ftp | 0.3.9 | git-pinned | `issue_net_ftp_missing.rb` | — |
| net-http | 0.9.1 | git-pinned | `gem_net_http.rb` | — |
| net-protocol | 0.2.2 | git-pinned | — | — |
| net-smtp | 0.5.1 | git-pinned | `gem_net_smtp.rb` | — |
| nkf | 0.3.0 | zeo Ruby half | `nkf.rb` | some options only; `guess` uses a different method ([compat](docs/COMPATIBILITY.md)) |
| observer | 0.1.2 | git-pinned | `gem_observer.rb` | — |
| open3 | 0.2.1 | git-pinned | `open3_capture.rb` | — |
| openssl | 4.0.2 | zeo Ruby half | `openssl_cipher.rb` and 6 more | no PKey generation, no X509 issue, no `SSLServer` ([compat](docs/COMPATIBILITY.md)) |
| optparse | 0.8.1 | zeo Ruby half | `optparse_subset.rb` | the common `OptionParser` methods |
| ostruct | 0.6.3 | upstream | `issue_3331_poly_to_sym_arm.rb` | — |
| pp | 0.6.4 | upstream | `pp_pretty_print.rb` | — |
| prettyprint | 0.2.0 | upstream | — | — |
| prism | 1.9.0 | upstream +zeo | `gem_prism.rb` | no `translation/` and no `ffi.rb` |
| psych | 5.4.0 | zeo Ruby half | `psych_load_file_and_stream.rb` | uses yaml-rust2, not libyaml ([compat](docs/COMPATIBILITY.md)) |
| pty | 0.5.9 | zeo Ruby half | `pty_spawn.rb` | — |
| racc | 1.8.1 | git-pinned | `issue_racc_parser_missing.rb` | — |
| reline | 0.6.3 | upstream +zeo | `reline_line_editor.rb` | one `zeo:` change in `io.rb` |
| resolv | 0.7.1 | git-pinned | `issue_resolv_missing.rb` | — |
| rubygems | 4.0.16 | git-pinned | `gem_rubygems.rb` | the test starts below the top-level require ([roadmap](docs/ROADMAP.md)) |
| shellwords | 0.2.2 | upstream | `shellwords.rb` | — |
| singleton | 0.3.0 | upstream | — | `singleton_class.include?` ([gap](tests/gaps/issue_singleton_class_include_after_extend.rb)) |
| strscan | 3.1.6 | zeo Ruby half | `strscan_capture_surface.rb` | zeo supplies its own code ([compat](docs/COMPATIBILITY.md)) |
| syslog | 0.4.0 | zeo Ruby half | `syslog.rb` | — |
| tempfile | 0.3.1 | git-pinned | `issue_require_tempfile_codegen_path_attr.rb` | — |
| time | 0.4.1 | git-pinned | `time_parse.rb` | — |
| timeout | 0.6.1 | upstream | — | — |
| tmpdir | 0.3.1 | git-pinned | `io_encoding.rb` | — |
| tsort | 0.2.0 | upstream | — | — |
| un | 0.3.0 | git-pinned | `issue_un_missing.rb` | — |
| uri | 1.1.1 | git-pinned | `uri_parse_and_build.rb` | — |
| zlib | 3.2.3 | zeo Ruby half | `zlib_classes.rb` | uses flate2; 4 functions are not available ([compat](docs/COMPATIBILITY.md)) |

To use a gem that is not in this list, give `--gem-path` and `--lockfile`.

If a gem needs a C extension that zeo has no built-in for, the `require` fails
and the error gives the name of the gem. The function `is_known_native_gem` in
`crates/zeo/src/parse/loader.rs` holds 14 such names: `sqlite3`, `nokogiri`,
`pg`, `mysql2`, `bcrypt`, `nio4r`, `puma`, `grpc`, `protobuf`, `oj`, `msgpack`,
`eventmachine`, `sass` and `rmagick`. The list is deliberately not complete.
For all other names, zeo gives CRuby's usual message, `cannot load such file`.
The list does not include `ffi`, because zeo supplies `ffi`.

## Standard-library extensions

The native part of a gem is a Rust module in `crates/zeo-rt/src/ext/`. CRuby
uses the same `ext/` model. Each module uses the `ruby_class!` DSL, which is
also used for the core classes.

Two independent gates control each extension:

1. **A Ruby `require` gate.** The constant of the extension stays invisible
    until the program executes its `require`.
2. **A cargo feature**, `ext-<name>`. The default feature set is `ext-all`, so
    a usual build includes all of the extensions.

Each method in these modules is complete, and zeo compares it with real Ruby.
No method is an empty `todo!()`. If the library behind a module is different
from CRuby's, the compile writes one warning and records it in
`zeo-gems.json`. `docs/COMPATIBILITY.md` gives the reason for each library.

| Extension | `require` | Cargo feature | Uses |
|---|---|---|---|
| base64 | `base64` | `ext-base64` | zeo code |
| bigdecimal | `bigdecimal` | `ext-bigdecimal` | `num-bigint` |
| cgi | `cgi/escape`, `cgi`, `cgi/util` | `ext-cgi` | zeo code; escape and unescape only |
| coverage | `coverage` | `ext-coverage` | zeo code; line coverage only |
| date | `date` | `ext-date` | zeo code |
| digest | `digest`, `digest/*` | `ext-digest` | RustCrypto (`md-5`, `sha1`, `sha2`) |
| etc | `etc` | `ext-etc` | `libc` |
| fcntl | `fcntl` | `ext-fcntl` | `libc` |
| ffi | `ffi` | `ext-ffi` | `libffi`, included in this repository |
| json | `json` | `ext-json` | `serde_json` |
| monitor | `monitor` | `ext-monitor` | zeo thread primitives |
| nkf | `nkf`, `kconv` | `ext-nkf` | the zeo encoding engine |
| openssl | `openssl` | `ext-openssl` | OpenSSL 3 through rust-openssl, included in this repository |
| pathname | `pathname` | `ext-pathname` | zeo code |
| prism | `prism` | `ext-prism` (with `eval-vm`) | `ruby-prism` |
| psych | `psych`, `yaml` | `ext-psych` | `yaml-rust2` |
| pty | `pty` | `ext-pty` | `libc`, function `openpty(3)` |
| readline | `readline` | `ext-readline` | `rustyline` |
| socket | `socket` | `ext-socket` | `libc` |
| stringio | `stringio` | `ext-stringio` | zeo code |
| strscan | `strscan` | `ext-strscan` | the zeo Regexp engine |
| syslog | `syslog`, `syslog/logger` | `ext-syslog` | `libc`, function `syslog(3)` |
| tracepoint | none; part of the core | `ext-tracepoint` | zeo code |
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
does nothing. `rbconfig` resolves through a file that zeo generates.

## Benchmarks

The `bench/` directory holds 58 Ruby programs, and each one has its correct
output. `cargo xtask bench` compiles each program with `zeo -o`, which links
the release runtime and makes a static binary. This is the same configuration
that a user gets.

Before it measures a program, the tool compares the output of that program with
the correct output, byte for byte. A speed number is meaningless if the answer
is wrong. The tool then measures the time more than one time and keeps the
smallest value. It measures CRuby 4.0.6 in the same run.

zeo gives two results:

| Programs | Geometric mean |
|---|---|
| all 58 programs | **1.78 times faster than CRuby** |
| the 37 programs where CRuby needs 0.10 s or more | **1.23 times faster** |

The difference between the two results is the start time. For 13 programs,
CRuby needs less than 50 ms. A native binary starts immediately, but the
interpreter needs approximately 35 ms to start. This is a real advantage of a
binary, but it says nothing about the quality of the generated code. The second
result covers the programs that run long enough for the generated code to
control the time. zeo is faster for 47 programs, and slower for 11 programs.

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

zeo is a Cargo workspace with six crates. It uses edition 2024, and the minimum
Rust version is 1.87.

```
crates/
  zeo         the compiler and the CLI: parse ▸ lower ▸ analyze ▸ codegen ▸ backend
  zeo-rt      the runtime that links into each compiled program (the largest crate)
  zeo-abi     a leaf crate with no dependencies: the ClassId numbers that both sides use
  zeo-dsl     the shared `syn` grammar for the ruby_class! and ruby_module! DSL
  zeo-macros  the macro that expands that DSL into runtime code
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

- **Rust 1.87 or later**, because zeo uses edition 2024. Read `rust-version`.
- **A C compiler**, for Prism and Oniguruma.
- **Ruby 4.0.6**, but only to record the expected test output again. The file
  `mise.toml` gives this version.

```console
$ git clone https://github.com/ryanseys/zeo && cd zeo
$ cargo build --release -p zeo
$ target/release/zeo yourprogram.rb -o yourprogram
```

zeo builds the runtime `zeo-rt` automatically, the first time that you compile
a program. It builds each combination of profile, runtime variant and linkage
one time only. To build the runtime yourself, use
`cargo build --release -p zeo-rt`. Add `--features eval-vm` for programs that
use a dynamic `eval`.

The file `mise.toml` sets the development tools: `ruby = "4.0.6"` and
`rust = "1.97.1"`.

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

- **`spinel`** is the conformance corpus, with approximately 2,509 programs.
  The suite compares each one with real Ruby, byte for byte.
- **`examples`** holds the programs that zeo authors wrote, with their correct
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
crates/      the six crates of the workspace (above)
docs/        COMPATIBILITY, EXTENSIONS, EVAL_VM, TODO
tests/       the test suites: examples, the spinel corpus, the gaps tracker
gems/        51 gems (gems.toml controls the git-pinned ones)
bench/       the performance suite (`cargo xtask bench`); read bench/README.md
conformance/ what the ruby oracle reports, recorded for the drift tests
vendor/      rubygems and the generated files
tools/       Ruby helper scripts (the arity oracle, method coverage)
scripts/     corpus import, gap promotion, Ruby-against-zeo comparison
```

## Limits

zeo is experimental. Here are the known limits:

- **There is no tracing garbage collector.** Memory management uses `Arc`
  reference counts, so a cycle of references leaks its memory. This is a known
  limit. `GC.start` runs the finalizers that it can.
- **Four extensions give only a part of their methods.** These are `coverage`,
  `nkf`, `openssl` and `TracePoint`. Read the extension section above.
- **CRuby is faster for 11 of the 58 benchmark programs.** These programs make
  and release many objects. Read the benchmark section above.
- **A dynamic `eval` and `Ruby::Box` isolation are not complete.** Read
  `docs/`.
- **zeo cannot use a gem with a C extension.** Use the `ffi` gem API instead.
  zeo compiles it ahead of time. Read `docs/EXTENSIONS.md`.

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

You can use zeo under the [MIT](LICENSE-MIT) license or the
[Apache 2.0](LICENSE-APACHE) license. Select the license that you prefer.
