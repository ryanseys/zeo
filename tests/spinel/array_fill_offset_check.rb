# fill(value, start[, length]): start and length are offsets, and a value with
# no integer conversion is CRuby's TypeError. They went into the offset slot
# as-is, so a String start read as a pointer and the fill quietly did nothing.
p(([1,2].fill(0,"x") rescue $!.message))
p(([1,2].fill(0,nil)))
p(([1,2,3].fill(0,1)))
p([1,2,3].fill(9))
p([1,2,3].fill(9,1,1))
p((["a"].fill("z",:s) rescue $!.message))
