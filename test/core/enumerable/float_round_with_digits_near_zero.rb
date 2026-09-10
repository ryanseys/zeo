# round(2) and round(3) on tiny negatives, on both zeros, and through a local
# and an array element.
p((-0.0001).round(2))
p((-0.0).round(3))
p((0.0).round(3))
p((1.567).round(2))
p((-1.567).round(2))
x = -0.0001
p(x.round(2))
a = [-0.0001, 2.5]
p(a.map { |v| v }.first.round(2))
__END__
0.0
-0.0
0.0
1.57
-1.57
0.0
0.0
