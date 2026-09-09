# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Zeo is pre-1.0. Until 1.0, a minor version may change behaviour that a 1.0
release would treat as breaking. Each such change is listed under **Changed**
with the reason.

## [Unreleased]

The first public release, not yet cut.

### Added

- **The compiler.** Zeo compiles a whole Ruby program to one native
  executable. It reads the source with Prism, resolves every `require` at
  compile time, lowers the result into a typed HIR arena, and emits Cranelift
  IR linked against a precompiled runtime. The program needs no Ruby on the
  machine that runs it.
- **Two dispatch paths.** A call the compiler can resolve becomes a direct
  call. Everything else goes through the runtime method registry, which also
  serves `send`, `define_method`, `method_missing`, singletons, and classes
  built at run time with `Class.new`.
- **The runtime** (`zeo-rt`): the numeric tower (`Integer`, `Bignum`,
  `Rational`, `Complex`), byte strings with encodings, real OS threads,
  stack-switching fibers, Ruby 4.0's `Ractor` port model, and exceptions that
  propagate as a Rust `Result`. The GVL is armed only when a C extension
  loads, because an extension may assume it.
- **An opt-in cycle collector** (`ZEO_GC=1`): root-free refcount
  reconciliation over an allocation registry, which reclaims what refcounting
  cannot.
- **C extensions from source.** A gem's `ext/**/*.c` compiles against MRI's
  own headers, fetched from `ruby/ruby@v4.0.6` at first use with zeo's edits
  applied.
  Prebuilt MRI binaries never load: zeo is source-compatible and
  ABI-incompatible by design.
- **RubyGems and Bundler support.** `--gem-path` and `--bundle-gemfile` read
  the store and lockfile that Bundler already wrote, so a compile agrees with
  `bundle exec`. Zeo resolves no dependency graph and contacts no network.
- **A bundled standard library.** Every gem `Gemfile.lock` names ships with
  the compiler and resolves without a `Gemfile`.
- **Standard-library extensions** in `crates/zeo-rt/ext/`, each behind a
  Ruby `require` gate and a cargo feature.
- **Three install tiers.** A relocatable tarball from `cargo xtask dist`,
  `cargo install zeo` from crates.io, and a per-platform binary gem.
- **The disclosure record.** Where Zeo substitutes its own implementation for
  a library, the compile warns, and `--report` writes a `zeo-gems.json` record
  naming every substitution.
- **`zeo backend`.** `zeo backend f.clif -o bin` links a program from
  Cranelift IR written as text by another front end, with a `.zeodata`
  sidecar carrying what the text cannot say; `--emit-clif` names every
  function and symbol so its text reads back, and `--emit-zeodata` writes
  the sidecar.
- **Located codegen diagnostics.** A backend refusal carries the offending
  node's source span, and the CLI renders the same annotated excerpt the
  parse and lower stages show. The location left the message text; a
  run-time `eval`'s `NotImplementedError` keeps its `(file:line)` suffix
  unchanged.

### Fixed

- `Proc#==`/`#eql?` invoked through `send` with no argument raised the
  correct `ArgumentError` instead of crashing the process. The fix came out
  of the runtime-wide sweep that moved builtin argument handling onto the
  shared macro kit (declared parameter lists, `check_arity`, `arg_int!`,
  `arg_str!`).

### Distribution

- **One `Gemfile.lock` decides what Zeo ships.** The bundled stdlib is
  resolved out of `vendor/bundle` at the version the lock states, and the
  same lock is what the Ruby oracle resolves — so a golden cannot record
  a difference between two library versions and call it a Zeo bug.
  `rubygems` and `bundler` are fetched from their git tags into `vendor/`,
  because `bundle install` cannot supply the bundler that runs it.
- **One payload, two published artifacts.** `cargo xtask dist` assembles the
  release tarball; `cargo xtask gem` rearranges that same staging into a
  per-platform gem. Neither the compiler nor the runtime knows what a gem
  is: `exe/zeo` is a Ruby launcher that `exec`s `libexec/zeo`, and both sit
  where the existing executable-relative payload probe already looks.
- The gem declares **no runtime dependencies**. Zeo bundles a stdlib, and
  bundling is not depending — `gem install zeo` must not force versions into
  a user's store, and for the libraries Zeo reimplements a dependency would
  be a false claim.

### Compatibility

- Targets **CRuby 4.0.6**. `zeo-abi` is the single source of that version, so
  the compiler's version tests and the runtime's `RUBY_VERSION` cannot
  disagree.
- Every module, method, constant and visibility reachable in Ruby 4.0.6 has
  a Zeo answer; what remains is behavioural, and recorded.
- The **conformance corpus** under `test/` compiles thousands of programs and
  compares stdout, stderr and the exit status with real Ruby byte for byte.
  Programs that do not yet agree live in `test/gaps/` as tests that must
  fail.

### Known limits

- Reference counting plus an opt-in cycle collector, not a tracing GC. With
  `ZEO_GC` unset, a reference cycle leaks.
- Four extensions (`coverage`, `nkf`, `openssl`, `TracePoint`) implement part
  of CRuby's surface and raise at the boundary.
- Every `eval` links the compiler into the binary, which costs about 14 MB.
- Zeo does not cross-compile to Linux; each platform builds natively.

[Unreleased]: https://github.com/ryanseys/zeo/commits/main
