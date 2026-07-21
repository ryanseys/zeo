# User-defined instance_exec / instance_eval methods shadow the
# compile-time intrinsic. Ordinary method dispatch resolves to the
# user's method instead of the lift.
#
# Before the override check landed in iexec_rewrite_call /
# ieval_rewrite_call, Spinel silently bypassed user overrides by
# rewriting the call to __sp_iexec_<N> at the analyze level. CRuby
# would have dispatched to the user's method; Spinel did not.
#
# A fuller approach (registering BasicObject and routing every
# intrinsic through method-resolution from the start) would
# rearchitect the class table. The lighter version here -- a
# cls_find_method check that bails out of the rewrite when the
# user has their own method -- gets the correctness win without
# the rearchitecture risk.

class Wrap
  def initialize
    @marker = 0
  end

  # User-defined instance_exec on Wrap. Spinel should NOT lift the
  # block at the call site below; it should dispatch to this method
  # instead, matching CRuby.
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
# Block body is a no-op -- the user method ignores it. We use a noop
# rather than `@tag = "intrinsic-ran"` because the latter would force
# Spinel to compile the block as a proc literal even though it's
# never invoked, and proc compilation of a toplevel-ivar string
# assignment trips an unrelated proc-return-type issue.
w3.instance_eval { 1 }   # block ignored by user method
puts w3.tag           # user-instance-eval-ran

puts "done"
