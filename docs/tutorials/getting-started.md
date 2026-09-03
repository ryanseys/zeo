# Getting started

You will build zeo from a clean clone, run a Ruby program with it, run the
test suite, and add a test of your own. It takes about ten minutes, most of
it the first build.

## What you need

- A **Rust toolchain**. `rust-toolchain.toml` pins the version and rustup
  installs it on first use.
- A **C compiler** (`cc`). The tree holds no C of its own, but a few `-sys`
  crates vendor some (Prism, Oniguruma, libffi, OpenSSL), and every compiled
  program is linked.
- **Network access**, once, to fetch the libraries `Gemfile.lock` names.

You do **not** need Ruby. It is needed only to record a test's answer, which
is the last step on this page.

## Build

```console
$ git clone https://github.com/ryanseys/zeo && cd zeo
$ cargo xtask deps
$ cargo build
```

`cargo xtask deps` fetches the gem store into `vendor/`, verifying every
download against the checksum `Gemfile.lock` states. It needs `curl` and
`git` and nothing else. A second run says nothing.

`cargo build` produces two artifacts side by side:

- `target/debug/zeo` — the compiler and CLI, with the runtime and the JIT
  linked in.
- `target/debug/libzeo.a` — the runtime archive every `-o` binary links
  against.

## Run a program

```console
$ echo 'puts [1, 2, 3].map { |x| x * 2 }.sum' > hello.rb
$ target/debug/zeo hello.rb
12
```

That compiled the whole program in memory and ran it. To write a binary
instead:

```console
$ target/debug/zeo -o hello hello.rb
$ ./hello
12
```

The binary needs no Ruby and no zeo. It carries the runtime.

## Run the suite

```console
$ cargo nextest run
```

Every program under `test/` is compiled, run, and held to the answer
recorded in its own file. That answer came from real ruby, so a failure means
zeo and ruby disagree.

The whole gate, including the tests that are slow one at a time:

```console
$ cargo nextest run -P full
```

## Add a test

A test is one `.rb` file. The program is at the top, and the answer goes
under `__END__`:

```ruby
# What the program is about, in a sentence.
puts "ab".upcase
__END__
AB
```

Put it in the topic directory it belongs to — `test/core/string/` for a
String question, `test/lang/blocks/` for a block one — and run it:

```console
$ cargo nextest run -E 'test(upcase)'
```

To have ruby write that trailer for you rather than typing it, you need the
pinned ruby (`.ruby-version` says which) on PATH or named by `ZEO_RUBY`:

```console
$ cargo xtask bless core::string/upcase.rb
```

`bless` runs ruby, records what it printed, and refuses to record anything
if the ruby it finds is not the pinned one — a trailer from another ruby
records that ruby's differences as zeo's bugs.

## Where to go next

- [Add a test](../how-to/add-a-test.md) — directives, fixtures, and the
  programs that are supposed to differ.
- [Architecture](../explanation/architecture.md) — what happens between
  `hello.rb` and `hello`.
- [Run the tests](../how-to/run-the-tests.md) — profiles and filters.
