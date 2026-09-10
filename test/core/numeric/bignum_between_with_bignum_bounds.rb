# `between?` on a number too large for a machine word, against bounds that are
# also too large.
p((2 ** 150).between?(1, 2 ** 100))
p((2 ** 150).between?(2 ** 100, 2 ** 200))
p((10 ** 40).between?(10 ** 39, 10 ** 41))
__END__
false
true
true
