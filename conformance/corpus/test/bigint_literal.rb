# An integer literal wider than int64 is an arbitrary-precision Bignum in every
# overflow mode (CRuby semantics), not a saturated int. Underscores are allowed.
puts 100000000000000000000
puts(-100000000000000000000)
puts 10_000_000_000_000_000_000_000
x = 100000000000000000000
puts x + 1
puts x * 3
puts 18446744073709551616        # 2**64
puts 99999999999999999999999999999999 - 1
