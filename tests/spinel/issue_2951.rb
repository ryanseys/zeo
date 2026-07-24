bits = [true, false, true]
p bits.reduce(:&)
p bits.reduce(:|)
p bits.reduce(:^)
p [1,3,5,7].reduce(:&)
p [1,2,4].reduce(:|)
