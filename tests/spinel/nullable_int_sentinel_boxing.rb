# An int slot that can hold the nil sentinel has to box as nil, not as the
# raw INTPTR_MIN, or a value that prints as nil compares unequal to nil. The
# slot reaches that state four ways: an outright nil write, a destructuring
# target the right side cannot supply, a parameter bound from such a value,
# and a proc parameter the caller did not supply.
class Box
  def initialize(v)
    @v = v
  end

  def eq(other)
    @v == other
  end
end

def takes(n)
  [n == nil, n.inspect]
end

x, y = nil
x = true || false or y = 1
p [y, y == nil, Box.new(y).eq(nil)]

a, b, *c, d, e = 1
p [a, b, c, d, e]
p [a, b, c, d, e] == [1, nil, [], nil, nil]

p takes("abc".index("z"))
p takes(0)

p proc { |m, *n, o| [m, n, o] }.call == [nil, [], nil]
p proc { |m, *n, o| [m, n, o] }.call(7) == [7, [], nil]

i = "hello".index("z")
p [i, i == nil, Box.new(i).eq(nil)]
