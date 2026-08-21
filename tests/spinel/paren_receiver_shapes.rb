# Parentheses around a receiver carry no meaning, but they hid the receiver
# from every shape check that asks what it IS: the scan for a Hash.new used as
# a receiver, the anonymous-struct receiver scan, the empty-literal receiver
# marking. Each left the value untyped, so its methods read as unresolved calls
# ("undefined method 'size' for unknown") even though the same call without the
# parentheses compiled. They are stripped once now, as `&(expr)` already was.
p((Hash.new(0)).size)
p((Hash.new(0)).inspect)
p((Hash.new(0))[:missing])
p((Struct.new(:a)).new(1))
p((Struct.new(:a, :b)).new(1, 2).to_a)
p((Data.define(:b)).new(2))
p(([]).size)
p(({}).size)
p(([]).push(1))
p(({}).size)
p((("x")).size)
p((1..3).to_a)
p(([1, 2].map { |x| x * 2 }).size)
p((Hash.new { |h, k| k.to_s }))
h = (Hash.new(7))
p h[:nope]
p (["a", "b"]).join("-")
