# Proc#arity / #lambda? / #curry all read facts about a proc's PARAMETERS,
# which a compiled Rust closure can't answer about itself -- the compiler
# records them from the block/lambda's static signature at construction.
#
# arity encodes: min = required + post + (1 if any required keyword);
# a signature whose maximum is unbounded (or, for a lambda, merely differs
# from the minimum) reports -(min + 1) instead.

# Required-only signatures stay positive.
p proc {}.arity
p proc { |a| }.arity
p proc { |a, b| }.arity
p ->() {}.arity
p ->(a, b) {}.arity
p ->(a, b, c) {}.arity

# A block parameter isn't an argument slot.
p ->(&b) {}.arity

# An optional parameter makes a LAMBDA negative...
p ->(a, b = 1) {}.arity
p lambda { |a, b = 1, c = 2| }.arity
# ...but a plain proc reports its minimum (its maximum is still bounded).
p proc { |a, b = 1| }.arity

# A rest parameter is unbounded: negative either way.
p proc { |*a| }.arity
p proc { |a, *b| }.arity
p ->(a, *b) {}.arity
p proc { |a, b, *c| }.arity

# Post parameters are required, so they raise the minimum.
p ->(a, *b, c) {}.arity
p ->(a, *b, c, d) {}.arity

# A required keyword adds exactly one mandatory slot, however many there are.
p ->(b:) {}.arity
p ->(a, b:) {}.arity
p ->(a, b:, c:) {}.arity
p ->(a, b:, c: 1) {}.arity
p ->(a, e:, **g) {}.arity
p proc { |a, b:| }.arity

# An optional keyword or **rest alone widens the maximum.
p ->(a, b: 1) {}.arity
p ->(a, **k) {}.arity
p proc { |a, b: 1| }.arity
p proc { |a, **k| }.arity

# Everything at once.
p ->(a, b = 1, *c, d, e:, f: 2, **g, &h) {}.arity

# lambda? distinguishes the two kinds.
p ->() {}.lambda?
p lambda {}.lambda?
p proc {}.lambda?
p Proc.new {}.lambda?

# curry collects arguments across calls until the arity is satisfied.
add = ->(a, b, c) { a + b + c }
p add.curry[1][2][3]
p add.curry.(1).(2).(3)
p add.curry[1, 2][3]
p add.curry[1][2, 3]
# Every curry step is itself a var-args lambda, so its own arity is -1.
p add.curry.arity
p add.curry[1].arity
p add.curry.lambda?
p add.curry[1].lambda?

# Each partial application is a FRESH proc -- reusable, never accumulating.
step = add.curry[10]
p step[1][2]
p step[3][4]
p step[5][6]

# A plain proc curries to its minimum arity.
mul = proc { |a, b| a * b }
p mul.curry[3][4]
p proc { |a, b| }.curry.arity

# An explicit count curries a proc whose own arity is unbounded.
p ->(*a) { a.sum }.curry(3)[1][2][3]
p ->(*a) { a.length }.curry(2)[:x][:y]
__END__
0
1
2
0
2
3
0
-2
-2
1
-1
-2
-2
-3
-3
-4
1
2
2
2
2
2
-2
-2
1
1
-4
true
true
false
false
6
6
6
6
-1
-1
true
true
13
17
21
12
-1
6
2
