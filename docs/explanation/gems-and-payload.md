# Gems and the payload

Zeo ships a Ruby standard library. The upstream gems are fetched, not
committed, and Ruby is not needed to get them.

## Four tiers answer a `require`

Highest precedence first, so a name two tiers carry resolves to zeo's own.

| Tier | Where | What it holds |
|---|---|---|
| zeo's own | `crates/zeo-rt/ext/<name>/` | a Ruby half beside the Rust that implements it |
| bootstrap | `vendor/ruby/{rubygems,bundler}/` | rubygems and bundler, at the pinned tag |
| resolved | `vendor/gems/` | every other library, from `Gemfile.lock` |
| the project's | a user's own store | whatever their `Gemfile.lock` names |

`crates/zeo/src/gems/bundled.rs` decides that list, and it is the one answer
both a compile and `cargo xtask dist` read, so the payload and the compiler
cannot disagree about what ships.

## Why the lock is the only writer

A gem's version written in two places — a committed tree's own gemspec,
and the lock the ruby oracle resolves — drifts, and the corpus then records
the difference between two library versions as a zeo bug.

`Gemfile.lock` states every version once. The compiler reads it and the
oracle reads it, so a golden cannot record such a difference.

## Why the bootstrap pair is fetched differently

`rubygems` and `bundler` come from a git tag rather than the gem store,
because the gem store is what zeo's own bundler fills. The bundler that runs
the install cannot come out of the thing the install produces.

`rubygems-update` could not supply them in any case: its `require_paths` is
deliberately not `lib` — that is exactly what stops installing it from
shadowing the running RubyGems — so a store copy would contribute a name and
no files.

One release ships both, and `crates/xtask/rubygems.lock` pins the tag and the
revision it resolved to. A tag that has moved is refused rather than
followed.

## Why Ruby is only for recording

`cargo xtask deps` needs `curl` and `git`. It reads the lock with `zeo-gem`,
downloads one `.gem` per row, verifies each against the lock's own
`CHECKSUMS` digest before anything reads it, and unpacks them into a
RubyGems store. No interpreter is involved, because nothing in that sequence
needs one.

It takes the **source** row every time, never a precompiled platform gem: zeo
compiles a gem's own `ext/**/*.c` and can never load a binary built against
MRI's ABI. That also makes the store platform-neutral, which is why the Linux
container reads the same `vendor/` the host filled.

Ruby is needed for exactly one thing: `cargo xtask deps --oracle`, which
`bless` runs before recording. Only RubyGems can build a gem's native half
against the running interpreter and write the serialized gemspecs
`bundler/setup` reads.

## The payload

`dist` and `stage-crate` flatten all three shipped tiers into one
`share/zeo/lib/ruby/`, so an installed zeo has a single directory. Every
path in it is `<name>-<version>` read out of the lock, and a library the
lock names whose directory is missing is refused rather than quietly left
out — a distribution built from a stale store would not be the locked set.
