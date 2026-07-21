class Flag ; end
class Config
  attr_reader :flags
  def initialize = @flags = []
  def add = add_to(flags)
  def add_to(collection) = collection << Flag.new
end
config = Config.new
config.add
config.add
p config.flags.size
p config.flags[0].is_a?(Flag)

# direct @ivar arg + a manual reader
class Box
  def initialize; @items = []; end
  def push_one; fill(@items); end
  def fill(coll); coll << Flag.new; end
  def items; @items; end
end
b = Box.new
b.push_one
p b.items[0].is_a?(Flag)
