# `recv&.meth(*args)` / `recv&.meth(**opts)` -- safe navigation on a call the
# splat path handles. A nil receiver skips the arguments as well as the call,
# and the receiver itself is evaluated exactly once either way. This is the
# shape webrick's config dispatch uses: `@config[cb]&.call(*args)`.
def add(a, b) = a + b
def kw(a:, b: 10) = a * b

args = [1, 2]
opts = { a: 3, b: 4 }

m = method(:add)
p m&.call(*args)
p((nil)&.call(*args))

k = method(:kw)
p k&.call(**opts)
p k&.call(a: 5)

arr = [1]
p arr&.push(*args)
p arr

# the receiver is evaluated exactly once
def side(v) = (puts "recv"; v)
p side(nil)&.push(*args)
p side([9])&.push(*args)

# a block rides along
p [3, 1, 2]&.sort_by { |x| -x }
p [[1, 2]]&.map { |a, b| a + b }

# non-nil object receiver with a class type
class Bag
  def initialize = @items = []
  def add_all(*xs) = (@items.concat(xs); @items)
end
b = Bag.new
p b&.add_all(*args)
__END__
3
nil
12
50
[1, 1, 2]
[1, 1, 2]
recv
nil
recv
[9, 1, 2]
[3, 2, 1]
[3]
[1, 2]
