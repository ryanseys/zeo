S = Struct.new(:a, :b)
s = S.new(1, 2)
i = 1
s[i] = 9
p s.b
r = (begin; s[9] = 1; "no error"; rescue => e; e.class; end); p r
r2 = (begin; p s[9]; rescue => e; p e.class; end)
r3 = (begin; p s[:zz]; rescue => e; p e.class; end)
p s["a"]
p s.values_at
k = :b
p s.dig(k)
r4 = (begin; p s.values_at(k); rescue => e; p e.class; end)
s[:a] = 5
p s.a
s["b"] = 6
p s.b
p s.to_a
