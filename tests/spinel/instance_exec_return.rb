# Direct instance_exec in expression position: the call's value is the
# block's last expression (CRuby semantics), not the receiver.
#
# Heap receivers are inlined at the call site -- the block body runs
# with self rebound to the receiver, so @ivar reads and bare method
# calls resolve against it, and the block's final value flows out as
# the call's value. The prior void-lift baseline wrapped the call in a
# comma-expression that yielded the receiver instead.

class Box
  def initialize(v)
    @v = v
  end

  def doubled
    @v * 2
  end

  def greet
    "hi"
  end
end

# A subclass keeps Box heap-allocated (multi-instance classes are not
# SRA-promoted to value types), so the receiver is inlined at the call
# site. Value-typed receivers are handled separately.
class BoxPlus < Box
end

b = Box.new(10)

# Expression position with an arg: value flows from the block.
n = b.instance_exec(5) { |x| @v + x }
puts n                                   #=> 15

# Zero-arg block, bare method call against the rebound receiver.
m = b.instance_exec { doubled }
puts m                                   #=> 20

# String-valued block.
g = b.instance_exec { greet }
puts g                                   #=> hi

# Nested inside a larger expression.
puts b.instance_exec(3) { |x| @v + x } + 100   #=> 113

# Body-local variable inside the block.
p = b.instance_exec(4) { |x| t = @v * x; t + @v }
puts p                                   #=> 50

# Statement position still inlines (value discarded).
b.instance_exec(7) { |x| puts @v + x }   #=> 17

puts "done"
