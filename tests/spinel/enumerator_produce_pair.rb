p(Enumerator.produce([0, 1]) { |a, b| [b, a] }.first(2))
p(Enumerator.produce([0, 1]) { |a, b| [b, a + b] }.take(5))
p(Enumerator.produce(1) { |n| n + 1 }.first(3))
