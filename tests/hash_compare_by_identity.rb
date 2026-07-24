# Hash#compare_by_identity — keys compared by object identity (equal?),
# not structure (eql?/hash).

h = {}
p h.compare_by_identity?          # false
r = h.compare_by_identity
p r.equal?(h)                     # true — returns self
p h.compare_by_identity?          # true

# Two equal-but-distinct String objects are DISTINCT keys.
a = "x" + ""
b = "x" + ""
h[a] = 1
h[b] = 2
p h.size                          # 2
p h[a]                            # 1
p h[b]                            # 2
p h["x" + ""]                     # nil — a fresh, different object
p h.key?(a)                       # true

# delete uses identity too.
h.delete(a)
p h.size                          # 1
p h[a]                            # nil
p h[b]                            # 2

# Immediates (Integer/Symbol/nil/true/Float) key by value even in identity mode.
hi = {}.compare_by_identity
hi[1] = "one"
hi[1] = "ONE"
p hi.size                         # 1
p hi[1]                           # "ONE"
hi[:s] = 10
hi[:s] = 20
p hi[:s]                          # 20
hi[1.5] = "f"
p hi[1.5]                         # "f"

# Enabling on a POPULATED hash re-projects existing entries by identity:
# the entries survive, but lookups now use identity (a fresh equal object misses).
hp = {}
hp["p" + ""] = 1
hp["q" + ""] = 2
p hp.size                         # 2
hp.compare_by_identity
p hp.compare_by_identity?         # true
p hp.size                         # 2 — entries preserved
p hp["p" + ""]                    # nil — fresh, different object

# A frozen hash raises FrozenError.
begin
  {}.freeze.compare_by_identity
rescue => e
  p e.class                       # FrozenError
end
