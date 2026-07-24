# A user #<=> returning a non-Integer/nil value (Array/String/...) is a
# protocol violation rejected at compile time (#2961); this guards that valid
# Comparable usage still compiles and runs.
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
