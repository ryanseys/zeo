# `-(-7)` puts the two signs side by side in the generated C, where `--7LL`
# reads as a pre-decrement of a literal and the C compiler rejects it. Routing
# the inner value through a local first always worked, which is what isolates
# the doubled sign on a literal (#4008).
p(-(-7))
p(- -7)
p(-(-3.5))
p(-(-(-7)))
p(-(-7) + 1)
x = -(-2)
p x

# the same doubling with a plus, and the mixed pairs that were never ambiguous
p(+(+7))
p(-(+7))
p(+(-7))
p(+(+2.5))

# a bignum negates through its own helper, not a C sign
p(-(2**70))
p(-(-(2**70)))

# and the ordinary spellings keep their tight form
a = -7
p(-a)
p(-7)
p(- 7)
