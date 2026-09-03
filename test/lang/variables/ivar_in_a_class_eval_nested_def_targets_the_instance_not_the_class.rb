# Ivar codegen resolves a RUNTIME `self` ahead of the LEXICAL class context
# it inherited. A `def` nested in a `class_eval` block sits inside
# `Gadget`'s body (so `class_self` is set) but runs under a dynamic self, and
# its `@v` must reach the INSTANCE, not the class's own ivar table. Read and
# write must agree on this: when only the read was fixed, `@v += 1` read the
# instance and wrote the class table, so the increment vanished.
#
# The `Counter`/`Mixed` halves pin the other direction -- a real class-method
# `self` still reaches the class's own table, and a class-level `@cls` stays
# distinct from an instance's same-named ivar.

class Gadget
  def initialize(v); @v = v; end
  class_eval do
    def doubled; @v * 2; end
    def bump!; @v += 1; self; end
  end
end
g = Gadget.new(10)
p g.doubled
p g.bump!.doubled

class Counter
  def self.tick; @n = (@n || 0) + 1; end
  def self.n; @n; end
end
Counter.tick; Counter.tick
p Counter.n

class Mixed
  @cls = "class-level"
  def self.cls; @cls; end
  def initialize; @cls = "instance-level"; end
  def inst; @cls; end
end
p Mixed.cls
p Mixed.new.inst
p Mixed.cls
__END__
20
22
2
"class-level"
"instance-level"
"class-level"
