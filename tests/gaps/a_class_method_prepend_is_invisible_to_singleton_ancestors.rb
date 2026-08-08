# `prepend M` inside `class << self` DISPATCHES correctly -- M's instance
# methods shadow the class's own `def self.x`, and `super` reaches the
# original. Only reflection disagrees: the module is absent from
# `singleton_class.ancestors`.
#
# It was expected to fall out of the singleton-surrogate work, and it did not.
# `class_method_prepends` is a compile-time MRO input (see analyze/mro.rs) that
# nothing carries into the runtime registry, and this body mints no surrogate
# for the registry to carry it ON -- the surrogate exists only where the body
# needs a compile-time class. Making the ancestry right means minting one for a
# mixin too, and registering the module in its ancestors.
module Loud
  def greet = "LOUD " + super
end
class Speaker
  class << self
    prepend Loud
  end
  def self.greet = "hi"
end

# Dispatch: right.
p Speaker.greet

# Reflection: wrong.
p Speaker.singleton_class.ancestors.include?(Loud)
p Speaker.singleton_class.ancestors.first
p Speaker.singleton_class.include?(Loud)
