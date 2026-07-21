# Direct-path instance_exec: `recv.instance_exec(args) { |params| body }`
# called without a wrapping trampoline method. The analyze pass lifts
# the block to a static function `sp_iexec_<N>(self, args...)` and
# rewrites the call site to dispatch through it.
#
# Sibling of test/instance_exec_trampoline.rb (codegen-time inlining
# of the def-m-instance_exec trampoline pattern). The direct form is
# what most CRuby code writes: `obj.instance_exec(5) { |n| ... }` at
# the call site, no wrapper method involved.
#
# Boundaries:
#   - Strict arity: call-site args count == block params count.
#   - No outer-local capture (the lifted function cannot see the
#     caller's locals). Convert to the trampoline form for that.
#   - Receiver must be a statically-resolvable obj_<C> -- a typed
#     local, ivar, method param, or constant constructor.
#   - return / break / next / yield / block_given? inside the block
#     are rejected (silently for now; a follow-up upgrades to a hard error).
#   - Value-typed receivers are unsupported (TODO carried over from
#     compile_instance_eval_inlined_stmt's same issue). Multi-instance
#     classes keep the receiver heap-allocated.

# 1. Bare direct call with one arg. The block body adds the call-site
#    arg to the receiver's @sum via the typed method `add`. Bare
#    method calls inside the block resolve against the receiver's
#    class (same dispatch path the trampoline tests exercise).
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
end

b = Builder.new
b.instance_exec(10) { |n| add(n) }
b.instance_exec(20) { |n| add(n) }
b.instance_exec(12) { |n| add(n) }
puts b.total                  #=> 42

# 2. Two args at the call site forward into two block params. Block
#    body computes against them, mutates the receiver's ivar via the
#    rebound-self path, and returns nothing (the baseline locks the
#    return type to void).
class Acc < Builder
end

a1 = Acc.new
a2 = Acc.new
a1.instance_exec(3, 4) { |s, k| add(s * k) }
a2.instance_exec(7, 2) { |s, k| add(s * k) }
a1.instance_exec(1, 0) { |s, k| add(s + k) }
puts a1.total                 #=> 13
puts a2.total                 #=> 14

# 3. Zero-arg form (degenerate, equivalent to instance_eval): the
#    detector accepts arity-0, the lifted function takes only the
#    receiver. Tests that the baseline doesn't conflate arity-0
#    with the instance_eval lift (they're independent registries
#    even when behavior overlaps).
b2 = Builder.new
b2.instance_exec { add(100) }
b2.instance_exec { add(50) }
puts b2.total                 #=> 150

puts "done"
