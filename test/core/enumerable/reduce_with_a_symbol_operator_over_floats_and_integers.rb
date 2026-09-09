# reduce(0, :+) and reduce(1, :*) over Floats, Integers, a mix, and a Range.
# (spinel issue #3181)
p [1.0, 2.0].reduce(0, :+)
p [1.0, 2.0, 3.0].reduce(1, :*)
p [1, 2].reduce(0, :+)
p [1.5, 2.5].reduce(0, :+)
p (1..3).reduce(0, :+)
p [1.0, 2.0].reduce(:+)
__END__
3.0
6.0
3
4.0
6
3.0
