arr = [1, 2, 3, 4]

p arr.inject(0) { |a, b| a + b }
p arr.inject(10) { |a, b| a + b }
p arr.reduce([]) { |a, b| a << b }
p arr.inject("") { |a, b| a += b.to_s; a }

# The block's VALUE is the next accumulator, so reassigning either param
# inside the body only matters through what the body evaluates to.
p arr.inject(0) { |a, b| b = 100; a + b }
p(arr.inject(0) { |a, b| next a })

# An empty receiver never calls the block and answers the seed.
p [].inject(:seed) { |a, b| :other }

# `break` overrides the whole call.
p(arr.inject(0) { |a, b| break :early })

# Without an explicit seed the first element IS the seed and the block is not
# called for it -- a different shape, which takes the dynamic row.
p arr.inject { |a, b| a + b }
p arr.inject(:+)

# A block with any count other than two params also takes the dynamic row.
p arr.inject(0) { |a| 7 }

# Nested drivers, each with its own accumulator.
p arr.inject(0) { |a, b| a + [b, b].inject(0) { |c, d| c + d } }
__END__
10
20
[1, 2, 3, 4]
"1234"
400
0
:seed
:early
10
10
7
20
