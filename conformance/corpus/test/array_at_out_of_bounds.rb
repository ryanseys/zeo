# Array#at returns nil for an out-of-range index. For a scalar element
# array the result is now typed int?/float?, so the out-of-bounds
# sentinel (sp_IntArray_get already returns SP_INT_NIL; sp_FloatArray_get
# now returns the float-nil) is recognised by `== nil`, `.nil?`, and
# inspect. (Array#[] keeps the plain element type -- making it nullable
# would cascade the sentinel through every hot index.)

p([1, 2, 3, 4, 5, 6].at(99) == nil)        #=> true
p [1, 2, 3].at(99)                          #=> nil
p [1, 2, 3].at(1)                           #=> 2
p [1, 2, 3].at(-1)                          #=> 3
p([1, 2, 3].at(99).nil?)                    #=> true
p([1, 2, 3].at(1).nil?)                     #=> false
p ["a", "b"].at(5)                          #=> nil

# In-bounds results still behave as plain scalars in arithmetic.
puts [10, 20, 30].at(1) + 5                 #=> 25
puts [1.5, 2.5].at(0) + 1.0                 #=> 2.5

# Float out-of-range.
p([1.5, 2.5].at(9) == nil)                  #=> true
p [1.5, 2.5].at(9)                          #=> nil
puts "done"
