# GAP -- imported from the spinel corpus at c55d9bdb.
# Enumerator.product with a block returns the Enumerator::Product instead
# of running the block over each tuple.
#
Enumerator.product([1, 2], [3]) { |a, b| p [a, b] }
r = Enumerator.product([1, 2], [3]) { |a, b| }
p r
p Enumerator.product([1, 2], [3]).to_a
