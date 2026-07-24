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
# Same AOT compromise as instance_eval: detected at compile time by
# shape match (def m(p1..pN, &b); instance_exec(p1..pN, &b); end),
# inlined at the call site with self rebound to the receiver and block
# params bound to fresh suffixed C locals carrying the call-site arg
# values. A non-trampoline shape falls through to ordinary dispatch; an
# arity mismatch between block params and forwarded args is a hard error.
#
# Limitations exercised by the asymmetry note in section A below:
#   - Direct `recv.instance_exec(args) { ... }` (outside a trampoline
#     method) lifts to a static function and cannot capture outer
#     locals (a follow-up adds pointer capture on the direct path).
#   - Trampoline body must forward params 1:1, no literals / ivars /
#     splat (relaxed in follow-ups).
#   - Value-typed receivers crash on the pointer cast in the splice
#     today; multi-instance classes (which Spinel keeps heap-allocated)
#     are the workaround until the TODO in
#     compile_instance_exec_inlined_stmt is fixed.

# A. Asymmetry note
#
# Outer-local capture works on THIS path (trampoline) because the
# block body is spliced at the call site, so outer locals are in
# scope. The direct form `obj.instance_exec(args) { ... }` does NOT
# capture outer locals -- its lifted static function cannot see the
# caller's locals. Use the trampoline form when you need capture; a
# follow-up closes this gap on the direct path.

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
