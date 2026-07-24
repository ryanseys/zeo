# Numbered block params across a yield-inline boundary: the callee's tap
# block and the caller's block both use _1 with different types; each must
# keep its own variable (the splice must not capture the caller's _1 with
# the callee inline's renames).
class Config
  def str(*opts, default: nil, required: false, choices: [])
    "str:#{opts.first}:#{required}"
  end
end

class Main
  attr_reader :config

  def initialize
    @config = Config.new
  end

  def label = "main-label"

  def parse(argv)
    "parsed:#{argv.length}"
  end
end

module Slap
  def self.parse(argv = [])
    Main.new.tap { yield _1.config if block_given? }.parse(argv)
  end
end

p Slap.parse { p _1.str "--name", required: true }

module Slap2
  def self.run
    Main.new.tap { yield _1.config if block_given?; p _1.label }
  end
end
Slap2.run { p _1.str("num") }
