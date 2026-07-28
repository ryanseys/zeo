# bundler vendored -- the same repo as rubygems, reached through `gems.toml`'s
# `subdir` key. `bundle install` can never work in an AOT binary (activation is
# `$LOAD_PATH` mutation followed by `require`), but reading a lockfile and
# resolving requirements is pure computation, and is what most programs that
# touch bundler actually want.
require "bundler"

lock = <<~LOCK
  GEM
    remote: https://rubygems.org/
    specs:
      concurrent-ruby (1.3.4)
      rack (3.1.7)
      rack-test (2.1.0)
        rack (>= 1.3)
      rake (13.2.1)

  PLATFORMS
    arm64-darwin
    ruby

  DEPENDENCIES
    rack (~> 3.0)
    rack-test
    rake (>= 13)

  BUNDLED WITH
     2.5.9
LOCK

# --- Bundler::LockfileParser ------------------------------------------------
parsed = Bundler::LockfileParser.new(lock)
p parsed.specs.map { |s| [s.name, s.version.to_s] }
p parsed.specs.map { |s| s.dependencies.map(&:name) }
p parsed.dependencies.keys.sort
p parsed.dependencies["rack"].requirement.to_s
p parsed.dependencies["rake"].requirement.to_s
p parsed.platforms.map(&:to_s).sort
p parsed.bundler_version.to_s
p parsed.sources.map { |s| s.class.name }
p Bundler::LockfileParser.sections_in_lockfile(lock).sort

# --- Bundler::Dependency ----------------------------------------------------
d = Bundler::Dependency.new("rack", "~> 3.0")
p d.name, d.requirement.to_s, d.groups, d.type
p d.to_lock
p Bundler::Dependency.new("pry", ">= 0", "group" => :development).groups
p Bundler::Dependency.new("mri-only", ">= 0", "platforms" => [:mri]).platforms
p d.should_include?

# --- the index/spec-set side, which is plain requirement arithmetic ---------
spec = Gem::Specification.new do |s|
  s.name = "rack"
  s.version = "3.1.7"
end
p Bundler::SpecSet.new([spec]).map(&:name)
p Bundler.local_platform.is_a?(Gem::Platform)

# --- module surface ---------------------------------------------------------
p Bundler::VERSION.is_a?(String)
p Bundler::VERSION.split(".").size >= 2
p Bundler.feature_flag.class
p Bundler.rubygems.class.name
p Bundler::Settings::BOOL_KEYS.is_a?(Array)

# --- the error hierarchy is real, and carries CRuby's exit codes ------------
p Bundler::BundlerError.ancestors.include?(StandardError)
p Bundler::LockfileError.ancestors.include?(Bundler::BundlerError)
p Bundler::GemNotFound.new("nope").status_code
begin
  raise Bundler::LockfileError, "bad lockfile"
rescue Bundler::BundlerError => e
  p [e.class, e.message, e.status_code]
end
