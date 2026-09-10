# Inline-at-call-site instance_exec trampoline.
#
# Sibling of test/instance_eval_trampoline.rb. instance_exec differs
# from instance_eval by forwarding positional args from the call site
# into the block's parameters:
#
#   def configure(x, &block)
#     instance_exec(x, &block)   # x flows into block param at call site
#   end
#   b.configure(10) { |n| add(n) }   # n == 10 inside the spliced block
#
# Self is rebound to the receiver for the length of the block, so a bare call
# inside it reaches the receiver's own methods, and an arity mismatch between
# the block's parameters and the forwarded arguments raises.
#
# Section 3 below is the one worth keeping in mind: an outer local is still
# in scope inside the block, so the block reads and writes the caller's
# variables as well as the receiver's state.

# 1. Single fixed arg, statement form: forwards the call-site arg
#    into the block param, then bare method calls dispatch against
#    the receiver's class (same rebound-self mechanism as
#    instance_eval).
class Builder
  def initialize
    @sum = 0
  end

  def add(n)
    @sum = @sum + n
  end

  def total
    @sum
  end

  def configure(x, &block)
    instance_exec(x, &block)
  end
end

b = Builder.new
b.configure(10) { |n| add(n) }
b.configure(20) { |n| add(n) }
b.configure(12) { |n| add(n) }
puts b.total              #=> 42

# 2. Two fixed args, ivar read/write inside the block: each arg
#    becomes a typed C local, the block body computes against them
#    and the rebound receiver's @ivars. Validates that the
#    rebound-self retrofit composes with block-param renaming. Builder is
#    reused (already heap-allocated above) instead of a fresh
#    class -- single-instance classes with only ivar mutation in
#    the block tend to value-promote, which the TODO in
#    compile_instance_exec_inlined_stmt does not yet handle.
class Acc < Builder
  def bump_by(step, k, &block)
    instance_exec(step, k, &block)
  end
end

a1 = Acc.new
a2 = Acc.new                  # second instance keeps heap allocation
a1.bump_by(3, 4) { |s, k| add(s * k) }
a2.bump_by(7, 2) { |s, k| add(s * k) }
a1.bump_by(1, 0) { |s, k| add(s + k) }
puts a1.total                 #=> 13
puts a2.total                 #=> 14

# 3. Outer-local capture via the splice: total is in scope at the
#    call site, so the block body reads and writes it. Validates
#    that the trampoline path does NOT lift the block to a static
#    function (which would lose closure scope) -- it splices,
#    keeping outer locals reachable.
total = 0
b2 = Builder.new
b2.configure(5) { |d| add(d); total = total + d }
b2.configure(7) { |d| add(d); total = total + d }
puts b2.total             #=> 12
puts total                #=> 12

puts "done"
__END__
42
13
14
12
12
done
