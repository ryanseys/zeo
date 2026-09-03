pairs = [["ab", "cd"]]
pairs.each { |a, b| p a.chars; p b.chars }
p [["xy", 1]].map { |s, n| s.chars }
__END__
["a", "b"]
["c", "d"]
[["x", "y"]]
