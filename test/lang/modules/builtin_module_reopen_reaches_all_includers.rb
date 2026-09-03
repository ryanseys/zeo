# Reopening a builtin MODULE (D3): the added method reaches every includer --
# Enumerable across Array/Hash, Comparable across Integer.

module Enumerable
  def second
    first(2).last
  end
end
module Comparable
  def clamp_low(lo)
    self < lo ? lo : self
  end
end
puts [10, 20, 30].second
puts({ a: 1, b: 2 }.map { |k, v| v }.second)
puts 5.clamp_low(8)
puts 12.clamp_low(8)
__END__
20
2
8
12
