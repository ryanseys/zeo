r001 = ({ a: 1, b: 2 }.sum([]) rescue $!.class); p r001
p({ a: 1 }.sum("") { |k, _v| k.to_s })
p([[1], [2]].sum([]))
p({ a: 1, b: 2 }.sum(10) { |_k, v| v })
p([1, 2].sum(10))
