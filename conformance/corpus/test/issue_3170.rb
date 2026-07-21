class Money
  include Comparable
  attr_reader :cents
  def initialize(c) = @cents = c
  def <=>(o) = cents <=> o.cents
end
arr = [Money.new(50), Money.new(150), Money.new(100)]
p arr.select { |m| m.between?(Money.new(60), Money.new(120)) }.map(&:cents)
p arr.map { |m| m.clamp(Money.new(80), Money.new(120)).cents }
# not-between
p Money.new(200).between?(Money.new(1), Money.new(10))
