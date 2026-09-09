# MILESTONE: `require "rubygems"` then `require "bundler"`, and the bundler
# monkeypatches over RubyGems that the second require installs.
#
# It holds stderr as well as stdout, which is where this one bites:
# `bundler/rubygems_ext.rb`'s `class Platform` body is a single
# `unless respond_to?(:generic)` statement, and a guard that asks the wrong
# receiver re-writes twelve constants `rubygems/platform.rb` already wrote,
# one warning each. `test/compiler/guards/a_class_body_guard_asks_about_the_class.rb`
# is the narrow test on that rule.
#
# Shapes, never versions: ruby has RubyGems loaded before the program starts.

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
__END__
"constant"
"constant"
true
true
true
true
true
"ruby"
[["minitest", "5.25.1"], ["rake", "13.2.1"]]
["ruby"]
["minitest", "rake"]
"2.5.0"
