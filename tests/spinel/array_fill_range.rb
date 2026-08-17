# fill(value, range): a negative end counts from the end and an endless one
# runs to the last element. Unnormalized, `fill(v, 2..)` grew the array until
# the process died and `fill(v, 1..-1)` filled nothing.
a=[1,2,3,4,5]
p a.fill(9, 1..-1)
b=[1,2,3,4,5]
p b.fill(9, 2..)
c=[1,2,3,4,5]
p c.fill(9, 1..2)
d=[1,2,3,4,5]
p d.fill(9, 2)
e=[1,2,3,4,5]
p e.fill(9, 1...3)
f=[1,2,3,4,5]
p f.fill(9, -2..)
g=[1,2,3]
p g.fill(9, 0..9)
h=["a","b","c"]
p h.fill("z", 1..-1)
