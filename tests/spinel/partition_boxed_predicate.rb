# A partition block whose value is boxed: `if (rbval)` is not valid C, and
# Ruby's truthiness is "not nil and not false" anyway. The C would not compile.
e = [1, nil, 2]
p e.partition { |x| x }
p [1,2,3].partition { |x| x > 1 }
p [nil, false, 1].partition { |x| x }
p ["a", nil].partition { |x| x }
