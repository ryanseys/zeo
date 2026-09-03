add = ->(a, b, c) { a + b + c }
p add.curry[1][2][3]
p add.curry[1, 2][3]
step = add.curry[10]
p step[1][2]
p step[3][4]
p add.curry.arity
p add.curry.lambda?
p proc { |a, b| a * b }.curry[3][4]
__END__
6
6
13
17
-1
true
12
