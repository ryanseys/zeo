# MILESTONE (pending): `require "rubygems"` then `require "bundler"`.
#
# The PROGRAM already works -- every line below answers what ruby answers.
# What keeps this pending is stderr: zeo emits 24 warning lines ruby does not,
# `already initialized constant Gem::Platform::X` for twelve constants.
#
# The cause is a load-order fact, not a `class << self` one.
# `bundler/rubygems_ext.rb:56` guards a block with
# `unless respond_to?(:generic)`; `rubygems/platform.rb:296-300` defines
# `def generic` inside `class << self`. The guard reads false DURING the
# require and true afterwards, so bundler re-assigns twelve constants
# platform.rb had already set. (Probed: `class A; class << self; def g; end;
# end; end; class A; p respond_to?(:g); end` is `true` under both engines, so
# the singleton row itself is fine.)
#
# A golden compares stdout AND stderr, which is what makes this XFAIL rather
# than a pass with a note.
#
# Shapes, never versions -- see `tests/milestones.rs`.

require "rubygems"
require "bundler"

p defined?(Gem)
p defined?(Bundler)
p Bundler::VERSION.is_a?(String)
p Bundler.respond_to?(:setup)
p Bundler.gem_version.is_a?(Gem::Version)

# Bundler's monkeypatches over RubyGems landed.
p Gem::Specification.new.respond_to?(:source)
p Gem::Platform.respond_to?(:generic)
p Gem::Platform.generic(Gem::Platform::RUBY).to_s

# The lockfile reader -- pure text, and what every `bundle` command starts at.
lock = <<~LOCK
  GEM
    remote: https://rubygems.org/
    specs:
      rake (13.2.1)
      minitest (5.25.1)

  PLATFORMS
    ruby

  DEPENDENCIES
    minitest (~> 5.0)
    rake

  BUNDLED WITH
     2.5.0
LOCK
parsed = Bundler::LockfileParser.new(lock)
p parsed.specs.map { |s| [s.name, s.version.to_s] }.sort
p parsed.platforms.map(&:to_s)
p parsed.dependencies.keys.sort
p parsed.bundler_version.to_s
