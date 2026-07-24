# A proc closing over an ENCLOSING lambda/proc's variables (#2648): the
# enclosing proc's frame owns the variable, so its prologue materializes the
# heap cell and the inner proc captures the cell pointer. The proc's own **kw
# param also binds (the whole trailing kwargs hash).
g = ->(x) { ->{ x } }
p(g.call(5).call)
p(g.call(7).call)
# counter factory: reassignment through the cell, one cell per outer call
make = ->(start) { ->{ start += 1 } }
c1 = make.call(10)
p c1.call
p c1.call
c2 = make.call(100)
p c2.call
p c1.call
# string capture and multi-level
tag = ->(prefix) { ->(x) { "#{prefix}:#{x}" } }
p tag.call("id").call(5)
adder = ->(a) { ->(b) { a + b } }
p adder.call(3).call(4)
# own **kw param
p(->(a, **kw) { [a, kw] }.call(1, x: 2))
p(->(a, **kw) { [a, kw] }.call(1))
opts = ->(a, **kw) { kw.length + a }
p opts.call(1, x: 2, y: 3)
