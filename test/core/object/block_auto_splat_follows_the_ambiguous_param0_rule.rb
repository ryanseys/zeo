# A lone Array argument spreads across a block's positional params -- but a
# block with exactly ONE plain param receives it whole (`ambiguous_param0`,
# the `each { |pair| }` idiom), and so does a lone optional. Oracle-derived;
# see `clif::params::auto_splats` for the full truth table.

def one(x) = yield x
one([1, 2]) { |a| p a }
one([1, 2]) { |a = 9| p a }
one([1, 2]) { |*a| p a }
one([1, 2]) { |a, **k| p a }
one([1, 2]) { |a, b| p [a, b] }
one([1, 2]) { |a, *b| p [a, b] }
one([1, 2]) { |*a, b| p [a, b] }
__END__
[1, 2]
[1, 2]
[[1, 2]]
[1, 2]
[1, 2]
[1, [2]]
[[1], 2]
