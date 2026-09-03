# The enumerable drivers accumulate into state the block can reach again: a
# block that calls the SAME method on the SAME receiver runs a second driver
# while the first is mid-element. Pinned because the accumulator moved from a
# shared cell to one the driver owns, and a reentrant call must still get an
# accumulator of its own.
a = [1, 2, 3]
double = proc { |x| x * 2 }
nested = proc { |x| a.map(&double).sum + x }

p a.map(&nested)
p a.sum { |x| a.count { |y| y > x } }
p a.count { |x| a.map(&double).include?(x * 2) }
p a.map { |x| a.select { |y| y <= x }.length }

# Three levels deep, and the receiver is mutated by an inner level.
b = [1, 2]
p b.map { |x| b.map { |y| b.count { |z| z >= y } * x }.sum }

# Break out of the OUTER driver from inside a nested one.
p(a.map { |x| x == 2 ? (break :stopped) : x })
p(a.count { |x| x == 3 ? (break :halted) : true })
p(a.sum { |x| x })

# A non-Array Enumerable takes the sending path, not the own path.
class Bag
  include Enumerable
  def initialize(*v) = @v = v
  def each(&b) = @v.each(&b)
end
bag = Bag.new(4, 5, 6)
p bag.map { |x| bag.count { |y| y > x } }
p bag.sum { |x| x * 2 }
p bag.count { |x| x.odd? }
__END__
[13, 14, 15]
3
3
[1, 2, 3]
[3, 6]
:stopped
:halted
6
[2, 1, 0]
30
1
