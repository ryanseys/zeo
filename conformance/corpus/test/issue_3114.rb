def f(*a, foo: nil); p a; p foo; end
f("xyz", "bar")
f("xyz", "bar", foo: 9)
def g(a, foo: 1); p [a, foo]; end
g(10)
g(10, foo: 2)
def h(*a, foo: 5, bar: 6); p [a, foo, bar]; end
h(1, 2, 3)
h(1, bar: 9)
