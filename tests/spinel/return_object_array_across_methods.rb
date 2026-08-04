# A method that accumulates objects into a local and returns it: the local and
# the method's value name one storage location, so the caller's slot has to
# follow the callee's return type. Also covers the shapes the pass must leave
# alone -- a returned array read back through a runtime protocol (`to_a`), and
# one whose element classes conflict.
class P
  attr_reader :x
  def initialize(x)
    @x = x
  end
end

class Q
  attr_reader :y
  def initialize(y)
    @y = y
  end
end

def build(n)
  out = []
  i = 0
  while i < n
    out.push(P.new(i * 2))
    i += 1
  end
  out
end

def total(ps)
  s = 0
  i = 0
  while i < ps.length
    s += ps[i].x
    i += 1
  end
  s
end

def mixed(n)
  n > 0 ? [P.new(1), Q.new(2)] : []
end

class Pair
  def initialize(a, b)
    @a = a
    @b = b
  end

  def to_a
    [@a, @b]
  end
end

ps = build(5)
p total(ps)
p build(3).length
p mixed(1).length
p Pair.new(7, "x").to_a.length
p Pair.new(7, "x").to_a[0]
