# slice!(start, len) with a start past the end is nil, not an empty slice --
# only start == length answers []. The start was clamped to the length, so
# every out-of-range call answered [].
a=[1,2,3]; p a.slice!(9,2); p a
b=[1,2,3]; p b.slice!(3,2); p b
c=[1,2,3]; p c.slice!(1,2); p c
d=[1,2,3]; p d.slice!(-1,1); p d
e=[1,2,3]; p e.slice!(1,0); p e
f=["a","b"]; p f.slice!(9,1); p f
g=[1,:x]; p g.slice!(9,1); p g
__END__
nil
[1, 2, 3]
[]
[1, 2, 3]
[2, 3]
[1]
[3]
[1, 2]
[]
[1, 2, 3]
nil
["a", "b"]
nil
[1, :x]
