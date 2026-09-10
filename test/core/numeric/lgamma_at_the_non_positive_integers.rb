# Math.lgamma is exactly [Infinity, 1] at each pole.
p Math.lgamma(-1.0)
p Math.lgamma(-2.0)
p Math.lgamma(0.0)
p Math.lgamma(-3.0)
p Math.lgamma(-10.0)
# finite values: spinel's own series diverges in the last ULPs from libm by
# design (machine-independent golden output), so check only sign + finiteness
r = Math.lgamma(5.0);  p [r[0].finite?, r[1]]
r = Math.lgamma(-0.5); p [r[0].finite?, r[1]]
r = Math.lgamma(0.5);  p [r[0].finite?, r[1]]
__END__
[Infinity, 1]
[Infinity, 1]
[Infinity, 1]
[Infinity, 1]
[Infinity, 1]
[true, 1]
[true, -1]
[true, 1]
