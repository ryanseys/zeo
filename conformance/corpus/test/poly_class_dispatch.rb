# poly.class — runtime dispatch on the sp_RbVal tag. Heterogeneous
# array elements were previously emitting "0" through the
# unresolved-call path.
a = [1, "hello", 2.0, :sym, nil, true, false]
puts a[0].class
puts a[1].class
puts a[2].class
puts a[3].class
puts a[4].class
puts a[5].class
puts a[6].class
