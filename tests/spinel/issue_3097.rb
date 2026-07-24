r1 = (:pow.to_proc.call(2, 3) rescue $!.class); p r1
s = :+; r2 = (s.to_proc.call(4, 5) rescue $!.class); p r2
p :+.to_proc.call(4, 5)
p :upcase.to_proc.call("ab")
p :sub.to_proc.call("hello", "l", "L")
p :upcase.to_proc.arity
p ["x", "y"].map(&:upcase)
