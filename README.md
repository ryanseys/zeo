# Zeo

[![CI](https://github.com/ryanseys/zeo/actions/workflows/ci.yml/badge.svg)](https://github.com/ryanseys/zeo/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![MSRV 1.94](https://img.shields.io/badge/MSRV-1.94-orange.svg)](rust-toolchain.toml)

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

## Status

**Experimental**, and moving fast. Read
[Limitations](docs/reference/limitations.md) before depending on it.

The corpus ([`test/`](test)) runs green, each program compared with real Ruby
on stdout, stderr and the exit status. Every module, method, constant and
visibility that Ruby 4.0.6 reaches has a Zeo answer, so what remains is
behavioural, not missing surface.

Every known difference is recorded: in
[Compatibility](docs/reference/compatibility.md) when a user can see it, in
[`test/divergences/`](test/divergences) when it is deliberate, and in
[`test/gaps/`](test/gaps) when it is not fixed yet.

## Install

The first release is not cut yet, so today the way in is a build from
source (below). The release will ship three ways: `cargo install zeo`,
`gem install zeo` (a per-platform gem that carries the binary), and a
relocatable tarball:

```console
$ tar xzf zeo-<version>-<triple>.tar.gz -C /usr/local
```

Building a binary needs a linker (`cc`) on the machine, the same requirement
any native toolchain has.

## Build it yourself

```console
$ git clone https://github.com/ryanseys/zeo && cd zeo
$ cargo xtask deps    # Gemfile.lock -> vendor/, no ruby needed
$ cargo build
$ cargo nextest run
```

Ruby is needed for one thing only: recording a test's answer.
[Getting started](docs/tutorials/getting-started.md) walks the whole loop.

## Documentation

[**docs/**](docs/README.md) is the map. The pages people reach for first:

- [Getting started](docs/tutorials/getting-started.md) — clone to first test
- [CLI reference](docs/reference/cli.md) — every verb and flag
- [Architecture](docs/explanation/architecture.md) — what happens between
  `hello.rb` and `hello`
- [Compatibility](docs/reference/compatibility.md) — library by library
- [Performance](docs/explanation/performance.md) — measured, with the method
  beside it

## Contributing

Issues and pull requests are welcome. A bug report is most useful as the
smallest `.rb` that shows it, what `ruby` printed, what `zeo` printed, and
`zeo --version`; the issue template asks for exactly that. The one rule
that matters: **keep every difference from Ruby visible.**

1. Check each new behaviour against real Ruby.
2. Leave a comment at the site of any difference you accept.
3. If a user can observe it, add a row to
   [Compatibility](docs/reference/compatibility.md) and a note under
   [`test/gaps/`](test/gaps).

[CONTRIBUTING.md](CONTRIBUTING.md) has the conventions.

## License

MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache 2.0
([LICENSE-APACHE](LICENSE-APACHE)), at your option.
