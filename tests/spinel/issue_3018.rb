class Point
  def initialize(x); @x = x; end
end
a = Point.new(1)
b = Point.new(1)
p(a === b)
p(a === a)
class Eq
  def initialize(v); @v = v; end
  def ==(o); o.is_a?(Eq) && o.val == @v; end
  def val; @v; end
end
p(Eq.new(3) === Eq.new(3))
class Own
  def ===(o); "custom"; end
end
p(Own.new === 1)
