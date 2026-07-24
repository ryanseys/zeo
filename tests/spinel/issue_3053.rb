p :upcase.to_proc.arity
p :+.to_proc.arity
s = :upcase
p s.to_proc.arity
p :upcase.to_proc.call("ab")
p :+.to_proc.call(4, 5)
p ["x", "y"].map(&:upcase)
p :upcase.to_proc.lambda?
