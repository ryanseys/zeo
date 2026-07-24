# Bundled tests:
#   - super_arg_cross_class_cast
#   - super_forwarding

# === super_arg_cross_class_cast ===
# Two subclasses each call `super` from `initialize` forwarding a
# differently-typed arg. The parent's param-type inference picks one
# concrete C type, so the other subclass's super-forwarded arg ends
# up with a mismatching C type at the call site without an explicit
# cast.

class T_super_arg_cross_class_cast_Box
  def initialize(v)
    @v = v
  end
end

class T_super_arg_cross_class_cast_StrBox < T_super_arg_cross_class_cast_Box
  def initialize(s)
    super
    @len = s.length
  end

  def len
    @len
  end
end

class T_super_arg_cross_class_cast_IntBox < T_super_arg_cross_class_cast_Box
  def initialize(n)
    super
    @doubled = n * 2
  end

  def doubled
    @doubled
  end
end

s = T_super_arg_cross_class_cast_StrBox.new("hello")
puts s.len

n = T_super_arg_cross_class_cast_IntBox.new(7)
puts n.doubled

# === super_forwarding ===
# Bare `super` (no args, no parens) inside a constructor parses as
# `ForwardingSuperNode`, not `SuperNode`. Without the
# ForwardingSuperNode case, the call to the parent's `initialize`
# was silently dropped — the parent's ivar setup never ran and the
# child object was left in a half-initialized state.

class T_super_forwarding_Base
  def initialize(x)
    @x = x
    @y = x * 10
  end
end

class T_super_forwarding_Child < T_super_forwarding_Base
  attr_reader :x, :y, :z
  def initialize(x)
    super              # bare — forwards x to T_super_forwarding_Base#initialize
    @z = x + 1
  end
end

c = T_super_forwarding_Child.new(3)
puts c.x   # 3
puts c.y   # 30
puts c.z   # 4

