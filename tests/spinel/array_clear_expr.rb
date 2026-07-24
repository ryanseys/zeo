# Array#clear in expression position was only implemented for
# poly_array; on a typed array (`[1,2,3].clear`) it fell through to the
# unresolved-call path, returning int 0 and mis-typing the result. The
# statement-form clear already worked for every typed array.

p [1, 2, 3, 4].clear                 #=> []
p ["a", "b"].clear                   #=> []
p [1.0, 2.0].clear                   #=> []
p [:x, :y].clear                     #=> []
p([1, 2, 3, 4].clear == [])          #=> true (same-typed empty arrays)
p ["a", "b"].clear.length            #=> 0
p [1.0, 2.0].clear.length            #=> 0

# clear resets the int_array sliding window, so a later push refills
# from index 0.
a = [1, 2, 3]
a.shift
a.clear
a.push(9)
p a                                   #=> [9]

# Statement form still empties in place.
b = ["x", "y", "z"]
b.clear
p b.length                            #=> 0
puts "done"
