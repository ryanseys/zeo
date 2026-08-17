# `obj.attr op= v` when the writer is a hand-written `def attr=`: Ruby means a
# reader call and a writer call, and the writer's body has to run (#3809).
class Rect
  attr_reader :x, :writes
  def initialize
    @x = 0
    @writes = 0
  end

  def x=(v)
    @writes += 1
    @x = v
  end
end

r = Rect.new
r.x += 1
r.x += 2
r.x -= 1
p r.x
p r.writes

class Counted
  def initialize
    @n = 5
  end
  def n
    @n
  end
  def n=(v)
    @n = v * 10
  end
end

cc = Counted.new
cc.n += 1
p cc.n

# the attr_accessor form still works
class Plain
  attr_accessor :y
  def initialize
    @y = 3
  end
end
p2 = Plain.new
p2.y += 4
p p2.y
