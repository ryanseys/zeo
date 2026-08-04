# `Array[...]` and `Range.new(...)` are the constructor spellings of the
# `[...]` and `(a..b)` literals. Both raised NoMethodError while the literal
# for the same value worked (#3485, #3486). They desugar to the literal, so
# what has to hold is that they type and behave as one -- including the
# element type an array literal carries, and the exclusive form of a range.
p Array[1, 2, 3]
p Array[]
p Array["a", "b"]
p Array[1.5, 2.5]
p Array[[1, 2], [3, 4]]

a = Array[3, 1, 2]
p a.sort
p a.length
p a.push(9)
p a.map { |x| x * 2 }
p a.sum

p Range.new(1, 3).to_a
p Range.new(1, 3, true).to_a
p Range.new(1, 3, false).to_a
p Range.new("a", "c").to_a
p Range.new(1, 5).sum
p Range.new(1, 3).include?(2)
p Range.new(1, 3) == (1..3)
p Range.new(1, 3, true) == (1...3)

r = Range.new(2, 4)
t = 0
r.each { |i| t += i }
p t

# the sibling constructors that already worked stay working
p Hash[[[:a, 1]]]
p Array.new(3, 0)
p [1, 2, 3]
p (1..3).to_a
