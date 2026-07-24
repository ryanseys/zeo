# big Rational arithmetic and comparison (#2469, follow-up to the basic type).
# One operand is already a big Rational; every numeric operand is coerced to a
# num/den bigint pair. (big Rational == a raw Bignum is a separate codegen path,
# still pending.)
r = (10**30).to_r
p(r + (1r/3))
p(r - r)
p(r * 2)
p(r / 2)
p(r + 1)
p((10**30).quo(3) + (10**30).quo(6))
p((10**30).quo(3) * 3)
p(r <=> (10**30).to_r)
p(r <=> (10**31).to_r)
p(r > (10**29).to_r)
p(r < (10**31).to_r)
p((2**70).quo(2**69) == 2)
p(r == (10**30).to_r)
p(r.to_f)
p([(10**30).to_r, (1r/2), (10**20).to_r].sort)
