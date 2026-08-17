require "ostruct"
class Config
  attr_reader :flags, :lookup

  def initialize
    @flags, @lookup = [], {}
  end

  def int(*opts) = add_flag(Flag.new(:int, opts))
  def str(*opts) = add_flag(Flag.new(:str, opts))
  def flag(switch) = lookup[switch]
  def add_flag(flag)
    flags << flag
    lookup[flag.key] = flag
    flag.switches.each { lookup[_1] = flag }
  end
end

class Flag
  SWITCH_RE = /\A-(\w|-\w[\w-]*)\z/
  attr_reader :kind, :switches
  def initialize(kind, opts)
    @kind, @switches = kind, opts
  end

  def parse(switch, param)
    case kind
    when :float then Float(param)
    when :int then Integer(param, 10)
    when :str then param
    when :sym then param.to_sym
    end
  end

  # def bool? = kind == :bool
  def key = @key ||= switch.sub(/^-+/, "").tr("-", "_").to_sym
  def switch = switches.last
end

module Slap
  def self.parse(argv = ARGV)
    Main.new.tap { yield _1.config if block_given? }.parse(argv)
  end

  class Main
    attr_reader :config
    def initialize
      @config = Config.new
    end

    def parse(argv)
      options = Parser.new(config).parse(argv)
      OpenStruct.new(options).freeze
    end
  end
end

class Parser
  attr_reader :config, :options, :queue
  def initialize(config)
    @config = config
  end

  def parse(argv)
    @options, @queue = {}, argv
    operands = []
    while (item = queue.shift)
      case item
      when Flag::SWITCH_RE then parse_switch(item, Regexp.last_match)
      when "", /\A[^-]/ then operands << item
      end
    end
    options[:_args] = operands
    options
  end

  def parse_switch(item, match)
    switch = "-#{match[1]}"
    flag = config.flag(switch)
    options[flag.key] = flag.parse(switch, queue.shift)
  end
end

options = Slap.parse(["--name", "Lee", "--count", "2", "--", "file"]) do |o|
  o.str "--name"
  o.int "--count"
end

raise "FAIL" unless options[:name] == "Lee"
raise "FAIL" unless options[:count] == 2
raise "FAIL" unless options[:_args] == ["file"]
