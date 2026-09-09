# Negative zero, positive zero, and two negative integers.
# (spinel issue #3116)
p Math.lgamma(-0.0)
p Math.lgamma(0.0)
p Math.lgamma(-1.0)
p Math.lgamma(-2.0)
__END__
[Infinity, -1]
[Infinity, 1]
[Infinity, 1]
[Infinity, 1]
