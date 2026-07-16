# `clone`/`dup` run the object's `initialize_copy` hook after the shallow
# copy, so a class can un-share a copied member. A bare `super` in that hook
# reaches Object's default (no-op) copy hook instead of raising.
class Board
  def initialize
    @rows = [[1], [2]]
  end
  def initialize_copy(orig)
    super
    @rows = @rows.clone   # a fresh outer array for the copy
  end
  def add_row
    @rows.push([9])
  end
  def size
    @rows.length
  end
end
b = Board.new
c = b.clone
c.add_row
puts c.size               # 3
puts b.size               # 2  -- the original is independent

# Reopening `Object` adds methods to EVERY receiver -- built-in and user alike
# -- dispatched with the real receiver as `self`.
class Object
  def type_name
    "a " + self.class.name
  end
end
puts "hi".type_name       # a String
puts 42.type_name         # a Integer
puts [1, 2].type_name     # a Array
puts Board.new.type_name  # a Board
