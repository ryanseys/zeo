# A machine-word Integer tested against bignum bounds, and the reverse.
# (spinel issue #2893)
p 50.between?(1, 2 ** 100)
p 50.between?(2 ** 100, 2 ** 200)
p (2 ** 100).between?(1, 2 ** 200)
p 5.between?(1, 10)
__END__
true
false
true
true
