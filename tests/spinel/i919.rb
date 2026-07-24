add = -> (a, b) { a + b }
add5 = add.curry[5]
puts add5[3]
puts add.curry[5][3]
mul = -> (a, b, c) { a * b * c }
f = mul.curry[2]
g = f[3]
puts g[4]
c = add.curry
puts c[10][20]
sub = -> (a, b) { a - b }
puts sub.curry[10][3]
