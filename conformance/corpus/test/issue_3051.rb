f = proc { |x| x + 1 }
g = proc { |x| x * 2 }
l = lambda { |x| x + 10 }
h = f >> g
p h.lambda?
p h.arity
p h.call(3)
p (l >> f).lambda?
p (f << l).lambda?
p (l << f).lambda?
pr = proc { |a, b| a }
p pr.curry.lambda?
p l.curry.lambda?
