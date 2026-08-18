# An `include`/`prepend` written in a REOPEN is a runtime event with a
# position: code between the two bodies must not see the edit. zeo recorded it
# as a compile-time ancestry fact, so it applied from program start.
module Extra
  def tag = :from_module
end

class Thing
end

p Thing.new.respond_to?(:tag)
p Thing.instance_methods.include?(:tag)
p Thing.ancestors.map(&:to_s)

class Thing
  include Extra
end

p Thing.new.respond_to?(:tag)
p Thing.new.tag
p Thing.ancestors.map(&:to_s)

# A prepend layers most-recent-first, ahead of everything already prepended.
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

# ...and a SUBCLASS minted before the reopen sees it too.
class Puppy < Dog; end
p Puppy.new.speak
p Puppy.ancestors.take(5).map(&:to_s)

# An include in the FIRST body stays a compile-time fact.
module Early
  def e = :early
end
class WithEarly
  include Early
end
p WithEarly.new.e
p WithEarly.ancestors.map(&:to_s).first(2)
