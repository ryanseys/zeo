# Array#== on an array of arrays/objects (a ptr_array) had no deep
# comparison -- the dispatch covered int/str/float/poly arrays but not
# ptr_array, so `[[1]] == [[1]]` fell through to FALSE. Compare
# element-wise inline using each element's statically-known type.

p([[1]] == [[1]])                         #=> true
p([[1, 2], [3]] == [[1, 2], [3]])         #=> true
p([[1]] == [[2]])                         #=> false
p([[1], [2]] == [[1]])                    #=> false (length)
p([["a"], ["b"]] == [["a"], ["b"]])       #=> true
p([["a"]] == [["b"]])                     #=> false
p([[1.0]] == [[1.0]])                     #=> true
p([[1]] != [[2]])                         #=> true
p([[1]] != [[1]])                         #=> false

x = [[1, 2, 3]]
y = [[1, 2, 3]]
p(x == y)                                 #=> true
x[0] << 4
p(x == y)                                 #=> false

puts "done"
