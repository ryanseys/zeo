# Nested instance_exec: the rewrite descends into a lifted block body
# so an inner instance_exec call lifts too, rebinding self per level.
# The inner receiver is an outer-scope local captured into the block;
# codegen splices the inner inline within the outer inline.

class Outer
  def initialize
    @label = 10
  end

  def val
    @label
  end
end

class Inner
  def initialize
    @label = 20
  end

  def val
    @label
  end
end

o = Outer.new
i = Inner.new

# Inner self rebinds to `i`; its `val` reads Inner's ivar.
puts(o.instance_exec { i.instance_exec { val } })   #=> 20

# Outer self rebinds to `o`; the cross-level capture of `i` and the
# rebound-self call compose: 10 + 20.
puts(o.instance_exec { val + i.instance_exec { val } })   #=> 30
puts "done"
