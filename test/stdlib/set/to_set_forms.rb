# GAP -- imported from the spinel corpus at c55d9bdb.
# Enumerable#to_set does not apply the block form, so the untransformed
# elements land in the set.
#
require 'set'
p([1, 2].to_set.to_set.to_a)
s = [1, 2].to_set
p s.to_set.to_a
p [3, 4].to_set { |x| x * 2 }.to_a rescue p $!.class
t = Set[1,2]
p t.to_set.equal?(t)
__END__
[1, 2]
[1, 2]
[6, 8]
true
