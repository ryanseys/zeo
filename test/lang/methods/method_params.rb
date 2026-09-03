class Greeter
  def greet(name, greeting = "Hello")
    "#{greeting}, #{name}!"
  end
end

g = Greeter.new
puts g.greet("Ada")
puts g.greet("Ada", "Hi")

class Collector
  def count(*nums)
    nums.length
  end

  def between(a, *mid, z)
    "#{a}-#{mid.length}-#{z}"
  end
end

c = Collector.new
puts c.count(1, 2, 3)
puts c.count
puts c.between(1, 2, 3, 9)

class Kw
  def greet(x:, y: 10)
    x + y
  end

  def opts(**rest)
    rest.length
  end
end

k = Kw.new
puts k.greet(x: 1)
puts k.greet(x: 1, y: 2)
puts k.greet(y: 3, x: 4)
puts k.opts(a: 1, b: 2, c: 3)

class Box
  attr_accessor :value
  attr_reader :ro

  def initialize
    @value = 1
    @ro = 7
  end
end

b = Box.new
puts b.value
b.value = 99
puts b.value
puts b.ro

class Calc
  def double(x) = x * 2
end
puts Calc.new.double(21)

class Adder
  def add(a, b)
    a + b
  end
end
puts Adder.new.add(3, 4)
__END__
Hello, Ada!
Hi, Ada!
3
0
1-2-9
11
3
7
3
1
99
7
42
7
