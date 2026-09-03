# GAP -- imported from the spinel corpus at c55d9bdb.
# Integer#gcd with a non-integer raises the coercion message; ruby raises
# "not an integer".
#
# Integer#gcd / #lcm / #gcdlcm take an Integer; anything else is CRuby's
# "not an integer" TypeError. Only a Float was caught, so a String went into
# the mrb_int slot as a pointer and the result was computed from its address.
p((5.gcd("x") rescue $!.message))
p((5.lcm(nil) rescue $!.message))
p((5.gcd(1.5) rescue $!.message))
p((5.gcd(:a) rescue $!.message))
p((5.lcm([1]) rescue $!.message))
p(5.gcd(10))
p(4.lcm(6))
p(4.gcdlcm(6))
__END__
"not an integer"
"not an integer"
"not an integer"
"not an integer"
"not an integer"
5
12
[2, 12]
