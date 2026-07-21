h = {"a" => 1, "b" => 2}
# Missing-key reads are value-level nil (#801).
p h["miss"]
p h["miss"].nil?
p h["miss"].inspect
p(h["miss"] || 42)
# Stored in a local, then observed (the case the consumption-site rewrites miss).
v = h["miss"]
p v.nil?
p(v || 7)
# Present keys are unaffected.
p h["a"]
p h["a"].nil?
p(h["a"] + 1)
w = h["b"]
p(w + 10)
