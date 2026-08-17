# Array#to_h wants each element to be a two-element array. A longer or shorter
# one is an ArgumentError and a non-array a TypeError; the extra elements were
# simply dropped and a short one read a nil value.
p(([[1,2,3]].to_h rescue $!.message))
p(([[1]].to_h rescue $!.message))
p(([[1,2],[3]].to_h rescue $!.message))
p([[1,2],[3,4]].to_h)
p([["a",1]].to_h)
p([[:a,1],[:b,2]].to_h)
p(([[1,2],3].to_h rescue $!.message))
