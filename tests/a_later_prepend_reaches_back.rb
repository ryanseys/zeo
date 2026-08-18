module Loud
  def speak = "!" + super + "!"
end

module Also
  def speak = "[" + super + "]"
end

class Animal
  def speak = "raw"
end

class Dog < Animal
  prepend Loud

  def speak = "woof(" + super + ")"
end

p Dog.new.speak
p Dog.ancestors.take(4).map(&:to_s)

class Dog
  prepend Also
end

p Dog.new.speak
p Dog.ancestors.take(5).map(&:to_s)
