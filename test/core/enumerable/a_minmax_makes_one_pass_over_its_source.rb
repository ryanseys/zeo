# `Enumerable#minmax` ran `min` and then `max`, so a comparison block saw
# every element TWICE and the source was walked twice. CRuby's `enum_minmax`
# buffers in PAIRS: one comparison orders the pair, then at most one more
# lowers the min and one more raises the max. The call SEQUENCE is
# observable, so it is pinned here, not just the result.
def probe(a)
  seen = []
  r = a.minmax { |x, y| seen << [x, y]; x <=> y }
  [r, seen]
end

p probe([])
p probe([7])
p probe([5, 1])
p probe([5, 1, 9])
p probe([5, 1, 9, 3])
p probe([5, 1, 9, 3, 0])

# The block-less spelling, and a non-Array Enumerable, which reaches the same
# driver through `each`.
p [].minmax
p [4].minmax
p [3, 1, 2].minmax

class Bag
  include Enumerable
  def initialize(*a) = @a = a
  def each(&b) = @a.each(&b)
end
p Bag.new(4, 2, 8, 6, 1).minmax
n = 0
p Bag.new(4, 2, 8).minmax { |x, y| n += 1; x <=> y }
p n
__END__
[[nil, nil], []]
[[7, 7], []]
[[1, 5], [[5, 1]]]
[[1, 9], [[5, 1], [9, 1], [9, 5]]]
[[1, 9], [[5, 1], [9, 3], [3, 1], [9, 5]]]
[[0, 9], [[5, 1], [9, 3], [3, 1], [9, 5], [0, 1], [0, 9]]]
[nil, nil]
[4, 4]
[1, 3]
[1, 8]
[2, 8]
3
