# Math.lgamma is exactly [Infinity, 1] at each pole.
p Math.lgamma(-1.0)
p Math.lgamma(-2.0)
p Math.lgamma(0.0)
p Math.lgamma(-3.0)
p Math.lgamma(-10.0)
# finite values: the last few ULPs depend on the machine, so this checks only
# the sign and that the answer is finite
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
