# dup/clone of a user object must produce a SEPARATE instance (not alias the
# original), and must invoke a user-defined initialize_copy hook so deep-copy
# semantics work. Previously dup/clone fell through to an identity shortcut and
# returned the receiver itself, silently aliasing mutable ivars.

# No hook: shallow copy still yields independent objects for scalar ivars.
class Counter
  attr_accessor :n
  def initialize(n); @n = n; end
end
a = Counter.new(5)
b = a.dup
b.n = 99
p [a.n, b.n]              # [5, 99]

# initialize_copy hook: deep-copy a mutable ivar so mutation does not leak back.
class Box
  attr_accessor :items
  def initialize(items); @items = items; end
  def initialize_copy(orig); @items = orig.items.dup; end
end
src = Box.new([1])
cp = src.dup
cp.items << 2
p [src.items, cp.items]   # [[1], [1, 2]]

# clone goes through the same hook.
cl = src.clone
cl.items << 9
p [src.items, cl.items]   # [[1], [1, 9]]

# Multiple ivars are all copied.
class Point
  attr_accessor :x, :y
  def initialize(x, y); @x = x; @y = y; end
end
p1 = Point.new(1, 2)
p2 = p1.dup
p2.x = 10
p [p1.x, p1.y, p2.x, p2.y]  # [1, 2, 10, 2]

# Hook defined on a parent, dup of a subclass instance.
class Base
  attr_accessor :v
  def initialize(v); @v = v; end
  def initialize_copy(o); @v = o.v.dup; end
end
class Child < Base; end
ch = Child.new([7])
ch2 = ch.dup
ch2.v << 8
p [ch.v, ch2.v]          # [[7], [7, 8]]

# Hook on a parent, with BOTH the parent and the subclass dup'd. Dup'ing both
# unifies initialize_copy's param to the parent class, so the subclass dup must
# still reach the hook (the original is cast to the param's class).
b0 = Base.new([1])
b1 = b0.dup
b1.v << 2
c0 = Child.new([3])
c1 = c0.dup
c1.v << 4
p [b0.v, b1.v, c0.v, c1.v]  # [[1], [1, 2], [3], [3, 4]]
