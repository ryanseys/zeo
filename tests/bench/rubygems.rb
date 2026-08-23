# rubygems vendored (`gems.toml`: rubygems/rubygems). Version arithmetic,
# requirement matching and platform parsing are what every gemspec and lockfile
# is written against, and they are pure computation.
#
# Entered at those files rather than at `rubygems`, for a reason worth stating
# rather than hiding. The method-body require hoist that used to block the
# umbrella is FIXED, and `require "rubygems"` now compiles and runs. What
# blocks it now is `Module#autoload`, which zeo runs at the DECLARATION where
# ruby runs it at the constant's first read: `rubygems.rb`'s
# `autoload :RequestSet` pulls in `request_set/gem_dependency_api.rb` before
# `rubygems/platform` is required, and it dies on `uninitialized constant
# Gem::Platform`. See `tests/gaps/autoload_is_lazy.rb`, which carries the
# diagnosis -- the fix is not a hook at the constant read.
#
# `Gem::Platform.local` is left out for the same reason as the umbrella file:
# it reads `Gem.target_rbconfig`, which `rubygems.rb` defines.
#
# Naming the pieces is a no-op under ruby, which has rubygems loaded already,
# so both sides see the same world. That the FULL `require "rubygems"` graph
# still reaches codegen with every class intact is asserted separately, without
# `rustc`, in `crates/zeo/tests/e2e/gems_vendored.rs`.
require "rubygems/deprecate"
require "rubygems/version"
require "rubygems/requirement"
require "rubygems/dependency"
require "rubygems/platform"

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
