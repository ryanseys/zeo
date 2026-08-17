class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end
p(Nums.new(1, 2, 3).find(-> { :none }) { |x| x > 9 })
p(Nums.new(1, 2, 3).detect(-> { :none }) { |x| x > 9 })
c007 = Nums.new(1, 2, 3).find(-> { :none }) { |x| x > 9 }
p c007.class
p(Nums.new(1, 2, 3).find(-> { :none }) { |x| x > 1 })
p(Nums.new(1, 2, 3).find { |x| x > 1 })
p([1, 2, 3].find(-> { :none }) { |x| x > 9 })
