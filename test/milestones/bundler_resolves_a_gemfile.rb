# MILESTONE: Bundler reads a real Gemfile, builds a Definition, and RESOLVES
# it -- the rungs above `LockfileParser`, which is pure text.
#
# `Bundler.definition` is where the Gemfile DSL, the source objects and the
# resolver all have to be present at once, so it is the first rung that fails
# if any one of them is missing. The `Bundler::Source::Git` NameError that
# blocked it for a long time is what this pins closed.
#
# It writes its own Gemfile and lock into a temp directory and resolves
# against the LOCK alone, so it touches no network and reads nothing from the
# machine's own gem store -- see this directory's README.
#
# Shapes, never versions.

# The locked rake, so the program resolves against what the oracle store
# holds rather than a version that goes stale with the next bump.
LOCK = File.read(File.expand_path("../../Gemfile.lock", __dir__))
RAKE = LOCK[/^    rake \(([^)]+)\)/, 1]
MINITEST = LOCK[/^    minitest \(([^)]+)\)/, 1]
require "tmpdir"
require "rubygems"
require "bundler"

Dir.mktmpdir do |dir|
  File.write(File.join(dir, "Gemfile"), <<~GEMFILE)
    source "https://rubygems.org"
    gem "rake", "#{RAKE}"
    gem "minitest", "#{MINITEST}"
  GEMFILE

  File.write(File.join(dir, "Gemfile.lock"), <<~LOCK)
    GEM
      remote: https://rubygems.org/
      specs:
        minitest (#{MINITEST})
        rake (#{RAKE})

    PLATFORMS
      ruby

    DEPENDENCIES
      minitest (= #{MINITEST})
      rake (= #{RAKE})

    BUNDLED WITH
       2.5.0
  LOCK

  ENV["BUNDLE_GEMFILE"] = File.join(dir, "Gemfile")
  Bundler.reset!

  # The source classes every Definition reaches for.
  p Bundler::Source::Git.is_a?(Class)
  p Bundler::Source::Rubygems.is_a?(Class)
  p Bundler::Source::Path.is_a?(Class)

  definition = Bundler.definition
  p definition.is_a?(Bundler::Definition)
  p definition.dependencies.map(&:name).sort
  p definition.locked_gems.is_a?(Bundler::LockfileParser)

  # The resolve itself, off the lock rather than the network.
  resolved = definition.resolve
  p resolved.map(&:name).sort
  p resolved.map { |s| s.version.is_a?(Gem::Version) }.uniq

  # The Gemfile DSL round-trips the requirement it was given.
  rake = definition.dependencies.find { |d| d.name == "rake" }
  p rake.requirement.to_s == "= #{RAKE}"
  p rake.groups
end
__END__
true
true
true
true
["minitest", "rake"]
true
["drb", "minitest", "prism", "rake"]
[true]
true
[:default]
