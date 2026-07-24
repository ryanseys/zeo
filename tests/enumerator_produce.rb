# Enumerator.produce -> a fiber-backed infinite generator (#2483)
e = Enumerator.produce(1) { |n| n * 2 }
p(e.take(3))
p(Enumerator.produce(1) { |n| n + 1 }.first(4))
g = Enumerator.produce(0) { |n| n + 2 }
p(g.next)
p(g.next)
p([Enumerator.produce(2) { |n| n * n }.take(3)])
