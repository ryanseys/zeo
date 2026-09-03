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
__END__
(3000000000000000000000000000001/3)
(0/1)
(2000000000000000000000000000000/1)
(500000000000000000000000000000/1)
(1000000000000000000000000000001/1)
(500000000000000000000000000000/1)
(1000000000000000000000000000000/1)
0
-1
true
true
true
true
1.0e+30
[(1/2), (100000000000000000000/1), (1000000000000000000000000000000/1)]
