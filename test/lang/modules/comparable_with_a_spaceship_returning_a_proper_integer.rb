# A Money class whose <=> answers an Integer gets the whole Comparable surface.
class Money
  include Comparable
  attr_reader :cents
  def initialize(c); @cents = c; end
  def <=>(o); cents <=> o.cents; end
end
p(Money.new(150) < Money.new(299))
p(Money.new(300) > Money.new(299))
p([Money.new(3), Money.new(1), Money.new(2)].sort.map(&:cents))
p(Money.new(5).between?(Money.new(1), Money.new(9)))
p(Money.new(150) == Money.new(150))
__END__
true
true
[1, 2, 3]
true
true
