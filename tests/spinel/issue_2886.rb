class Money
  attr_reader :cents
  def initialize(c) = @cents = c
  def +(o) = Money.new(@cents + o.cents)
  def to_s = "$#{@cents}"
end
xs = [Money.new(1), Money.new(2), Money.new(4)]
p xs.reduce(Money.new(0)) { |a, x| a + x }.to_s
p [1r, 2r].reduce(0r) { |a, x| a + x }
p [1, 2, 3].reduce(0) { |a, x| a + x }
p [Complex(1, 1), Complex(2, -1)].reduce(Complex(0, 0)) { |a, x| a + x }
