# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Zeo is pre-1.0. Until 1.0, a minor version may change behaviour that a 1.0
release would treat as breaking. Each such change is listed under **Changed**
with the reason.

## [Unreleased]

## [0.1.0] - 2026-08-05

The first public release.

### Added

- **The compiler.** Zeo compiles a whole Ruby program to one native
  executable. It reads the source with Prism, resolves every `require` at
  compile time, lowers the result into a typed HIR arena, writes Rust, and
  links a precompiled runtime. The program needs no Ruby on the machine that
  runs it.
- **Two dispatch paths.** A call the compiler can resolve becomes a direct
  Rust call. Everything else goes through the runtime method registry, which
  also serves `send`, `define_method`, `method_missing`, singletons, and
  classes built at run time with `Class.new`.
- **The runtime** (`zeo-rt`): the numeric tower (`Integer`, `Bignum`,
  `Rational`, `Complex`), byte strings with encodings, real OS threads with no
  GVL, stack-switching fibers, Ruby 4.0's `Ractor` port model, and exceptions
  that propagate as a Rust `Result`.
- **RubyGems and Bundler support.** `--gem-path` and `--bundle-gemfile` read
  the store and lockfile that Bundler already wrote, so a compile agrees with
  `bundle exec`. Zeo resolves no dependency graph and contacts no network.
- **51 bundled gems** in `gems/`, resolving without a `Gemfile`.
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
- The **method census** compares every module, method, constant and visibility
  reachable in Ruby 4.0.6 against Zeo's surface. Its gap ledger holds zero
  rows.
- The **conformance corpus** compiles roughly 2,509 programs and compares
  stdout and stderr with real Ruby byte for byte. Programs that do not yet
  agree live in `tests/gaps/` as tests that must fail.

### Known limits

- No tracing garbage collector. Memory is `Arc`-refcounted, so a reference
  cycle leaks.
- A gem with a C extension stops the compile. Use the `ffi` gem API instead.
- Four extensions (`coverage`, `nkf`, `openssl`, `TracePoint`) implement part
  of CRuby's surface and raise `NoMethodError` at the boundary.
- `Ruby::Box` allocation is compile-time only.

[Unreleased]: https://github.com/ryanseys/zeo/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ryanseys/zeo/releases/tag/v0.1.0
