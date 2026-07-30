# A short-circuiting `&&` over a scalar left operand yields that operand's nil,
# so nil? answers true and compact drops it. `||` keeps a truthy left as itself.
s = "a"
i = s.rindex("/")
v = i && s.upcase
p v
p v.nil?
p [v, "z"].compact
p [v, "z"].filter_map { |x| x }
j = s.rindex("a")
w = j && s.upcase
p w
p w.nil?
f = [].first
u = f && 1.5
p u.nil?
q = i && [1, 2]
p q.nil?
r = i || 7
p r
