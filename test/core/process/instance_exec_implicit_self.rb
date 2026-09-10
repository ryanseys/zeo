# Implicit-self direct call inside a class method:
#   def configure
#     instance_exec(10) { |n| add(n) }   # no explicit receiver
#   end
#
# This resolves to `self.instance_exec(10) { |n| add(n) }`, so the block runs
# against the receiver and `add` reaches the instance method.
#
# The shape that must NOT be treated the same way is a trampoline body, whose
# `instance_exec(args, &b)` passes a block ARGUMENT rather than a literal
# block. `instance_exec_trampoline.rb` covers that one.

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
__END__
42
done
