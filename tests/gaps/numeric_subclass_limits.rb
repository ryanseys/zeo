# `Numeric#i` builds `Complex(0, self)`, and zeo's Complex holds only its own
# numeric lanes (`complex::is_component`), so a user subclass gets a TypeError
# where ruby builds `(0+Deg(7)*i)`. The fix shape belongs with Complex, not
# here: every `cpx_*` routine assumes native parts.
#
# This file used to record a second, unrelated limit -- that nothing called
# `Numeric#singleton_method_added`, so `def n.foo` on a number succeeded where
# ruby refuses. The definition hooks now fire; see
# `tests/definition_hooks_at_runtime.rb`.

class Deg < Numeric
  def initialize(v) = @v = v
  def to_f = @v.to_f
  def inspect = "Deg"
end

n = Deg.new(7)

begin
  p n.i
rescue TypeError => e
  puts "i: #{e.message}"
end
