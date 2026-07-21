c = ->(a, b) { a + b }.curry
p(c.arity)
p(c.lambda?)
p(c.class)
