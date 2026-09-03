class Money
  include Comparable
  attr_reader :cents

  def initialize(cents)
    @cents = cents
  end

  def <=>(other)
    cents <=> other.cents
  end

  def to_s
    "$#{cents / 100}.#{cents % 100}"
  end

  def ==(other)
    other.is_a?(Money) && cents == other.cents
  end

  def hash
    cents.hash
  end

  def eql?(other)
    other.is_a?(Money) && cents == other.cents
  end
end

a = Money.new(1250)
b = Money.new(3499)
puts a
puts "price: #{b}"
puts a < b
puts a.between?(Money.new(1000), Money.new(2000))
puts a.clamp(Money.new(2000), Money.new(5000))
puts a == Money.new(1250)
puts [a, b].min
puts [a, b].max
puts [Money.new(1), Money.new(2)] == [Money.new(1), Money.new(2)]

prices = {}
prices[Money.new(500)] = "cheap"
puts prices[Money.new(500)]
__END__
$12.50
price: $34.99
true
true
$20.0
true
$12.50
$34.99
true
cheap
