# Parameter type inference must pick a class whose surface contains
# ALL methods called on the param, regardless of class definition
# order. Issue #35: previously the inferer locked in the first class
# matching ANY single reader, ignoring later accesses, so swapping
# Block and Cell changed the outcome.

class Block
  attr_reader :x

  def initialize(x)
    @x = x
  end
end

class Cell
  attr_reader :x, :block

  def initialize
    @x = 0
  end

  def block=(block)
    @block = block
  end
end

def move_to(other)
  other.block = Block.new(other.x)
  other
end

c = Cell.new
move_to(c)
puts c.x         # 0
puts c.block.x   # 0
