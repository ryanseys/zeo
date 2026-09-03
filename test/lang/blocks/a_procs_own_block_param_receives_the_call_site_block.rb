# A proc/lambda that declares its own `&block` parameter receives the
# block passed to its `#call` -- the block rides the closure's third
# parameter into the body, where `b.call`/`b.nil?` reflect it. Covers a
# Proc-typed receiver (the lambda literal, a fast-path `#call`), a Poly
# local (`f`/`g`, the dynamic `Proc#call` builtin), a blockless call
# (binds nil), and a passed block that captures an enclosing local.

p(->(&b) { b.call(9) }.call { |x| x + 1 })
f = ->(&b) { b.nil? ? "none" : b.call(1) }
p f.call
p(f.call { |x| x * 100 })
g = ->(a, &b) { a + b.call(a) }
p(g.call(5) { |x| x * 2 })
base = 50
p(->(&b) { b.call(3) }.call { |x| base + x })
__END__
10
"none"
100
15
53
