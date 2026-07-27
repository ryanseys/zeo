# zeo's seeded PRNG does not reproduce CRuby's Mersenne Twister sequence, so
# `Random.new(seed)` yields different numbers than ruby for the same seed.
r = Random.new(42)
p r.rand(100)
p r.rand(100)
p r.rand
