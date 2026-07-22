s = [["a", "b"]][0][0]
p(s.ljust(3))
p(s.rjust(3))
p(s.center(5))
p(s.ljust(5, "*"))
p(s.rjust(5, "ab"))
p(s.center(7, "-"))
p(s.upcase)
h = {k: ["x", "y"]}
t = h[:k][0]
p(t.ljust(4))
pair = [["hi", 1]].map { |k, _v| k.ljust(4) }
p(pair)
