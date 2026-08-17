[[1, 2]].each { |a001| p a001.each.to_a }
p [[1, 2], [3]].map { |a| a.each.to_a }
[[1, 2]].each { |a| e = a.each; p e.next; p e.next }
[["x", "y"]].each { |a| p a.each.map { |s| s.upcase } }
h = { k: [1, 2] }
p h[:k].each.to_a
