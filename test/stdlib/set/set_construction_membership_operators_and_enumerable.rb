s = Set[3, 1, 2, 1]
p s.size
p s.include?(2)
p (Set[1, 2] | Set[2, 3]).to_a.sort
p Set[1, 2].subset?(Set[1, 2, 3])
p s.map { |x| x * 2 }.sort
__END__
3
true
[1, 2, 3]
true
[2, 4, 6]
