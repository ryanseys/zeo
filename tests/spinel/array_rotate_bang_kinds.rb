# rotate! over every element kind, including the wrap-around counts and the
# empty / single-element edges: the block-move form has to agree with the
# rotation it replaced.
a = [1, 2, 3, 4, 5]
a.rotate!
p a
a.rotate!(2)
p a
a.rotate!(-1)
p a
a.rotate!(0)
p a
a.rotate!(5)
p a
a.rotate!(7)
p a
b = ["a", "b", "c", "d"]
b.rotate!(3)
p b
c = [1.5, 2.5, 3.5]
c.rotate!(1)
p c
d = [1, "x", :y, nil]
d.rotate!(2)
p d
e = []
e.rotate!(3)
p e
f = [9]
f.rotate!(4)
p f
big = (1..40).to_a
big.rotate!(35)
p big.first(3)
p big.last(3)
