# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# A proc reaching itself through a captured local.
#@ gccheck: cycle leak: 4 objects (Proc x2, cell x2)
# A proc/lambda that refers to its own binding recurses through the
# captured cell. Each body has a single recursive `.call` per expression;
# two poly-returning proc calls in one expression (e.g. naive fib) instead
# exercise the shared poly-return slot, an orthogonal limitation.
fact = proc { |n| n <= 1 ? 1 : n * fact.call(n - 1) }
p fact.call(5)

f = ->(n) { n <= 1 ? 1 : n * f.call(n - 1) }
p f.call(6)
__END__
120
720
