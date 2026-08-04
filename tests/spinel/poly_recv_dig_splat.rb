# `dig(*keys)` on a receiver reached with more than one container class: the
# key list only exists at run time, and the arms for it are per container kind,
# so with none matching the call was coerced to a hash and raised NoMethodError
# on an Array that answers it. The runtime walk dispatches on the receiver's
# own kind at each step (#3509).
def lookup(config, keys)
  config.dig(*keys)
end
p lookup({ a: { b: 1 } }, [:a, :b])
p lookup([[1, 2]], [0, 1])
p lookup({ a: { b: 1 } }, [:a])
p lookup([[1, 2]], [0])
p lookup({ a: { b: 1 } }, [:missing])
p lookup([[1, 2]], [9])

# the block-parameter form, which reaches it through a different emitter
box = [{ a: { b: 1 } }, [[1, 2]]]
keys = [0, 1]
box.each { |c| p c.dig(*keys) }

# a positional key list over the same polymorphic receiver
def lookup2(config, k1, k2)
  config.dig(k1, k2)
end
p lookup2({ a: { b: 5 } }, :a, :b)
p lookup2([[7, 8]], 0, 1)

# an Integer key on a symbol-keyed hash is a miss, not the symbol at that number
h = { a: 1, b: 2 }
p [h].map { |c| c.dig(0) }
p [h].map { |c| c.dig(0, 1) }
p [h].map { |c| c.dig(:a) }
