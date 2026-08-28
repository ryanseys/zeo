# A first-class proc publishes its result boxed and the `.call` site unboxes it.
# That unbox was a second, hand-written copy of emit_unbox_text and had drifted:
# Range, Time and Class were named, so Rational and Complex fell to the generic
# `(T)slot.v.p` at the end -- a pointer cast to a struct, which does not compile.
# The hash variants fell there too, and those read another variant's layout
# without a diagnostic. The list is gone; the one unbox answers for every kind.

pr = proc { Rational(3, 2) }
puts pr.call.to_s

pc = proc { Complex(1, 2) }
puts pc.call.to_s

pg = proc { 1..3 }
puts pg.call.to_s

pf = proc { 1.5..3.5 }
puts pf.call.to_s

pk = proc { 1.5 }
puts pk.call.to_s

ps = proc { "ab" }
puts ps.call

# the value flowing on, not just printed
puts (pr.call + Rational(1, 2)).to_s
puts (pc.call * Complex(0, 1)).to_s
puts pg.call.to_a.length.to_s

# a lambda, and a proc stored and called later
lm = lambda { Rational(5, 4) }
puts lm.call.to_s
store = [proc { Complex(2, 3) }]
puts store[0].call.to_s

# Hash#to_proc boxes the hash's VALUE type through the same ABI: the boxing
# half had the same drift, with no Range arm at all
h = { "a" => (1..3), "b" => (4..6) }
p ["a", "b"].map(&h)
r = { "a" => Rational(1, 2), "b" => Rational(3, 4) }
p ["a", "b"].map(&r)
c = { "a" => Complex(1, 2) }
p ["a"].map(&c)
