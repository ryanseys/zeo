class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end
p Nums.new(1, 2, 3).reduce(0) { |a, x| a + x * 0.5 }
p [1, 2, 3].reduce(0) { |a, x| a + x * 0.5 }
p Nums.new(1, 2, 3).reduce(0.0) { |a, x| a + x * 0.5 }
p Nums.new(1, 2, 3).reduce(0) { |a, x| a + x }
p Nums.new(1, 2, 3).sum
p [1, 2, 3].reduce(0) { |a, x| a + x }
