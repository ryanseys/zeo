# Build the probe's fixture gems into `<out>/cache`, which is the
# `vendor/cache` both sides install from.
#
# Run by the ruby oracle with the VENDORED RubyGems on `-I`, so the `.gem`
# files the two engines unpack are written by the same RubyGems that reads
# them back. Nothing here reaches the network.
#
# usage: build_gems.rb <fixture-dir> <out-dir>
require "rubygems"
require "rubygems/package"
require "fileutils"

fixtures, out = ARGV
abort "usage: build_gems.rb <fixture-dir> <out-dir>" unless fixtures && out

cache = File.join(out, "cache")
stage = File.join(out, "stage")
FileUtils.rm_rf([cache, stage])
FileUtils.mkdir_p(cache)

Dir[File.join(fixtures, "gems", "*")].sort.each do |src|
  root = File.join(stage, File.basename(src))
  FileUtils.mkdir_p(File.dirname(root))
  FileUtils.cp_r(src, root)

  # A file with a NUL in the middle and no trailing newline. `Gem::Package`
  # extracts with `IO.copy_stream(tar.io, out, entry.size)`, and a side that
  # loses the length writes this entry followed by the rest of the archive --
  # which a "the file is there" check would pass and a byte compare will not.
  File.binwrite(File.join(root, "data.bin"), "head\x00tail") if File.basename(src) == "pureleaf"

  spec = Dir.chdir(root) { Gem::Specification.load(Dir["*.gemspec"].first) }
  Dir.chdir(root) { FileUtils.mv(Gem::Package.build(spec), File.join(cache, "#{spec.full_name}.gem")) }
end
