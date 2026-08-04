# A `<=>` with a `return nil` guard has an sp_RbVal return, and its body's
# compare goes through the polymorphic helper as soon as the ivar holds more
# than one type -- or simply when a second class in the program also defines
# `<=>`. The helper answers an mrb_int (the sentinel for nil), which went out
# through the boxed signature unboxed and failed the C build (#3498).
class A
  include Comparable
  attr_reader :v

  def initialize(v)
    @v = v
  end

  def <=>(other)
    return nil unless other.is_a?(A)
    @v <=> other.v
  end
end

# never instantiated: defining it is enough to route A's compare through the
# polymorphic helper
class B
  attr_reader :v

  def initialize(v)
    @v = v
  end

  def <=>(other)
    return nil unless other.is_a?(B)
    @v <=> other.v
  end
end

p(A.new(1) <=> A.new(2))
p(A.new(2) <=> A.new(1))
p(A.new(1) <=> A.new(1))

# the guard's own answer
p(A.new(1) <=> 5)
p((A.new(1) <=> 5).nil?)

# Comparable's derived operators over the boxed compare
p(A.new(1) < A.new(2))
p(A.new(2) >= A.new(1))
p [A.new(3), A.new(1), A.new(2)].sort.map(&:v)
p A.new(2).between?(A.new(1), A.new(3))
p A.new(2).clamp(A.new(1), A.new(3)).v

# a mixed-type ivar takes the same route
p(A.new(1) <=> A.new(1))
p(A.new(Rational(1, 2)) <=> A.new(Rational(1, 3)))
