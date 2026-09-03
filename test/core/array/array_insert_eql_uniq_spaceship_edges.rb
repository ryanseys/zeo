# insert past the end pads with nil; eql?/uniq are class-strict (1 != 1.0);
# <=> of an array with itself (incl. a cycle) is 0 without deadlock.
#
# The self-comparison needs a self-referential array, and it is still one
# at exit -- the program never breaks the ring.
#@ gccheck: cycle leak: 1 objects (Array x1)

b = [1, 2, 3]
b.insert(5, 8)
p b
p [1, 2].eql?([1, 2.0])
p [1, 2].eql?([1, 2])
p [1.0, 1].uniq
a = [1, 2, 3]
p(a <=> a)
r = [1, 2]
r.push(r)
p(r <=> r)
__END__
[1, 2, 3, nil, nil, 8]
false
true
[1.0, 1]
0
0
