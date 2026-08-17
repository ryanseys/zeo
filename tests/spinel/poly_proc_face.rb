# The Proc face on a value read out of a container: lambda?, parameters, curry
# and to_proc had no arm for a boxed Proc, so each raised NoMethodError.
fs = [->(x) { x }]
p fs[0].lambda?
p fs[0].arity
p fs[0].parameters
p fs[0].curry.class
p fs[0].to_proc.lambda?

h = { a: ->(y) { y } }
p h[:a].lambda?
p h[:a].arity
p h[:a].parameters

ps = [proc { |z| z }]
p ps[0].lambda?
p ps[0].arity

# the direct forms are unchanged
f = ->(w) { w }
p f.lambda?
p f.arity
p f.parameters
p f.curry.class
