# `<=>` both ways, with a negative bignum, with a Float too large for one, and
# against a plain Integer.
p(3.0 <=> (10**30))
p((10**30) <=> 3.0)
p(3.0 <=> (10**30) * -1)
p(1e40 <=> (10**30))
p(3.0 <=> 3)
__END__
-1
1
1
1
0
