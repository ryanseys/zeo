class Node
  attr_accessor :right

  def initialize
    @right = self
  end
end

class Column < Node
  def peek
    n = right
    p n.equal?(self)
  end

  def relink
    right.right = self
  end
end

col = Column.new
col.peek
col.relink
puts "ok"

class Node2
  attr_accessor :nxt
  def initialize; @nxt = self; end
  def peek; t = nxt; p t.equal?(self); end
end
class Column2 < Node2
  def relink; t = nxt; t.nxt = self; end
end
c2 = Column2.new
c2.peek
c2.relink

class Node3
  attr_accessor :left, :right
  def initialize; @left = self; @right = self; end
  def peek; a = left; b = right; p [a.equal?(self), b.equal?(self)]; end
end
class Column3 < Node3
  def relink; right.right = self; left.left = self; end
end
class Cell3 < Column3
  def hop; n = right; n.left = self; end
end
x = Cell3.new
x.peek
x.relink
x.hop
puts "ok"
