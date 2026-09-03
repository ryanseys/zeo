# More than one OPTIONAL param auto-splats even with no required param at
# all, while a single optional plus a rest does not. This pair is what rules
# out the tempting "count the positional slots" formulation of the rule.

def one(x) = yield x
one([1, 2]) { |a = 5, b = 4| p [a, b] }
one([1, 2]) { |a = 5, *b| p [a, b] }
one([1, 2]) { |a = 5, b = 4, *c| p [a, b, c] }
one([1, 2]) { |a = 5, *b, c| p [a, b, c] }
__END__
[1, 2]
[[1, 2], []]
[1, 2, []]
[1, [], 2]
