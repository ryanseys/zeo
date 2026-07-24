# The combined container-read shapes coexist in one program: with Set
# required, a poly map! call used to (a) force set.rb's each into a bogus
# recursive-yield reject, and (b) silently miss the builtin-array arm in
# the user-method block dispatch (#3234).
require 'set'
m = [[1, 2], [3, 4]]; m[1].map! { |x| x * 10 }; p m
h = { a: Set.new([1, 2, 3]) }
p h[:a].map { |x| x }
s2 = Set.new([1, 2])
hs = { b: s2 }
hs[:b].map! { |x| x + 5 }
p s2.to_a.sort
