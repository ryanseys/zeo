a = ->(n) { n.to_s }
b = ->(s) { s.length }
p((a >> b).call(100))
c = ->(s) { s.upcase }
d = ->(n) { "n#{n}" }
p((c << d).call(5))
# variable-bound composition
comp = a >> b
p(comp.call(42))
