# Each of them called on the array bound by an outer map's block parameter,
# including a nested destructuring reject and an unused each_index parameter.
# (spinel issue #2930)
p([[1, 2, 0], [3, 0, 4]].map { |row| row.reject(&:zero?) })
p([[1, 2, 0]].map { |r| r.select { |x| x > 1 } })
p([[1, 2, 0]].map { |r| r.filter { |x| x > 0 } })
p([[[1, 2], [3, 0]]].map { |r| r.reject { |a, b| b == 0 } })
p([[10, 20, 30]].map { |row| s = 0; row.each_index { |i| s += i }; s })
# an unused each_index param must not bind an undeclared local
p([[10, 20, 30]].map { |row| c = 0; row.each_index { |i| c += 1 }; c })
__END__
[[1, 2], [3, 4]]
[[2]]
[[1, 2]]
[[[1, 2]]]
[3]
[3]
