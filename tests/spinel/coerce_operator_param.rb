# `3 + q` reaches the user operator through #coerce: Ruby asks q.coerce(3),
# which answers [Q(3), q], and calls `+` on the first with the SECOND as its
# argument -- the user object itself. Only the direct call site has a call
# node, so the parameter settled on Integer and the coerce path read a Q
# pointer out of the pair as a raw integer: a pointer-sized number that
# changed between runs (#3491).
class Q
  attr_reader :value, :units

  def initialize(value, units = {})
    @value = value
    @units = units
  end

  def self.scalar(v)
    new(v, {})
  end

  def coerce(other)
    [Q.scalar(other), self]
  end

  def +(other)
    o = other.is_a?(Q) ? other : Q.scalar(other)
    Q.new(@value + o.value, @units)
  end

  def *(other)
    o = other.is_a?(Q) ? other : Q.scalar(other)
    Q.new(@value * o.value, @units)
  end

  def to_s
    @value.to_s
  end
end

def q(value, units = {})
  Q.new(value, units)
end

# the direct form and the coerce form, both present
puts(q(4) + 3)
puts(3 + q(4))
puts(q(4) * 3)
puts(3 * q(4))

# an instance on both sides still works
puts(q(4) + q(5))

# a Float on the left takes the same route
puts(1.5 + q(4))
