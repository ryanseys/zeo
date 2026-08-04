# The coerce route emits two calls that have no node of their own: the
# operator, and #coerce itself. Nothing bound coerce's parameter, so it stayed
# unknown until a late backstop -- too late to widen the factory it feeds, and
# `Q.scalar(other)` took an sp_Q* that read a boxed Integer as a pointer
# (#3497). Where an explicit `m.coerce(4)` did bind it, the two disagreed and
# the build failed (#3499). Both shapes are here, each in its own class so the
# other's call sites cannot mask it.
class Fac
  attr_reader :v

  def initialize(v)
    @v = v
  end

  def self.scalar(v)
    new(v)
  end

  def coerce(other)
    [Fac.scalar(other), self]
  end

  def +(other)
    o = other.is_a?(Fac) ? other : Fac.scalar(other)
    Fac.new(@v + o.v)
  end

  def to_s
    "F(" + @v.to_s + ")"
  end
end

# coerce-only: no direct call site to bind anything
puts (10 + Fac.new(Rational(3, 2))).to_s
puts (2 + Fac.new(5)).to_s
puts (1.5 + Fac.new(2.5)).to_s

class Exp
  attr_reader :v

  def initialize(v)
    @v = v
  end

  def coerce(other)
    [Exp.new(other), self]
  end

  def *(other)
    o = other.is_a?(Exp) ? other : Exp.new(other)
    Exp.new(@v * o.v)
  end

  def to_s
    "E(" + @v.to_s + ")"
  end
end

# an explicit coerce call alongside an operator-routed one
m = Exp.new(3)
p m.coerce(4).map(&:to_s)
puts (2 * m).to_s
puts (m * 2).to_s
