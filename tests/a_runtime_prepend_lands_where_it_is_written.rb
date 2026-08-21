# `K.prepend M` with an explicit receiver is an ordinary send, so the
# ancestry changes where the CALL is written. zeo recorded it as a
# compile-time ancestry edit instead, and the prepend was in place from the
# program's first line -- `K.ancestors` read ABOVE the call already showed
# the module. What the edit bought was reach: a statically dispatched `K#m`
# never consults the runtime tables the send writes, so the prepend would
# override nothing. That is what `defer_mixin_to_runtime` pays for, and the
# `K.singleton_class.prepend M` form has paid it all along.

module Loud
  def speak = "LOUD(#{super})"
end

class Dog
  def speak = "woof"
end

p Dog.ancestors.map(&:to_s)
p Dog.new.speak
Dog.prepend Loud
p Dog.ancestors.map(&:to_s)
p Dog.new.speak

# One multi-argument call keeps its arguments in SOURCE order; separate
# calls put the latest closest to self.
module A; def tag = "A[#{super}]"; end
module B; def tag = "B[#{super}]"; end

class Multi
  def tag = "Multi"
end
Multi.prepend A, B
p Multi.ancestors.map(&:to_s)
p Multi.new.tag

class Sep
  def tag = "Sep"
end
Sep.prepend A
Sep.prepend B
p Sep.ancestors.map(&:to_s)
p Sep.new.tag
