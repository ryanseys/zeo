# The PRNG contract: same seed -> same stream, for every consumer of the
# Kernel-level stream (rand in all its forms, shuffle, sample, the Random
# default instance). Values are implementation-specific (spinel is not
# MT19937), so only invariants are asserted.

srand(42); a = rand(100)
srand(42); b = rand(100)
p a == b
p a >= 0 && a < 100

srand(42); f1 = rand
srand(42); f2 = rand
p f1 == f2
p f1 >= 0.0 && f1 < 1.0
p rand(0).is_a?(Float)

# a dynamic (non-literal) bound goes through the boxed emission
n = 50
srand(9); d1 = rand(n)
srand(9); d2 = rand(n)
p d1 == d2
p d1.is_a?(Integer)

# ranges draw from the same stream, so srand governs them too
srand(3); r1 = rand(1..1000)
srand(3); r2 = rand(1..1000)
p r1 == r2
p r1 >= 1 && r1 <= 1000
srand(3); fr1 = rand(1.5..2.5)
srand(3); fr2 = rand(1.5..2.5)
p fr1 == fr2
p fr1 >= 1.5 && fr1 < 2.5

# shuffle/sample across the typed-array variants share the stream as well
ints = (1..16).to_a
srand(7); s1 = ints.shuffle
srand(7); s2 = ints.shuffle
p s1 == s2
p s1.sort == ints
strs = %w[a b c d e f g h]
srand(7); t1 = strs.shuffle
srand(7); t2 = strs.shuffle
p t1 == t2
mixed = [1, "x", :y, 2.5, nil, true, 7, "z"]
srand(11); m1 = mixed.shuffle
srand(11); m2 = mixed.shuffle
p m1 == m2
srand(13); v1 = ints.sample
srand(13); v2 = ints.sample
p v1 == v2

# srand returns the PREVIOUS seed
srand(5)
p srand(9)
p srand

# per-instance Random streams are independent and reproducible
p Random.new(42).rand(1000) == Random.new(42).rand(1000)
p Random.new(42).rand(1000) != Random.new(43).rand(1000)
r = Random.new(42)
p r.seed
p Random.new(42).bytes(8) == Random.new(42).bytes(8)
p Random.new(42).bytes(8).length
