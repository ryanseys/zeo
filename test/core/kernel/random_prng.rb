# `Random` -- a seedable PRNG. zeo's generator is xorshift64*, not CRuby's
# MT19937, so raw sequences differ; this example prints only the DETERMINISTIC
# guarantees that hold for any correct PRNG (reproducibility, ranges, return
# types), never a raw random value.

# Same seed -> same sequence; different seeds -> (almost surely) different.
p(Random.new(42).rand(1000) == Random.new(42).rand(1000))
p(Random.new(1).rand(1000) != Random.new(2).rand(1000))

# Return types follow the bound: Integer bound -> Integer, Float bound ->
# Float, no bound -> a Float in [0, 1).
p Random.new(5).rand(10).class
p Random.new(5).rand(2.5).class
p Random.new(5).rand.class

# Ranges are respected.
p((r = Random.new(9).rand(6)) >= 0 && r < 6)
p(Random.new(9).rand < 1.0)
p((v = Random.new(3).rand(5..9)) >= 5 && v <= 9)

# `bytes` returns exactly n bytes.
p Random.new(1).bytes(8).bytesize

# `seed` reports the integer seed it was built with (a Float is truncated).
p Random.new(123).seed
p Random.new(3.9).seed

# A non-positive bound is an ArgumentError, not a silent 0.
begin; Random.new(1).rand(0); rescue ArgumentError => e; puts e.message; end
begin; Random.new(1).rand(-3); rescue ArgumentError => e; puts e.message; end
__END__
true
true
Integer
Float
Float
true
true
true
8
123
3
invalid argument - 0
invalid argument - -3
