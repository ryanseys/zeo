# Variable-stored proc/lambda calls: keyword params bind boxed and type poly
# (#2728), and a splat may sit anywhere among the arguments (#2729).
f = ->(a, k:) { [a, k] }
p(f.call(1, k: 2))
g = ->(a, k: 9) { a + k }
p(g.call(1))
p(g.call(1, k: 5))
h = ->(x, **kw) { [x, kw] }
p(h.call(7, a: 1))
f3 = ->(a, b, c) { a + b + c }
rest = [2, 3]
p(f3.call(1, *rest))
p(f3.call(*[1, 2], 3))
gv = ->(*xs) { xs.length }
p(gv.call(1, *[2, 3], 4))
