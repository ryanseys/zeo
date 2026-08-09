# A subclass of a builtin whose CONSTRUCTOR takes the receiver class --
# `ObjectSpace::WeakMap`, `WeakRef` -- is registered with that constructor
# directly, so `.new` reaches the native builder and the subclass's own
# `initialize` never runs. Ruby runs it, and `super()` reaches the builtin.
#
# The answer is silently WRONG rather than a raise: `@label` is simply never
# written, so the reader answers nil. A subclass that also changes the ARITY
# raises `ArgumentError` from the native constructor instead, which at least
# says something.
class LabelledWeakMap < ObjectSpace::WeakMap
  def initialize(label)
    super()
    @label = label
  end

  attr_reader :label
end

m = LabelledWeakMap.new("cache")
p m.class
p m.label
key = "k"
m[key] = "v"
p m[key]
