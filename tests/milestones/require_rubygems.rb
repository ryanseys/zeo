# MILESTONE: `require "rubygems"` works, and the RubyGems API is usable.
#
# The umbrella require, not an entry at a sub-file. `tests/bench/rubygems.rb`
# used to enter at `rubygems/version` and friends precisely because this did
# not work; this file is what retires that workaround.
#
# Shapes, never versions. Ruby has RubyGems loaded before the program starts,
# so its `require` answers false where zeo's answers true, and zeo's bundled
# copy is ahead of the reference ruby's. Both sides agree on what the API
# does; printing either of those two records a difference that means nothing.

require "rubygems"

p defined?(Gem)
p Gem::VERSION.is_a?(String)
p Gem::VERSION.split(".").size >= 3

# The five classes every gemspec and lockfile is written against.
p Gem::Specification.is_a?(Class)
p Gem::Requirement.default.to_s
p Gem::Version.new("1.2.3").to_s
p Gem::Version.new("1.2.3") > Gem::Version.new("1.2.2")
p Gem::Version.correct?("2.0.0.beta1")
p Gem::Dependency.new("rake", ">= 13").name
p Gem::Platform::RUBY

# Requirement matching -- what resolution is built on.
req = Gem::Requirement.new("~> 2.1")
p req.satisfied_by?(Gem::Version.new("2.1.4"))
p req.satisfied_by?(Gem::Version.new("2.2.0"))
p req.satisfied_by?(Gem::Version.new("3.0.0"))
p Gem::Requirement.new(">= 1.0", "< 2.0").to_s

# Version arithmetic and sorting.
p Gem::Version.new("1.4.9").bump.to_s
p %w[1.10.0 1.9.3 2.0.0.pre 1.9.10].map { |v| Gem::Version.new(v) }.sort.map(&:to_s)
p Gem::Version.new("2.0.0.pre").prerelease?

# A specification built the way a gemspec builds one.
spec = Gem::Specification.new do |s|
  s.name = "zeo-milestone"
  s.version = "0.1.0"
  s.summary = "a milestone"
  s.authors = ["nobody"]
  s.files = ["lib/zeo_milestone.rb"]
  s.require_paths = ["lib"]
  s.add_dependency "rake", ">= 13.0"
  s.add_development_dependency "minitest", "~> 5.0"
end
p spec.name
p spec.version.to_s
p spec.full_name
p spec.file_name
p spec.require_paths
p spec.dependencies.map { |d| [d.name, d.type, d.requirement.to_s] }
p spec.runtime_dependencies.map(&:name)

# Platform parsing -- how a lockfile's platform column is read.
p Gem::Platform.new("arm64-darwin").to_a
p Gem::Platform.new("x86_64-linux").to_a
p Gem::Platform.new("ruby").to_s
