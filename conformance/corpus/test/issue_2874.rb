fns = [->(x) { x + 1 }, ->(x) { x * 2 }]
composed = fns.reduce(->(x) { x }) { |acc, f| ->(x) { f.call(acc.call(x)) } }
p composed.call(5)

more = [->(x) { x + 1 }, ->(x) { x * 2 }, ->(x) { x - 3 }]
c2 = more.reduce(->(x) { x }) { |acc, f| ->(x) { f.call(acc.call(x)) } }
p c2.call(10)
p c2.call(0)
