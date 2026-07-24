# Array#object_id on typed arrays: a stable per-object integer. (Exact
# values are pointer-dependent, so check identity/distinctness/type.)
a = [1, 2, 3]
b = ["x", "y"]
c = [1.5, 2.5]
d = [1, "two"]
p(a.object_id == a.object_id)
p(a.object_id == b.object_id)
p(a.object_id.is_a?(Integer))
p(b.object_id.is_a?(Integer))
p(c.object_id.is_a?(Integer))
p(d.object_id.is_a?(Integer))
