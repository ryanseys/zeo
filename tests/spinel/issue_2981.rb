class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end
p(Nums.new(1, 2, 3, 4).each_slice(2) { |sl| }.class)
p(Nums.new(1, 2, 3, 4).each_cons(2) { |c| }.class)
p(Nums.new(1, 2, 3).each_with_index { |x, i| }.class)
p([1, 2, 3, 4].each_slice(2) { |sl| }.class)
n = Nums.new(1, 2, 3, 4)
sl = []
r = n.each_slice(2) { |s| sl << s }
p sl
p r.equal?(n)
