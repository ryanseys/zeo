# Enumerator::Chain (Enumerator#+ / Enumerable#chain), Enumerator.product,
# and Enumerator::Lazy#size -- all of which answer without iterating.

# `+` and `chain` both build an Enumerator::Chain over the held sources.
a = [1, 2].each
b = [3, 4].each
p((a + b).class)
p((a + b).to_a)
p((a + b).size)
p([1, 2].chain([3], [4, 5]).to_a)
p((1..2).chain([3]).to_a)

# The sources are held, not flattened, so each is driven by its own #each.
class Letters
  include Enumerable
  def initialize(*xs) = @xs = xs
  def each(&blk) = @xs.each(&blk)
end
p([9].chain(Letters.new(7, 8)).to_a)
p(Letters.new(1, 2).chain([3]).map { |x| x * 2 })

# Terminals inherited from Enumerable work over a chain.
p([1, 2].chain([3]).select { |x| x > 1 })
p([1, 2].chain([3]).sum)
p([1, 2].chain([3]).include?(3))

# Enumerator.product: rightmost source varies fastest.
p(Enumerator.product([1, 2], [3, 4]).class)
p(Enumerator.product([1, 2], [3, 4]).to_a)
p(Enumerator.product([1, 2], [3], [4, 5]).to_a)
p(Enumerator.product([1, 2], [3, 4]).size)

# Lazy#size folds the ops that have a knowable effect; a filter makes it nil.
p((1..10).lazy.size)
p([1, 2, 3].lazy.map { |x| x * 2 }.size)
p([1, 2, 3].lazy.select { |x| x > 1 }.size)
p([1, 2, 3].lazy.take(5).size)
p([1, 2, 3, 4, 5].lazy.drop(1).take(2).size)
p((1..Float::INFINITY).lazy.size)
p((1..Float::INFINITY).lazy.take(3).size)
p((1..Float::INFINITY).lazy.map { |x| x * 2 }.first(3))
__END__
Enumerator::Chain
[1, 2, 3, 4]
4
[1, 2, 3, 4, 5]
[1, 2, 3]
[9, 7, 8]
[2, 4, 6]
[2, 3]
6
true
Enumerator::Product
[[1, 3], [1, 4], [2, 3], [2, 4]]
[[1, 3, 4], [1, 3, 5], [2, 3, 4], [2, 3, 5]]
4
10
3
nil
3
2
Infinity
3
[2, 4, 6]
