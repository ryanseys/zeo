# Proc#>> / #<< with a Method operand composes through Method#to_proc, whose
# trampoline publishes its boxed result -- composing the raw method object
# mis-typed the intermediate (#2692).
inc = ->(x) { x + 1 }
dbl = 2.method(:*)
g = (inc >> dbl)
p(g.call(4))
p((dbl >> inc).call(4))
p((inc << dbl).call(4))
half = ->(x) { x * 10 }
p((dbl >> half).call(3))
