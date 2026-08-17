# An assignment's value is the right-hand side: `h.default_proc = p` answers
# the proc. Answering the hash put an sp_XHash* into a Proc* slot (#3833).
h = {}
r = (h.default_proc = ->(hh, k) { k.to_s })
p r.class
p h[:missing]
g = {}
g.default_proc = ->(hh, k) { 0 }
p g[:x]
p g.default_proc.class
