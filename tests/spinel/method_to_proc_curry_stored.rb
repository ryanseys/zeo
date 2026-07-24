# A Method stored in a local, then .to_proc.curry'd, still resolves the
# target's arity/return through the assignment chain (#3244).
def greet(a, b) = "#{a},#{b}"
m = method(:greet)
g = m.to_proc.curry
p g["x"]["y"]
h = method(:greet).to_proc.curry
p h["a"]["b"]
