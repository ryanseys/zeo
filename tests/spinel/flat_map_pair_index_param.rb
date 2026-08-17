h = { 1 => [1], 2 => [2] }
n = 0
h.each_with_index.flat_map { |(k, g), i| n += 1; [k, i] }.each { |v| }
p n

a = [[1, 2], [3, 4]]
m = 0
p(a.each_with_index.flat_map { |(x, y), i| m += 1; [x, y, i] })
p m

p(a.each_with_index.flat_map { |(x, y), i| [i] })
