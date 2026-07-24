# `extend` works on every receiver kind -- ordinary objects, classes, AND
# modules. Extending a class or module mixes the argument's instance methods in
# as the receiver's own singleton (class/module) methods, running with the
# receiver as `self`. This is what lets SecureRandom.extend(Random::Formatter)
# and rubygems' Gem::SecureRandom work.

# 1. Extend an ordinary object (per-object methods).
module Introspect
  def describe; "a #{self.class}"; end
end
obj = Object.new
obj.extend(Introspect)
puts obj.describe

# 2. Extend a CLASS: the module's methods become class methods.
module Counter
  def bump; @count = (@count || 0) + 1; end
end
class Tally
  extend Counter
end
puts Tally.bump
puts Tally.bump

# 3. Extend a MODULE via a runtime call (not the in-body directive).
module Sayer
  def say(word); "#{word}!"; end
end
module Announcer; end
Announcer.extend(Sayer)
puts Announcer.say("hello")
puts Announcer.respond_to?(:say)

# 4. Extend a class with a NATIVE module (Comparable) -- its methods run with
#    the class as self and drive the class's own <=>. This "redispatch to the
#    receiver" is exactly how SecureRandom.extend(Random::Formatter) reaches the
#    host's own entropy leaf.
module Bag
  def self.<=>(other); 0; end
end
Bag.extend(Comparable)
puts Bag.respond_to?(:clamp)
puts Bag.between?(Bag, Bag)

# 5. An own singleton method outranks an extended module's method of the same
#    name (CRuby's "closest singleton" ancestry rule).
module Named
  def label; "mixed-in"; end
end
module Thing
  def self.label; "own"; end
end
Thing.extend(Named)
puts Thing.label
