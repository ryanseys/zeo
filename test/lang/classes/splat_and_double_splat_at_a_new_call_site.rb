# A `*args` positional splat and a `**h` double-splat at `.new`
# dispatch through the runtime constructor rather than being rejected.

class Point
  def initialize(x, y); @x = x; @y = y; end
  def to_s; "(#{@x}, #{@y})"; end
end
Pair = Data.define(:a, :b)
args = [1, 2]
h = { a: 3, b: 4 }
puts Point.new(*args)
p Pair.new(**h)
p Pair.new(**{ a: 5, b: 6 })
__END__
(1, 2)
#<data Pair a=3, b=4>
#<data Pair a=5, b=6>
