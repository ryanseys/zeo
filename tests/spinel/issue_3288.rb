require "ostruct"

class Config
  attr_reader :flags
  def int(*opts, default: nil) = (@flags ||= []) << Flag.new(opts, default:)
end

class Flag
  attr_reader :default, :switches
  def initialize(switches, default: nil)
    @default, @switches = default, switches
  end

  def key = switches.last.sub(/^-+/, "").tr("-", "_").to_sym
end

class Main
  attr_reader :config
  def initialize = @config = Config.new
  def parse(argv) = OpenStruct.new(Parser.new.parse(config, argv)).freeze
end

class Parser
  def parse(config, argv)
    res = {}
    config.flags.each { res[_1.key] = _1.default }
    res
  end
end

def parse(argv)
  Main.new.tap { yield _1.config if block_given? }.parse(argv)
end

options = parse(["-i", "xx"]) do |o|
  o.int "-m", "--max-time"
  o.int "--retry", default: 3
end

raise "FAIL" if !options.max_time.nil?

class F2
  attr_reader :d
  def initialize(d = nil) = @d = d
end
r2 = {}
r2[:a] = F2.new.d
r2[:b] = F2.new(3).d
p r2[:a].nil?
p r2
