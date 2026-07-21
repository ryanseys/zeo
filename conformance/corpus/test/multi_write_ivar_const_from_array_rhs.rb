# `compile_multi_write`'s int_array-returning RHS branch (RHS is a
# non-literal expression that evaluates to an array, e.g.
# `.map { ... }`, a method call) only emitted assignments for
# `LocalVariableTargetNode` targets — `InstanceVariableTargetNode`
# and `ConstantTargetNode` targets were silently dropped. The C
# code compiled fine, but the ivars / constants stayed at their
# default values.
#
# Repro: `@a, @b = [10, 20].map { |x| x + 1 }` where the targets
# are ivars. Master emitted neither write, so c.a / c.b returned 0.
#
# (Constant-target coverage requires the multi-write-to-constants
# parser support, which lives in a sibling PR; the InstanceVariable
# fix here is independent and lands first.)

# (1) ivar targets, RHS is .map
class C1
  def initialize
    @a, @b = [10, 20].map { |x| x + 1 }
  end
  attr_reader :a, :b
end
c = C1.new
puts c.a              # 11
puts c.b              # 21

# (2) mixed: local + ivar from one int_array RHS.
class C2
  def initialize
    local, @i = [1, 2].map { |x| x * 10 }
    @captured_local = local
  end
  attr_reader :i, :captured_local
end
c2 = C2.new
puts c2.captured_local  # 10
puts c2.i               # 20

# (3) ivar targets where the RHS is a top-level method returning
# a fresh array. The intervening method-boundary forces the
# `sp_IntArray *` rhs path (RHS is not a literal ArrayNode).
def make_pair
  [7, 8]
end
class C3
  attr_reader :x, :y
  def initialize
    @x, @y = make_pair
  end
end
c3 = C3.new
puts c3.x   # 7
puts c3.y   # 8
