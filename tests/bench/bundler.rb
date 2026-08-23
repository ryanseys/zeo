# bundler vendored -- the same upstream repo as rubygems, reached through
# `gems.toml`'s `subdir` key.
#
# What runs here is deliberately small, and the reason is worth stating: every
# other bundler entry point reaches `bundler/rubygems_ext`, which requires
# rubygems. The method-body require hoist that used to flatten that cycle into
# one wrongly-ordered program is FIXED; what blocks the umbrella now is eager
# `Module#autoload` (`tests/gaps/autoload_is_lazy.rb`). Until an autoload runs
# at the constant's first read, `Bundler::LockfileParser` and friends are
# covered by the compile-and-register assertions in
# `crates/zeo/tests/e2e/gems_vendored.rs` rather than here.
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
