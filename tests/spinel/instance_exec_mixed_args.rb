# Mixed-args instance_exec trampoline: the trampoline body forwards
# a mix of forwarded locals, the receiver's ivars, and literals --
# not just call-site params by index.
#
#   def fire(x, &b)
#     instance_exec(x, @base, 7, &b)   # local, ivar, literal
#   end
#
# Each positional arg is evaluated per-kind at the call site: the
# local picks up the call-site value, the ivar reads the rebound
# receiver's slot, and the literal is the same C value in any scope.
#
# The class keeps a mutating method so it stays heap-allocated
# (value-type receivers route through a different call path that is
# a separate change). The block runs in statement position --
# expression-position return values are a separate change.

class Mix
  def initialize
    @base = 100
    @sum = 0
  end

  def add(n)
    @sum = @sum + n
  end

  def sum
    @sum
  end

  def fire(x, &b)
    instance_exec(x, @base, 7, &b)
  end
end

m = Mix.new
m2 = Mix.new   # second instance keeps Mix heap-allocated

m.fire(3) { |a, b, c| add(a + b + c) }      # add(3 + 100 + 7 = 110)
m.fire(10) { |a, b, c| add(a * b + c) }     # add(10 * 100 + 7 = 1007)
puts m.sum                                   # 1117

puts "done"
