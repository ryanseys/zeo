# Issue #14: classes referenced before their definition must still
# produce a working struct layout. Earlier classes that embed a
# value-type instance of a later class need the later class's struct
# to be emitted first.

class Box
  def initialize
    @ticket = Ticket.new(7)
  end
  def num; @ticket.num; end
end

class Ticket
  def initialize(num)
    @num = num
  end
  def num; @num; end
end

puts Box.new.num                # 7

# Multi-level chain: Outer -> Middle -> Leaf, all defined back-to-front.
class Outer
  def initialize
    @mid = Middle.new
    @leaf = Leaf.new(11)
  end
  def show
    puts @mid.value
    puts @leaf.value
  end
end

class Middle
  def initialize
    @leaf = Leaf.new(3)
  end
  def value; @leaf.value; end
end

class Leaf
  def initialize(v); @v = v; end
  def value; @v; end
end

Outer.new.show                  # 3 / 11

# Parent class has a value-type ivar of a class defined later: child
# inherits the (flattened) field, so the dep walk must follow the
# parent chain too.
class Holder
  def initialize
    @item = Item.new(42)
  end
  def value; @item.value; end
end

class SubHolder < Holder
end

class Item
  def initialize(v); @v = v; end
  def value; @v; end
end

puts SubHolder.new.value        # 42
