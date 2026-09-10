# User-defined instance_exec / instance_eval methods shadow the
# compile-time intrinsic. Ordinary method dispatch resolves to the
# user's method instead of the lift.
#
# The hazard the program guards is a compiler that rewrites the call site
# before it looks for a user method: the override is then silently bypassed
# and the block runs against the receiver anyway.

class Wrap
  def initialize
    @marker = 0
  end

  # User-defined instance_exec on Wrap. The call site below must reach THIS
  # method, not the compile-time lift.
  def instance_exec(x, &b)
    @marker = x + 100
    @marker
  end

  def marker
    @marker
  end
end

# Two instances so Wrap stays heap-allocated (value-type promotion
# would otherwise complicate the test independent of the override
# question).
w1 = Wrap.new
w2 = Wrap.new
ret = w1.instance_exec(5) { |x| 9999 }  # block ignored -- user method runs
puts ret              # 105
puts w1.marker        # 105

# Sibling override of instance_eval. Same expected dispatch.
class Wrap2
  def initialize
    @tag = ""
  end

  def instance_eval(&b)
    @tag = "user-instance-eval-ran"
  end

  def tag
    @tag
  end
end

w3 = Wrap2.new
w4 = Wrap2.new
# The block body is a no-op, since the user method ignores it. Writing an
# ivar here instead would compile the block as a proc that never runs, which
# is a different thing to test.
w3.instance_eval { 1 }   # block ignored by user method
puts w3.tag           # user-instance-eval-ran

puts "done"
__END__
105
105
user-instance-eval-ran
done
