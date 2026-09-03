class Collector
  def each_num(a, b, c)
    yield a
    yield b
    yield c
  end
end

class Greeter
  def maybe_greet
    if block_given?
      yield "Ada"
    else
      "no block"
    end
  end
end

g = Greeter.new
puts g.maybe_greet { |name| "Hello, #{name}!" }
puts g.maybe_greet

total = 0
Collector.new.each_num(1, 2, 3) { |n| total += n }
puts total

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

result = Collector.new.each_num(1, 2, 3) { |n| next if n == 2; break "stopped" if n == 3; puts n }
puts result

class Finder
  def find_even
    c = Collector.new
    c.each_num(1, 3, 4) { |n| return n if n % 2 == 0 }
    -1
  end
end
puts Finder.new.find_even

total2 = 0
Collector.new.send(:each_num, 1, 2, 3) { |n| total2 += n }
puts total2

3.times { |i| puts i * 10 }
Collector.new.each_num(5, 6, 7) { puts _1 * 2 }
Collector.new.each_num(8, 9, 10) { puts it + 1 }

class Box2
  def call_with(&blk)
    blk.call(5)
  end
end
puts Box2.new.call_with { |x| x * 2 }
__END__
Hello, Ada!
no block
6
6
1
stopped
4
6
0
10
20
10
12
14
9
10
11
10
