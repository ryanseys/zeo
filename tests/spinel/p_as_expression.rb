# Kernel#p returns its argument, so it composes as an expression:
# x = p(y), puts p(y), f(p(y)). Statement-position p is unchanged.
x = p(42)
puts x
puts p("s")
def f(v) = v * 2
puts f(p(5))
y = p([1, 2])
puts y.length
h = p({ a: 1 })
puts h[:a]
z = p(3.5)
puts z + 0.5
p 7
