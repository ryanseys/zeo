# The bundled RubyGems, entered at the umbrella. This is the compile-cost
# instrument's largest rung: `cargo xtask bench --compile` times how long the
# front end takes on the whole `require "rubygems"` graph and how much it
# holds while doing it.
#
# The body below is deliberately small and pure -- version arithmetic,
# requirement matching, platform parsing, what every gemspec and lockfile is
# written against. Almost all of the cost this measures is the require graph,
# not these lines.
#
# This file used to enter at `rubygems/version` and four siblings because the
# umbrella did not compile. It does now; `tests/milestones/require_rubygems.rb`
# is the regression test, and `crates/zeo/tests/e2e/gems_vendored.rs` asserts
# separately that every class in the graph reaches codegen.
require "rubygems"

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
begin
  Gem::Version.new("not a version")
rescue ArgumentError => e
  puts e.class
end

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
p Gem::Requirement.new(">= 1.2").specific?
p Gem::Requirement.new("~> 1.2").specific?
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
p d.specific?
p Gem::Dependency.new("rails").requirement.to_s
begin
  Gem::Dependency.new("rails", "~> 7.0").merge(Gem::Dependency.new("other", ">= 0"))
rescue ArgumentError => e
  puts e.class
end

# --- Gem::Platform ----------------------------------------------------------
p Gem::Platform.new("x86_64-linux").to_s
p Gem::Platform.new("x86_64-linux").cpu
p Gem::Platform.new("x86_64-linux").os
p Gem::Platform.new("universal-darwin-19").to_s
p Gem::Platform.new("x86_64-linux") == Gem::Platform.new("x86_64-linux")
p Gem::Platform::RUBY
p Gem::Platform.new("x86_64-linux") === Gem::Platform.new("x86_64-linux")
# Reads `Gem.target_rbconfig`, which only `rubygems.rb` itself defines -- so
# this line is reachable only from the umbrella.
p Gem::Platform.local.is_a?(Gem::Platform)

# --- Gem::Specification: the class the umbrella exists to reach -------------
spec = Gem::Specification.new do |s|
  s.name = "zeo-bench"
  s.version = "0.1.0"
  s.summary = "a compile-cost input"
  s.authors = ["nobody"]
  s.files = ["lib/zeo_bench.rb"]
  s.require_paths = ["lib"]
  s.add_dependency "rake", ">= 13.0"
end
p spec.full_name, spec.file_name, spec.require_paths
p spec.dependencies.map { |d| [d.name, d.type, d.requirement.to_s] }
p spec.satisfies_requirement?(Gem::Dependency.new("zeo-bench", "~> 0.1"))
