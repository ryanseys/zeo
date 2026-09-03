# Record an answer

`cargo xtask bless` is the only thing that writes a trailer.

```console
$ cargo xtask bless core::string/upcase.rb   # one program
$ cargo xtask bless core::string/            # a directory
$ cargo xtask bless --all                    # everything (refuses a vague filter otherwise)
```

A filter is a substring of the case name — `<suite>::<path>`, the spelling
nextest reports — or a path under `test/`.

## What it needs

**The pinned ruby**, for every suite whose answer comes from ruby.
`.ruby-version` says which version. `bless` finds it on PATH, or wherever
`ZEO_RUBY` points, and refuses to record if the version does not match: a
trailer recorded by another ruby records that ruby's differences as zeo's
bugs.

```console
$ ZEO_RUBY=/opt/ruby-4.0.6/bin/ruby cargo xtask bless core::string/
```

`bless` runs `cargo xtask deps --oracle` first. That installs the lock's gems
into the oracle's own store with RubyGems' installer, so the ruby it asks
resolves the same libraries zeo ships. It is idempotent and says nothing when
the store is already complete.

**A built `target/release/zeo`** (or `ZEO_BIN`) for `test/errors/`,
`test/features/` and `test/divergences/`, which record zeo's own answer.

## What it does not do

It does not decide whether the answer is right. It records what ruby printed.
Reading the diff is the job:

```console
$ git diff test/
```

A trailer that changed when you did not expect it to is the finding.

## Comparing one snippet without writing a file

```console
$ cargo xtask diff 'puts [1,2].sum'
```

Runs the snippet under both engines and shows the difference. With a
divergence worth keeping, it files the program for you.
