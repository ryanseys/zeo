def fn(opts) = opts.pop
def fn2(*opts) = fn(opts)
p fn([""])
p fn([1, 2, 3])
p fn([1.5, 2.5])
def g(a) = a.first
p g([10, 20])
