# Two `Numeric` defaults exist and answer correctly, but the machinery behind
# them is narrower than CRuby's.
#
# 1. `singleton_method_added` is defined and raises the right TypeError when
#    called, but nothing calls it: zeo has no `singleton_method_added` hook, so
#    `def n.foo` on a Numeric succeeds where ruby refuses. Fix shape: emit the
#    hook call from codegen's singleton-def path (and from `define_singleton_method`),
#    the way `method_added` would also need.
#
# 2. `Numeric#i` builds `Complex(0, self)`, and zeo's Complex holds only its own
#    numeric lanes (`complex::is_component`), so a user subclass gets a
#    TypeError where ruby builds `(0+Deg(7)*i)`. Fix shape belongs with Complex,
#    not here: every `cpx_*` routine assumes native parts.

class Deg < Numeric
  def initialize(v) = @v = v
  def to_f = @v.to_f
  def inspect = "Deg"
end

n = Deg.new(7)

def n.frob = :frobbed
puts "singleton defined: #{n.frob.inspect}"

begin
  p n.i
rescue TypeError => e
  puts "i: #{e.message}"
end
