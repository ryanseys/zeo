# Proc#curry with an arity larger than the proc's own returns a Proc where
# ruby raises ArgumentError.
#
c001 = proc { |a, b, c| [a, b, c] }.curry(2); p c001[1][2].class
c002 = ->(*a) { a }.curry(3); p c002[1][2][3].class
c003 = ->(*a) { a }.curry; p c003[1].class
r004 = (->(a, b) { a + b }.curry(3) rescue $!.class); p r004
add = ->(a, b) { a + b }.curry
p add[1][2]
mul = proc { |a, b, c| a * b * c }.curry
p mul[2][3][4]
p proc { |a, b| [a, b] }.curry(2)[1][2]
__END__
Array
Array
Array
ArgumentError
3
24
[1, 2]
