# A `Numeric` subclass (D3) is an ordinary ivar-carrying object: it defines its
# own `<=>`/state, and `Comparable` (inherited through `Numeric`) drives
# `<`/`>`/`min` off that `<=>`.

class Money < Numeric
  def initialize(cents); @cents = cents; end
  def cents; @cents; end
  def <=>(o); cents <=> o.cents; end
end
m = Money.new(500)
n = Money.new(300)
puts m.cents
puts(m > n)
puts(m == Money.new(500))
puts m.is_a?(Numeric)
puts m.is_a?(Comparable)
puts m.class
puts [m, n].min.cents
__END__
500
true
true
true
true
Money
300
