# A chained poly `+` whose later operand allocates.
#
# `a + b + c` on poly operands compiles to sp_poly_add(sp_poly_add(a, b), c).
# The inner result is a freshly built string living only in a C temporary, and
# `c` is evaluated after it -- so an allocation there collects with the
# intermediate held nowhere the root scan can see. The sweep freed it and the
# outer concat read freed memory: a use-after-free that ASAN names and a plain
# build turns into a segfault or silent corruption, depending on what reused
# the block.
#
# The monomorphic chain never had this; only the poly path left the
# intermediate unrooted.

def build_name(prefix, mid, i)
  prefix + mid + i.to_s
end

# an Integer call site widens the parameters to poly, which is what routes the
# `+` through sp_poly_add
def widen_them
  build_name(1, 2, 0)
end

s = ""
i = 0
while i < 2000
  s = build_name("blk.", "attn_k.head_", i)
  i += 1
end
p s
p s.length

# the same shape with the allocating operand in the middle
def middle(a, b)
  a + b.to_s + "!"
end

def widen_middle
  middle(1, 2)
end

p middle("n=", 41)

# a longer chain: every intermediate has to survive the next operand
def four(a, b, c, d)
  a + b + c + d
end

def widen_four
  four(1, 2, 3, 4)
end

p four("a", "b", 3.to_s, "d")
