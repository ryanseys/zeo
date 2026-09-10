# each_slice, each_cons and the rest, over the each it defines.
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
__END__
3.0
3.0
3.0
6
6
6
