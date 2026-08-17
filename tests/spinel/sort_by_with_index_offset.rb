a = ["bb", "a", "cccc"]
p a.sort_by.with_index(1) { |s, i| i }
p a.sort_by.with_index(1) { |s, i| -i }
p a.sort_by.with_index { |s, i| -i }
p a.sort_by.with_index(10) { |s, i| s.length * i }
p [3, 1, 2].sort_by.with_index(2) { |x, i| x }
e = [3, 1, 2].sort_by
p e.class
p [3, 1, 2].sort_by.class
