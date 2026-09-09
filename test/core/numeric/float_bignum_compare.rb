# From the spinel corpus (fa06b601).
#
# Comparing a Float against a BIGNUM answers the wrong way for the values
# that do not fit a double's integer range.
#
# CRuby compares exactly. zeo agrees on the ordinary pairs and diverges on
# the ones where the conversion loses -- which is the dangerous direction:
# the comparison silently answers, rather than raising.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# A Float against a Bignum was coerced INTO a bignum, which truncates the
# double to int64 and saturates there, so `1.0 / 0 > 10 ** 100` answered false
# (with a conversion warning from the C compiler on the way past).
#
# Ordering compares as doubles, which is what the runtime's own sp_poly_cmp
# does for the same pair. Equality is decided exactly, because 1.0e100 and
# 10 ** 100 differ by one ulp and as doubles would compare equal.
big = 10 ** 100
p(1.0e200 > big)
p(1.0e50 > big)
p(1.0 > big)
p(big > 1.0)
p(big > 1.0e200)
p(1.0 / 0 > big)
p(-1.0 / 0 < big)
p(2.5 < 10 ** 30)
p(big <= 1.0e200)
p(big >= 1.0e200)
p(1.0e200 <=> big)

p(1.0e100 == big)
p(big == 1.0e100)
p(big != 1.0e100)
p(1.0e200 == big)
p(big.to_f == big)
p(1.5 == big)
p(1.0 / 0 == big)
p(2.0 == 2 ** 1)
p(2.0 == 2 ** 70)
p((2 ** 70).to_f == 2 ** 70)
__END__
true
false
false
true
false
true
true
true
true
false
1
false
false
true
false
false
false
false
true
false
true
