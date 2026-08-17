class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end
p(Nums.new(1, 2).cycle.first(5))
r001 = (Nums.new(1, 2).cycle.class rescue $!.class); p r001
p [1,2].cycle.class
p [1,2].cycle(2).class
