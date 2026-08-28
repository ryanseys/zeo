# A captured-and-reassigned local needs a heap cell. Float, poly and class each
# got a cell of their own C type; Range, Rational and Complex are the same shape
# -- small by-value structs with no GC pointer -- but were left on the
# "non-integer capture" reject, in the cell prologue AND in the capture struct.
# And the block-local reset had no by-value arm at all, so a captured class
# there fell through to the sp_int else and assigned an sp_int * to an
# sp_Class *: ill-typed C for a program CRuby runs.

2.times do |n|
  v = (n..3)
  pr = proc { v = (n..4); nil }
  pr.call
  puts v.to_s
end

2.times do |n|
  v = Rational(n + 1, 2)
  pr = proc { v = Rational(n + 1, 3); nil }
  pr.call
  puts v.to_s
end

2.times do |n|
  v = Complex(n, 1)
  pr = proc { v = Complex(n, 2); nil }
  pr.call
  puts v.to_s
end

2.times do |n|
  v = (n + 0.5)..3.5
  pr = proc { v = (n + 0.5)..4.5; nil }
  pr.call
  puts v.to_s
end

# the class cell, whose reset arm was the missing one
2.times do |n|
  k = Struct.new(:x)
  pr = proc { k.new(n) }
  puts pr.call.x.to_s
end

# reading a captured by-value struct without reassigning it
2.times do |n|
  span = (n..(n + 2))
  rat = Rational(n + 1, 4)
  pr = proc { span.to_a.length + rat.denominator }
  puts pr.call.to_s
end
