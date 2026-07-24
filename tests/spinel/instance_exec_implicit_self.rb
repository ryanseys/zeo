# Implicit-self direct call inside a class method:
#   def configure
#     instance_exec(10) { |n| add(n) }   # no explicit receiver
#   end
#
# CRuby resolves this to `self.instance_exec(10) { |n| add(n) }`.
# Spinel's analyze pass picks up @current_class_idx for the
# receiver type when both:
#   - The call has no explicit receiver
#   - The block is a literal `{ ... }` (not `&proc_var`)
#
# Distinguishing from the trampoline-body case is critical: a
# trampoline body's `instance_exec(args, &b)` has the &b arg
# stored in @nd_block as a BlockArgumentNode (not a literal
# BlockNode), so the analyze rewrite skips it and leaves the
# call site for codegen's trampoline-pattern detector.

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

  # Implicit-self direct call -- no explicit receiver. Resolves
  # to self.instance_exec(...) at analyze time.
  def configure
    instance_exec(10) { |n| add(n) }
    instance_exec(20) { |n| add(n) }
    instance_eval { add(12) }   # sibling: instance_eval implicit-self too
  end
end

b = Builder.new
b2 = Builder.new
b.configure
puts b.total   # 42

puts "done"
