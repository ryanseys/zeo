hs001 = [{ b: 2 }]
p({ a: 1 }.merge(*hs001))
ks002 = [:a]
p({ a: 1, b: 2 }.except(*ks002))
p({ a: 1 }.merge({ b: 2 }))
p({ a: 1, b: 2 }.except(:a))
