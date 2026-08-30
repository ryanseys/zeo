# The committed bootstrap tier

Two directories, both vendored verbatim from
[rubygems/rubygems](https://github.com/rubygems/rubygems): `rubygems/` is the
repository's `lib/` tree, and `bundler/` is its `bundler/` subtree. One
release ships both, and `Gemfile.lock` states its version under the name
`rubygems-update` — the only published gem that carries the `lib/rubygems/**`
tree. `crates/zeo/tests/e2e/gems_vendored.rs` holds the two trees and that
lock entry to one version.

## Why these two are committed and nothing else is

Every other library zeo ships is resolved out of `vendor/bundle` at the
version the lock states, so its version is written in exactly one place. That
cannot work for these two: `vendor/bundle` is what `bundle install` writes,
and it is zeo's own bundler that runs the install. A machine with no ruby on
it must still reach a working `zeo bundle install`, so the pair cannot come
from the thing it produces.

`rubygems-update` could not supply them in any case. Its `require_paths` is
deliberately not `lib` — that is what stops installing it from shadowing the
running RubyGems — so a store copy would contribute a name and no files.

## The other two tiers

| Tier | Where | What |
|---|---|---|
| zeo's own | `crates/zeo-rt/ext/<name>/` | a Ruby half beside the Rust that implements it (`crates/zeo-rt/ext/UPSTREAM.md`) |
| bootstrap | here | rubygems and bundler |
| resolved | `vendor/bundle`, from `Gemfile.lock` | every other library |

`crates/zeo/src/bundled.rs` is where that list is decided, and it is the one
answer both a compile and `cargo xtask dist` read. `dist` and `stage-publish`
flatten all three into `share/zeo/lib/ruby/`, so an installed zeo has one
directory.

## Moving the version

Re-vendor both trees from the tag matching the `rubygems-update` version in
`Gemfile.lock`, keeping each directory's stub gemspec. Bumping the lock
without re-vendoring fails `the_committed_versions_agree_with_the_lock`.
