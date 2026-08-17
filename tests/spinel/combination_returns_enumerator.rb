p([1, 2, 3].combination(2).class)
p([1, 2, 3].permutation(2).class)
p([1, 2, 3].repeated_combination(2).class)
a001 = [1, 2, 3]
p a001.combination(2).class
v001 = a001.permutation(2); p v001.class
p([1,2,3].combination(2).to_a)
p([1,2,3].permutation(2).to_a.size)
r = []; [1,2,3].combination(2) { |x| r << x }; p r
