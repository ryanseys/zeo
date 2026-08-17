# `obj == 2**64` must dispatch the object's own `==`: the bigint comparison arm
# fired whenever EITHER side was a bignum, so the receiver pointer was coerced
# through sp_bigint_new_int and the address was compared instead (#3975 sweep).
class Exp
  def initialize(v) = @v = v
  def ==(other)
    @v == other ? "eq" : "ne"
  end
  def <(other) = "lt-called"
end

def wrap(v) = Exp.new(v)
p(wrap(2**64) == 18446744073709551616)
p(wrap(1) == 18446744073709551616)
p(wrap(1) < 18446744073709551616)

# the numeric comparisons themselves are unchanged
p(2**64 == 18446744073709551616)
p(2**64 == 5)
p(5 == 2**64)
p((2**64) > 5)
p(5 < 2**64)
p((2**64) <=> 5)
