class Greeter
  def hello
    "hi"
  end
  alias hola hello
  alias :bonjour :hello
end
g = Greeter.new
puts g.hello
puts g.hola
puts g.bonjour

class Animal
  def speak
    "generic"
  end
end
class Dog < Animal
  def speak
    "woof"
  end
  alias original_speak speak
end
d = Dog.new
puts d.speak
puts d.original_speak

class Counter
  def initialize
    @count = 0
  end
  def increment
    @count += 1
  end
  alias inc increment
  def value
    @count
  end
end
c = Counter.new
c.inc
c.inc
c.increment
puts c.value

class MathUtils
  class << self
    def square(x)
      x * x
    end
    def cube(x)
      x * x * x
    end
  end
end
puts MathUtils.square(4)
puts MathUtils.cube(3)

module MyMath
  class << self
    def double(x)
      x * 2
    end
  end
end
puts MyMath.double(5)

class Registry
  class << self
    def reset
      @@total = 0
    end
    def bump
      @@total += 1
    end
    def total
      @@total
    end
  end
end
Registry.reset
Registry.bump
Registry.bump
puts Registry.total
