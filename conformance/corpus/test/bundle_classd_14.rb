# Bundled tests:
#   - no_attr_write_shortcut_multi
#   - obj_array

# === no_attr_write_shortcut_multi ===
# `obj.x = v` must dispatch to `def x=(v)` when x= is not a registered
# attr_writer. A multi-statement body never matched the auto-attr_writer
# pattern (which requires a single `InstanceVariableWriteNode` body), so
# the fix at the call site is sufficient on its own to make this case
# work — no auto-classification change required.

class T_no_attr_write_shortcut_multi_C
  attr_accessor :real

  def initialize
    @real = 0
    @logged = ""
  end

  # Multi-statement writer. Side-effects on a *different* ivar so we can
  # tell whether the def actually ran.
  def logged=(v)
    @logged = "set:" + v
    @real = v.length
  end

  def get_logged
    @logged
  end
end

c = T_no_attr_write_shortcut_multi_C.new

# attr_accessor path: still short-circuits to field write.
c.real = 7
puts c.real           # 7

# Multi-statement def x=: must dispatch (not bypass).
c.logged = "hello"
puts c.get_logged     # set:hello
puts c.real           # 5  (overwritten by the side effect)

puts "done"

# === obj_array ===
class T_obj_array_Point
  attr_accessor :x
  attr_accessor :y
  def initialize(x, y)
    @tag = "p"
    @x = x
    @y = y
  end
  def to_s
    x.to_s + "," + y.to_s
  end
end

points = [T_obj_array_Point.new(1, 2), T_obj_array_Point.new(3, 4), T_obj_array_Point.new(5, 6)]
points.each { |p|
  puts p.to_s
}
puts points.length

# push to obj array
more = []
more.push(T_obj_array_Point.new(10, 20))
more.push(T_obj_array_Point.new(30, 40))
more.each { |p|
  puts p.x + p.y
}

