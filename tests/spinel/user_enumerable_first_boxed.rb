class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end

p([Nums.new(1, 2), Nums.new(3, 4)].map { |n001| n001.first })

[Nums.new(1, 2)].each { |n002| p n002.first }   # Ruby: 1   Spinel: nil

arr = [Nums.new(1, 2), Nums.new(3, 4)]
p(arr.map { |n003| n003.sum })         # => [3, 7]
p(arr.map { |n004| n004.count })       # => [2, 2]
p(arr.map { |n005| n005.max })         # => [2, 4]
p(arr.map { |n006| n006.min })         # => [1, 3]
p(arr.map { |n007| n007.include?(1) }) # => [true, false]
p(arr.map { |n008| n008.sort })        # => [[1, 2], [3, 4]]
def hd(n010); n010.first; end
p(hd(Nums.new(1, 2)))                  # => 1
p(Nums.new(1, 2).first)                # => 1
