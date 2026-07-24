# reject / select / filter / each_index on an Array that is a block param of an
# outer map (a poly value only known to be an array at runtime).
p([[1, 2, 0], [3, 0, 4]].map { |row| row.reject(&:zero?) })
p([[1, 2, 0]].map { |r| r.select { |x| x > 1 } })
p([[1, 2, 0]].map { |r| r.filter { |x| x > 0 } })
p([[[1, 2], [3, 0]]].map { |r| r.reject { |a, b| b == 0 } })
p([[10, 20, 30]].map { |row| s = 0; row.each_index { |i| s += i }; s })
# an unused each_index param must not bind an undeclared local
p([[10, 20, 30]].map { |row| c = 0; row.each_index { |i| c += 1 }; c })
