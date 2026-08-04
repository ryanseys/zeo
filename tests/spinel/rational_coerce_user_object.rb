# `Rational(3, 2) * money`, where money defines #coerce, refused to compile at
# all -- an "unsupported arithmetic" abort that took the whole program down
# even when the expression sat behind a rescue. The coerce protocol was wired
# for an Integer or Float on the left only, so Rational and Bignum fell off
# the end of it (#3489).
class Money
  attr_reader :cents

  def initialize(cents)
    @cents = cents
  end

  def coerce(other)
    [Money.new(other), self]
  end

  def *(other)
    other = Money.new(other) unless other.is_a?(Money)
    Money.new(@cents * other.cents)
  end

  def +(other)
    other = Money.new(other) unless other.is_a?(Money)
    Money.new(@cents + other.cents)
  end

  def to_s
    "M(#{@cents})"
  end
end

m = Money.new(6)
puts(m * 3)
puts(3 * m)
puts(1.5 * m)
puts(Rational(3, 2) * m)
puts(Rational(1, 2) + m)
puts(m + 4)
puts(4 + m)
