# Block-form each (and each_value/each_key/each_pair/each_with_index/
# reverse_each) returns its receiver, so the value composes and chains.
r = [1, 2, 3].each { |x| x }
p r
b = [1, 2, 3].each { |x| x }.map { |x| x * 2 }
p b
h = { a: 1 }.each_value { |v| v }
p h.size
k = { a: 1, b: 2 }.each_key { |x| x }
p k.keys
w = [1, 2].each_with_index { |x, i| x }
p w
v = [3, 4].reverse_each { |x| x }
p v
sum = 0
res = (1..3).each { |i| sum += i }
p sum
