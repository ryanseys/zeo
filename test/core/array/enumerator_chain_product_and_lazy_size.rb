# Enumerator::Chain over held (not flattened) sources, Enumerator.product's
# rightmost-fastest ordering, and Lazy#size folding ops without iterating.

p(([1, 2].each + [3, 4].each).class)
p(([1, 2].each + [3, 4].each).to_a)
p([1, 2].chain([3], [4, 5]).to_a)
p([1, 2].chain([3]).size)

class Letters
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each(&blk); @xs.each(&blk); end
end
p([9].chain(Letters.new(7, 8)).to_a)
p([1, 2].chain([3]).select { |x| x > 1 })

p(Enumerator.product([1, 2], [3, 4]).class)
p(Enumerator.product([1, 2], [3], [4, 5]).to_a)
p(Enumerator.product([1, 2], [3, 4]).size)

p([1, 2, 3].lazy.map { |x| x * 2 }.size)
p([1, 2, 3].lazy.select { |x| x > 1 }.size)
p([1, 2, 3, 4, 5].lazy.drop(1).take(2).size)
p((1..Float::INFINITY).lazy.size)
p((1..Float::INFINITY).lazy.take(3).size)
__END__
Enumerator::Chain
[1, 2, 3, 4]
[1, 2, 3, 4, 5]
3
[9, 7, 8]
[2, 3]
Enumerator::Product
[[1, 3, 4], [1, 3, 5], [2, 3, 4], [2, 3, 5]]
4
3
nil
2
Infinity
3
