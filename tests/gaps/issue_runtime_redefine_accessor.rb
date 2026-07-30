# Redefining an already-compiled method AT RUNTIME, under a name no static
# analysis can read, does not take effect: neither `send(:define_method, name)`
# with a computed name, nor a per-object singleton, displaces the compiled
# entry -- the old body keeps answering on both the static and the `send` path.
#
# Not specific to accessors, and not caused by devirtualizing them (identical
# output before `zeo_tramp!` grew its `rd`/`wr` heads); an `attr_accessor` is
# just the shortest way to show it. The cause is the whole-process
# `OVERLAY_LIVE` latch and the frozen dispatch registry it gates -- a
# runtime-defined method lands in the overlay, but a class whose flat map was
# already built never consults it. Fixed by narrowing that latch.
class Node
  attr_accessor :value

  def initialize(v)
    @value = v
  end
end

n = Node.new(7)
p n.value

# A COMPUTED name, so the literal-name `define_method` lowering (which zeo
# compiles as an ordinary `def`) cannot see it.
name = [:va, :lue].join.to_sym
Node.send(:define_method, name) { "dyn-#{@value}" }
p [n.value, n.send(:value), Node.new(1).value]

m = Node.new(5)
m.define_singleton_method(:value) { "sing" }
p [m.value, m.send(:value), n.value]

class Other
  attr_reader :z

  def initialize
    @z = 1
  end
end

o = Other.new
def o.z
  "obj-singleton"
end
p [o.z, Other.new.z]
