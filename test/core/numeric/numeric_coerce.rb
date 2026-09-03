# The coerce protocol lets a user numeric type take part in arithmetic even
# when it is the RIGHT operand of a built-in number. When `5 + obj` finds that
# `obj` isn't a built-in number, it asks `obj.coerce(5)` for a `[a, b]` pair in
# the object's own domain, then evaluates `a + b`.
class Money
  attr_reader :cents
  def initialize(cents)
    @cents = cents
  end

  def coerce(other)
    # Interpret a bare number as whole dollars.
    [Money.new(other * 100), self]
  end

  def +(other)
    Money.new(@cents + other.cents)
  end

  def to_s
    format("$%.2f", @cents / 100.0)
  end
end

wallet = Money.new(250)
puts(wallet + Money.new(100))     # $3.50  -- ordinary dispatch
puts(5 + wallet)                  # $7.50  -- 5 coerces to $5.00, then + $2.50

# It works from the Float lane too, and for any operator.
class Scaled
  attr_reader :value
  def initialize(value)
    @value = value
  end
  def coerce(n)
    [Scaled.new(n), self]
  end
  def *(other)
    Scaled.new(@value * other.value)
  end
  def to_s
    "Scaled(#{@value})"
  end
end
puts(3 * Scaled.new(4))           # Scaled(12)
puts(2.0 * Scaled.new(5.0))       # Scaled(10.0)

# A non-numeric operand that can't coerce still raises the ordinary TypeError.
begin
  1 + "nope"
rescue TypeError => e
  puts e.message                  # String can't be coerced into Integer
end
__END__
$3.50
$7.50
Scaled(12)
Scaled(10.0)
String can't be coerced into Integer
