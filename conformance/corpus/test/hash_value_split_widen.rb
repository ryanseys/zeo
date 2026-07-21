# Sibling to #437. `h = {}; h[pa[i]] = pb[i]` where both `pa` and
# `pb` come from `split` results inside a method body. #437 added
# str_int_hash → str_str_hash widening in refine_locals_multi_pass_full
# but the analogous merge rule in refine_method_body_locals was
# missing, so `def f; ... end` shapes stayed at str_int_hash even
# after pass 2 resolved both key and value as string.

def build_str_str(a, b)
  pa = a.split("/")
  pb = b.split("/")
  h = {}
  i = 0
  while i < pa.length
    h[pa[i]] = pb[i]
    i += 1
  end
  h
end

r = build_str_str("a/b", "x/y")
puts r["a"]
puts r["b"]
