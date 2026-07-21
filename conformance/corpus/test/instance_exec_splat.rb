# Splat-trampoline instance_exec:
#
#   def m(*args, &b)
#     instance_exec(*args, &b)
#   end
#   recv.m(a, b, c) { |x, y, z| ... }   # x=a, y=b, z=c
#
# The detector accepts a SplatNode in the trampoline body when its
# expression is a LocalVariableRead matching the method's rest
# param name. At inlining time, each call-site positional arg
# fans out into a block param (per-call-site specialisation:
# different call sites can pass different numbers of args).
#
# Distinguished from fixed-arity trampolines by the SplatNode in
# the trampoline body's args list; the splat detector requires
# the splat is the sole positional arg (mixing splat with other
# positional shapes isn't supported).

class Builder
  def initialize
    @s = 0
  end
  def add(n)
    @s = @s + n
  end
  def total
    @s
  end
  def with_all(*args, &b)
    instance_exec(*args, &b)
  end
end

b = Builder.new
b2 = Builder.new   # second instance keeps heap allocation

# Three args at one call site, two args at another -- the splat
# trampoline fans out each call's args to match its block params.
b.with_all(1, 2, 3) { |a, b, c| add(a + b + c) }      # +6
b.with_all(10, 20)  { |x, y|    add(x + y) }          # +30
b.with_all(100)     { |n|       add(n) }              # +100
puts b.total                                           # 136

# Zero-arg splat edge case: no positional args, zero-param block. The splat
# fans out to zero block params (effective arity 0); the body still splices.
b2.with_all { add(7) }                                        # +7
# Four args on the same trampoline, second instance -- per-call-site fan-out.
b2.with_all(5, 5, 5, 5) { |a, b, c, d| add(a + b + c + d) }   # +20
puts b2.total                                                  # 27

puts "done"
