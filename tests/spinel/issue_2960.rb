class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end

n = Nums.new(1, 2, 3)
p n.all?(1..3)
p n.any?(2..2)
p n.none?(2..2)
p n.one?(2..2)
p Nums.new("a", "b").all?(/a|b/)
p Nums.new("a", "b").any?(/a/)
