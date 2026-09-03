# Update the gem versions

`Gemfile.lock` is the only place a bundled library's version is written. Both
the compiler and the ruby oracle read it, so the two cannot disagree about a
release.

## Bump one gem

Edit the pin in `Gemfile` — every gem is pinned exactly, on purpose — then
re-resolve with real bundler:

```console
$ bundle lock
$ cargo xtask deps
$ cargo nextest run
```

`deps` downloads what changed and unpacks it, and removes what the lock no
longer names. A library whose behaviour moved shows up as a corpus failure;
re-record those and read the diff:

```console
$ cargo xtask bless stdlib::json/
$ git diff test/
```

## Bump rubygems and bundler

They ship from one release, `rubygems-update`, and come from a git tag rather
than the store — the store is what zeo's own bundler fills, so the bundler
that runs the install cannot come out of it.

Bump both files together:

- the `rubygems-update` pin in `Gemfile`, then `bundle lock`
- `tag`, `rev` and `version` in `crates/xtask/rubygems.lock`

`deps` refuses a pair that disagrees, and refuses a tag that has moved off
the recorded revision.

```console
$ git ls-remote --tags https://github.com/rubygems/rubygems v4.0.20
$ cargo xtask deps
```

## Refetch everything

```console
$ cargo xtask deps --refresh
```

Re-unpacks the store from the cache, even where it looks complete.
