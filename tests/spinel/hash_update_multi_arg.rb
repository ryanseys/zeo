# Hash#update / #merge! with several hash arguments folds each in order and
# returns the receiver (a value the caller can use, not nil).
p({ a: 1 }.update({ b: 2 }, { c: 3 }))
h = { x: 1 }
r = h.merge!({ y: 2 }, { z: 3 })
p r
p h.equal?(r)
