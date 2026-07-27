module Slap
  class Flag
    def initialize(kind)
    end

    def takes_param? = nil

    def parse_param(switch, param)
      param
    end
  end

  class Parser
    attr_reader :options
    def initialize
      @options = {}
    end

    def parse_flag(flag, switch, separator, param)
      flag.takes_param?
      options[0] = flag.parse_param(0, param)
    end
  end
end
values = [
  true,
  1,
  Slap::Parser.new.parse_flag(Slap::Flag.new(0), 0, 0, "x"),
]
raise "FAIL" unless values == [true, 1, "x"]
puts "ok"
