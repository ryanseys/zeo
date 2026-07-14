class Animal
  def speak
    "generic"
  end
end

class Dog < Animal
end
puts Dog.new.speak

module Greetable
  def greet
    "hi"
  end
end

class Person
  include Greetable
  def greet
    "overridden"
  end
end

class Robot
  include Greetable
end

puts Person.new.greet
puts Robot.new.greet

module Tagged
  def tag
    "base(#{super})"
  end
end

class Widget
  include Tagged
  def tag
    "widget"
  end
end
puts Widget.new.tag

module M1
  def label
    "m1"
  end
end
module M2
  def label
    "m2"
  end
end

class Stacked
  prepend M1
  prepend M2
  def label
    "own"
  end
end
puts Stacked.new.label

module D
  def who
    "D"
  end
end
module B
  include D
end
module C
  include D
end
class A
  include B
  include C
end
puts A.new.who
puts A.new.is_a?(D)

module MathHelpers
  def double(x)
    x * 2
  end
end

class Calc
  extend MathHelpers
end
puts Calc.double(21)

module Utility
  def self.triple(x)
    x * 3
  end
end
puts Utility.triple(4)

class Base
  @@count = 0
  def bump
    @@count += 1
  end
  def count
    @@count
  end
end

class Sub < Base
  def bump_twice
    @@count += 1
    @@count += 1
  end
end

b = Base.new
s = Sub.new
b.bump
s.bump_twice
puts b.count
puts s.count

class MyError < StandardError
end

class Risky
  def check(n)
    raise MyError, "bad value: #{n}" if n < 0
    n * 2
  end
end

r = Risky.new
puts r.check(5)
