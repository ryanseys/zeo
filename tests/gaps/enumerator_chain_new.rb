a = [1, 2].each
b = [3, 4].each
p Enumerator::Chain.new(a, b).to_a
p (a + b).to_a
