# Array#member? (an alias of include?) works on an empty array held in a
# variable, not only on the empty literal.
a = []
p a.member?(1)
b = [1, 2, 3]
p b.member?(2)
p b.member?(9)
c = [1, "x"]
p c.member?("x")
