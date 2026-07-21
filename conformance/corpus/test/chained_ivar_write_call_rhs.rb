# Chained `@a = @b = foo` where the rhs is a method call with a side
# effect. The naive per-slot lowering re-evaluates the rhs once per
# target, calling foo() N times instead of once. The chain handler
# must evaluate the rhs once into a temp and store from the temp into
# each ivar slot.

$count = 0

def foo
  $count = $count + 1
  $count
end

class C
  def initialize
    @a = "hello"
    @b = 42
  end

  def reset
    @a = @b = foo
  end

  attr_reader :a, :b
end

c = C.new
c.reset
puts $count
puts c.b
