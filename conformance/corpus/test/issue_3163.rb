l = ->(a) { a }
p l.dup == l
p l.clone == l
p l.dup.eql?(l)
p l.dup.equal?(l)   # equal? is identity -> false
# distinct blocks are not ==
f = ->(a) { a }
g = ->(a) { a }
p f == g
# same literal evaluated separately -> not ==
procs = 2.times.map { ->(x) { x } }
p procs[0] == procs[1]
