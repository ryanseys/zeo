# Array set-operation op-assign: `a |= b` (union), `a &= b` (intersection),
# `a -= b` (difference) desugar to `a = a OP b`, mirroring the already-working
# binary operators. Values route through a per-kind identity method so each
# local keeps a concrete array type and the typed runtime helper is exercised
# rather than a compile-time fold.
def ai(x); x; end   # Integer arrays
def aw(x); x; end   # String arrays
def af(x); x; end   # Float arrays
def am(x); x; end   # poly arrays

# Integer arrays
g = ai([1, 2, 3])
g |= ai([3, 4])
p g
g &= ai([1, 4])
p g
g -= ai([4])
p g

# String arrays
w = aw(["a", "b"])
w |= aw(["b", "c"])
p w
w -= aw(["a"])
p w

# Float arrays
f = af([1.0, 2.0])
f |= af([2.0, 3.0])
p f

# poly arrays
m = am([1, "a", 2])
m |= am(["a", 3])
p m

# union dedups preserving first-seen order
d = ai([1, 2, 2, 3])
d |= ai([3, 3, 4])
p d

# empty rhs
e = ai([1, 2, 3])
e &= []
p e
e2 = ai([1, 2, 3])
e2 |= []
p e2
