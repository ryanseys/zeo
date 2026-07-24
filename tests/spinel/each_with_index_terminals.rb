p [10, 20, 30].each.with_index.to_a
p [10, 20, 30].each.with_index.map { |v, i| i }
p [10, 20, 30].each.with_index.any? { |v, i| i > 1 }
p [10, 20, 30].each.with_index.select { |v, i| i > 0 }
puts [10, 20, 30].each.with_index.count { |v, i| i > 0 }
p [10, 20, 30].each.with_index.reject { |v, i| i > 0 }
p [10, 20, 30].each.with_index.all? { |v, i| v > 5 }
p [10, 20, 30].each.with_index.none? { |v, i| i > 5 }
p %w[a b c].each.with_index.to_a
p %w[a b c].each.with_index.select { |s, i| i.odd? }
p [5, 6, 7].each_with_index.map { |v, i| v * i }
p [10, 20].each.with_index(100).to_a
p [1, 2, 3, 4].each.with_index.count { |v, i| v.even? }
p [10, 20, 30].each.with_index.collect { |v, i| v + i }
