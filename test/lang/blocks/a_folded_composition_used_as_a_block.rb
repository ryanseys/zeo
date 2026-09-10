# `reduce(:>>)` and `reduce(:<<)` over lambdas, called directly and passed with
# &.
pl = [->(x) { x * x }, ->(x) { x + 1 }].reduce(:>>)
p pl.call(3)
p [1, 2, 3].map(&pl)

pl2 = [->(x) { x + 1 }, ->(x) { x * 2 }].reduce(:<<)
p pl2.call(3)
p [1, 2, 3].map(&pl2)
__END__
10
[2, 5, 10]
7
[3, 5, 7]
