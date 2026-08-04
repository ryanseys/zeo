# The empty literal on the right of `||` / `&&` was emitted with its own
# default representation (an Integer array, whatever the left held), so the C
# conditional mixed sp_StrArray * with sp_IntArray *. The arm has to take the
# expression's container type, the way a ternary arm already did. #3462.
a = ["x"]
p(a || [])
h = { "a" => 1 }
p(h.keys || [])
rows = [{ "a" => 1 }]
p(rows.first.keys || [])
f = [1.5]
p(f || [])
i = [1]
p(i || [])
p(a || ["z"])
p(a && [])
p((nil || []).size)
syms = [:a]
p(syms || [])
objs = [[1], [2]]
p(objs || [])
p((h.values || []).size)
def pick(x) = x || []
p pick(["y"])
p pick(nil).size
