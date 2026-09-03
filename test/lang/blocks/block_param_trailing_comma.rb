# GAP -- imported from the spinel corpus at c55d9bdb.
# A block parameter list with a trailing comma (`|a,|`) does not discard
# the extra element, so the body sees the wrong binding.
#
# `|x,|` -- a trailing comma in a block's parameter list -- means "destructure
# the element and take the leading names, drop the rest". Prism spells the
# comma as an ImplicitRestNode, which nothing read, so `|x,|` behaved as `|x|`
# and bound the whole element.
pairs = [[10, 20], [30, 40]]
p pairs.map { |x,| x }
p pairs.flat_map { |x,| [x] }
p pairs.each { |x,| p x }
p pairs.select { |x,| x > 10 }
p pairs.each_with_object([]) { |x, acc| acc << x }

nested = [[[[1, 2]], "yes"]]
p nested.flat_map { |ring,| ring }
p nested.map { |ring,| ring }
p nested.flat_map { |ring,| ring.map { |pt| pt[0] } }

# two names and a comma: the third value is the one dropped
triples = [[1, 2, 3], [4, 5, 6]]
p triples.map { |a, b,| [a, b] }

# a plain scalar element is bound whole, not destructured away
p [1, 2].map { |x,| x }

# reflection: the synthesized tail is not part of the signature
pr = proc { |x,| x }
p pr.arity
p pr.call([1, 2])

# a lambda is strict about its list, and there the comma changes nothing:
# exactly one argument, and it is not destructured
l = lambda { |a,| a }
p l.arity
p l.call(1)
p(begin; l.call(1, 2); rescue ArgumentError; "ArgumentError"; end)

# the ordinary forms are unchanged
p pairs.map { |x| x }
p pairs.map { |x, y| x + y }
__END__
[10, 30]
[10, 30]
10
30
[[10, 20], [30, 40]]
[[30, 40]]
[[10, 20], [30, 40]]
[[1, 2]]
[[[1, 2]]]
[1]
[[1, 2], [4, 5]]
[1, 2]
1
1
1
1
"ArgumentError"
[[10, 20], [30, 40]]
[30, 70]
