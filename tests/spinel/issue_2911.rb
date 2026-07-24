counts = Hash.new(0)
[0].product([0]).each { |k| counts[k] += 1 }
p counts[[0, 0]]

grid = Hash.new(0)
[1, 2].product([3, 4]).each { |pair| grid[pair] += 1 }
p grid[[1, 4]]
p grid[[2, 3]]

# eql? strictness preserved: [1] and [1.0] are distinct keys
h = {}
h[[1]] = "int"
h[[1.0]] = "float"
p h.size
