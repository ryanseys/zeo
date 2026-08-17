require 'set'

# A blockless Set#each raised LocalJumpError -- the method is written with
# yield -- where CRuby answers an Enumerator, and the constructor refused an
# Enumerator operand because nothing said an Enumerator is enumerable.

# a blockless Set#each is an Enumerator over the members
p Set[1, 2].each.to_a

# the constructor takes any Enumerable, including an Enumerator
p Set.new([1, 2].each).to_a
p Set.new([1, 2]).to_a
p Set.new((1..3)).to_a
p Set.new({ a: 1 }).to_a
p Set.new([1, 2]) { |x| x * 2 }.to_a
p Set.new(Set[5, 6]).to_a

# the block form and the rest of the surface are unchanged
r = []
Set[3, 4].each { |x| r << x }
p r
p Set[1, 2].map { |x| x + 1 }
p Set[1, 2].to_a
p Set[1, 2].include?(2)
