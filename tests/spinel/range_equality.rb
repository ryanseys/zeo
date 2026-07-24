# Range#== / #eql? compare first, last, and exclusivity; a non-range is
# never equal.
p((1..5) == (1..5))
p((1..5) == (1..6))
p((1...5) == (1..5))
p((1..5) == 5)
p((1..5).eql?(1..5))
p((1..5) != (1..6))
p((1..5) != (1..5))
