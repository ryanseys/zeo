$counter = 0
puts $counter
puts $never_set
$counter += 1
puts $counter

MAX = 100
puts MAX

class Base
  GREETING = "hi"
end
class Sub < Base
  def greet
    GREETING
  end
end
puts Sub.new.greet
puts Base::GREETING

begin
  puts NOT_DEFINED
rescue NameError => e
  puts "caught: #{e.message}"
end

x = nil
x ||= 5
puts x
y = 10
y &&= 20
puts y

class Box
  attr_accessor :n
  def initialize
    @n = 0
  end
end
b = Box.new
b.n += 3
puts b.n
b.n ||= 999
puts b.n

arr = [1, 2, 3]
arr[0] += 10
puts arr[0]
arr[1] ||= 42
puts arr[1]

(a, b2), c = [[1, 2], 3]
puts a
puts b2
puts c

class Thing
  attr_reader :x
  def set_both(g)
    @x, $g2 = 10, g
  end
end
t = Thing.new
t.set_both(99)
puts t.x
puts $g2

for p1, p2 in [[1, 2], [3, 4]]
  puts p1 + p2
end

class Adder
  def add3(p, q, r)
    p + q + r
  end
end
adder = Adder.new
nums = [1, 2, 3]
puts adder.add3(*nums)
puts adder.add3(1, *[2, 3])
__END__
0

1
100
hi
hi
caught: uninitialized constant NOT_DEFINED
5
20
3
3
11
2
1
2
3
10
99
3
7
6
6
