# Two sets with the same members in a different order hash alike; a different
# one does not.
require "set"
a = Set[1, 2, 3]
b = Set[3, 2, 1]
c = Set[1, 2]
p a.hash == b.hash
p a.hash == c.hash
p a.eql?(b)
p Set["x", "y"].hash == Set["y", "x"].hash
__END__
true
false
true
true
