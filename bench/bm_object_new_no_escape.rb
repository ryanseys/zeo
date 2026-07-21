# object-new-no-escape benchmark (from yjit-bench)

class Point
  attr_reader :x, :y
  def initialize(x, y)
    @x = x
    @y = y
  end
end

def test
  a = Point.new(1, 2)
  b = Point.new(1, 2)
  if a.x == b.x && a.y == b.y
    1
  else
    0
  end
end

total = 0
i = 0
while i < 1000000
  total = total + test
  i = i + 1
end
puts total
puts "done"
