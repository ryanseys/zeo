p [1, 2].min_by { |x| [x] }
p [1, 2].max_by { |x| [x] }
p [[3, 1], [1, 2]].min_by { |a| a }
p [[3, 1], [1, 2]].max_by { |a| a }
p ["b", "aa"].min_by { |s| [s.length, s] }
p [1, 2, 3].min_by { |x| [x % 2, x] }
p [1, 2].min_by { |x| x }
p ["b", "a"].min_by { |s| s }
require "set"
p Set.new([1, 2]).min_by { |x| [x] }
p({ a: 1, b: 2 }.min_by { |k, v| [v] })
