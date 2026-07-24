# Comparable operators/clamp validate the user <=> result type: a Float
# result compares by sign. A statically non-Integer/nil <=> result (String,
# Array, ...) is a protocol violation caught at compile time (#2961), not a
# runtime error, so it is not exercised here.
class FDiff
  include Comparable
  attr_reader :n
  def initialize(n); @n = n; end
  def <=>(o); (@n - o.n).to_f; end
end
p(FDiff.new(9) < FDiff.new(5))
p(FDiff.new(5).between?(FDiff.new(1), FDiff.new(9)))
p(FDiff.new(12).clamp(FDiff.new(1), FDiff.new(9)).n)
# a <=> that returns nil (incomparable) stays a runtime concern: ordering
# raises ArgumentError, == is just false.
class NilCmp
  include Comparable
  def <=>(o); nil; end
end
r = (NilCmp.new < NilCmp.new rescue $!.class); p r
p(NilCmp.new == NilCmp.new)
