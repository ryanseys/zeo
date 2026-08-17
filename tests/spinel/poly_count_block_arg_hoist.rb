# `cell.neighbors.count { |n| Kind.walkable?(n.kind) }` on a poly receiver.
#
# The count arm wrote "if (sp_poly_truthy(" into the statement stream and then
# emitted the condition into that same buffer, so a rooted argument temp the
# condition had to hoist -- a declaration -- landed in the middle of the
# expression and the generated C did not parse.

module Kind
  def self.walkable?(k)
    k == 1
  end
end

class Cell
  attr_accessor :kind, :neighbors
  def initialize(k)
    @kind = k
    @neighbors = []
  end

  def link(c)
    @neighbors << c
  end
end

class Blank
  attr_accessor :kind, :neighbors
  def initialize
    @kind = nil
    @neighbors = {}
  end
end

def pick(flag, k)
  flag ? Cell.new(k) : Blank.new
end

a = pick(true, 1)
a.link(pick(true, 1))
a.link(pick(true, 0))
a.link(pick(true, 1))
p a.neighbors.count { |n| Kind.walkable?(n.kind) }
