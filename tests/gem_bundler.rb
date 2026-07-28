# bundler vendored -- the same upstream repo as rubygems, reached through
# `gems.toml`'s `subdir` key.
#
# What runs here is deliberately small, and the reason is worth stating: every
# other bundler entry point reaches `bundler/rubygems_ext`, which requires
# rubygems, which reaches `require "bundler"` from a method body. zeo's require
# graph is static, so that cycle flattens into one 2.7M-line program whose load
# ORDER is not CRuby's -- bundler's `rubygems_ext` runs before
# `rubygems/specification`. Until a method-body require stops being hoisted,
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
