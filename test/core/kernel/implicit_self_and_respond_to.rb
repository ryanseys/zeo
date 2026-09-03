class Greeter
  def greet
    hello
  end

  def hello
    "hi from hello"
  end
end

puts Greeter.new.greet

class Counter
  def initialize
    @count = 0
  end

  def bump(n)
    add(n)
    @count
  end

  def add(n)
    @count = @count + n
  end
end

c = Counter.new
puts c.bump(5)
puts c.bump(2)

class Box
  def initialize(v)
    @v = v
  end

  def value
    @v
  end

  def describe
    self.value
  end

  def identity
    self
  end
end

b = Box.new(42)
puts b.describe
puts b.identity.value

class Dog
  def bark
    "woof"
  end
end

d = Dog.new
puts d.respond_to?(:bark)
puts d.respond_to?(:meow)

class Collector
  def initialize(tag)
    @tag = tag
  end

  def tag
    @tag
  end

  def each_num(a, b)
    yield a
    yield b
  end

  def run(a, b)
    each_num(a, b) { |n| puts "tag=#{self.tag}:#{n}" }
  end
end

Collector.new("x").run(1, 2)
__END__
hi from hello
5
7
42
42
true
false
tag=x:1
tag=x:2
