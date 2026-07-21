# An &block-param method called with a literal block returns the
# value of blk.call, typed by the block body's return type rather than
# the int default. The block value is carried through sp_proc_call's
# mrb_int slot (a uniform C signature) and cast back per call site, so
# distinct literal blocks coexist without an ABI change.

def applies(&blk)
  blk.call("hi")
end

# string-returning block: the carried pointer prints as a string,
# not a raw address.
puts applies { |x| x.upcase }

# int-returning block at the SAME method: per-call-site typing, no
# signature change.
puts applies { |x| x.length }

# block body that does not depend on the param's type works for
# instance methods too (value-return typing is receiver-agnostic).
class Runner
  def run(&blk)
    blk.call("hi")
  end
end
puts Runner.new.run { "fixed" }
puts Runner.new.run { |x| "got" }

# instance &block method whose block body depends on the yielded arg's
# type: the block param is typed from blk.call's arg (string here), so
# string methods resolve.
puts Runner.new.run { |x| x.upcase }
