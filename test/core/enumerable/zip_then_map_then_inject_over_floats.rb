# An array zipped with itself, multiplied pairwise and folded, with an Integer
# and a Float seed.
a = [3.0, 4.0]
p a.zip(a).map { |x, y| x * y }.inject(0, :+)
p [9.0, 16.0].inject(0, :+)
p a.zip(a).map { |x, y| x * y }.inject(0.0, :+)
p [1, 2, 3].inject(0, :+)
p [1.5, 2.5].inject(0, :+)
__END__
25.0
25.0
25.0
6
4.0
