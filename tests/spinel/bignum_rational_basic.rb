# to_r / rationalize / quo on a Bignum build a boxed big Rational (numerator
# exceeds mrb_int); construction, inspect/to_s/puts, to_f, class, and == are
# wired. Arithmetic and <=> on a big Rational are follow-up work. The by-value
# int Rational is unchanged. (#2469)
p((10**30).to_r)
p((10**30).rationalize)
p((10**30).quo(3))
p((10**30).quo(2**64))
p((-(10**30)).to_r)
p((10**30).to_r.to_f)
p((10**30).to_r.class)
p((10**30).to_r == (10**30).to_r)
p((10**30).to_r == (10**31).to_r)
puts((10**30).quo(7))
a = [(10**30).quo(7), (10**40).to_r]
p a
# the int Rational path is unaffected
p((1r/3))
p(3.to_r)
p((1r/2) + (1r/3))
p(2.quo(3))
p(0.5.to_r)
