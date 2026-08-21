# A `<=>` whose operand parameter settled on a boxed BUILTIN -- a Rational
# operand types it that way -- had no arm in the dispatch table at all, so the
# table came out empty and every comparison through Comparable reported the
# pair as incomparable, even though the class compares them perfectly well
# (#4038):
#
#   comparison of Fixed with Rational failed (ArgumentError)
#
# The user's own `<=>` also has to come before the Rational arm of the runtime
# comparison, exactly as its own &/|/^ come before the integer coercion: that
# arm answers for a numeric receiver, and a user object is not one.
class Fixed
  include Comparable
  attr_reader :units
  def initialize(units) = @units = units

  def <=>(other)
    return nil unless other.is_a?(Rational)

    @units <=> (other * 100).round
  end
end

p(Fixed.new(1999) < Rational(40, 2))
p(Fixed.new(1999) > Rational(40, 2))
p(Fixed.new(1999) <=> Rational(40, 2))
p(Fixed.new(2000) == Rational(20, 1))
p(Fixed.new(1999).between?(Rational(1, 1), Rational(100, 1)))
p [Fixed.new(300), Fixed.new(100)].map(&:units)

# an operand the guard rejects is CRuby's ArgumentError, as before
begin
  p(Fixed.new(1) < 5)
rescue ArgumentError => e
  p e.class
end

# a Time-typed operand takes an arm the same way
class Stamp
  include Comparable
  def initialize(t) = @t = t
  def <=>(other)
    return nil unless other.is_a?(Time)

    @t <=> other.to_i
  end
end
p(Stamp.new(100) < Time.at(200))
p(Stamp.new(300) <=> Time.at(200))
