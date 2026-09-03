# Cut a release

A tag does it. Everything below is what the tag sets off, and how to rehearse
it first.

## Rehearse

`workflow_dispatch` on the Release workflow builds and uploads every artifact
without creating a GitHub Release. Do that before tagging.

Locally, for the platform you are on:

```console
$ cargo xtask dist
```

That builds the tree, stages the payload, and then proves the staged binary
works: it resolves its payload relative to `bin/`, links against the staged
`libzeo.a`, and compiles and runs a Ruby program. The result is
`target/dist/zeo-<version>-<triple>.tar.gz`.

```
zeo-<version>-<triple>/
  bin/zeo
  share/zeo/{lib/ruby, lib/<triple>/libzeo.a, dist-manifest.json}
  share/doc/zeo/
```

The binary finds its payload through `bin/../share/zeo`, so the tree
relocates anywhere; `ZEO_HOME` overrides the search.

## The gems

`zeo.gemspec` at the root builds two different gems, and which one depends on
what is beside it.

```console
$ gem build zeo.gemspec   # the SOURCE gem: launcher and docs, no binary
$ cargo xtask gem         # the PLATFORM gem: the staging above, packaged
```

The source gem is what a `gem install zeo` gets when RubyGems has no build
for the platform. It installs, and its `zeo` says which platform gem to fetch
rather than failing obscurely.

The platform gem is `cargo xtask dist`'s tree with `bin/` renamed `libexec/`,
because RubyGems binstubs an executable by `load`ing it as Ruby and zeo is a
native binary — `exe/zeo` is the Ruby that gets loaded and `exec`s the real
one. `cargo xtask gem` sets `ZEO_GEM_PLATFORM`, stages, and runs `gem build`,
RubyGems' own packager, so the published gem is not built by zeo.

## What the tag does

Push `v<version>` and the Release workflow builds each target natively —
cross-compiling is ruled out because `dist` finishes by running a compiled
program — uploads the tarballs and gems, checks that the tag matches the
workspace version, and creates the Release.

## The published crates

The `zeo` crate carries two artifacts the repo does not commit: the
pregenerated class surface, and the bundled libraries as a tarball extracted
on first run.

```console
$ cargo xtask stage-crate
$ cargo xtask stage-crate --check
```

`--check` verifies a staged copy still matches a fresh generation. CI runs
it, so a release cannot ship a stale staging.

Publish in dependency order: `zeo-abi`, `zeo-dsl`, `zeo-gem`, `zeo-macros`,
`zeo-rt`, `zeo-capi`, `zeo`. The family is version-locked with `=`, so they
go out together.
