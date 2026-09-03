# Exercises the self:Rc<Self> migration + self-capture path: the block
# is written inside `Box#run` (so `self`/`@sum` are in scope there) and
# passed to a call on an EXPLICIT other receiver (`c`, a Collector
# constructed directly as a LOCAL, not taken as a method parameter --
# method params are always statically `Poly`, a separate
# pre-existing gap; a `New`-assigned local's class IS statically known,
# so this routes around that while still exercising real self-capture.
# Implicit-self calls to a user method are ALSO a separate, unrelated,
# still-unsupported gap, avoided here the same way).

class Collector
  def each_num(a, b, c)
    yield a
    yield b
    yield c
  end
end
class Box
  def initialize
    @sum = 0
  end
  def total
    @sum
  end
  def run
    c = Collector.new
    c.each_num(1, 2, 3) { |n| @sum += n }
  end
end
b = Box.new
b.run
puts b.total
__END__
6
