class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end
p Nums.new(1.0, 2.0, 3.0).sum(0)
p Nums.new(1.0, 2.0, 3.0).sum(0.0)
p Nums.new(1.0, 2.0, 3.0).sum
