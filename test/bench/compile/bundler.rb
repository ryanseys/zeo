# The bundled Bundler, entered at the umbrella. The top rung of the
# compile-cost ladder: `require "bundler"` pulls rubygems in as well, so this
# is the largest require graph zeo compiles on demand.
#
# This file used to enter at `bundler/version` because the umbrella did not
# compile. It does now; `tests/milestones/require_bundler.rb` and
# `rubygems_then_bundler.rb` are the regression tests.
require "bundler"

p Bundler::VERSION.is_a?(String)
p Bundler::VERSION.split(".").size >= 2
p Bundler::VERSION.frozen?
p Bundler.gem_version.is_a?(Gem::Version)
p Bundler.gem_version.to_s == Bundler::VERSION
p Bundler.gem_version > Gem::Version.new("1.0")
p Bundler.gem_version.segments.first.is_a?(Integer)
p Bundler.gem_version.release.to_s == Bundler::VERSION

# `LockfileParser` reads a lockfile's CONTENTS standalone -- no Gemfile, no
# network, no bundle context. It is what makes a lockfile a machine-readable
# format rather than bundler's private state.
lock = <<~LOCK
  GEM
    remote: https://rubygems.org/
    specs:
      rake (13.2.1)
      rspec (3.13.0)
        rspec-core (~> 3.13.0)
      rspec-core (3.13.0)

  PLATFORMS
    arm64-darwin
    x86_64-linux

  DEPENDENCIES
    rake
    rspec (~> 3.13)

  BUNDLED WITH
     4.0.18
LOCK
parsed = Bundler::LockfileParser.new(lock)
p parsed.specs.map { |s| [s.name, s.version.to_s] }.sort
p parsed.dependencies.keys.sort
p parsed.platforms.map(&:to_s).sort
p parsed.bundler_version.to_s
