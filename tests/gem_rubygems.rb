# rubygems vendored (`gems.toml`: rubygems/rubygems). `Gem.activate` mutates
# `$LOAD_PATH` and then `require`s, which is inert in an AOT binary -- but
# `Gem::Version`/`Requirement`/`Dependency`/`Specification`/`Platform` are pure
# computation, and are what every gemspec and lockfile is written against.
#
# Entered at the files it uses rather than at `rubygems`: the umbrella file
# reaches `require "bundler"` from a method body, and zeo's require graph is
# static, so the whole of bundler (and its vendored thor/net-http/uri) would
# ride along -- a 2.7M-line program for a test about version arithmetic. Ruby
# loads rubygems at startup, so naming the pieces explicitly is a no-op there
# and the two sides still see the same world. That the FULL `require "rubygems"`
# graph reaches codegen is asserted separately, without `rustc`, in
# `crates/zeo/tests/e2e/gems_vendored.rs`.
require "rubygems/defaults"
require "rubygems/specification"

# --- Gem::Version: the ordering every gemspec and lockfile depends on -------
v = Gem::Version.new("1.2.3")
p v.to_s, v.version, v.segments, v.release.to_s, v.prerelease?
p Gem::Version.new("1.0.0") == Gem::Version.new("1.0")
p Gem::Version.new("1.0.0").eql?(Gem::Version.new("1.0"))
p Gem::Version.new("2.0.0.beta1").prerelease?
p Gem::Version.new("2.0.0.beta1").release.to_s
p Gem::Version.new("1.2.3").bump.to_s
p Gem::Version.new("1.2").bump.to_s
p Gem::Version.new("1.9") <=> Gem::Version.new("1.10")
p ["1.10", "1.9", "1.9.a", "1.0", "2.0.0.rc1", "2.0.0"]
  .map { |s| Gem::Version.new(s) }.sort.map(&:to_s)
p Gem::Version.correct?("1.2.3"), Gem::Version.correct?("nope")
p Gem::Version.create("3.1").to_s
p Gem::Version.new("1.2.3").hash == Gem::Version.new("1.2.3").hash
p Gem::Version.new("1.2.3").inspect

# --- Gem::Requirement: every operator ---------------------------------------
[">= 1.2", "> 1.2", "<= 1.2", "< 1.2", "= 1.2", "!= 1.2", "~> 1.2"].each do |spec|
  r = Gem::Requirement.new(spec)
  p [spec, r.to_s, r.satisfied_by?(Gem::Version.new("1.2")),
     r.satisfied_by?(Gem::Version.new("1.5")),
     r.satisfied_by?(Gem::Version.new("2.0"))]
end
p Gem::Requirement.new(">= 1.2", "< 2.0").to_s
p Gem::Requirement.new(">= 1.2", "< 2.0").satisfied_by?(Gem::Version.new("1.5"))
p Gem::Requirement.default.to_s, Gem::Requirement.default.none?
p Gem::Requirement.create("~> 2.1").to_s
p Gem::Requirement.new("~> 1.4").prerelease?
p Gem::Requirement.new("~> 1.4.a").prerelease?
p Gem::Requirement.parse(">= 1.2")
begin
  Gem::Requirement.new("garbage")
rescue Gem::Requirement::BadRequirementError => e
  puts e.class
end

# --- Gem::Dependency --------------------------------------------------------
d = Gem::Dependency.new("rails", "~> 7.0")
p d.name, d.requirement.to_s, d.type, d.runtime?, d.prerelease?
p d.match?("rails", "7.1.0"), d.match?("rails", "6.0.0"), d.match?("sinatra", "7.1.0")
p d.to_s
p Gem::Dependency.new("rspec", ">= 0", :development).type
p d == Gem::Dependency.new("rails", "~> 7.0")
p d.merge(Gem::Dependency.new("rails", ">= 7.0.1")).requirement.to_s

# --- Gem::Specification -----------------------------------------------------
s = Gem::Specification.new do |spec|
  spec.name = "demo"
  spec.version = "0.1.0"
  spec.summary = "a demo"
  spec.description = "a longer demo"
  spec.authors = ["zeo"]
  spec.email = "zeo@example.com"
  spec.homepage = "https://example.com"
  spec.licenses = ["MIT"]
  spec.files = ["lib/demo.rb"]
  spec.require_paths = ["lib"]
  spec.required_ruby_version = ">= 3.1"
  spec.add_dependency "rails", ">= 7"
  spec.add_development_dependency "rspec", "~> 3.0"
end
p s.name, s.version.to_s, s.summary, s.description, s.authors, s.licenses
p s.full_name, s.file_name
p s.dependencies.map { |dep| [dep.name, dep.type, dep.requirement.to_s] }
p s.runtime_dependencies.map(&:name)
p s.development_dependencies.map(&:name)
p s.required_ruby_version.to_s
p s.platform
p s.to_yaml.is_a?(String)
p s.to_ruby.include?("demo")

# --- Gem::Platform ----------------------------------------------------------
p Gem::Platform.new("x86_64-linux").to_s
p Gem::Platform.new("x86_64-linux").cpu
p Gem::Platform.new("x86_64-linux").os
p Gem::Platform.new("universal-darwin-19").to_s
p Gem::Platform.local.is_a?(Gem::Platform)
p Gem::Platform::RUBY
p Gem::Platform.match_spec?(s)

# --- module-level surface ---------------------------------------------------
p Gem::VERSION.is_a?(String)
p Gem.ruby_version.is_a?(Gem::Version)
p Gem.rubygems_version.is_a?(Gem::Version)
p [true, false].include?(Gem.win_platform?)

# --- the error hierarchy is real, and rescuable -----------------------------
p Gem::LoadError.ancestors.include?(LoadError)
p Gem::MissingSpecError.ancestors.include?(Gem::LoadError)
begin
  raise Gem::MissingSpecError.new("nope", Gem::Requirement.new(">= 0"))
rescue Gem::LoadError => e
  p [e.class, e.name]
end
