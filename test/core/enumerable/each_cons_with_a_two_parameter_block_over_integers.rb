# each_cons(2).map { |a, b| a + b } over an Integer array, and last on the
# source.
# (spinel issue #2915)
codes = (0...8).map { |n| n * 2 }
d = codes.each_cons(2).map { |a, b| a + b }
w = codes.last
p d
p w
p [1.0, 2.0, 3.0].each_cons(2).map { |a, b| a * b }
__END__
[2, 6, 10, 14, 18, 22, 26]
14
[2.0, 6.0]
