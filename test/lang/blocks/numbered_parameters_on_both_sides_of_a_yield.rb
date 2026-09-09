# The callee's tap block and the caller's block each use _1 for a different
# object, and neither captures the other's.
# (spinel issue #3281)
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
__END__
"str:--name:true"
"parsed:0"
"str:num:false"
"main-label"
