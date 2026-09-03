class Other
  def unrelated = 0
end

class N
  def initialize(v)
    @v = v
  end
  attr_reader :v
  def step(a) = @v + a
end

n = N.new(10)
puts n.step(1)
puts n.v

Other.define_method(:unrelated) { 1 }
puts n.step(2)
puts n.v

N.define_method(:step) { |a| @v * a }
puts n.step(3)

N.attr_accessor :v
n.v = 99
puts n.v
puts n.step(1)

o = N.new(5)
def o.step(a) = -a
puts o.step(4)
puts n.step(4)

module Doubler
  def step(a) = 1000 + a
end
N.prepend(Doubler)
puts n.step(5)
puts N.new(1).step(6)
__END__
11
10
12
10
30
99
99
-4
396
1005
1006
