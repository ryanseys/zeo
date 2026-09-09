# `(2..10).lazy.each_cons(2).first(3)` gives three overlapping pairs.
# (spinel issue #3171)
p (2..10).lazy.each_cons(2).first(3)
__END__
[[2, 3], [3, 4], [4, 5]]
