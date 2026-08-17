# Range#== compares its endpoints with ==, and 1 == 1.0, so an Integer Range
# equals the Float Range with the same endpoints (#3841).
p((1..5) == (1.0..5.0))
p((1.0..5.0) == (1..5))
p((1..5) == (1.0..6.0))
p((1...5) == (1.0..5.0))
p((1...5) == (1.0...5.0))
p((1..5) == (1..5))
p((1..5) != (1.0..5.0))
