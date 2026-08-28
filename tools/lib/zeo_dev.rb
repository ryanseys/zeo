# frozen_string_literal: true

# The shared half of `tools/zeo-dev`.
#
# Everything here is stdlib only, and everything here runs under BOTH `ruby`
# from PATH and a zeo-compiled binary. A tool that only runs when zeo is
# correct cannot diagnose zeo when it is not, and `bless` is exactly what you
# reach for when something is broken.
#
# One rule keeps both engines working (this list once had three; the
# autoload and builtin-alias bugs closed and became passing goldens):
#
#   - Never gate the body on `__FILE__ == $PROGRAM_NAME`. Under a compiled
#     binary `__FILE__` is the source path and `$0` is the executable, so the
#     guard reads false and the tool does nothing.

require "zeo_dev/cli"
require "zeo_dev/exec"
require "zeo_dev/jobs"
require "zeo_dev/manifest"
require "zeo_dev/ruby"
require "zeo_dev/tsv"
require "zeo_dev/vendor"

module ZeoDev
  ROOT = File.expand_path("../..", __dir__)

  # Every command, in the order `--help` lists them.
  COMMANDS = %w[
    bless
    diff
    probe
    promote-gap
    bench
    gem
    gemtests
    linux
  ].freeze

  class Error < StandardError; end

  # The command class for `name`, loaded on demand so a tool pays only for
  # what it runs.
  def self.command(name)
    raise Error, "unknown command #{name.inspect}" unless COMMANDS.include?(name)

    require "zeo_dev/commands/#{name.tr("-", "_")}"
    Commands.const_get(name.split("-").map { |w| w[0].upcase + w[1..] }.join)
  end

  module Commands; end
end
