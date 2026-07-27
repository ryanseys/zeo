module Slap
  def self.parse(argv = ARGV) = Main.new.parse(argv)

  class Main
    def parse(argv) = Parser.new.parse(argv)
  end

  class Parser
    attr_reader :queue

    def parse(argv)
      @queue = argv
      parse_argv
    end

    def parse_argv
      (item = queue.shift)
    end
  end
end

argv = []
Slap.parse(argv)
puts "ok"
