# `x op= v` on a local captured by a proc must take the same typed arm as an
# uncaptured one. The celled path used to carry its own short list of arms and
# sent every other type through the int helpers.
def acc(init, a, b)
  s = init
  f = proc { |x| s += x }
  f.call(a)
  f.call(b)
  s
end

p acc("", "a", "b")
p acc([1], [2], [3])
p acc(["a"], ["b"], ["c"])
p acc(0, 3, 4)
p acc(0.0, 1.5, 2.5)
p acc(Rational(1, 2), Rational(1, 3), Rational(1, 6))
p acc(Complex(1, 2), Complex(3, 4), Complex(5, 6))

# the shape that first surfaced it: one poly param reached by both an Array and
# a user class, so the block became a real proc and the accumulator a cell.
class Bag
  def initialize(a) = @a = a
  def each(&b) = @a.each(&b)
end

def joined(list)
  s = ""
  list.each { |x| s += x.to_s }
  s
end

p joined([1, 2])
p joined(Bag.new([3, 4]))
