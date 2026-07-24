require "ostruct"
class Flag
  attr_reader :required
  alias_method :required?, :required

  def initialize(required: false) = @required = required
  def key = :name
end

class Config
  attr_reader :flags

  def initialize = @flags = []
  def str(required: false) = flags << Flag.new(required:)
end

class Parser
  attr_reader :config

  def initialize(config) = @config = config

  def parse
    config.flags.each { |flag| raise "FAIL" if flag.required? && !{name: true}[flag.key] }
    {name: "Lee"}
  end
end

class Main
  attr_reader :config

  def initialize = @config = Config.new
  def parse = OpenStruct.new(Parser.new(config).parse)
end

main = Main.new
main.config.str(required: true)
main.parse
puts "OK"
