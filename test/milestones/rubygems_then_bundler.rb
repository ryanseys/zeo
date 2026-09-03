# MILESTONE: `require "rubygems"` then `require "bundler"`, and the bundler
# monkeypatches over RubyGems that the second require installs.
#
# The stdout matched long before the stderr did. What kept this XFAIL was 24
# `already initialized constant Gem::Platform::X` warnings ruby does not
# print: `bundler/rubygems_ext.rb:54`'s `class Platform` body is a single
# `unless respond_to?(:generic)` statement, and codegen used to lift a
# sole-statement class-body guard out into the enclosing scope. Asked of `Gem`
# the probe answered false, so twelve constants `rubygems/platform.rb` had
# already written were written again. See
# `tests/a_class_body_guard_asks_about_the_class.rb`.
#
# A golden compares stdout AND stderr, which is what kept this honest.
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
