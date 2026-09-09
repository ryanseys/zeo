# `each { |a, b| }` and `map { |s, n| }` over pairs, calling chars on the bound String.
# (spinel issue #2909)
pairs = [["ab", "cd"]]
pairs.each { |a, b| p a.chars; p b.chars }
p [["xy", 1]].map { |s, n| s.chars }
__END__
["a", "b"]
["c", "d"]
[["x", "y"]]
