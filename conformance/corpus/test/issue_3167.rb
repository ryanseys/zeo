pl = [->(x) { x * x }, ->(x) { x + 1 }].reduce(:>>)
p pl.call(3)
p [1, 2, 3].map(&pl)

pl2 = [->(x) { x + 1 }, ->(x) { x * 2 }].reduce(:<<)
p pl2.call(3)
p [1, 2, 3].map(&pl2)
