class Config
  def str(*opts) = add_flag(Flag.new(opts))
  def prepare! = @help_flag = add_flag(Flag.new(["x"]))
  def add_flag(flag) = flag
end

class Flag
  attr_reader :switches
  def initialize(opts)
    @switches = []
    switches.concat(opts)
  end
end

flag = Config.new.str("y")
raise "FAIL" unless flag.switches == ["y"]
puts "ok"
f2 = Config.new.prepare!
p f2.switches
