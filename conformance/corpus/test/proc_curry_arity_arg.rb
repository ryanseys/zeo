p(->(a, b, c) { a + b + c }.curry(3)[1][2][3])
add = ->(a, b) { a + b }
p(add.curry(2)[10][20])
