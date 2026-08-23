# bundler vendored -- the same upstream repo as rubygems, reached through
# `upstream.rb`'s `subdir` key.
#
# What runs here is deliberately small, and the reason is worth stating: every
# other bundler entry point reaches `bundler/rubygems_ext`, which requires
# rubygems. The method-body require hoist and eager `Module#autoload` that
# used to block that path are both FIXED (`tests/autoload_is_lazy.rb`); a
# splice-ORDER bug in rubygems.rb still blocks the umbrella, and
# `tests/bench/rubygems.rb`'s header carries the diagnosis. Until it lands,
# `Bundler::LockfileParser` and friends are covered by the compile-and-register
# assertions in `crates/zeo/tests/e2e/gems_vendored.rs` rather than here.
require "rubygems/version" # `Bundler.gem_version` answers a `Gem::Version`
require "bundler/version"

p Bundler::VERSION.is_a?(String)
p Bundler::VERSION.split(".").size >= 2
p Bundler::VERSION.frozen?
p Bundler.gem_version.is_a?(Gem::Version)
p Bundler.gem_version.to_s == Bundler::VERSION
p Bundler.gem_version > Gem::Version.new("1.0")
p Bundler.gem_version.segments.first.is_a?(Integer)
p Bundler.gem_version.release.to_s == Bundler::VERSION
