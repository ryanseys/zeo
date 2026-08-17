# A poly receiver's operator with a BUILTIN operand is served by the runtime
# operator, so it must not bind a same-named user method's parameter: doing so
# pinned the bundled Set#superset?'s `other` to Time and made set.rb itself
# uncompilable.
require "set"

# `t` is a second ahead so the ordering is decided by arithmetic, not by
# whether the clock ticked between two Time.now calls (it did not on macOS).
vals = [Time.now, 1]
t = Time.now + 1
p (vals[0] >= t).class
p (vals[0] > t).class
p (vals[0] <= t).class

mixed = [[1, 2], 3]
p (mixed[0] == [1, 2])

# the Set surface still works, and its own methods still type
a = Set.new([1, 2])
b = Set.new([1, 2, 3])
p a.subset?(b)
p b.superset?(a)
p (b >= a)
p (a >= b)

# a user class with the same operator name still binds from a real call
class Ver
  attr_reader :n
  def initialize(n)
    @n = n
  end

  def >=(other)
    n >= other.n
  end
end

p (Ver.new(3) >= Ver.new(2))
