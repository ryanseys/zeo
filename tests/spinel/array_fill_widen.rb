# Array#fill whose value another element kind cannot hold must widen the array,
# not store the value's raw bits (an int array filled with :a held garbage).
a = [1, 2, 3]
a.fill(:a)
p a
b = [1, 2, 3, 4]
b.fill(:z, 2)
p b
c = [1, "x", 2]
c.fill("y")
p c
d = [1, 2, 3]
d.fill(9)
p d
e = [1.0, 2.0]
e.fill(0.5)
p e
