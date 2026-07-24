# Array#grep / #grep_v on an empty array literal match nothing and return an
# empty array (the bare literal has no element type to dispatch from).
p([].grep(Integer))
p([].grep_v(Integer))
p([1, "a", 2].grep(Integer))
p([].grep(Integer) { |x| x * 2 })
