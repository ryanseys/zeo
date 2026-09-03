# Found while testing `class << self`: `current_class` is
# `None` inside a class method's own `Ctx` (no concrete `self` receiver
# exists there), so a no-receiver call to a SIBLING class method
# (`def self.a; b; end` calling `def self.b`) always panicked --
# the compiler's implicit-self call lowering only ever consulted
# `current_class`. Fixed by also checking `defining_class` against
# `Compiler::class_method_in_chain`, dispatching as a direct
# associated-function call, same as `ClassName.foo(...)`.

class Widget
  def self.create
    helper
  end
  def self.helper
    "made"
  end
end
puts Widget.create

class MathUtils
  class << self
    def sum_of_squares(a, b)
      square(a) + square(b)
    end
    def square(x)
      x * x
    end
  end
end
puts MathUtils.sum_of_squares(3, 4)
__END__
made
25
