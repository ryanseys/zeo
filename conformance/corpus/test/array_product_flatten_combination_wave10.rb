# Array#product (block form and no-arg), flatten/flatten! with a depth,
# and value-captured block forms of the combination/slice family
# (all return self per Ruby >= 3.1).
r = []
[1, 2].product([3, 4]) { |pair| r << pair }
p r
p([1, 2].product)
a = [1, 2, [3, [4]]]
p(a.flatten!(1))
p a
b = [1, 2]
p(b.flatten!(1))
v = [1, 2, 3].each_slice(2) { |s| p s }
p v
w = [1, 2, 3].combination(2) { |c| p c }
p w
x = [1, 2].repeated_permutation(2) { |q| p q }
p x
r = []
[1, 2].product([3, 4]) { |pair| r << pair }
p r
p([1, 2].product)
v = %w[a b].product([1]) { |q| p q }
p v
a = [1, 2, [3, [4]]]
p(a.flatten!(1))
p a
b = [1, 2]
p(b.flatten!(1))
c = [1, [2, [3, [4]]]]
p(c.flatten(2))
p c
v = [1, 2, 3].each_slice(2) { |s| p s }
p v
w = [1, 2, 3].combination(2) { |c| p c }
p w
x = [1, 2].repeated_permutation(2) { |q| p q }
p x
y = [1, 2].repeated_combination(2) { |q| p q }
p y
z = [1, 2, 3].each_cons(2) { |q| p q }
p z
u = [1, 2, 3].permutation(2) { |q| p q }
p u
