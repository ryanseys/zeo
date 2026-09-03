# Bignum-receiver methods that return an Integer/Float/bool without a Rational
# (to_r/rationalize/quo need a bigint-backed Rational and stay unsupported).
p(~(2**100))
p(~(-(2**100)))
p((2**70).div(3))
p((2**64).div(-(2**32)))
p((2**70).gcd(2**69))
p((2**70).gcd(0))
p((2**70).lcm(6))
p((2**70).lcm(0))
p((2**70).gcdlcm(6))
p((2**100).nonzero?)
p((0 - (2**100)) + (2**100))         # zero bignum path
x = 2**100; p x.size
p((2**127).size)
p((10**30).numerator)
p((10**30).denominator)
p((10**30).fdiv(2))
p((2**70).pow(2))
p((2**70).ord)
p((14 * 2**64).allbits?(6))
p((2**64).anybits?(1))
p((2**64).nobits?(1))
p((2**70).ceildiv(3))
# Fixnum receivers of the same methods still work
p(6.gcd(4))
p(12.lcm(8))
p(7.ceildiv(2))
__END__
-1267650600228229401496703205377
1267650600228229401496703205375
393530540239137101141
-4294967296
590295810358705651712
1180591620717411303424
3541774862152233910272
0
[2, 3541774862152233910272]
1267650600228229401496703205376
0
13
16
1000000000000000000000000000000
1
5.0e+29
1393796574908163946345982392040522594123776
1180591620717411303424
false
false
true
393530540239137101142
2
24
4
