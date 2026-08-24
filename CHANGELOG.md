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
  own headers, vendored verbatim from `ruby/ruby@v4.0.6` plus a patch series.
  Prebuilt MRI binaries never load: zeo is source-compatible and
  ABI-incompatible by design.
- **RubyGems and Bundler support.** `--gem-path` and `--bundle-gemfile` read
  the store and lockfile that Bundler already wrote, so a compile agrees with
  `bundle exec`. Zeo resolves no dependency graph and contacts no network.
- **66 bundled gems** in `gems/`, resolving without a `Gemfile`.
- **Standard-library extensions** in `crates/zeo-rt/src/ext/`, each behind a
  Ruby `require` gate and a cargo feature.
- **Three install tiers.** A relocatable tarball from `tools/zeo-dev dist`,
  `cargo install zeo` from crates.io, and a per-platform binary gem.
- **The disclosure record.** Where Zeo substitutes its own implementation for
  a library, the compile warns, and `--report` writes a `zeo-gems.json` record
  naming every substitution.

### Compatibility

- Targets **CRuby 4.0.6**. `zeo-abi` is the single source of that version, so
  the compiler's version tests and the runtime's `RUBY_VERSION` cannot
  disagree.
- The **method census** compared every module, method, constant and visibility
  reachable in Ruby 4.0.6 against Zeo's surface. It was retired at zero rows.
- The **conformance corpus** compiles 4,288 programs and compares stdout,
  stderr and the exit status with real Ruby byte for byte. Programs that do
  not yet agree live in `tests/gaps/` as tests that must fail.

### Known limits

- Reference counting plus an opt-in cycle collector, not a tracing GC. With
  `ZEO_GC` unset, a reference cycle leaks.
- Four extensions (`coverage`, `nkf`, `openssl`, `TracePoint`) implement part
  of CRuby's surface and raise at the boundary.
- Every `eval` links the compiler into the binary, which costs about 14 MB.
- Zeo does not cross-compile to Linux; each platform builds natively.

[Unreleased]: https://github.com/ryanseys/zeo/commits/main
