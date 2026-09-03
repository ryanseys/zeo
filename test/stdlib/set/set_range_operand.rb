require 'set'
p((Set[1, 2] & (2..4)).to_a)
p((Set[1, 2] | (2..4)).to_a)
p((Set[1, 2] + (2..4)).to_a)
p(Set[1,2].union(2..4).to_a)
p(Set[1,2].intersection(2..4).to_a)
p((Set["a", "b"] & ("a".."c")).to_a)
p(Set.new(1..3).to_a)
s = Set[1]; s.merge(2..3); p s.to_a
t = Set[1,2,3]; t.subtract(2..3); p t.to_a
u = Set[9]; u.replace(1..2); p u.to_a
p((Set[1,2,3] - (2..4)).to_a)
__END__
[2]
[1, 2, 3, 4]
[1, 2, 3, 4]
[1, 2, 3, 4]
[2]
["a", "b"]
[1, 2, 3]
[1, 2, 3]
[1]
[1, 2]
[1]
